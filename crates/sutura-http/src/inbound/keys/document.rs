//! A key set document is a JWK set, or one PEM public key or certificate.
//!
//! The document decides which: RFC 7468 armour (`-----BEGIN `) cannot open a JSON text, so a
//! document that opens with it is read as PEM and anything else as a JWK set. A PEM carries ONE
//! signing key - a single `PUBLIC KEY` block, or the first `CERTIFICATE` block, taken as the leaf
//! without a check. The `CERTIFICATE` blocks after it are its chain: skipped, never decoded, never a key. Its key
//! id is the key's RFC 7638 SHA-256 thumbprint, so a signer derives the same `kid` from the same
//! material. A certificate's validity and chain are not read: the key verifies signatures and
//! nothing else.

use std::collections::BTreeMap;

use base64::Engine as _;
use jsonwebtoken::DecodingKey;
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, JwkSet, ThumbprintHash};
use spki::der::{Reader as _, SliceReader, Tag, TagNumber};
use spki::{ObjectIdentifier, SubjectPublicKeyInfoRef};
use sutura_config::KeyFamily;

use super::{InvalidKeySet, KeyId, KeySet, Verifier};

const CERTIFICATE: &str = "CERTIFICATE";
const PUBLIC_KEY: &str = "PUBLIC KEY";

const RSA_ENCRYPTION: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const EC_PUBLIC_KEY: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const P_256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");
const P_384: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.34");
const ED25519: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.112");

/// Parses a key set document: a PEM when it opens with armour, a JWK set otherwise.
///
/// A private key block is refused first, wherever it sits and whatever surrounds it, on its `BEGIN`
/// line alone: its body is never decoded. No JSON text holds such a line.
pub(super) fn parse(document: &str) -> Result<KeySet, InvalidKeySet> {
    if document
        .lines()
        .filter_map(|line| armour(line.trim(), "BEGIN"))
        .any(|label| label.ends_with("PRIVATE KEY"))
    {
        return Err(InvalidKeySet::PrivateKeyMaterial);
    }
    if document.trim_start().starts_with("-----BEGIN ") {
        return from_pem(document);
    }
    let set: JwkSet = serde_json::from_str(document).map_err(|cause| InvalidKeySet::NotAJwkSet { cause })?;
    let mut keys: BTreeMap<KeyId, Verifier> = BTreeMap::new();
    for jwk in &set.keys {
        let id = jwk
            .common
            .key_id
            .as_deref()
            .ok_or(InvalidKeySet::KeyWithoutAnId)
            .and_then(|raw| KeyId::parse(raw).map_err(|cause| InvalidKeySet::UnusableKeyId { cause }))?;
        if keys.contains_key(&id) {
            return Err(InvalidKeySet::DuplicateKeyId { id });
        }
        drop(keys.insert(id, verifier_for(jwk)?));
    }
    if keys.is_empty() {
        return Err(InvalidKeySet::NoUsableKey);
    }
    Ok(KeySet { keys })
}

/// The one key a PEM document carries, under its thumbprint.
fn from_pem(document: &str) -> Result<KeySet, InvalidKeySet> {
    let KeyBlock { label, body } = key_block(document)?;
    let der = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|_undecodable| InvalidKeySet::NotPem)?;
    let spki = if label == CERTIFICATE {
        subject_public_key_info(&der).map_err(|_unreadable| InvalidKeySet::UnreadableCertificate)?
    } else {
        &der
    };
    let jwk = jwk_of(spki)?;
    let thumbprint = jwk
        .thumbprint(ThumbprintHash::SHA256)
        .map_err(|cause| InvalidKeySet::UnusableKey { cause })?;
    let id = KeyId::parse(thumbprint).map_err(|cause| InvalidKeySet::UnusableKeyId { cause })?;
    Ok(KeySet {
        keys: BTreeMap::from([(id, verifier_for(&jwk)?)]),
    })
}

/// The label and the still-encoded body of the one block that carries the key.
struct KeyBlock<'doc> {
    label: &'doc str,
    body: String,
}

/// Finds the [`KeyBlock`].
///
/// The label is decided on the `BEGIN` line, before any body is read. Outside a block only blank
/// lines are accepted, and inside one any other armour line is malformed - so nothing can hide in a
/// skipped chain certificate.
fn key_block(document: &str) -> Result<KeyBlock<'_>, InvalidKeySet> {
    let mut key: Option<KeyBlock<'_>> = None;
    // The open block's label, and whether its body is the key's.
    let mut open: Option<(&str, bool)> = None;
    for line in document.lines().map(str::trim) {
        match open {
            None if line.is_empty() => {}
            None => {
                let label = armour(line, "BEGIN").ok_or(InvalidKeySet::NotPem)?;
                if label != CERTIFICATE && label != PUBLIC_KEY {
                    return Err(InvalidKeySet::UnsupportedPemBlock);
                }
                let is_key = match key.as_ref().map(|held| held.label) {
                    None => true,
                    Some(CERTIFICATE) if label == CERTIFICATE => false,
                    Some(_) => return Err(InvalidKeySet::MoreThanOneKey),
                };
                if is_key {
                    key = Some(KeyBlock {
                        label,
                        body: String::new(),
                    });
                }
                open = Some((label, is_key));
            }
            Some((label, _)) if armour(line, "END") == Some(label) => open = None,
            Some(_) if line.starts_with("-----") => return Err(InvalidKeySet::NotPem),
            Some((_, true)) => {
                if let Some(held) = key.as_mut() {
                    held.body.push_str(line);
                }
            }
            Some((_, false)) => {}
        }
    }
    if open.is_some() {
        return Err(InvalidKeySet::NotPem);
    }
    key.ok_or(InvalidKeySet::NoUsableKey)
}

/// The label of a `-----BEGIN <label>-----` or `-----END <label>-----` line.
fn armour<'line>(line: &'line str, edge: &str) -> Option<&'line str> {
    line.strip_prefix("-----")?
        .strip_prefix(edge)?
        .strip_prefix(' ')?
        .strip_suffix("-----")
}

/// The DER subject public key info of an X.509 certificate, RFC 5280 section 4.1.
///
/// A walk over the certificate's fields and not a certificate parse: nothing before or after the
/// key is interpreted, so neither validity nor signature is read.
fn subject_public_key_info(certificate: &[u8]) -> Result<&[u8], spki::der::Error> {
    let mut reader = SliceReader::new(certificate)?;
    let spki = reader.sequence(|certificate| {
        let spki = certificate.sequence(|tbs| {
            let version = Tag::ContextSpecific {
                constructed: true,
                number: TagNumber(0),
            };
            if Tag::peek(tbs)? == version {
                let _version = tbs.tlv_bytes()?;
            }
            for _field in ["serialNumber", "signature", "issuer", "validity", "subject"] {
                let _skipped = tbs.tlv_bytes()?;
            }
            let spki = tbs.tlv_bytes()?;
            let _extensions = tbs.read_slice(tbs.remaining_len())?;
            Ok::<_, spki::der::Error>(spki)
        })?;
        let _signature = certificate.read_slice(certificate.remaining_len())?;
        Ok::<_, spki::der::Error>(spki)
    })?;
    reader.finish()?;
    Ok(spki)
}

/// The public JWK of a DER subject public key info.
///
/// An elliptic-curve key must name P-256 or P-384 and carry a point of that curve's length: the
/// library infers the curve from the length alone, so a point that disagrees with its named curve
/// is refused here rather than read as the other curve.
fn jwk_of(spki: &[u8]) -> Result<Jwk, InvalidKeySet> {
    let info = SubjectPublicKeyInfoRef::try_from(spki).map_err(|_unreadable| InvalidKeySet::UnreadablePublicKey)?;
    let bits = info.subject_public_key.as_bytes().ok_or(InvalidKeySet::UnreadablePublicKey)?;
    let key = match info.algorithm.oid {
        RSA_ENCRYPTION => DecodingKey::from_rsa_der(bits),
        EC_PUBLIC_KEY => match (info.algorithm.parameters_oid(), bits.len()) {
            (Ok(P_256), 65) | (Ok(P_384), 97) => DecodingKey::from_ec_der(bits),
            _ => return Err(InvalidKeySet::UnreadablePublicKey),
        },
        ED25519 => DecodingKey::from_ed_der(bits),
        _ => return Err(InvalidKeySet::UnsupportedKeyFamily),
    };
    Jwk::from_decoding_key(&key, None).map_err(|cause| InvalidKeySet::UnusableKey { cause })
}

/// Builds a verifier for one JWK, refusing the kinds that must not be here.
///
/// Split out of [`parse`] so the symmetric refusal is one named branch rather than an arm
/// inside a loop inside a fold. The family comes from the JWK's own parameters rather than from its
/// `alg`, which is optional and advisory.
fn verifier_for(jwk: &Jwk) -> Result<Verifier, InvalidKeySet> {
    let family = match jwk.algorithm {
        // See `InvalidKeySet::SymmetricKey`. This arm exists before the `from_jwk` call and not
        // after, because `from_jwk` would happily build an HMAC verifier out of it.
        AlgorithmParameters::OctetKey(_) => return Err(InvalidKeySet::SymmetricKey),
        AlgorithmParameters::RSA(_) => KeyFamily::Rsa,
        AlgorithmParameters::EllipticCurve(_) => KeyFamily::EllipticCurve,
        AlgorithmParameters::OctetKeyPair(_) => KeyFamily::EdwardsCurve,
        _ => return Err(InvalidKeySet::UnsupportedKeyFamily),
    };
    let key = DecodingKey::from_jwk(jwk).map_err(|cause| InvalidKeySet::UnusableKey { cause })?;
    Ok(Verifier { key, family })
}

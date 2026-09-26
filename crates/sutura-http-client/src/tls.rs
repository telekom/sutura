//! Folding a deployment's `security.outbound.transport_anchors` (`github.com/telekom/sutura#125`)
//! into a `ureq` agent - the one place either reader's HTTP client does it, so the conversion is
//! written once. Moved here (issue #970's review) from a byte-for-byte `tls_roots.rs` copy each of
//! `sutura-catalog-datahub` and `sutura-catalog-openmetadata` carried.
//!
//! **What this module is not.** It reads no file and never fails: `sutura_tls::load_anchors`
//! already did the fallible half (a composition root calls it, once, at boot), and this module's
//! only job is folding an already-loaded [`sutura_tls::LoadedAnchors`] into the shape
//! `ureq::tls::TlsConfig` wants - `RootCerts::Specific`, over `Certificate::from_der` - or leaving
//! `ureq`'s own default, `RootCerts::WebPki`, untouched when nothing was declared. A client
//! identity is folded the same way over [`client_cert`], for a peer in front of the endpoint that
//! demands one.
//!
//! **Why the conversion lives here and not in `sutura-tls`.** `sutura-tls` depends on
//! `rustls-pki-types` only, never on a network client (`xtask/src/boundaries/edges.rs`'s
//! `sutura-tls -> ring` forbidden edge is what a `ureq` dependency there would reintroduce, since
//! this workspace pins `rustls` to the `ring` provider); `sutura-exec-postgres`'s `tls` folds the
//! same loaded certificates into a `rustls::RootCertStore` instead. Sharing stops at "these are the
//! bytes the declaration named"; which library reads them is each adapter's own decision.
//!
//! **Why the key fold goes through PEM rather than `ureq::tls::PrivateKey::from_der`.** That
//! constructor's own first argument is `ureq::tls::KeyKind` - and `ureq-3.4.2` never re-exports
//! that type from `ureq::tls`, so no crate outside `ureq` can name a value of it at all. [`pem`]
//! re-armors the already-loaded DER this crate holds and hands it to `Certificate::from_pem`/
//! `PrivateKey::from_pem` instead - the one pair of constructors `ureq` actually exposes, which
//! recover the kind from the PEM label themselves. [`sutura_tls::LoadedIdentity::key_kind`] is what
//! chooses that label. `sutura_exec_clickhouse::tls`'s `owned_key` re-armors for the identical
//! `ureq::tls::KeyKind` limitation, though that crate's certificates go through
//! `Certificate::from_der` directly rather than a second PEM round trip.

use sutura_tls::LoadedAnchors;

/// The `ureq` TLS configuration a reader's agent is built with.
///
/// `None` leaves `ureq`'s own default (`RootCerts::WebPki`) in place - the compiled-in root set
/// every deployment verified against before `#125`. `Some` replaces it with `RootCerts::Specific`,
/// built from exactly the certificates the composition root loaded - never a union of the two, and
/// never a second read of the bundle. `identity` is folded the same way, over [`client_cert`].
pub(crate) fn config(anchors: Option<LoadedAnchors>, identity: Option<sutura_tls::LoadedIdentity>) -> ureq::tls::TlsConfig {
    let builder = ureq::tls::TlsConfig::builder();
    let builder = match anchors {
        None => builder,
        Some(loaded) => builder.root_certs(root_certs(loaded)),
    };
    let builder = match identity {
        None => builder,
        Some(identity) => builder.client_cert(Some(client_cert(identity))),
    };
    builder.build()
}

/// The one conversion: a loaded client identity's chain and key, each armored as PEM and handed to
/// `ureq`'s own PEM parser, collected into a [`ureq::tls::ClientCert`].
#[expect(
    clippy::unreachable,
    reason = "sutura_tls already parsed this identity once; re-armoring its own DER as PEM and \
              handing it back to ureq's own parser cannot fail on a value that already parsed"
)]
fn client_cert(identity: sutura_tls::LoadedIdentity) -> ureq::tls::ClientCert {
    let label = match identity.key_kind() {
        sutura_tls::KeyKind::Pkcs1 => "RSA PRIVATE KEY",
        sutura_tls::KeyKind::Sec1 => "EC PRIVATE KEY",
        sutura_tls::KeyKind::Pkcs8 => "PRIVATE KEY",
    };
    let (chain, key) = identity.into_parts();
    let private_key = match ureq::tls::PrivateKey::from_pem(pem(label, key.secret_der()).as_bytes()) {
        Ok(key) => key,
        // `sutura_tls` already parsed this key once; re-armoring its own DER and handing it back
        // to `ureq`'s own parser cannot fail on a value that already parsed.
        Err(cause) => unreachable!("a re-armored, already-loaded private key failed to re-parse: {cause}"),
    };
    let certificates: Vec<ureq::tls::Certificate<'static>> = chain
        .iter()
        .map(
            |der| match ureq::tls::Certificate::from_pem(pem("CERTIFICATE", der.as_ref()).as_bytes()) {
                Ok(certificate) => certificate,
                Err(cause) => unreachable!("a re-armored, already-loaded certificate failed to re-parse: {cause}"),
            },
        )
        .collect();
    ureq::tls::ClientCert::new_with_certs(&certificates, private_key)
}

/// Armors raw DER as PEM under `label`, wrapped at 64 base64 columns.
fn pem(label: &str, der: &[u8]) -> String {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let mut armored = format!("-----BEGIN {label}-----\n");
    let mut rest = encoded.as_str();
    while !rest.is_empty() {
        // Base64's alphabet is pure ASCII, so every byte offset here is a char boundary.
        let (line, remainder) = rest.split_at(rest.len().min(64));
        armored.push_str(line);
        armored.push('\n');
        rest = remainder;
    }
    armored.push_str("-----END ");
    armored.push_str(label);
    armored.push_str("-----\n");
    armored
}

/// The one conversion: `sutura_tls::LoadedAnchors`'s `CertificateDer` bytes, each turned into an
/// owned `ureq::tls::Certificate` and collected into `RootCerts::Specific`.
///
/// `Certificate::from_der` borrows; `.to_owned()` is what makes the result `'static` and lets it
/// outlive the borrow of each `CertificateDer` this function consumes - `ureq`'s own
/// `RootCerts::Specific(Arc<Vec<Certificate<'static>>>)` needs owned certificates, not borrowed ones.
fn root_certs(anchors: LoadedAnchors) -> ureq::tls::RootCerts {
    let certificates: Vec<ureq::tls::Certificate<'static>> = anchors
        .into_iter()
        .map(|der| ureq::tls::Certificate::from_der(der.as_ref()).to_owned())
        .collect();
    ureq::tls::RootCerts::new_with_certs(&certificates)
}

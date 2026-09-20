//! Folding a deployment's `security.outbound.transport_anchors` (`github.com/telekom/sutura#125`)
//! into the `DataHub` reader's own `ureq` agent - the one place [`super::http`]'s
//! [`super::http::HttpAspectReader`] does it, so the conversion is written once.
//!
//! **Split out of `http.rs` for the file-length gate the way `sutura-exec-bigquery`'s `wire/tls.rs`
//! is split out of `wire.rs`**: the fold is a separable, ureq-specific decision and both files stay
//! under `cargo xtask max-lines`'s 1000-line cap.
//!
//! **What this module is not.** It reads no file and never fails: `sutura_tls::load_anchors` already
//! did the fallible half (a composition root calls it, once, at boot), and this module's only job is
//! folding an already-loaded [`sutura_tls::LoadedAnchors`] into the shape `ureq::tls::TlsConfig`
//! wants - `RootCerts::Specific`, over `Certificate::from_der` - or leaving `ureq`'s own default,
//! `RootCerts::WebPki`, untouched when nothing was declared. That default is what an absent
//! `security.outbound` resolves to, byte-identical to every deployment before `#125`.
//!
//! **A client identity is optional and deployment-wide too** -
//! `security.outbound.client_certificate`/`client_key`, `github.com/telekom/sutura#911`. `DataHub`
//! still takes a bearer token, not mTLS, so this presents nothing to the endpoint itself; what it
//! is for is a peer in front of it a deployment configures to demand one. [`super::http::
//! HttpAspectReader::rotating_agent`] resolves the declaration and [`config`] folds it into
//! `ureq::tls::TlsConfigBuilder::client_cert`, over [`client_cert`].
//!
//! **Why the conversion lives in this crate and not in `sutura-tls`.** `sutura-tls` depends on
//! `rustls-pki-types` only, never on a network client - `sutura_exec_postgres::tls` folds the same
//! loaded certificates into a `rustls::RootCertStore` instead, and `sutura_exec_bigquery::wire::tls`
//! makes the same ureq fold this module does. Sharing stops at "these are the bytes the declaration
//! named"; which library reads them is each adapter's own decision, stated in `sutura-tls`'s own
//! module header.
//!
//! **Why the fold goes through PEM rather than `ureq::tls::PrivateKey::from_der`.** That
//! constructor's own first argument is `ureq::tls::KeyKind` - and `ureq-3.4.2` never re-exports
//! that type from `ureq::tls` (measured against its own `pub use cert::{...}` list, not assumed),
//! so no crate outside `ureq` can name a value of it at all. [`pem`] re-armors the already-loaded
//! DER this crate holds and hands it to `Certificate::from_pem`/`PrivateKey::from_pem` instead -
//! the one pair of constructors `ureq` actually exposes, which recover the kind from the PEM label
//! themselves. [`sutura_tls::LoadedIdentity::key_kind`] is what chooses that label.
//!
//! > **The fold is nearly verbatim the same as `sutura_exec_bigquery::wire::tls::{root_certs,
//! > client_cert, pem}`.** `jscpd` does not flag it - both folds sit below its 30-line / 250-token
//! > floor (`xtask/src/jscpd.rs`), not above an unflagged threshold. The only honest dedupe would
//! > be a shared leaf that hosts the ureq-specific mapping itself, which cannot live in
//! > `sutura-tls` without the `ureq -> rustls -> ring` edge `check-boundaries` forbids - declined
//! > here, the same argument that already declined it for the anchors half.

use sutura_tls::LoadedAnchors;

/// The `ureq` TLS configuration [`super::http::HttpAspectReader::new`] builds its agent with.
///
/// `None` leaves `ureq`'s own default (`RootCerts::WebPki`) in place - the compiled-in root set
/// every deployment verified against before `#125`. `Some` replaces it with `RootCerts::Specific`,
/// built from exactly the certificates the composition root loaded - never a union of the two, and
/// never a second read of the bundle. `identity` is folded the same way, over [`client_cert`].
pub(super) fn config(anchors: Option<LoadedAnchors>, identity: Option<sutura_tls::LoadedIdentity>) -> ureq::tls::TlsConfig {
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
/// `ureq`'s own PEM parser, collected into a [`ureq::tls::ClientCert`] - verbatim
/// `sutura_exec_bigquery::wire::tls::client_cert`, see this module's own header for why PEM and
/// why the duplication is declined rather than shared.
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
        // to `ureq`'s own PEM parser cannot fail on a value that already parsed.
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

/// Armors raw DER as PEM under `label`, wrapped at 64 base64 columns - verbatim
/// `sutura_exec_bigquery::wire::tls::pem`.
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

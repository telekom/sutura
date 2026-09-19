//! Turning `sutura_tls`'s loaded certificates into `ureq`'s own TLS configuration - the one place
//! either `WireAgent` constructor does this, so the conversion is written once.
//!
//! **What this module is not.** It reads no file and never fails: `sutura_tls::load_anchors` already
//! did the fallible half (a composition root calls it, once, at boot), and this module's only job is
//! folding an already-loaded [`sutura_tls::LoadedAnchors`] into the shape `ureq::tls::TlsConfig`
//! wants - `RootCerts::Specific`, over `Certificate::from_der` - or leaving `ureq`'s own default,
//! `RootCerts::WebPki`, untouched when nothing was declared. That default is `WireAgent::pinned`'s
//! entire behaviour before `github.com/telekom/sutura#125` and stays what an absent
//! `security.outbound` resolves to.
//!
//! **A client identity is optional, deployment-wide, and travels through the SAME rebuild path as
//! the anchors** - `github.com/telekom/sutura#911`. `security.outbound.client_certificate`/
//! `client_key` is what this crate's own `WireAgent::rotating_agent` resolves into a
//! `sutura_tls::LoadedIdentity`, and [`client_cert`] is the one place it is folded into `ureq`'s own
//! `ClientCert` - so `ureq::tls::TlsConfigBuilder::client_cert` is called exactly here. Absent means
//! no certificate is presented, which is every deployment's behaviour before this change.
//!
//! **Why the conversion lives in this crate and not in `sutura-tls`.** `sutura-tls` depends on
//! `rustls-pki-types` only, never on a network client - `sutura_exec_postgres::tls` folds the same
//! loaded certificates into a `rustls::RootCertStore` instead, for `tokio-postgres-rustls`. Sharing
//! stops at "these are the bytes the declaration named"; which library reads them is each adapter's
//! own decision, stated in `sutura-tls`'s own module header.
//!
//! **Why the fold goes through PEM rather than `ureq::tls::PrivateKey::from_der`.** That
//! constructor's own first argument is `ureq::tls::KeyKind` - and `ureq-3.4.2` never re-exports
//! that type from `ureq::tls` (measured against its own `pub use cert::{...}` list, not assumed),
//! so no crate outside `ureq` can name a value of it at all. [`pem`] re-armors the already-loaded
//! DER this crate holds and hands it to `Certificate::from_pem`/`PrivateKey::from_pem` instead -
//! the one pair of constructors `ureq` actually exposes, which recover the kind from the PEM label
//! themselves. [`sutura_tls::LoadedIdentity::key_kind`] is what chooses that label.

use sutura_tls::LoadedAnchors;

/// The `ureq` TLS configuration [`super::WireAgent::secured`] builds its agent with.
///
/// `None` leaves `ureq`'s own default (`RootCerts::WebPki`) in place - the compiled-in root set every
/// deployment verified against before `#125`, and what `WireAgent::pinned` still resolves to via
/// `secured(bounds, None)`. `Some` replaces it with `RootCerts::Specific`, built from exactly the
/// certificates the composition root loaded - never a union of the two, and never a second read.
/// `identity` is folded in the same way, over [`client_cert`]: `None` presents nothing (`ureq`'s own
/// default), `Some` presents exactly the declared pair.
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
/// `ureq`'s own PEM parser, collected into a [`ureq::tls::ClientCert`] - see this module's own
/// header for why PEM and not `from_der`.
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

/// Armors raw DER as PEM under `label`, wrapped at 64 base64 columns - the width every other PEM
/// writer uses, though `ureq`'s own parser accepts any.
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
/// outlive the borrow of each `CertificateDer` this function consumes - `ureq-3.4.0`'s own
/// `RootCerts::Specific(Arc<Vec<Certificate<'static>>>)` needs owned certificates, not borrowed ones.
fn root_certs(loaded: LoadedAnchors) -> ureq::tls::RootCerts {
    let certificates: Vec<ureq::tls::Certificate<'static>> = loaded
        .into_iter()
        .map(|der| ureq::tls::Certificate::from_der(der.as_ref()).to_owned())
        .collect();
    ureq::tls::RootCerts::new_with_certs(&certificates)
}

/// Builds a fresh `ureq::Agent` with this crate's pins over the given TLS configuration - the one
/// place the six pins are written, so every constructor (fixed and rotating) uses the same client.
pub(super) fn agent_from_tls(timeout: std::time::Duration, tls: ureq::tls::TlsConfig) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .max_redirects(0)
            .timeout_global(Some(timeout))
            .max_response_header_size(super::MAX_HEADER_BYTES)
            .proxy(ureq::Proxy::try_from_env())
            .tls_config(tls)
            .build(),
    )
}

/// A wire's rotating agent handle and (when a declaration exists) the poll handle that keeps it
/// current - named for `type_complexity`, the same reason `wire.rs`'s `Wired`/`Grid` aliases are.
pub(super) type OutboundAgent = (sutura_tls::Rotating<ureq::Agent>, Option<sutura_tls::Rotator<ureq::Agent>>);

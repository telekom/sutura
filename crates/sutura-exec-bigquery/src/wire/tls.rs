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
//! **No client identity travels through here.** `security.outbound` is anchors only - Google's
//! endpoints take a bearer token, not mTLS - so `ureq::tls::TlsConfigBuilder::client_cert` is never
//! called, and there is no `sutura_tls::LoadedIdentity` parameter to accept one from.
//!
//! **Why the conversion lives in this crate and not in `sutura-tls`.** `sutura-tls` depends on
//! `rustls-pki-types` only, never on a network client - `sutura_exec_postgres::tls` folds the same
//! loaded certificates into a `rustls::RootCertStore` instead, for `tokio-postgres-rustls`. Sharing
//! stops at "these are the bytes the declaration named"; which library reads them is each adapter's
//! own decision, stated in `sutura-tls`'s own module header.

use sutura_tls::LoadedAnchors;

/// The `ureq` TLS configuration [`super::WireAgent::secured`] builds its agent with.
///
/// `None` leaves `ureq`'s own default (`RootCerts::WebPki`) in place - the compiled-in root set every
/// deployment verified against before `#125`, and what `WireAgent::pinned` still resolves to via
/// `secured(bounds, None)`. `Some` replaces it with `RootCerts::Specific`, built from exactly the
/// certificates the composition root loaded - never a union of the two, and never a second read.
pub(super) fn config(anchors: Option<LoadedAnchors>) -> ureq::tls::TlsConfig {
    let builder = ureq::tls::TlsConfig::builder();
    match anchors {
        None => builder.build(),
        Some(loaded) => builder.root_certs(root_certs(loaded)).build(),
    }
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

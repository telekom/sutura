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
//! **No client identity travels through here.** `security.outbound` is anchors only - `DataHub` takes
//! a bearer token, not mTLS - so `ureq::tls::TlsConfigBuilder::client_certificate` is never called,
//! and there is no `sutura_tls::LoadedIdentity` parameter to accept one from.
//!
//! **Why the conversion lives in this crate and not in `sutura-tls`.** `sutura-tls` depends on
//! `rustls-pki-types` only, never on a network client - `sutura_exec_postgres::tls` folds the same
//! loaded certificates into a `rustls::RootCertStore` instead, and `sutura_exec_bigquery::wire::tls`
//! makes the same ureq fold this module does. Sharing stops at "these are the bytes the declaration
//! named"; which library reads them is each adapter's own decision, stated in `sutura-tls`'s own
//! module header.
//!
//! > **The fold is nearly verbatim the same as `sutura_exec_bigquery::wire::tls::root_certs`.**
//! > `just ship-check`'s jscpd runs minutes in a build lane. If it flags this pair, the honest fix
//! > is a tiny `sutura_tls` `pub fn` returning the DER list (`Vec<CertificateDer>`), which both
//! > ureq adapters then feed to their own `RootCerts` - still no `ureq` edge into `sutura-tls`. Not
//! > pre-built here because adding it changes an already-accepted adapter `wire/tls.rs` for lines a
//! > gate has not yet measured; the build lane that sees the violation makes that call.

use sutura_tls::LoadedAnchors;

/// The `ureq` TLS configuration [`super::http::HttpAspectReader::new`] builds its agent with.
///
/// `None` leaves `ureq`'s own default (`RootCerts::WebPki`) in place - the compiled-in root set
/// every deployment verified against before `#125`. `Some` replaces it with `RootCerts::Specific`,
/// built from exactly the certificates the composition root loaded - never a union of the two, and
/// never a second read of the bundle.
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
/// outlive the borrow of each `CertificateDer` this function consumes - `ureq`'s own
/// `RootCerts::Specific(Arc<Vec<Certificate<'static>>>)` needs owned certificates, not borrowed ones.
fn root_certs(anchors: LoadedAnchors) -> ureq::tls::RootCerts {
    let certificates: Vec<ureq::tls::Certificate<'static>> = anchors
        .into_iter()
        .map(|der| ureq::tls::Certificate::from_der(der.as_ref()).to_owned())
        .collect();
    ureq::tls::RootCerts::new_with_certs(&certificates)
}

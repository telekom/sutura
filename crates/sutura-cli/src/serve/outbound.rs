//! `security.outbound`, resolved ONCE for this composition root - `github.com/telekom/sutura#125`/
//! `#911`.
//!
//! Split out of `serve.rs` for the file-length gate the way `broker.rs`/`catalog.rs`/`bigquery.rs`
//! already are: `serve.rs` sat at (and then over) `cargo xtask max-lines`'s 1000-line cap, and this
//! one boot-time resolution is a separable concept.
//!
//! **The load itself is `crate::sources::resolve_outbound_anchors`, not a second copy of it.**
//! Issue #970's review measured this module's own body byte-for-byte identical to that one (both
//! composition roots - `sutura serve` and `sutura query`'s command path - need the same
//! `security.outbound.transport_anchors`/`client_certificate` read), so this wrapper adds only the
//! boot-time log line every SERVING composition root wants and `sutura query` does not.

use sutura_config::Settings;

/// **`None` is not a gap.** A deployment with no `security.outbound` block is every deployment before
/// `#125`: a catalog HTTP reader verifies against `ureq`'s compiled-in roots and presents no client
/// certificate - see `sutura_config::security::OutboundAnchors`'s own doc. `Some` is loaded by
/// `crate::sources::resolve_outbound_anchors`, through `sutura_tls::load_anchors`/`load_identity`,
/// before the listener opens - the same argument `super::inbound_gate` makes for leg 1's key set: an
/// unreadable or empty declaration has to stop the process, not become a deployment that dials
/// `bigquery.googleapis.com` under a trust set (or presents a client identity) nobody can name.
///
/// Read and logged unconditionally, on every build - a `files`-only or `postgres`-only binary has no
/// reader for the loaded value (see `super::bigquery::open_bigquery`'s refusal for the parallel case:
/// a declared `kind: bigquery` with no linked adapter), which is a stated limit rather than a second
/// refusal this settings crate cannot see the feature set to make.
///
/// # Errors
///
/// The declared bundle or client identity cannot be loaded at boot - `resolve_outbound_anchors`'s
/// own errors.
pub(crate) fn resolve(settings: &Settings) -> Result<Option<sutura_tls::Declared>, String> {
    let declared = crate::sources::resolve_outbound_anchors(settings)?;
    // Read straight off `settings` rather than off `declared`, which by design carries only what a
    // rotating handle re-loads (the declaration), not the already-loaded value this log reports on.
    if let Some(outbound) = settings.security().outbound() {
        tracing::info!(
            anchors = match outbound {
                sutura_config::OutboundAnchors::System => "system",
                sutura_config::OutboundAnchors::Bundle(_) => "bundle",
            },
            identity = settings.security().outbound_identity().is_some(),
            "security.outbound: declared trust material was read for this deployment's catalog and source readers"
        );
    }
    Ok(declared)
}

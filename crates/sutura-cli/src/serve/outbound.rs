//! `security.outbound`, resolved ONCE for this composition root - `github.com/telekom/sutura#125`/
//! `#911`.
//!
//! Split out of `serve.rs` for the file-length gate the way `broker.rs`/`catalog.rs`/`bigquery.rs`
//! already are: `serve.rs` sat at (and then over) `cargo xtask max-lines`'s 1000-line cap, and this
//! one boot-time resolution is a separable concept.

use sutura_config::Settings;

/// **`None` is not a gap.** A deployment with no `security.outbound` block is every deployment before
/// this change: the `BigQuery` wire and the STS exchange verify against `ureq`'s own compiled-in
/// roots and present no client certificate, exactly as they always have - see
/// `sutura_config::security::OutboundAnchors`'s own doc. `Some` is loaded here, through
/// `sutura_tls::load_anchors`/`load_identity`, before the listener opens - the same argument
/// `super::inbound_gate` makes for leg 1's key set: an unreadable or empty declaration has to stop the
/// process, not become a deployment that dials `bigquery.googleapis.com` under a trust set (or
/// presents a client identity) nobody can name.
///
/// Read and logged unconditionally, on every build - a `files`-only or `postgres`-only binary has no
/// reader for the loaded value (see `super::bigquery::open_bigquery`'s refusal for the parallel case:
/// a declared `kind: bigquery` with no linked adapter), which is a stated limit rather than a second
/// refusal this settings crate cannot see the feature set to make.
pub(super) fn resolve(settings: &Settings) -> Result<Option<sutura_tls::Declared>, String> {
    let Some(declared) = settings.security().outbound() else {
        return Ok(None);
    };
    let anchors = match declared {
        sutura_config::OutboundAnchors::System => sutura_tls::Anchors::System,
        sutura_config::OutboundAnchors::Bundle(path) => sutura_tls::Anchors::Bundle(path.clone()),
    };
    // Fail-fast: an unreadable or empty declaration stops the process. The DECLARATION (not the
    // loaded value) is what flows onward - the rotating handles each consumer builds re-read it on
    // `sutura_tls::POLL_INTERVAL`, so what is handed to `open_catalog`/`open_engine`/`build_broker`
    // is the thing they can re-load, which the pre-rotation loaded bytes were not.
    sutura_tls::load_anchors(&anchors)
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    let identity = match settings.security().outbound_identity() {
        None => None,
        Some(identity) => {
            let identity = sutura_tls::Identity::new(identity.certificate().clone(), identity.key().clone());
            sutura_tls::load_identity(&identity)
                .map_err(|cause| format!("`security.outbound.client_certificate`/`client_key` could not be loaded: {cause}"))?;
            Some(identity)
        }
    };
    tracing::info!(
        anchors = match declared {
            sutura_config::OutboundAnchors::System => "system",
            sutura_config::OutboundAnchors::Bundle(_) => "bundle",
        },
        identity = identity.is_some(),
        "security.outbound: declared trust material was read for the BigQuery wire, the STS exchange and the datahub reader"
    );
    Ok(Some(sutura_tls::Declared::new(anchors, identity)))
}

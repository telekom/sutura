//! The ONE Cube composition, shared by both of this crate's composition roots.
//!
//! `crate::oracle`'s shape, for its reason: `build` is the only place a declared `cube` entry becomes
//! an open `sutura_domain::warehouse::Warehouse`, so the posture cross-check, the token read and the
//! refusal WORDING cannot differ between `sutura serve` and `sutura query`/`mcp`. What stays per root
//! is the feature-off refusal, because the two are different sentences.
//!
//! The WHOLE module is behind `#[cfg(feature = "cube")]` at its declaration in `main.rs`, so
//! everything here may name an adapter type unconditionally.

use sutura_catalog_cube::http::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, HttpMetaReader};
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_cube::CubeWarehouse;
use sutura_exec_cube::http::{Endpoint, HttpCube, ReadBounds};

use crate::commands::render;

/// A Cube source as this binary composes it: the adapter over its HTTP transport. Named once because
/// it appears in a registry type, a `Warehouse` bound and a constructor's return.
pub(crate) type CubeSource = CubeWarehouse<HttpCube>;

/// Builds one Cube adapter from a declared entry, after checking this build can deliver the source's
/// posture.
///
/// **Everything that can fail before a question is asked is done here**: the posture cross-check, the
/// token file and the endpoint. Nothing is dialled - the first request is the first question.
///
/// # Errors
///
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; a
/// posture this adapter cannot deliver; a token file that cannot be read or is empty; an endpoint the
/// client refuses; or a declared `security.outbound` bundle that cannot be loaded.
pub(crate) fn build(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<CubeSource, String> {
    let sutura_config::SourcePlacement::Cube {
        ref endpoint,
        ref token_file,
        ..
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the Cube connect step with a placement no Cube adapter \
             reads, which the dispatcher should have sent elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    identity
        .posture()
        .deliverable_by(CubeSource::IMPERSONATION, source)
        .map_err(|cause| render(&cause))?;
    let token = crate::password_file::read_key(&format!("sources.{source}.token_file"), token_file)?;
    let endpoint =
        Endpoint::parse(endpoint).map_err(|cause| format!("`sources.{source}.endpoint` is not a usable endpoint: {cause}"))?;
    let bounds = ReadBounds::parse(DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES)
        .map_err(|cause| format!("the Cube read bounds are not usable: {cause}"))?;
    let (agent, rotator) = HttpMetaReader::rotating_agent(bounds, outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (cube source)", rotator);
    Ok(CubeWarehouse::of(
        source.clone(),
        identity.posture().clone(),
        token,
        HttpCube::rotating(endpoint, bounds, agent),
    ))
}

/// The hops a `cube` source's `delegation` declares, in order - empty for a shared source and for
/// any other placement. The broker exchanges each caller's token through them for the request's
/// bearer.
pub(crate) fn declared_delegation(
    configured: &sutura_config::ConfiguredSource,
) -> &[sutura_config::sources::workload_identity::DelegationDeclared] {
    let sutura_config::SourcePlacement::Cube { ref delegation, .. } = *configured.placement() else {
        return &[];
    };
    delegation
}

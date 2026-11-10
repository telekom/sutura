//! The ONE Foundry source composition, shared by `sutura serve` and `sutura query`/`mcp`.
//!
//! The catalog adapter reads metadata; this module opens the data connection. The two credentials
//! are intentionally separate settings keys, so a catalog refresh cannot replace the query token.

use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_foundry::http::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpFoundry, ReadBounds};
use sutura_exec_foundry::{FoundryWarehouse, OntologyIdentifier};

use crate::commands::render;

/// A Foundry source as this binary composes it: the Ontology SQL adapter over HTTP.
pub(crate) type FoundrySource = FoundryWarehouse<HttpFoundry>;

/// Builds one Foundry SQL connection from a declared source entry.
pub(crate) fn build(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    outbound: Option<&sutura_tls::Declared>,
    working_set: sutura_exec_datafusion::WorkingSet,
) -> Result<FoundrySource, String> {
    let sutura_config::SourcePlacement::Foundry {
        ref endpoint,
        ref ontology,
        ref token_file,
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the Foundry connect step with a placement no Foundry adapter reads, which the dispatcher should have sent elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    identity
        .posture()
        .deliverable_by(FoundrySource::IMPERSONATION, source)
        .map_err(|cause| render(&cause))?;
    let token = crate::password_file::read_key(&format!("sources.{source}.token_file"), token_file)?;
    let endpoint =
        Endpoint::parse(endpoint).map_err(|cause| format!("`sources.{source}.endpoint` is not a usable endpoint: {cause}"))?;
    let ontology =
        OntologyIdentifier::parse(ontology).map_err(|cause| format!("`sources.{source}.ontology` is not usable: {cause}"))?;
    let bounds = ReadBounds::parse(DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES)
        .map_err(|cause| format!("the Foundry SQL read bounds are not usable: {cause}"))?;
    let (agent, rotator) = sutura_http_client::rotating_agent(bounds, outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (foundry source)", rotator);
    Ok(FoundryWarehouse::new(
        source.clone(),
        identity.posture().clone(),
        ontology,
        token,
        HttpFoundry::rotating(endpoint, bounds, agent),
        working_set.result_budget(),
    ))
}

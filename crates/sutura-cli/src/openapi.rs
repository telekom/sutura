//! The EXPERIMENTAL `openapi` catalog composition: a declared entry into an opened catalog.
//!
//! The WHOLE module is behind `#[cfg(feature = "openapi")]` at its declaration in `main.rs`; the
//! feature-off refusal is `crate::catalog`'s.

use sutura_catalog_openapi::http::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpAnswerReader, ReadBounds};
use sutura_catalog_openapi::{Contract, OpenApiCatalog};

/// An `openapi` catalog as this binary composes it.
pub(crate) type OpenApiSource = OpenApiCatalog<HttpAnswerReader>;

/// Opens one declared `openapi` catalog.
///
/// **Everything that can fail before a request is done here**: the document and the mapping are
/// read out of `dir` and checked against each other, the token file is read, and the endpoint and
/// bounds are parsed. Nothing is dialled. Every model is on the source the catalog's own name
/// aliases, as `cube`'s are.
pub(crate) fn open(
    settings: &sutura_config::CatalogSettings,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenApiSource, String> {
    let name = settings.name();
    let dir = settings
        .dir()
        .ok_or_else(|| format!("`catalogs.{name}.dir` is required for catalog.kind: openapi"))?;
    let contract = Contract::read(dir).map_err(|cause| {
        format!(
            "`catalogs.{name}.dir` does not hold a document and a mapping that agree: {}",
            crate::commands::render(&cause)
        )
    })?;
    let endpoint_raw = settings
        .endpoint()
        .ok_or_else(|| format!("`catalogs.{name}.endpoint` is required for catalog.kind: openapi"))?;
    let endpoint =
        Endpoint::parse(endpoint_raw).map_err(|cause| format!("`catalogs.{name}.endpoint` is not a usable endpoint: {cause}"))?;
    let token_file = settings
        .token_file()
        .ok_or_else(|| format!("`catalogs.{name}.token_file` is required for catalog.kind: openapi"))?;
    let token = crate::password_file::read_key(&format!("catalogs.{name}.token_file"), token_file)?;
    let bounds = ReadBounds::parse(
        settings.deadline_seconds().unwrap_or(DEFAULT_TIMEOUT_SECONDS),
        settings.max_response_bytes().unwrap_or(DEFAULT_MAX_RESPONSE_BYTES),
    )
    .map_err(|cause| format!("`catalogs.{name}.deadline_seconds`/`max_response_bytes` are not usable read bounds: {cause}"))?;
    let (agent, rotator) = HttpAnswerReader::rotating_agent(bounds, outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (openapi reader)", rotator);
    Ok(OpenApiCatalog::new(
        name.clone(),
        settings.version().clone(),
        name.clone(),
        contract,
        HttpAnswerReader::rotating(endpoint, token, bounds, agent),
    ))
}

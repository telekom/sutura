//! Opening `catalog.kind: foundry` entries - EXPERIMENTAL, behind this crate's default-off
//! `foundry` feature, which no release turns on.
//!
//! The ontology, the client identifier and the scopes are parsed here by the adapter's own types,
//! the single-owner split every HTTP catalog's endpoint already takes. The token endpoint is the
//! entry's own `endpoint` plus the declared path, through the one grant builder every Foundry entry
//! uses (`crate::foundry::client_credentials`).

use super::OpenedCatalogs;

/// Opens every declared `foundry` catalog.
#[cfg(feature = "foundry")]
pub(super) fn open_foundry_catalogs(
    catalogs: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    catalogs
        .each()
        .map(|settings| open_one(settings, outbound))
        .collect::<Result<Vec<_>, String>>()
        .map(OpenedCatalogs::Foundry)
}

/// The refusal for a build that did not link the Foundry adapter - `open_cube_catalogs`'s shape.
#[cfg(not(feature = "foundry"))]
pub(super) fn open_foundry_catalogs(
    _catalogs: &sutura_config::Catalogs,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    Err(String::from(
        "catalog.kind: foundry names a metadata adapter this binary was not built to link - \
         build sutura-cli with --features foundry, or declare markdown catalogs",
    ))
}

/// Opens one declared `foundry` catalog. Every object type is a model on the source the catalog's
/// own name aliases.
#[cfg(feature = "foundry")]
fn open_one(
    settings: &sutura_config::CatalogSettings,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_catalog_foundry::FoundryCatalog<sutura_catalog_foundry::http::HttpOntologyReader>, String> {
    let name = settings.name();
    Ok(sutura_catalog_foundry::FoundryCatalog::new(
        name.clone(),
        settings.version().clone(),
        name.clone(),
        reader(settings, outbound)?,
    ))
}

/// The reader one declared `foundry` catalog reads through, its credential read at boot - the one
/// composition `serve` and `foundry-check` both open.
#[cfg(feature = "foundry")]
pub(crate) fn reader(
    settings: &sutura_config::CatalogSettings,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_catalog_foundry::http::HttpOntologyReader, String> {
    use sutura_catalog_foundry::http::{
        DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpOntologyReader, OntologyIdentifier, ReadBounds,
        TokenSource,
    };

    let name = settings.name();
    let foundry = settings.foundry().ok_or_else(|| {
        format!("`catalogs.{name}` is `kind: foundry` yet carries no foundry settings, which CatalogSettings::parse should have refused")
    })?;
    let endpoint = settings
        .endpoint()
        .ok_or_else(|| format!("`catalogs.{name}.endpoint` is required for catalog.kind: foundry"))
        .and_then(|raw| {
            Endpoint::parse(raw).map_err(|cause| format!("`catalogs.{name}.endpoint` is not a usable endpoint: {cause}"))
        })?;
    let ontology = OntologyIdentifier::parse(foundry.ontology())
        .map_err(|cause| format!("`catalogs.{name}.foundry.ontology` is not usable: {cause}"))?;
    let bounds = ReadBounds::parse(
        settings.deadline_seconds().unwrap_or(DEFAULT_TIMEOUT_SECONDS),
        settings.max_response_bytes().unwrap_or(DEFAULT_MAX_RESPONSE_BYTES),
    )
    .map_err(|cause| format!("`catalogs.{name}.deadline_seconds`/`max_response_bytes` are not usable read bounds: {cause}"))?;
    let (agent, rotator) = HttpOntologyReader::rotating_agent(bounds, outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (foundry reader)", rotator);
    let token = match foundry.auth() {
        sutura_config::FoundryAuth::Token { token_file } => {
            let token = crate::password_file::read_key(&format!("catalogs.{name}.token_file"), token_file)?;
            TokenSource::fixed(token)
                .map_err(|cause| format!("`catalogs.{name}.token_file` does not hold a usable token: {cause}"))?
        }
        sutura_config::FoundryAuth::ClientCredentials(grant) => {
            TokenSource::ClientCredentials(crate::foundry::client_credentials(
                &format!("catalogs.{name}.foundry.client_credentials"),
                &endpoint,
                grant,
                agent.clone(),
            )?)
        }
    };
    Ok(HttpOntologyReader::new(endpoint, ontology, token, bounds, agent))
}

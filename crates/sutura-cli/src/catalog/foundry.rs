//! Opening `catalog.kind: foundry` entries - EXPERIMENTAL, behind this crate's default-off
//! `foundry` feature, which no release turns on.
//!
//! The ontology, the client identifier and the scopes are parsed here by the adapter's own types,
//! the single-owner split every HTTP catalog's endpoint already takes. The token endpoint is the
//! entry's own `endpoint` plus the declared path, so it is on that host by construction, and
//! `ClientCredentials::new` refuses it otherwise.

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
            TokenSource::ClientCredentials(client_credentials(name, &endpoint, grant, agent.clone())?)
        }
    };
    Ok(sutura_catalog_foundry::FoundryCatalog::new(
        name.clone(),
        settings.version().clone(),
        name.clone(),
        HttpOntologyReader::new(endpoint, ontology, token, bounds, agent),
    ))
}

/// The declared client-credentials grant, its secret read at boot.
#[cfg(feature = "foundry")]
fn client_credentials(
    name: &sutura_domain::model::SourceName,
    endpoint: &sutura_catalog_foundry::http::Endpoint,
    grant: &sutura_config::ClientCredentialsSettings,
    agent: sutura_catalog_foundry::http::RotatingAgent,
) -> Result<sutura_http_client::token::ClientCredentials, String> {
    use sutura_http_client::delegation::{ExchangeClient, TokenEndpoint};
    use sutura_http_client::token::{ClientCredentials, Scopes};

    let key = format!("catalogs.{name}.foundry.client_credentials");
    let secret = crate::password_file::read_key(&format!("{key}.client_secret_file"), grant.client_secret_file())?;
    let client = ExchangeClient::new(grant.client_id(), secret).map_err(|cause| format!("`{key}.client_id`: {cause}"))?;
    let scopes = Scopes::parse(grant.scopes().iter().map(String::as_str)).map_err(|cause| format!("`{key}.scopes`: {cause}"))?;
    let token_endpoint = TokenEndpoint::parse(&format!("{}{}", endpoint.as_str(), grant.token_path()))
        .map_err(|cause| format!("`{key}.token_path` is not a usable token endpoint: {cause}"))?;
    ClientCredentials::new(endpoint, token_endpoint, client, scopes, agent)
        .map_err(|cause| format!("`{key}.token_path`: {cause}"))
}

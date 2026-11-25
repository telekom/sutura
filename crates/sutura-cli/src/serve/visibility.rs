//! The sources that narrow each caller's catalog view, composed for the service.
//!
//! `sources.<alias>.catalog_visibility`, into [`sutura_app::visibility::CatalogVisibility`].
//!
//! A `foundry_login` source lists object types under the caller's own brokered token, which the
//! broker `super::broker::build_broker` already composes for its questions: this adds the listing
//! port and no second broker. `docs/adr/20261125083027-catalog-visibility-per-caller.md`.

use sutura_app::visibility::CatalogVisibility;
use sutura_config::Settings;

/// The declared narrowing, or `None` when no source declares `catalog_visibility`.
///
/// # Errors
///
/// A declaring source whose endpoint, ontology or outbound trust cannot be used.
#[cfg(feature = "foundry")]
pub(super) fn declared(settings: &Settings) -> Result<Option<CatalogVisibility>, String> {
    use sutura_catalog_foundry::caller::CallerObjectTypes;
    use sutura_catalog_foundry::http::{
        DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, OntologyIdentifier, ReadBounds,
    };

    let declaring: Vec<_> = settings
        .sources()
        .each()
        .filter(|&(_, source)| sutura_config::sources::catalog_visibility::CatalogVisibility::of(source).is_some())
        .collect();
    if declaring.is_empty() {
        return Ok(None);
    }
    let outbound = crate::sources::resolve_outbound_anchors(settings)?;
    let bounds = ReadBounds::parse(DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES)
        .map_err(|cause| format!("the Foundry listing read bounds are not usable: {cause}"))?;
    let mut visibility = CatalogVisibility::new();
    for (alias, source) in declaring {
        let sutura_config::SourcePlacement::Foundry {
            ref endpoint,
            ref ontology,
            ..
        } = *source.placement()
        else {
            // Refused at load unless the source declares `foundry_login`; answered rather than unreachable.
            return Err(format!(
                "`sources.{alias}.catalog_visibility` reached the composition on a source that is not `kind: foundry`"
            ));
        };
        let endpoint =
            Endpoint::parse(endpoint).map_err(|cause| format!("`sources.{alias}.endpoint` is not a usable endpoint: {cause}"))?;
        let ontology =
            OntologyIdentifier::parse(ontology).map_err(|cause| format!("`sources.{alias}.ontology` is not usable: {cause}"))?;
        let (agent, rotator) = sutura_http_client::rotating_agent(bounds, outbound.clone())
            .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
        crate::rotation::drive_rotation("security.outbound.transport_anchors (foundry catalog visibility)", rotator);
        visibility = visibility.listing(alias.clone(), CallerObjectTypes::new(endpoint, ontology, bounds, agent));
    }
    Ok(Some(visibility))
}

/// A build without the Foundry adapters opens no source that could declare it.
///
/// # Errors
///
/// Any source declaring `catalog_visibility`: nothing in this build can list for it.
#[cfg(not(feature = "foundry"))]
pub(super) fn declared(settings: &Settings) -> Result<Option<CatalogVisibility>, String> {
    match settings
        .sources()
        .each()
        .find(|&(_, source)| sutura_config::sources::catalog_visibility::CatalogVisibility::of(source).is_some())
    {
        Some((alias, _)) => Err(format!(
            "`sources.{alias}.catalog_visibility` lists with a Foundry login, and this binary was built without the `foundry` feature"
        )),
        None => Ok(None),
    }
}

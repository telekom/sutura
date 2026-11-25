//! The ONE Foundry source composition, shared by `sutura serve` and `sutura query`/`mcp`.
//!
//! The catalog adapter reads metadata; this module opens the data connection. The two credentials
//! are intentionally separate settings keys, so a catalog refresh cannot replace the query token.
//! A shared source presents a `token_file` or its `client_credentials` grant; an
//! `impersonation-at-source` source reads no token for a question - each carries the caller's own,
//! which `crate::serve::broker` retrieves through `foundry_login` - and its anchors run as its
//! declared `verification_client_credentials` grant, or not at all.

use sutura_catalog_foundry::caller::{CallerListingError, CallerObjectTypes};
use sutura_catalog_foundry::http::HttpReaderError;
use sutura_domain::identity::Secret;
use sutura_domain::model::SourceName;
use sutura_domain::source::SourcePosture::{ImpersonationAtSource, SharedServiceUser};
use sutura_domain::warehouse::Warehouse as _;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_exec_foundry::http::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpFoundry, ReadBounds};
use sutura_exec_foundry::{DirectoryFailed, FoundryWarehouse, Listed, ObjectTypeDirectory, OntologyIdentifier};

use crate::commands::render;

/// A Foundry source as this binary composes it: the Ontology SQL adapter over HTTP.
pub(crate) type FoundrySource = FoundryWarehouse<HttpFoundry>;

/// The source's object-type list, read by the catalog reader's own request under the token the
/// question is about to send - the RID each API name is queried by.
#[derive(Debug)]
struct ObjectTypeList(CallerObjectTypes);

impl ObjectTypeDirectory for ObjectTypeList {
    fn listed(&self, token: &Secret, deadline: Deadline) -> Result<Listed, DirectoryFailed> {
        self.0
            .resource_ids(token, deadline)
            .map_err(|cause| match cause {
                CallerListingError::Read(HttpReaderError::Refused { status, .. }) => DirectoryFailed::Refused { status },
                CallerListingError::NotACallerToken
                | CallerListingError::Unusable(_)
                | CallerListingError::DeadlineSpent
                | CallerListingError::Read(_) => DirectoryFailed::Unread,
            })
            .map(|list| {
                let (ids, unusable, excluded) = list.into_parts();
                Listed::new(ids, unusable, excluded)
            })
    }
}

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
        client_credentials: ref deployment_grant,
        ref verification_client_credentials,
        ref exclude_object_types,
        ..
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
    let endpoint =
        Endpoint::parse(endpoint).map_err(|cause| format!("`sources.{source}.endpoint` is not a usable endpoint: {cause}"))?;
    let listed_ontology = sutura_catalog_foundry::http::OntologyIdentifier::parse(ontology)
        .map_err(|cause| format!("`sources.{source}.ontology` is not usable: {cause}"))?;
    let ontology =
        OntologyIdentifier::parse(ontology).map_err(|cause| format!("`sources.{source}.ontology` is not usable: {cause}"))?;
    let bounds = ReadBounds::parse(DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES)
        .map_err(|cause| format!("the Foundry SQL read bounds are not usable: {cause}"))?;
    let (agent, rotator) = sutura_http_client::rotating_agent(bounds, outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (foundry source)", rotator);
    let grant = |key: &str, declared| {
        client_credentials(&format!("sources.{source}.{key}"), &endpoint, declared, agent.clone())
            .map(sutura_http_client::token::TokenSource::ClientCredentials)
    };
    let budget = working_set.result_budget();
    let directory = ObjectTypeList(
        CallerObjectTypes::new(endpoint.clone(), listed_ontology, bounds, agent.clone()).excluding(exclude_object_types.clone()),
    );
    // `sutura_config::sources::foundry::declare` holds these shapes at load; any other is answered
    // rather than unreachable.
    match (
        identity.posture(),
        token_file.as_deref(),
        deployment_grant.as_ref(),
        verification_client_credentials.as_ref(),
    ) {
        (ImpersonationAtSource, None, None, boot) => {
            let boot = boot
                .map(|declared| grant("verification_client_credentials", declared))
                .transpose()?;
            let transport = HttpFoundry::rotating(endpoint, bounds, agent);
            let warehouse = FoundryWarehouse::each_caller(source.clone(), ontology, transport, budget)
                .with_directory(directory)
                .with_exclusions(exclude_object_types.clone());
            Ok(match boot {
                Some(boot) => warehouse.with_boot_identity(boot),
                None => warehouse,
            })
        }
        (posture @ SharedServiceUser { .. }, Some(token_file), None, None) => {
            let token = crate::password_file::read_key(&format!("sources.{source}.token_file"), token_file)?;
            let transport = HttpFoundry::rotating(endpoint, bounds, agent);
            Ok(
                FoundryWarehouse::new(source.clone(), posture.clone(), ontology, token, transport, budget)
                    .with_directory(directory)
                    .with_exclusions(exclude_object_types.clone()),
            )
        }
        (posture @ SharedServiceUser { .. }, None, Some(declared), None) => {
            let token = grant("client_credentials", declared)?;
            let transport = HttpFoundry::rotating(endpoint, bounds, agent);
            Ok(
                FoundryWarehouse::new(source.clone(), posture.clone(), ontology, token, transport, budget)
                    .with_directory(directory)
                    .with_exclusions(exclude_object_types.clone()),
            )
        }
        (ImpersonationAtSource | SharedServiceUser { .. }, _, _, _) => Err(format!(
            "`sources.{source}` reached the Foundry connect step with identity keys that do not match its posture"
        )),
    }
}

/// The one client-credentials grant a Foundry entry declares, composed: its secret read once from
/// its file, its token endpoint the entry's own `endpoint` plus the declared path. `key` is the
/// block's settings key, which every refusal names. The catalog reader, a shared source and an
/// impersonating source's startup identity all build theirs here.
pub(crate) fn client_credentials(
    key: &str,
    endpoint: &sutura_http_client::Endpoint,
    grant: &sutura_config::ClientCredentialsSettings,
    agent: sutura_http_client::RotatingAgent,
) -> Result<sutura_http_client::token::ClientCredentials, String> {
    let secret = crate::password_file::read_key(&format!("{key}.client_secret_file"), grant.client_secret_file())?;
    sutura_http_client::token::ClientCredentials::declared(
        endpoint,
        grant.token_path().as_str(),
        grant.client_id(),
        secret,
        grant.scopes().clone(),
        agent,
    )
    .map_err(|cause| format!("`{key}` is not a usable client-credentials grant: {}", render(&cause)))
}

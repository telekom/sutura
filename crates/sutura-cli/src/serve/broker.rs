//! The credential broker a served deployment that links the `BigQuery` adapter is answered
//! through - built here, out of `serve.rs`, so the composition root keeps the dispatch and the one
//! thing that is genuinely its own logic reads next to the refusals that guard it.
//!
//! The whole file is `#[cfg(feature = "bigquery")]` by construction in `serve.rs`'s `mod broker;`,
//! so nothing here is compiled into a build that links no `BigQuery` adapter.
//!
//! **This replaces the exchanging broker's builder rather than restoring it.** That one wired
//! `StsOverHttp` and `IamCredentialsOverHttp`, both deleted with the `wire` transport, so it has no
//! implementor a composition root can reach. `sutura_exec_bigquery::DeclaredPrincipalBroker` is what
//! the ADBC transport can be served through: it presents the asking subject's OWN verified
//! assertion, and the declared pool is what resolves that subject to a principal. What the declared
//! map decides is only WHETHER this caller may be served here. Its own module header states what
//! that costs relative to the exchange.
//!
//! **A source declaring `workload_identity.delegation` presents the token its caller's own inbound
//! token is exchanged for**, through the client `delegation` builds - `direct` mode's second
//! document (`docs/adr/0014`, fourth and fifth amendments). Held in-process by this root's
//! `build_broker` cells; no served-binary cell reaches it, because a `bigquery` deployment needs the
//! ADBC driver to boot.
//!
//! It scans the WHOLE `sources:` registry rather than the `bigquery`-kind entries only, for the
//! reason the deleted builder did: a plan may read a shared source of one kind and an impersonating
//! one of another, and a broker is per answer. A non-`BigQuery` adapter that cannot deliver the
//! impersonating posture is already refused at its own posture cross-check, before any question.

use sutura_domain::source::SourcePosture;
use sutura_exec_bigquery::{DeclaredPrincipalBroker, DeclaredPrincipals};

/// Reads the declared sources into the broker a served question is answered through.
///
/// # Errors
///
/// Three startup refusals, and all three exist because the alternative is a deployment that boots
/// clean and refuses every impersonated question - which is the defect this whole composition was
/// rebuilt to remove:
///
/// - an impersonating source whose `impersonate` map names nobody, so no caller could ever be
///   served there;
/// - a declared target that is not a principal this adapter can name - **narrowed by
///   `telekom/sutura#929` F3 and not relaxed by it.** `PrincipalName::parse` alone accepts `/`,
///   `:` and `?`; since the account is now interpolated into one path segment of the credential
///   document's `service_account_impersonation_url`, `DeclaredPrincipals::parse` refuses anything
///   that is not a service-account address, which is what makes a bad declaration a startup
///   failure rather than a per-question one;
/// - a source declaring the pool expectations `telekom/sutura#817` added, which described a token
///   exchange no transport in this build performs;
/// - a declared `delegation` whose endpoint, client ID, audience or client secret file this
///   adapter cannot send - see [`delegation`].
pub(crate) fn build_broker(
    registry: &sutura_config::SourceRegistry,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<DeclaredPrincipalBroker, String> {
    let mut broker = DeclaredPrincipalBroker::empty();
    for (alias, source) in registry.each() {
        if let Some(SourcePosture::SharedServiceUser { declared }) = source.posture() {
            broker = broker.shared(alias.clone(), declared.clone());
        }
        let Some(workload) = source.workload_identity() else {
            continue;
        };
        // **A declaration nothing in this process checks is refused at boot, not ignored at boot.**
        // The exchange DOES happen now - Google's token service performs it against the declared
        // pool - but it happens THERE, so the pool's own provider configuration is what decides
        // which issuer and which audience are acceptable. These two keys would have this deployment
        // re-state that decision and then not enforce it, which is the exact shape
        // `sutura_config`'s own `VerificationIdentityOnASharedSource` refuses.
        if workload.expected_issuer().is_some() || workload.expected_audience().is_some() {
            return Err(format!(
                "`sources.{alias}.workload_identity` declares the pool expectations \
                 `expected_issuer`/`expected_audience`, and nothing in this process checks them - \
                 the asker's own assertion is federated to the identity pool, which applies its \
                 provider's own issuer and audience conditions. Declare them on the pool's provider \
                 and remove both keys here"
            ));
        }
        // The declared subject -> service-account map, re-expressed once here into the adapter's own
        // parsed shape: an adapter may not depend on the settings tree, so every value on this path
        // is parsed again by the crate that sends it. Both halves are read now - the KEYS decide
        // which callers a source may be asked as, the VALUES the account each of them executes as -
        // so `DeclaredPrincipals::parse` refusing here is what keeps a security-critical
        // declaration from being accepted and then ignored.
        let mut declared = std::collections::BTreeMap::new();
        for (subject, target) in workload.impersonate() {
            let name = sutura_domain::identity::PrincipalName::parse(target.as_str()).map_err(|cause| {
                format!("`sources.{alias}.workload_identity.impersonate` names a target this adapter cannot execute as: {cause}")
            })?;
            drop(declared.insert(subject.clone(), name));
        }
        let declared = DeclaredPrincipals::parse(declared)
            .map_err(|cause| format!("`sources.{alias}.workload_identity.impersonate` is unusable: {cause}"))?;
        broker = match workload.delegation() {
            None => broker.impersonating(alias.clone(), declared),
            Some(declaration) => {
                broker.impersonating_delegated(alias.clone(), declared, delegation(alias, declaration, outbound)?)
            }
        };
    }
    Ok(broker)
}

/// What one source's declared delegation exchange is composed into: the real RFC 8693 client at
/// the caller's identity provider, over `security.outbound`'s rotating agent.
///
/// **Every value is parsed by the adapter that sends it, here at boot**, so an unusable one stops
/// the process naming its key. The client secret is read once, from its file, into a `Secret`; no
/// refusal below carries it, the subject token, or the file's contents.
///
/// One client per source rather than one per deployment: nothing here is shared that would need
/// to be, and a second declared source pays one more TLS agent.
fn delegation(
    alias: &sutura_domain::model::SourceName,
    declared: &sutura_config::sources::workload_identity::DelegationDeclared,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_exec_bigquery::delegation::Delegation, String> {
    use sutura_exec_bigquery::delegation::http::{ExchangeClient, OverHttp, ReadBounds, TokenEndpoint, rotating_agent};
    use sutura_exec_bigquery::delegation::{Delegation, RequestedAudience};

    let key = format!("sources.{alias}.workload_identity.delegation");
    let endpoint = TokenEndpoint::parse(declared.token_endpoint())
        .map_err(|cause| format!("`{key}.token_endpoint` is not an endpoint this exchange can dial: {cause}"))?;
    let audience = RequestedAudience::parse(declared.audience())
        .map_err(|cause| format!("`{key}.audience` is not an audience this exchange can ask for: {cause}"))?;
    let secret = crate::password_file::read_key(&format!("{key}.client_secret_file"), declared.client_secret_file())?;
    let client =
        ExchangeClient::new(declared.client_id(), secret).map_err(|cause| format!("`{key}.client_id` is unusable: {cause}"))?;
    let bounds = ReadBounds::parse(EXCHANGE_TIMEOUT_SECONDS, EXCHANGE_MAX_ANSWER_BYTES)
        .map_err(|cause| format!("the delegation exchange's own read bounds are unusable: {cause}"))?;
    let (agent, rotator) = rotating_agent(bounds, outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (delegation exchange)", rotator);
    Ok(Delegation::through(
        std::sync::Arc::new(OverHttp::new(endpoint, client, agent, bounds)),
        audience,
    ))
}

/// How long one exchange may take. Fixed rather than declared: it runs inside a question's own
/// request timeout, so a key for it would be a second bound on the same wait.
const EXCHANGE_TIMEOUT_SECONDS: u64 = 10;

/// A token response is a few kilobytes; this bounds a misbehaving endpoint, not a real answer.
const EXCHANGE_MAX_ANSWER_BYTES: u64 = 64 * 1024;

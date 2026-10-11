//! The credential broker a served deployment with an impersonating source is answered
//! through - built here, out of `serve.rs`, so the composition root keeps the dispatch and the one
//! thing that is genuinely its own logic reads next to the refusals that guard it.
//!
//! The whole file is behind the three impersonating adapters' features in `serve.rs`'s
//! `mod broker;`, and the `workload_identity` half behind `bigquery` alone: only that adapter's `wire`
//! client can compose a delegation exchange.
//!
//! **This replaces the exchanging broker's builder rather than restoring it.** That one wired
//! `StsOverHttp` and `IamCredentialsOverHttp`, both deleted with the `wire` transport, so it has no
//! implementor a composition root can reach. `sutura_config::DeclaredPrincipalBroker` is what
//! the ADBC transport can be served through: it presents the asking subject's OWN verified
//! assertion, the declared pool federates it to that subject's own principal, and the data
//! system's grants on that principal decide what the caller reads.
//!
//! **A source declaring `workload_identity.delegation` presents the token its caller's own inbound
//! token is exchanged for**, through the client `delegation` builds - `direct` mode's second
//! document (`docs/adr/0014`, fourth and fifth amendments). Held in-process by this root's
//! `build_broker` cells and by `serve::tests::delegation_served`, which reaches it behind the real
//! router and leg-1 gate. The spawned binary reaches it only in `tests/served/delegation_adbc.rs`,
//! whose `#[ignore]`d cells need the ADBC driver to boot and run under `just e2e-datahub-adbc`.
//!
//! It scans the WHOLE `sources:` registry rather than the `bigquery`-kind entries only, for the
//! reason the deleted builder did: a plan may read a shared source of one kind and an impersonating
//! one of another, and a broker is per answer. A non-`BigQuery` adapter that cannot deliver the
//! impersonating posture is already refused at its own posture cross-check, before any question.

use sutura_config::DeclaredPrincipalBroker;
use sutura_domain::source::SourcePosture;

/// Reads the declared sources into the broker a served question is answered through.
///
/// # Errors
///
/// Startup refusals, and each exists because the alternative is a deployment that boots clean and
/// refuses every impersonated question:
///
/// - a switching or authenticating source whose declaration names nobody, so no caller could ever
///   be served there;
/// - a source declaring the pool expectations `telekom/sutura#817` added, which described a token
///   exchange no transport in this build performs;
/// - a declared `delegation` whose endpoint, client ID, audience or client secret file this
///   adapter cannot send - see [`delegation`].
pub(crate) fn build_broker(
    registry: &sutura_config::SourceRegistry,
    #[cfg_attr(
        not(feature = "bigquery"),
        expect(unused_variables, reason = "only a `bigquery` source composes a delegation over it")
    )]
    outbound: Option<&sutura_tls::Declared>,
) -> Result<DeclaredPrincipalBroker, String> {
    let mut broker = DeclaredPrincipalBroker::empty();
    for (alias, source) in registry.each() {
        if let Some(SourcePosture::SharedServiceUser { declared }) = source.posture() {
            broker = broker.shared(alias.clone(), declared.clone());
        }
        #[cfg(feature = "clickhouse")]
        if let Some(declared) = crate::clickhouse::declared_principals(alias, source)? {
            broker = broker.switching(alias.clone(), declared);
        }
        #[cfg(feature = "oracle")]
        if let Some(declared) = crate::oracle::declared_subjects(alias, source)? {
            broker = broker.authenticating(alias.clone(), declared);
        }
        #[cfg(feature = "bigquery")]
        {
            broker = federating(broker, alias, source, outbound)?;
        }
    }
    Ok(broker)
}

/// One `bigquery` source's `workload_identity` declaration, added to `broker` as a federating
/// source; a source without one is returned unchanged.
#[cfg(feature = "bigquery")]
fn federating(
    broker: DeclaredPrincipalBroker,
    alias: &sutura_domain::model::SourceName,
    source: &sutura_config::ConfiguredSource,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<DeclaredPrincipalBroker, String> {
    let Some(workload) = source.workload_identity() else {
        return Ok(broker);
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
    let delegation = workload
        .delegation()
        .map(|declaration| delegation(alias, declaration, outbound))
        .transpose()?;
    Ok(broker.federating(alias.clone(), delegation))
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
#[cfg(feature = "bigquery")]
fn delegation(
    alias: &sutura_domain::model::SourceName,
    declared: &sutura_config::sources::workload_identity::DelegationDeclared,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_domain::identity::Delegation, String> {
    use sutura_domain::identity::{Delegation, RequestedAudience};
    use sutura_exec_bigquery::delegation::{ExchangeClient, OverHttp, ReadBounds, TokenEndpoint, rotating_agent};

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

#[cfg(feature = "bigquery")]
/// How long one exchange may take. Fixed rather than declared: it runs inside a question's own
/// request timeout, so a key for it would be a second bound on the same wait.
const EXCHANGE_TIMEOUT_SECONDS: u64 = 10;

#[cfg(feature = "bigquery")]
/// A token response is a few kilobytes; this bounds a misbehaving endpoint, not a real answer.
const EXCHANGE_MAX_ANSWER_BYTES: u64 = 64 * 1024;

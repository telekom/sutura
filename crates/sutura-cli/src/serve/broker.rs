//! The credential broker a `bigquery` deployment is served under - built here, out of `main.rs`, so
//! the composition root stays under the line ceiling while the one thing that is genuinely its own
//! logic - mapping a `sources:` tree onto the exchanging broker - is read next to the refusals that
//! guard it.
//!
//! The whole file is `#[cfg(feature = "bigquery")]` by construction in `main.rs`'s `mod broker;`, so
//! nothing here is compiled into a build that links no `BigQuery` adapter.

/// The broker a `bigquery` deployment is served under: one that exchanges a subject's credential.
///
/// **It holds BOTH shapes, because one plan may read one of each and the broker is per answer, not
/// per source** - a declared witness for shared sources (exactly what
/// `sutura_config::StaticCredentialBroker::from_registry` builds) and an exchanged per-subject
/// credential for impersonating ones. A source this broker holds neither half for is refused as
/// `credential_unavailable` rather than answered as this process, which is the port's own fallback
/// and the reason a forgotten attachment here cannot silently read every row as the deployment.
///
/// **The exchange reuses the same pinned `WireAgent` and bounds the source composition declares** -
/// this is why `Credential::read` and `StsOverHttp` take a `WireAgent` at all: the exchange and
/// the job share one connection pool and one set of pins by construction.
///
/// **The expiry FLOOR is wired from `server.request_timeout_seconds`**, which is the deployment's
/// own statement of how long a question may take (`docs/adr/0008` part 6 puts the floor in the
/// broker adapter for exactly this reason): an exchanged credential that would age out during the
/// answer is refused by the broker rather than presented and left to fail at the source mid-query.
///
/// **The exchanged-credential cache (`docs/adr/0031`) is wired here too, off unless
/// `security.credential_cache.enabled` is `true`.** It shares the same floor: a cached leg is
/// served only if it would still clear the identical bound a fresh mint is held to, never a looser
/// one - `sutura_exec_bigquery::WorkloadIdentityBroker::with_cache`'s own doc has the fold.
/// The broker type [`build_broker`] returns: both hops, wired to their real HTTP implementors.
///
/// Named for `clippy::type_complexity`'s own threshold, the same reason `sutura_exec_bigquery::wire`'s
/// `Wired`/`Grid` aliases are: a three-argument generic nested inside a `Result` is over it, and this
/// is the one function in the crate whose signature would otherwise carry the whole thing inline.
pub(crate) type ExchangingBroker = sutura_exec_bigquery::WorkloadIdentityBroker<
    sutura_exec_bigquery::wire::StsOverHttp,
    sutura_exec_bigquery::wire::IamCredentialsOverHttp,
>;

pub(crate) fn build_broker(
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
    credential_cache: sutura_config::CredentialCacheSettings,
    outbound: Option<&sutura_tls::Anchors>,
    leg_one: Option<&sutura_config::InboundIdentity>,
) -> Result<ExchangingBroker, String> {
    use sutura_config::SourcePlacement;
    use sutura_domain::source::{SharedIdentityDeclared, SourcePosture};
    use sutura_exec_bigquery::wire::{
        BytesBilledCeiling, IamCredentialsOverHttp, JobBounds, QueryDeadline, StsOverHttp, WireAgent,
    };
    use sutura_exec_bigquery::{WorkloadIdentity, WorkloadIdentityBroker};

    // The ONE deadline is the deployment-wide query budget, and it is the only thing `StsOverHttp`
    // reads off its agent's bounds (a socket timeout for the exchange - `StsExchange::exchange` has
    // no port `Deadline` of its own to read, so this bound still opens fresh from `request_timeout`
    // directly rather than dividing it - `docs/adr/0029` retired the arithmetic that used to,
    // `within_request_timeout`, and nothing in this exchange yet carries the port's own instant).
    //
    // **`request_timeout.budget()` and NOT `.seconds()`, and the number this widens FROM is main's,
    // not a prior draft of this line.** On main, `within_request_timeout(30)` gave this exchange a
    // ten-second `QueryDeadline` - a fifteen-second socket window. `.budget()` (the timeout minus the
    // fixed one-second reply margin `sutura_config::RequestTimeout` already subtracts for the port)
    // gives twenty-nine seconds - a thirty-four-second socket window, wider than main's by nineteen
    // seconds, not by the five `CONNECT_MARGIN` alone: `StsOverHttp::exchange` opens its
    // `CallDeadline` at ITS OWN start (`wire/sts.rs`), which is after admission, parsing and planning
    // - `sutura_app::answer` mints the credential well after the port's own `Deadline` is already
    // open - so the socket's own end is `exchange_start + 34s`, not `arrival + 34s`, and the true
    // overrun past the caller's thirty-second wait depends on how late the exchange starts, not only
    // on `CONNECT_MARGIN`. The tower `408` still bounds the caller either way; what widens is how
    // long a STUCK exchange holds the admitted slot. Closing that for real means handing `mint` the
    // request's own `Deadline` - a change to `WorkloadIdentityBroker`'s own trait, out of this fix's
    // scope and worth its own row when it lands; this line only stops the exchange's own ceiling from
    // exceeding the port's budget, which is the narrower thing it can do without that trait change.
    // The ceiling is irrelevant to an STS metadata call - it is never sent to a billing endpoint - so
    // rather than invent one it is taken from the first declared `BigQuery` source, of which there is
    // always at least one here: `one_kind` has already refused a deployment with none.
    // `Zero`/`TooLarge` are unreachable here in practice: `RequestTimeout::parse` already refuses a
    // timeout that cannot afford the one-second reply margin and caps the key at 300s, so
    // `budget().seconds()` can only ever be a value `QueryDeadline::parse` accepts.
    let deadline = QueryDeadline::parse(request_timeout.budget().seconds())
        .map_err(|cause| format!("`server.request_timeout_seconds` leaves no token-exchange deadline: {cause}"))?;

    let mut ceiling_source: Option<u64> = None;
    let mut shared: Vec<(sutura_domain::model::SourceName, SharedIdentityDeclared)> = Vec::new();
    let mut impersonating: Vec<(sutura_domain::model::SourceName, WorkloadIdentity)> = Vec::new();
    // **The twin-root boot refusal, telekom/sutura#817.** When leg 1 is configured `direct` it pins
    // the exact issuer and audience a caller token must carry; a source that ALSO declares what its
    // pool accepts is tying the two roots together. If they can never agree, a document leg 1
    // verifies is one the pool would decline - so the deployment refuses at boot, before any
    // request, naming the source and both pairs. A `direct` leg 1's issuer and audience are the very
    // values that must equal the pool's expected ones for the SAME document to satisfy both sides.
    if let Some(sutura_config::InboundIdentity::Direct {
        authorization_server,
        resource,
        ..
    }) = leg_one
    {
        // Rendered as bare `str`s rather than the newtype values: neither `IssuerUrl` nor
        // `ResourceIdentifier` implements `Display`, and a refusal carries the exact configured
        // text an operator wrote, never a debug rendering that could differ.
        let accepted_issuer = authorization_server.as_str();
        let accepted_audience = resource.as_str();
        for (alias, source) in registry.each() {
            if let Some(workload) = source.workload_identity()
                && let (Some(expected_issuer), Some(expected_audience)) = (
                    workload.expected_issuer().map(|iss| iss.as_str()),
                    workload.expected_audience().map(|aud| aud.as_str()),
                )
                && (expected_issuer != accepted_issuer || expected_audience != accepted_audience)
            {
                return Err(format!(
                    "`sources.{alias}.workload_identity` names expectations no document leg 1 \
                     verifies can satisfy: a caller token `security.inbound` accepts carries issuer \
                     `{accepted_issuer}` / audience `{accepted_audience}`, but the pool declares \
                     issuer `{expected_issuer}` / audience `{expected_audience}`. The two trust \
                     roots are unconnected (telekom/sutura#817); either align them or remove the \
                     expectations.",
                ));
            }
        }
    }

    for (alias, source) in registry.each() {
        if let SourcePlacement::BigQuery { max_bytes_billed, .. } = source.placement() {
            ceiling_source.get_or_insert(*max_bytes_billed);
        }
        if let Some(SourcePosture::SharedServiceUser { declared }) = source.posture() {
            shared.push((alias.clone(), declared.clone()));
        }
        if let Some(workload) = source.workload_identity() {
            // The declared subject -> service-account map, telekom/sutura#376's second hop -
            // converted once here into the exec-bigquery crate's own map shape, the same reason
            // every other value on this line is re-expressed rather than passed through: an adapter
            // may not depend on the settings tree.
            let impersonate = workload
                .impersonate()
                .iter()
                .map(|(subject, target)| (subject.clone(), String::from(target.as_str())))
                .collect();
            impersonating.push((
                alias.clone(),
                WorkloadIdentity::of(
                    String::from(workload.audience().as_str()),
                    String::from(workload.scope().as_str()),
                )
                .with_impersonation(impersonate)
                .with_expectations(
                    workload.expected_issuer().map(|iss| iss.as_str()).map(String::from),
                    workload.expected_audience().map(|aud| aud.as_str()).map(String::from),
                ),
            ));
        }
    }
    let ceiling = BytesBilledCeiling::parse(ceiling_source.ok_or_else(|| {
        String::from(
            "this `bigquery` deployment declares no source with a `max_bytes_billed` to bound the token-exchange agent with",
        )
    })?)
    .map_err(|cause| format!("a declared `max_bytes_billed` is not a usable token-exchange bound: {cause}"))?;
    // `outbound` is `None` for the ordinary deployment, which is `WireAgent::secured`'s exact
    // `pinned` behaviour - `github.com/telekom/sutura#125`. The same declaration the job's own agent
    // reads: one boot-time read, shared by every `WireAgent` this composition root builds - the
    // second hop's own agent included, since `iamcredentials` and `sts` are reached with the same
    // pins and the same bounds. A declared bundle becomes a rotating handle, so a replaced bundle is
    // adopted by the next exchange.
    let (agent, rotator) = WireAgent::rotating_agent(JobBounds::of(deadline, ceiling), outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (STS exchange)", rotator);
    let agent = WireAgent::rotating(JobBounds::of(deadline, ceiling), agent);
    let exchange = StsOverHttp::new(agent.clone());
    let impersonation = IamCredentialsOverHttp::new(agent);

    let mut broker = WorkloadIdentityBroker::empty(exchange)
        .impersonating_via(impersonation)
        .with_floor(request_timeout.seconds());
    for (alias, declared) in shared {
        broker = broker.shared(alias, declared);
    }
    for (alias, workload) in impersonating {
        broker = broker.impersonating(alias, workload);
    }
    if credential_cache.enabled() {
        broker = broker.with_cache(credential_cache.capacity(), credential_cache.window().duration());
    }
    // The identity skill's own rule: the startup log prints the limit beside the mode. Read from
    // `broker.cache_capacity()` - THE BROKER'S OWN STATE - rather than `credential_cache` again, so
    // the line cannot say "off" while a cache sits attached above it: the two would have to drift
    // apart on purpose, not just by one `if` arm changing without the other. "Off by default" is
    // otherwise held by nothing but reading this function, the same shape `#684`'s spend ledger
    // states plainly of its own switch - this line, and the accessor it reads, are the mechanism,
    // and this crate's own test asserts the accessor beside the rendered line.
    if let Some(capacity) = broker.cache_capacity() {
        tracing::info!(
            capacity = capacity.get(),
            window_seconds = credential_cache.window().duration().as_secs(),
            "credential_cache: on"
        );
    } else {
        tracing::info!("credential_cache: off");
    }
    Ok(broker)
}

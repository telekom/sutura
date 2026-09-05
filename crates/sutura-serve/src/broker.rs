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
pub(crate) fn build_broker(
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
) -> Result<sutura_exec_bigquery::WorkloadIdentityBroker<sutura_exec_bigquery::wire::StsOverHttp>, String> {
    use sutura_config::SourcePlacement;
    use sutura_domain::source::{SharedIdentityDeclared, SourcePosture};
    use sutura_exec_bigquery::wire::{BytesBilledCeiling, JobBounds, QueryDeadline, StsOverHttp, WireAgent};
    use sutura_exec_bigquery::{WorkloadIdentity, WorkloadIdentityBroker};

    // The ONE deadline is the deployment-wide query budget, and it is the only thing `StsOverHttp`
    // reads off its agent's bounds (a socket timeout for the exchange). The ceiling is irrelevant to
    // an STS metadata call - it is never sent to a billing endpoint - so rather than invent one it is
    // taken from the first declared `BigQuery` source, of which there is always at least one here:
    // `one_kind` has already refused a deployment with none.
    let deadline = QueryDeadline::within_request_timeout(request_timeout.seconds())
        .map_err(|cause| format!("`server.request_timeout_seconds` leaves no token-exchange deadline: {cause}"))?;

    let mut ceiling_source: Option<u64> = None;
    let mut shared: Vec<(sutura_domain::model::SourceName, SharedIdentityDeclared)> = Vec::new();
    let mut impersonating: Vec<(sutura_domain::model::SourceName, WorkloadIdentity)> = Vec::new();
    for (alias, source) in registry.each() {
        if let SourcePlacement::BigQuery { max_bytes_billed, .. } = source.placement() {
            ceiling_source.get_or_insert(*max_bytes_billed);
        }
        if let Some(SourcePosture::SharedServiceUser { declared }) = source.posture() {
            shared.push((alias.clone(), declared.clone()));
        }
        if let Some(workload) = source.workload_identity() {
            impersonating.push((
                alias.clone(),
                WorkloadIdentity::of(
                    String::from(workload.audience().as_str()),
                    String::from(workload.scope().as_str()),
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
    let exchange = StsOverHttp::new(WireAgent::pinned(JobBounds::of(deadline, ceiling)));

    let mut broker = WorkloadIdentityBroker::empty(exchange).with_floor(request_timeout.seconds());
    for (alias, declared) in shared {
        broker = broker.shared(alias, declared);
    }
    for (alias, workload) in impersonating {
        broker = broker.impersonating(alias, workload);
    }
    Ok(broker)
}

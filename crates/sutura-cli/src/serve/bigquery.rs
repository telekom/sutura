//! The `BigQuery` half of this composition root: one declared source becomes an open `Warehouse`.
//!
//! **Split out of `main.rs` when a second adapter's composition pushed that file past the
//! unexemptable 1000-line cap**, and it takes the shape `sutura-cli`'s `sources/` directory already
//! has: the dispatcher stays in the composition root, and each kind's open-and-build pair lives in a
//! file of its own. Both `open_bigquery` definitions are here - the one that opens the adapter and
//! the refusal for a build that linked none - because the compiler picks between them at the one
//! call site, and keeping them apart is what let that refusal live in the composition root at all.
//!
//! What is NOT here: the credential it reads, the wire it dials and the broker that mints a subject
//! token are `sutura-exec-bigquery`'s and this root's `broker` module's, reached through
//! `sutura_config`'s parsed placement and this root's own helpers by their `super::` paths.

/// Opens one `BigQuery` adapter per declared source, over the wire, under a declared credential.
///
/// **Nothing is attached and nothing is registered, which is the difference from `open_files` that
/// matters:** the tables live in the dataset. What this function does instead is everything that can
/// fail before a listener is bound - the posture cross-check, the two bounds, and READING the
/// credential file, which is the one step that would otherwise fail on the first question.
#[cfg(feature = "bigquery")]
pub(crate) fn open_bigquery(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::BigQuerySource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = build_bigquery(source, configured, request_timeout, outbound)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns. Written as a fallback for the
    // reason `open_files` gives - the workspace denies `unwrap` and `expect`.
    engines
        .map(super::OpenedSources::BigQuery)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the `BigQuery` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, so the dispatcher above
/// has exactly one call and the compiler decides which of these it reaches. The parameters this body
/// does not read are named for it, which is what lets both signatures stay identical under
/// `dead_code = "deny"`.
///
/// The message names the FEATURE and not just the kind, because the two things an operator can do are
/// in two different files: change the `kind:`, or build with `--features bigquery`. A message that
/// only said "this binary links no `BigQuery` adapter" sent them to the first when they wanted the
/// second - which was this refusal's shape before the adapter was registered at all.
#[cfg(not(feature = "bigquery"))]
pub(crate) fn open_bigquery(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
    _request_timeout: sutura_config::RequestTimeout,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: bigquery`, and this binary was built without the `bigquery` \
         feature - so it links no BigQuery adapter and composes the in-process engine only. Build \
         `sutura-cli` with `--features bigquery`, or declare a `files` source"
    ))
}

/// Builds one `BigQuery` adapter, after checking this build can deliver the source's posture.
///
/// **Every value it needs is declared, and the two that are not on the source entry say where they
/// come from.** The billing project, the dataset, the credential file and the bytes-billed ceiling are
/// the entry's; the query deadline is `server.request_timeout_seconds`, which is what
/// `sutura_exec_bigquery::wire::QueryDeadline` asks a composition root for by name - a job that
/// outlives the request it is answering is billed for a result nobody is waiting for.
///
/// The ceiling is parsed HERE and not in `sutura-config`, and that is the single-owner rule rather
/// than laziness: the range belongs to the adapter, so a second copy of it in the settings tree would
/// be the duplicate that drifts. What the settings tree owns is that the key was written.
#[cfg(feature = "bigquery")]
fn build_bigquery(
    source: &sutura_domain::model::SourceName,
    configured: &sutura_config::ConfiguredSource,
    _request_timeout: sutura_config::RequestTimeout,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<super::BigQuerySource, String> {
    let sutura_config::SourcePlacement::BigQuery {
        ref billing_project,
        ref dataset,
        ..
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the BigQuery attach step with a placement no BigQuery adapter \
             reads, which `one_kind` should have dispatched elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    // The same cross-check `open_files` makes and against a DIFFERENT constant, which is the point of
    // it being per adapter rather than per deployment: this adapter declares `PerSubjectCredential`,
    // so a `shared-service-user` entry is deliverable and an `impersonation-at-source` entry passes
    // the adapter's capability half. The composition's half (the broker that mints a subject) is
    // attached in `run()`'s `bigquery` arm, not here - unchanged by the wire removal.
    identity
        .posture()
        .deliverable_by(
            <super::BigQuerySource as sutura_domain::warehouse::Warehouse>::IMPERSONATION,
            source,
        )
        .map_err(super::flatten)?;
    // **The ADBC driver authenticates itself, so there is no credential file to read and no token
    // rotation to drive - the removed `wire` half.** The on-disk driver is read at BOOT rather than on
    // the first question, which is the same argument the inbound key set is read before the listener
    // opens: a driver path that is missing has to stop the process, not become a deployment that
    // answers every question with a failure while its startup log says it opened a dataset.
    let driver_path = std::env::var("SUTURA_BIGQUERY_ADBC_DRIVER").map_err(|_| {
        format!(
            "`SUTURA_BIGQUERY_ADBC_DRIVER` is not set; point it at the self-built \
             libadbc_driver_bigquery.so for {source}"
        )
    })?;
    // The two resource newtypes are parsed a SECOND time here, and that is not a redundant check: the
    // settings tree's `BillingProject` and the transport's `ProjectId` are two types in two crates,
    // and the one whose value is written into a request path is the transport's. Neither can be
    // reached from the other without going through a `parse`.
    let project = sutura_exec_bigquery::transport::ProjectId::parse(billing_project.as_str())
        .map_err(|cause| format!("`sources.{source}.billing_project` is not a usable project id: {cause}"))?;
    let dataset = sutura_exec_bigquery::transport::DatasetId::parse(dataset.as_str())
        .map_err(|cause| format!("`sources.{source}.dataset` is not a usable dataset id: {cause}"))?;
    Ok(sutura_exec_bigquery::BigQueryWarehouse::over_adbc(
        source.clone(),
        identity.posture().clone(),
        project,
        dataset,
        driver_path,
    ))
}

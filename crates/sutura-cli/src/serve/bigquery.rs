//! The `BigQuery` half of this composition root: one declared source becomes an open `Warehouse`.
//!
//! **Split out of `main.rs` when a second adapter's composition pushed that file past the
//! unexemptable 1000-line cap**, and it takes the shape `sutura-cli`'s `sources/` directory already
//! has: the dispatcher stays in the composition root, and each kind's open-and-build pair lives in a
//! file of its own. Both `open_bigquery` definitions are here - the one that opens the adapter and
//! the refusal for a build that linked none - because the compiler picks between them at the one
//! call site, and keeping them apart is what let that refusal live in the composition root at all.
//!
//! What is NOT here: the ADBC driver it loads and the broker that resolves a subject to a principal
//! are `sutura-exec-bigquery`'s and this root's `broker` module's, reached through `sutura_config`'s
//! parsed placement and this root's own helpers by their `super::` paths.

/// Opens one `BigQuery` adapter per declared source, over the ADBC driver.
///
/// **Nothing is attached and nothing is registered, which is the difference from `open_files` that
/// matters:** the tables live in the dataset. What this function does instead is everything that can
/// fail before a listener is bound - the posture cross-check, the driver path, and PARSING the
/// declared impersonation scope, each of which would otherwise fail on a question rather than at
/// boot.
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
/// **Every value it needs is declared on the source entry.** The billing project, the dataset and -
/// where the source impersonates - the scope an impersonated credential is minted for. The request
/// timeout no longer reaches this function: the `wire` half's `QueryDeadline` and bytes-billed
/// ceiling went away with the transport, and the ADBC driver bounds a job under its own settings.
///
/// The scope is parsed HERE as well as in `sutura-config`, and that is the single-owner rule rather
/// than laziness: the crate that puts a value into a request is the one whose parse decides whether
/// it can be sent, and the driver's comma-split option parsing is a risk only this side knows about.
/// What the settings tree owns is that the key was written.
#[cfg(feature = "bigquery")]
fn build_bigquery(
    source: &sutura_domain::model::SourceName,
    configured: &sutura_config::ConfiguredSource,
    _request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<super::BigQuerySource, String> {
    crate::bigquery_driver::refuse_undeliverable_outbound(source, outbound)?;
    let sutura_config::SourcePlacement::BigQuery {
        ref billing_project,
        ref dataset,
        max_bytes_billed,
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
    // the adapter's capability half. The composition's half - the broker that resolves a subject to
    // the principal a source declared - is `crate::serve::broker`, attached in `run()`'s `bigquery`
    // and `mixed` arms rather than here. **The claim this comment used to make, that the attaching
    // half was unchanged by the wire removal, was wrong**: the broker it named was deleted with the
    // wire, so between that commit and this one an `impersonation-at-source` entry booted clean and
    // was refused `credential_unavailable` on every question.
    identity
        .posture()
        .deliverable_by(
            <super::BigQuerySource as sutura_domain::warehouse::Warehouse>::IMPERSONATION,
            source,
        )
        .map_err(super::flatten)?;
    // **The ADBC driver authenticates itself, so there is no credential file to read and no token
    // rotation to drive - the removed `wire` half.** The driver is opened at BOOT rather than on the
    // first question, which is the same argument the inbound key set is read before the listener
    // opens: a driver this process cannot open has to stop it, not become a deployment that answers
    // every question with a failure while its startup log says it opened a dataset.
    //
    // **And it opens the DRIVER and not only a declaration.** `AdbcBigQuery::probe` initialises it,
    // so a release artefact whose linked-in driver does not start, and a mounted path naming a
    // missing or wrong-ABI `.so`, both stop the process here. It is also the only place in a normal
    // boot that runs the driver's Go runtime beside tokio and the release allocator - a coexistence
    // a link check cannot see - which is what makes `just bigquery-driver-check` a real verification
    // rather than a compile. What it still does not establish is that a question can be ANSWERED:
    // the probe opens no connection and looks for no credential.
    //
    // WHICH driver is `crate::bigquery_driver`'s decision, shared with `sources::bigquery` and with
    // `doctor` - `nix/shipped.nix` links one into every published artefact now, which is what
    // `docs/adr/0018`'s sixth amendment records and its fifth left open.
    // **The money bound, parsed before anything expensive is opened.** `sources.<alias>.max_bytes_billed`
    // is required at boot and its RANGE belongs to the transport that sends it, so a `0` - BigQuery's
    // own spelling of *no ceiling* - and a value above what the transport will send both stop the
    // process here rather than reaching the driver as no bound at all. For one review round this key
    // was required, unparsed and read by nothing.
    //
    // **Ahead of the driver deliberately**, and it is the one ordering here a cell depends on: a
    // number the deployment wrote is the operator's own typo, and resolving the driver starts a Go
    // runtime in this process, so a refusal about the ceiling must not be preceded by one about an
    // artefact. `a_bigquery_ceiling_the_adapter_will_not_send_is_a_startup_refusal_naming_the_key`
    // asserts the refusal names the key and the source and does NOT name the driver.
    let max_bytes_billed = sutura_exec_bigquery::adbc::BytesBilledCeiling::parse(max_bytes_billed)
        .map_err(|cause| format!("`sources.{source}.max_bytes_billed` is not a ceiling this transport will send: {cause}"))?;
    let driver = crate::bigquery_driver::resolve(&format!("`sources.{source}`"))?;
    sutura_exec_bigquery::adbc::AdbcBigQuery::probe(&driver)
        .map_err(|cause| format!("the BigQuery ADBC driver ({driver}) did not initialise for {source}: {cause}"))?;
    // The two resource newtypes are parsed a SECOND time here, and that is not a redundant check: the
    // settings tree's `BillingProject` and the transport's `ProjectId` are two types in two crates,
    // and the one whose value is written into a request path is the transport's. Neither can be
    // reached from the other without going through a `parse`.
    let project = sutura_exec_bigquery::transport::ProjectId::parse(billing_project.as_str())
        .map_err(|cause| format!("`sources.{source}.billing_project` is not a usable project id: {cause}"))?;
    let dataset = sutura_exec_bigquery::transport::DatasetId::parse(dataset.as_str())
        .map_err(|cause| format!("`sources.{source}.dataset` is not a usable dataset id: {cause}"))?;
    // **Which of the two modes this source is, decided here and never per request.** They are XOR
    // rather than a ladder: a `shared-service-user` source runs on the deployment's own application
    // default credentials and impersonates nothing, and an `impersonation-at-source` source
    // federates the asking subject's own assertion against the pool it declares. `sutura_config`
    // refuses a `workload_identity` block on a shared entry and refuses its absence on an
    // impersonating one, so the presence of the block IS the posture and there is no third state.
    //
    // **`audience` is load-bearing again.** It was read by nothing for three rounds, while the
    // shipped mechanism was a principal switch on the deployment's own credential; the pool's
    // exchange needs it, and `externalaccount::Options::validate` refuses an empty one - so an
    // unusable declaration fails here, before a listener is bound, rather than on the first
    // impersonated question.
    //
    // **`scope` is NOT, and it is the only declared value on this path that reaches nothing.** The
    // pinned driver has nowhere to put it - `WorkloadPool` carries the measurement - so it is not
    // handed over, not screened twice, and named as unread where the operator declares it
    // (`sutura_config`'s `WifScope`). A value carried here and dropped later would be the shape this
    // whole change exists to remove.
    let impersonation = match configured.workload_identity() {
        None => sutura_exec_bigquery::adbc::Impersonation::Disabled,
        Some(workload) => sutura_exec_bigquery::adbc::Impersonation::ThroughPool(
            sutura_exec_bigquery::adbc::WorkloadPool::parse(workload.audience().as_str()).map_err(|cause| {
                format!("`sources.{source}.workload_identity` names a pool this transport cannot exchange against: {cause}")
            })?,
        ),
    };
    Ok(sutura_exec_bigquery::BigQueryWarehouse::over_adbc(
        source.clone(),
        identity.posture().clone(),
        project,
        dataset,
        driver,
        impersonation,
        max_bytes_billed,
    ))
}

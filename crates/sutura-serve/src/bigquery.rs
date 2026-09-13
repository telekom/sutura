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
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::BigQuerySource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = build_bigquery(source, configured, request_timeout)?;
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
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: bigquery`, and this binary was built without the `bigquery` \
         feature - so it links no BigQuery adapter and composes the in-process engine only. Build \
         `sutura-serve` with `--features bigquery`, or declare a `files` source"
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
    request_timeout: sutura_config::RequestTimeout,
) -> Result<super::BigQuerySource, String> {
    use sutura_exec_bigquery::transport::{DatasetId as WireDataset, ProjectId as WireProject};
    use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
    use sutura_exec_bigquery::wire::{BigQueryWire, BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};

    // Matched rather than read off accessors every kind would have to have, for the reason
    // `open_files` gives at the same shape: `one_kind` has already decided which arm this is, and a
    // second openable kind should arrive as a compile error at this line too.
    let sutura_config::SourcePlacement::BigQuery {
        ref billing_project,
        ref dataset,
        ref credential_file,
        max_bytes_billed,
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
    // the adapter's capability half - which is the change issue 87 landed. Passing the adapter's half
    // is not the whole story, and the composition's half is below.
    identity
        .posture()
        .deliverable_by(
            <super::BigQuerySource as sutura_domain::warehouse::Warehouse>::IMPERSONATION,
            source,
        )
        .map_err(super::flatten)?;
    // **The adapter can carry a subject, and the COMPOSITION's other half - the broker that mints
    // one - is attached in `run()`'s `bigquery` arm, not here.** The port, the
    // `WorkloadIdentityBroker` and the real `StsExchange` all exist; `build_broker` builds the broker
    // holding this source's declared `workload_identity`, and this line merely OPENING the source is
    // what lets a question against it be served as the asker rather than refused. The boot refusals
    // that still guard the cases with no broker are `Settings::refusals`'s `MissingWorkloadIdentity`
    // for an impersonating source with none declared, and the `cfg(not(feature = "bigquery"))` half
    // of `open_bigquery` for a build that links none of this.
    //
    // **What keeps an impersonating source from being read under the deployment's own identity if
    // somebody later forgets to attach a broker is not a refusal here - it is the port.** `build_broker`
    // refuses to mint for a source it holds no exchanging half for (`Minted::Refused`), so a question
    // against one is refused as `credential_unavailable` rather than answered as this process.
    // **`within_request_timeout` and NOT `parse`, and the difference is a bug that would only show up
    // under load.** What a job may spend is not the request timeout: an answer makes
    // `QueryDeadline::CALLS_PER_ANSWER` calls and each pays a connect margin on top of its own budget,
    // so a 30-second deadline inside a 30-second request timeout overruns the transport that promised
    // it. That arithmetic lives in the adapter, next to the constant it depends on, which is why a
    // composition root asks for the SHARE rather than computing one.
    let deadline = QueryDeadline::within_request_timeout(request_timeout.seconds())
        .map_err(|cause| format!("`server.request_timeout_seconds` leaves no BigQuery job deadline: {cause}"))?;
    let ceiling = BytesBilledCeiling::parse(max_bytes_billed)
        .map_err(|cause| format!("`sources.{source}.max_bytes_billed` is not a usable ceiling: {cause}"))?;
    let bounds = JobBounds::of(deadline, ceiling);
    // Read at BOOT rather than on the first question, which is the same argument the inbound key set
    // is read before the listener opens: a credential file that is missing, unreadable or not a
    // credential has to stop the process, not become a deployment that answers every question with a
    // failure while its startup log says it opened a dataset.
    // ONE agent, cloned, and not two `pinned` calls - which is what `Credential::read` taking an agent
    // is for: the token exchange and the job then share one connection pool and one set of pins by
    // construction rather than because two call sites happened to pass the same bounds. `WireAgent` is
    // `Clone` and a `ureq::Agent`'s clone shares its pool, so the clone is the cheap half of that.
    let agent = WireAgent::pinned(bounds);
    let credentials = Credential::read(&CredentialFile::at(credential_file.clone()), agent.clone()).map_err(|cause| {
        format!(
            "`sources.{source}.credential_file` could not be read: {}",
            super::flatten(cause)
        )
    })?;
    // The two resource newtypes are parsed a SECOND time here, and that is not a redundant check: the
    // settings tree's `BillingProject` and the transport's `ProjectId` are two types in two crates,
    // and the one whose value is written into a request path is the transport's. Neither can be
    // reached from the other without going through a `parse`.
    let project = WireProject::parse(billing_project.as_str())
        .map_err(|cause| format!("`sources.{source}.billing_project` is not a usable project id: {cause}"))?;
    let dataset = WireDataset::parse(dataset.as_str())
        .map_err(|cause| format!("`sources.{source}.dataset` is not a usable dataset id: {cause}"))?;
    Ok(sutura_exec_bigquery::BigQueryWarehouse::new(
        source.clone(),
        identity.posture().clone(),
        project,
        dataset,
        BigQueryWire::new(agent, credentials),
    ))
}

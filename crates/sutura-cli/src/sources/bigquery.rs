//! The `BigQuery` half of the composition root: one declared dataset becomes an open `Warehouse`.
//!
//! **Everything here is behind `#[cfg(feature = "bigquery")]` except the refusal for its absence**,
//! which is what makes this module the answer to "which kinds did THIS BUILD link" - a question
//! `sutura_config` cannot answer, because it cannot see a link and must not pretend to. The
//! vocabulary of kinds is the repository's; the set a binary opens is this file's.
//!
//! Its suite is at the bottom of this file rather than beside its callers, and that is deliberate:
//! an impl file whose tests live in the parent module is the partition `cargo xtask test-causality`
//! reads as a false *green against base behaviour*.

use sutura_domain::model::SourceName;

use crate::sources::Opened;
// Both reached only by the COMPOSING half below. A build without the feature gets the refusal, which
// names no type of its own and flattens no error - and `dead_code` is `deny` here, so an ungated
// import is a build that fails rather than one that warns. Measured by compiling the default set.
#[cfg(feature = "bigquery")]
use crate::commands::render;
#[cfg(feature = "bigquery")]
use crate::sources::OpenedWith;

/// A `BigQuery` source as this binary composes it: the adapter, over the wire, over a credential file.
///
/// **The same three layers `sutura-serve`'s own alias names, through the same public constructors**,
/// which is what "one composition per adapter" amounts to across two binaries that may not depend on
/// each other: a fix to the credential path lands in `sutura-exec-bigquery` and both roots get it.
/// `docs/adr/0018` is the record for the inner two.
#[cfg(feature = "bigquery")]
pub(crate) type BigQuerySource = sutura_exec_bigquery::BigQueryWarehouse<
    sutura_exec_bigquery::wire::BigQueryWire<sutura_exec_bigquery::wire::credential::Credential>,
>;

/// Opens one `BigQuery` dataset, over the wire, under the credential the deployment declared.
///
/// **Nothing is attached and nothing is registered, which is the difference from the files arm that
/// matters:** the tables live in the dataset, so [`OpenedWith::attached`] is `None` here. What this
/// does instead is everything that can fail before a question is asked - the posture cross-check, the
/// two bounds, and READING the credential file, which is the one step that would otherwise fail on the
/// first question.
///
/// **The composition is `sutura-serve`'s `build_bigquery`, line for line, through the same public
/// constructors** - which is what issue 121 asks for by "one composition per adapter, shared by both
/// roots". It is a copy rather than a shared function because the two composition roots are separate
/// binaries and neither may depend on the other; what is genuinely shared is
/// `sutura-exec-bigquery`'s own constructors, so a fix to the credential path lands once. The same
/// argument `crate::sources::refuse_unattached` carries.
///
/// # Errors
///
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; a
/// posture this adapter cannot deliver; the `impersonation-at-source` posture, which this binary
/// attaches no exchanging broker for; either bound out of range; a credential file that cannot be
/// read; and a project or dataset id the transport will not accept.
#[cfg(feature = "bigquery")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
) -> Result<Opened, String> {
    use sutura_exec_bigquery::transport::{DatasetId as WireDataset, ProjectId as WireProject};
    use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
    use sutura_exec_bigquery::wire::{BigQueryWire, BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};

    // Matched rather than read off accessors every kind would have to have, for the reason the files
    // arm gives at the same shape: the dispatcher has already decided which arm this is, and a second
    // openable kind should arrive as a compile error at this line too.
    let sutura_config::SourcePlacement::BigQuery {
        ref billing_project,
        ref dataset,
        ref credential_file,
        max_bytes_billed,
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the BigQuery attach step with a placement no BigQuery adapter \
             reads, which the dispatcher should have sent elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    // Against a DIFFERENT adapter's constant, which is the point of the cross-check being per adapter
    // rather than per deployment: this one declares `PerSubjectCredential`, so an
    // `impersonation-at-source` entry passes the adapter's capability half. That is not the whole
    // story, and the composition's half is below.
    identity
        .posture()
        .deliverable_by(<BigQuerySource as sutura_domain::warehouse::Warehouse>::IMPERSONATION, source)
        .map_err(|cause| render(&cause))?;
    // **The adapter can carry a subject, and this command wires no broker that mints one.** The port,
    // `WorkloadIdentityBroker` and the real `StsExchange` all exist and are tested; attaching one is
    // the step that awaits a deployable project. Until then such an entry would be opened and answered
    // under the credential the deployment declared - every row as this process while a reader believed
    // a subject's authorization was evaluated. Refused before the credential file is read, so an
    // operator fixes the posture rather than a file.
    if identity.posture() == &sutura_domain::source::SourcePosture::ImpersonationAtSource {
        return Err(format!(
            "`sources.{source}` is `impersonation-at-source`, and the `sutura` command does not attach \
             a broker that exchanges a subject's credential - refusing rather than reading every row \
             as this process; no fallback"
        ));
    }
    // `within_request_timeout` and NOT `parse`: an answer makes `QueryDeadline::CALLS_PER_ANSWER`
    // calls and each pays a connect margin, so the arithmetic lives in the adapter next to the
    // constant it depends on and a composition root asks for the SHARE. This command has no listener
    // whose timeout a job could outlive, and it reads `server.request_timeout_seconds` anyway: that
    // key is the one place a deployment says how long a question may take, and a second number
    // invented here would be the duplicate that drifts.
    let deadline = QueryDeadline::within_request_timeout(request_timeout.seconds())
        .map_err(|cause| format!("`server.request_timeout_seconds` leaves no BigQuery job deadline: {cause}"))?;
    let ceiling = BytesBilledCeiling::parse(max_bytes_billed)
        .map_err(|cause| format!("`sources.{source}.max_bytes_billed` is not a usable ceiling: {cause}"))?;
    let bounds = JobBounds::of(deadline, ceiling);
    // ONE agent, cloned, which is what `Credential::read` taking an agent is for: the token exchange
    // and the job share one connection pool and one set of pins by construction rather than because
    // two call sites happened to pass the same bounds.
    let agent = WireAgent::pinned(bounds);
    let credentials = Credential::read(&CredentialFile::at(credential_file.clone()), agent.clone())
        .map_err(|cause| format!("`sources.{source}.credential_file` could not be read: {}", render(&cause)))?;
    // Parsed a SECOND time here, and that is not a redundant check: the settings tree's
    // `BillingProject` and the transport's `ProjectId` are two types in two crates, and the one whose
    // value is written into a request path is the transport's.
    let project = WireProject::parse(billing_project.as_str())
        .map_err(|cause| format!("`sources.{source}.billing_project` is not a usable project id: {cause}"))?;
    let dataset = WireDataset::parse(dataset.as_str())
        .map_err(|cause| format!("`sources.{source}.dataset` is not a usable dataset id: {cause}"))?;
    let engine = sutura_exec_bigquery::BigQueryWarehouse::new(
        source.clone(),
        identity.posture().clone(),
        project,
        dataset,
        BigQueryWire::new(agent, credentials),
    );
    Ok(Opened::BigQuery(OpenedWith {
        engines: sutura_app::Warehouses::of(engine),
        // Nothing to compare: the tables are the dataset's. See the field's own note, and the caller's.
        attached: None,
        broker: sutura_config::StaticCredentialBroker::from_registry(registry),
    }))
}

/// The refusal for a build that linked no `BigQuery` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, so the dispatcher in
/// the parent module has exactly one call and the compiler decides which of these it reaches. The parameters this body
/// does not read are named for it, which is what lets both signatures stay identical under
/// `dead_code = "deny"`.
///
/// It names the FEATURE and not just the kind, because the two things an operator can do are in two
/// different files: change the `kind:`, or build with `--features bigquery`. A message that only said
/// "this binary links no BigQuery adapter" sent them to the first when they wanted the second.
#[cfg(not(feature = "bigquery"))]
pub(super) fn open(
    source: &SourceName,
    _configured: &sutura_config::ConfiguredSource,
    _registry: &sutura_config::SourceRegistry,
    _request_timeout: sutura_config::RequestTimeout,
) -> Result<Opened, String> {
    Err(format!(
        "`sources.{source}` is `kind: bigquery`, and this binary was built without the `bigquery` \
         feature - so it links no BigQuery adapter and composes the in-process engine over files \
         only. Build `sutura-cli` with `--features bigquery`, or declare a `files` source"
    ))
}

#[cfg(test)]
mod tests {
    // The fixtures live one module up so the dataset-argument refusal - which is `sources.rs`'s,
    // not this file's - can be tested beside the arm that makes it. One entry builder, two suites.
    #[cfg(feature = "bigquery")]
    use crate::sources::wif;
    use crate::sources::{bundle_naming, declaring_bigquery, open_engine, timeout};

    #[test]
    #[cfg(not(feature = "bigquery"))]
    fn a_kind_this_build_did_not_link_is_refused_by_name_and_says_what_to_build() {
        // The other half of reading the registry, and the reason the vocabulary of kinds is separate
        // from the set of adapters a given binary linked: `kind: bigquery` parses, because
        // `sutura-exec-bigquery` exists, and THIS binary links none of it. A message that only said
        // "not a data system this build can open" would send an operator looking for a typo.
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_bigquery("shared-service-user", ""),
            timeout(),
            None,
        )
        .map(|_| ())
        .expect_err("a kind this binary linked no adapter for must not open");
        assert!(error.contains("kind: bigquery"), "the kind is not named: {error}");
        assert!(
            error.contains("--features bigquery"),
            "the refusal must say what to build rather than only what is missing: {error}"
        );
    }

    #[test]
    #[cfg(feature = "bigquery")]
    fn a_declared_bigquery_source_reaches_the_credential_the_deployment_declared() {
        // **What this proves, and it is deliberately the furthest a test with no project can reach:**
        // the kind DISPATCHED to the BigQuery adapter, the shared posture was accepted against that
        // adapter's OWN `IMPERSONATION`, both bounds parsed, and the composition asked for the
        // credential file the settings tree named. A refusal about that path is the proof; a refusal
        // about the feature, the kind or the posture would mean it stopped earlier.
        //
        // It cannot go further here by construction: `wire::BigQueryWire`'s host is a `const` and its
        // agent is `https_only`, so there is no loopback to point it at - `docs/adr/0018` states that
        // as a coverage hole paid for with a security property, and `just bigquery-acceptance` is the
        // leg that closes it against a real dataset.
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_bigquery("shared-service-user", ""),
            timeout(),
            None,
        )
        .map(|_| ())
        .expect_err("the declared credential file is not there, so this command does not answer");
        assert!(
            error.contains("credential_file"),
            "the refusal must name the key that could not be read: {error}"
        );
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        // NOT the neighbouring arms, which is the half that stops this passing on the wrong branch: a
        // build that linked no adapter, or a posture cross-check that fired, would both be green on
        // the two assertions above if they only checked for a refusal.
        assert!(
            !error.contains("--features bigquery"),
            "this build DID link the adapter: {error}"
        );
        assert!(
            !error.contains("no fallback"),
            "the shared posture is deliverable by this adapter: {error}"
        );
    }

    #[test]
    #[cfg(feature = "bigquery")]
    fn a_bigquery_source_configured_to_impersonate_refuses_before_the_credential_is_read() {
        // The same cross-check the file engine gets, against a DIFFERENT adapter's constant - which is
        // the whole point of `deliverable_by` being called per adapter rather than per deployment.
        // `sutura-exec-bigquery` declares `PerSubjectCredential`, so the capability half PASSES for an
        // `impersonation-at-source` entry. What cannot happen is the COMPOSITION's half: this command
        // attaches no broker that exchanges a subject's credential, so answering would read every row
        // as this process while the declaration promised a subject's authorization was evaluated.
        //
        // **Refused BEFORE the credential file is read**, and the last assertion is what pins that
        // order: a posture the composition cannot honour is not worth a filesystem read, and an
        // operator told about a missing file would fix the wrong thing.
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_bigquery(
                "impersonation-at-source",
                &format!("{}    verification_identity: \"sutura_anchor_reader\"\n", wif()),
            ),
            timeout(),
            None,
        )
        .map(|_| ())
        .expect_err("an impersonating posture with no exchanging broker must not answer");
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(
            error.contains("does not attach a broker"),
            "the refusal must say the composition is the gap, not the adapter: {error}"
        );
        assert!(
            error.contains("no fallback"),
            "the refusal must say there is no fallback: {error}"
        );
        assert!(
            !error.contains("credential_file"),
            "the posture is refused before the credential file is read: {error}"
        );
    }
}

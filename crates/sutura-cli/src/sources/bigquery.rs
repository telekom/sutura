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

#[cfg(feature = "bigquery")]
use sutura_app::Warehouses;
#[cfg(feature = "bigquery")]
use sutura_app::preflight::Verdict;
use sutura_domain::model::SourceName;
#[cfg(feature = "bigquery")]
use sutura_domain::pinned::PinnedDefinitions;
#[cfg(feature = "bigquery")]
use sutura_domain::warehouse::Warehouse;

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
    //
    // **An exhaustive MATCH and not an `==`, which is a review correction rather than a rewrite.**
    // `deliverable_by` above PASSES an impersonating entry - this adapter declares
    // `PerSubjectCredential` - so this is the composition's whole refusal, and it was the one place in
    // this file where *prefer unrepresentable to checked* was not applied. Two variants exist today,
    // so the `==` was complete; a third would have fallen through it and been OPENED, where
    // `deliverable_by`'s own two exhaustive matches force somebody to answer for it. Now a third
    // posture is a compile error at this line, in both composition roots.
    //
    // **And this guard is doing DOUBLE DUTY for a check this root does not have.**
    // `sutura-serve` runs `refuse_unverifiable_anchors` - a bundle declaring an anchor on a source
    // with no identity to re-run it under does not boot - and `sutura-cli` has no equivalent. That is
    // vacuous today only BECAUSE of the arm below: `AnchorIdentity::NoneDeclared` is reachable only
    // for an `impersonation-at-source` source, and this refuses every one of those before an anchor
    // is looked at. So the day a broker is attached here, that check has to arrive with it.
    match *identity.posture() {
        sutura_domain::source::SourcePosture::SharedServiceUser { .. } => {}
        sutura_domain::source::SourcePosture::ImpersonationAtSource => {
            return Err(format!(
                "`sources.{source}` is `impersonation-at-source`, and the `sutura` command does not \
                 attach a broker that exchanges a subject's credential - refusing rather than reading \
                 every row as this process; no fallback"
            ));
        }
    }
    // `within_request_timeout` and NOT `parse`: an answer makes `QueryDeadline::CALLS_PER_ANSWER`
    // calls and each pays a connect margin, so the arithmetic lives in the adapter next to the
    // constant it depends on and a composition root asks for the SHARE. This command has no listener
    // whose timeout a job could outlive, and it reads `server.request_timeout_seconds` anyway: that
    // key is the one place a deployment says how long a question may take, and a second number
    // invented here would be the duplicate that drifts.
    //
    // **THE LIMIT, and it is a number rather than a caveat.** `CALLS_PER_ANSWER` is 2 and the connect
    // margin is 5s, so the shipped default of 30 gives a job **10 seconds**, and
    // `RequestTimeout::MAX_SECONDS` (300) caps it at **145** - against a `QueryDeadline::MAX_SECONDS`
    // of six hours. So on THIS binary that key bounds nothing that exists and imposes a ceiling
    // designed to protect an HTTP connection the command does not have: a twelve-second question is
    // cancelled by `jobTimeoutMs` with nobody waiting on any request, and no value of the key buys
    // more than 145 seconds. `QueryDeadline::parse` is the adapter's own door for "a deployment
    // stating a budget outright" and is deliberately NOT used here, because a second key on this
    // binary alone is the duplicate this comment's first half refuses. What would change it is a
    // settings key that means *how long a QUESTION may take* rather than how long a REQUEST may -
    // one number both roots could read - and that is a settings decision rather than this file's.
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
/// "this binary links no `BigQuery` adapter" sent them to the first when they wanted the second.
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

/// Refuses a bundle naming a table the dataset behind it does not hold.
///
/// **Issue 120's asymmetry, at the OTHER serving composition root.** `sutura-serve`'s
/// `boot::refuse_absent_tables` closed it for the HTTP surface; `sutura mcp` was left with it. The
/// agent surface serves for as long as its peer keeps the pipe open, and the `BigQuery` arm attaches
/// nothing - so [`crate::sources::refuse_unattached`] is skipped and, before this, nothing asked the
/// dataset anything. A mistyped `table:` bought a process that started, announced its capabilities,
/// and handed an agent a failure the first time it asked that metric.
///
/// **`crate::commands::query` deliberately does not call this**, and the difference is who finds
/// out. That command answers one question on a terminal and exits, so a table that is not there is a
/// failure the person who typed the command reads immediately - the outcome this check exists to
/// produce, already produced, at no metadata read. What issue 120 is about is a SERVING process,
/// where the operator learns from whoever asked.
///
/// **The DECISION is `sutura_app::preflight::ask` and is not duplicated**, which is a review
/// correction to what the first version of this file claimed. It argued a copy on
/// [`crate::sources::refuse_unattached`]'s precedent - two roots, separate binaries, neither may
/// depend on the other - and that argument is sound for the operator sentence and reaches no
/// further: review measured the two helpers underneath as byte-identical (`unmatched`) and identical
/// modulo signature wrapping (`models_by_table`), and neither holds a word an operator reads.
/// `sutura-app` is where both roots already get `Warehouses`, so a shared home points inward and
/// adds no root-to-root edge. What is genuinely this root's is below: the words, and the sink.
///
/// **Three sinks changed relative to `sutura-serve`'s copy, not one**, which is the other half of
/// that correction. Serve emits `warn!` for the soft outcome and `info!` for the two clean ones; all
/// three are standard error here, and that is decided rather than inherited. A locally launched
/// process installs no subscriber - [`crate::mcp`]'s own module doc states that for the audit sink -
/// so `tracing` would drop every one of the three for the whole session, and a check whose outcome
/// is invisible is a silent pass. Standard error rather than standard output because on this
/// transport standard output is the protocol channel; unconditional rather than filtered because the
/// capability notice beside it already is, and a surface granting every capability to whoever can
/// reach it is not one to be quiet about. **And it is assertable now rather than review-held:**
/// [`absent_tables_notices`] returns the lines instead of printing them, so this file's own suite
/// pins that the soft outcome is emitted and what it says.
///
/// **Two limits, the same two `sutura-serve` states**, and the second is a review correction too -
/// the first version of this file swapped it for a sentence about the catalog changing while the
/// surface serves, which is a window nothing re-reads and therefore nothing interesting. What a
/// pre-flight establishes is that a table EXISTS: not that the model's columns are on it, and not
/// that a question's identity may read it - an anchor is what covers both, for the metrics that have
/// one. And it reads the bundle loaded FIRST, so a model added to the catalog directory between this
/// root's two loads is caught on a `files` source by [`crate::sources::refuse_unattached`] and is not
/// caught here. That window is open in this root exactly as it is in serve: [`crate::mcp`]'s
/// `catalog.load()` is load one and this check reads it, `LocalService::start` inside `mcp_service`
/// loads a second time, and `refuse_unattached` closes the gap for the `Files` arm only. Closing it
/// here is an architecture decision rather than a call-site move - `sutura_app::surface::LocalService`
/// exposes no accessor for the engines it was given, so there is nothing to re-ask after the second
/// load.
///
/// # Errors
///
/// A dataset that REFUSED the listing - the identity may not ask - and a bundle naming a table the
/// dataset does not hold. A dataset that could not be asked for any other reason is a standard-error
/// line and not a refusal, because a process whose data system is briefly unreachable at startup
/// still has to be able to serve when it comes back. `Warehouse::preflight_was_refused` is what
/// splits the two, and the port documents why the split is the adapter's to make.
#[cfg(feature = "bigquery")]
pub(crate) fn refuse_absent_tables<W>(pinned: &PinnedDefinitions, engines: &Warehouses<W>) -> Result<(), String>
where
    W: Warehouse,
{
    for notice in absent_tables_notices(pinned, engines)? {
        eprintln!("sutura: {notice}");
    }
    Ok(())
}

/// The pre-flight's outcome as the lines this root would print, or the refusal it would make.
///
/// **Split out from [`refuse_absent_tables`] so the soft outcome is a value rather than a side
/// effect**, which is review's finding and the reason it is not simply a comment: the argument for
/// standard error is *a check whose soft outcome is invisible is a silent pass*, and while that line
/// was emitted from inside the decision no test could see it - so the mechanism the argument rests
/// on was held by review, which `AGENTS.md` does not accept as held.
///
/// One line per data system that was asked, in the registry's order. An empty vector is possible and
/// means the bundle names no model in anything this process opened, which
/// [`crate::sources::open_engine`] already refuses upstream - so it is the honest empty answer rather
/// than a case wanting a message of its own.
///
/// # Errors
///
/// [`refuse_absent_tables`]'s two, unchanged: this is the same decision with the printing lifted out.
#[cfg(feature = "bigquery")]
fn absent_tables_notices<W>(pinned: &PinnedDefinitions, engines: &Warehouses<W>) -> Result<Vec<String>, String>
where
    W: Warehouse,
{
    // `Warehouses` here always holds exactly ONE engine - `crate::sources::open_engine` refuses a
    // deployment that declares none and refuses one that declares many - so this loop runs once. It
    // is a loop anyway because `sutura_app::preflight::ask` answers per data system for both roots,
    // and a root that indexed `[0]` would be asserting the single-source shape at the wrong layer.
    let mut notices: Vec<String> = Vec::new();
    for asked in sutura_app::preflight::ask(pinned, engines) {
        let source = asked.source();
        match asked.into_verdict() {
            // An authorization failure is a REFUSAL: the fix is one grant, it will fail identically
            // on every launch, and a soft line is what the person running an agent client never
            // reads.
            Verdict::Refused { cause } => {
                return Err(format!(
                    "{source} refused to list the tables the catalog names, so this process cannot \
                     tell a mistyped `table:` from a table that is there. Grant the identity this \
                     source is opened with `bigquery.tables.list` on the dataset. The data system \
                     said: {}",
                    render(&cause)
                ));
            }
            Verdict::Absent(absent) => {
                return Err(format!(
                    "{source} does not hold {absent}. Refusing to serve a model whose questions \
                     would fail at query time - fix the catalog's `table:`, or create the table"
                ));
            }
            // Everything else that failed - an endpoint that did not answer, a dataset that is not
            // there - is the soft outcome, because a process whose data system is briefly
            // unreachable still has to be able to serve when it comes back.
            Verdict::Unverified { asked: tables, cause } => notices.push(format!(
                "could not verify that {source} holds the {tables} table(s) the catalog names - \
                 serving anyway, so a mistyped table name will fail the first question against it. \
                 The data system said: {}",
                render(&cause)
            )),
            Verdict::Present { asked: tables } => {
                notices.push(format!("every one of the {tables} table(s) the catalog names is in {source}"));
            }
            // **`NotReported` gets a line of its own** for the reason `sutura-serve`'s copy does: it
            // is the one outcome meaning *nothing verified this*, and silence makes it
            // indistinguishable from a verified dataset. Unreachable through the `BigQuery` arm,
            // which always asks, and reachable by any future adapter taking the port's default.
            Verdict::NotReported { asked: tables } => notices.push(format!(
                "{source} does not report which tables it holds, so nothing here verified the \
                 {tables} table(s) the catalog names"
            )),
        }
    }
    Ok(notices)
}

#[cfg(test)]
mod tests {
    // The fixtures live one module up so the dataset-argument refusal - which is `sources.rs`'s,
    // not this file's - can be tested beside the arm that makes it. One entry builder, two suites.
    #[cfg(feature = "bigquery")]
    use crate::sources::wif;
    use crate::sources::{bundle_naming, declaring_bigquery, open_engine, runtime, timeout};

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
            runtime(),
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
            runtime(),
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
            runtime(),
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

    /// [`super::refuse_absent_tables`]'s own suite.
    ///
    /// **A nested module rather than a `cfg` on each item, and neither obvious form works** - the
    /// same shape `sutura-serve`'s `boot::tests` had to invent. With the feature off there is no
    /// `refuse_absent_tables` to call, so an ungated module fails to compile on the DEFAULT feature
    /// set; writing `#[cfg(all(test, feature = "bigquery"))]` on the parent is what a reader reaches
    /// for and makes every test in here a lint error, because `clippy::tests_outside_test_module` and
    /// `clippy::expect_used` both key on the literal `#[cfg(test)]` attribute.
    #[cfg(feature = "bigquery")]
    mod preflight {
        use core::cell::RefCell;
        use std::collections::BTreeSet;

        use sutura_app::Warehouses;
        use sutura_domain::identity::Presented;
        use sutura_domain::model::{QualifiedTable, SourceName};
        use sutura_domain::pinned::PinnedDefinitions;
        use sutura_domain::plan::{AnchorPlan, Executable};
        use sutura_domain::source::{ImpersonationCapability, SourcePosture};
        use sutura_domain::warehouse::preflight::TablesPresent;
        use sutura_domain::warehouse::{AnchorRows, RowSet, Warehouse};

        use crate::sources::bigquery::{absent_tables_notices, refuse_absent_tables};

        /// A data system that answers the pre-flight from what a test handed it, and records the asking.
        ///
        /// **A fake above the port rather than a fake transport**, which is what makes every outcome
        /// reachable here: `sutura_exec_bigquery::wire::BigQueryWire`'s host is a `const` and its
        /// agent is `https_only`, so no test in this repository can point a real one at a loopback.
        /// What is under test is this composition root's DECISION about each answer, and a `Warehouse`
        /// fake is exactly what exercises that.
        struct Answers {
            source: SourceName,
            answer: Answering,
            asked: RefCell<Vec<usize>>,
            /// What this fake says about its own failure - the `preflight_was_refused` half.
            ///
            /// A field rather than a second fake type, because what this root has to get right is
            /// that it ASKS: two fakes whose `Err` is the same value and which answer this
            /// differently is the only shape that shows the delegation happening.
            refused: bool,
        }

        /// What a test hands the fake to answer a pre-flight with.
        ///
        /// A named function pointer, because the spelled-out type is over the `type_complexity`
        /// threshold this workspace tightened.
        type Answering = fn(&BTreeSet<QualifiedTable>) -> Result<TablesPresent, CouldNotAsk>;

        /// The one failure this fake can report: the dataset could not be asked.
        ///
        /// Written by hand rather than derived, because `sutura-cli` declares no `thiserror`
        /// dependency and `unused-deps` would be the next thing to complain if it did.
        #[derive(Debug)]
        struct CouldNotAsk;

        impl core::fmt::Display for CouldNotAsk {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("the dataset could not be listed")
            }
        }

        impl core::error::Error for CouldNotAsk {}

        impl Warehouse for Answers {
            type Error = CouldNotAsk;

            const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

            fn source(&self) -> &SourceName {
                &self.source
            }

            fn posture(&self) -> &SourcePosture {
                &SourcePosture::ImpersonationAtSource
            }

            fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
                Err(CouldNotAsk)
            }

            fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
                Err(CouldNotAsk)
            }

            fn preflight(&self, tables: &BTreeSet<QualifiedTable>) -> Result<TablesPresent, Self::Error> {
                self.asked.borrow_mut().push(tables.len());
                (self.answer)(tables)
            }

            fn preflight_was_refused(&self, _error: &Self::Error) -> bool {
                self.refused
            }
        }

        /// One open registry over one fake, under the alias the test bundle's models declare.
        fn opened(answer: Answering) -> Warehouses<Answers> {
            registry(answer, false)
        }

        /// The same, over a dataset that says its failure was a REFUSAL rather than an outage.
        fn refusing(answer: Answering) -> Warehouses<Answers> {
            registry(answer, true)
        }

        fn registry(answer: Answering, refused: bool) -> Warehouses<Answers> {
            Warehouses::of(Answers {
                source: SourceName::parse("warehouse").expect("a test source is a source"),
                answer,
                asked: RefCell::new(Vec::new()),
                refused,
            })
        }

        /// Two models on one source, one of which the dataset will be said not to hold.
        fn bundle() -> PinnedDefinitions {
            crate::sources::bundle_over(&[
                ("customers", "warehouse", "dim_customer"),
                ("orders", "warehouse", "fct_orders"),
            ])
        }

        /// TWO models over ONE table, which is what the refusal naming a set of models is for.
        fn two_models_one_table() -> PinnedDefinitions {
            crate::sources::bundle_over(&[("orders", "warehouse", "fct_orders"), ("returns", "warehouse", "fct_orders")])
        }

        #[test]
        fn a_bigquery_bundle_naming_a_table_the_dataset_does_not_hold_does_not_serve() {
            // **The parity issue 120 is about, at the surface it was still missing from.** `sutura
            // mcp` over a `files` source refuses this state through `refuse_unattached`, because the
            // engine is GIVEN a file per model. Over a dataset it started, printed its capability
            // notice, and handed the peer a failure the first time it asked that metric.
            let engines = opened(|asked| {
                Ok(TablesPresent::of(
                    asked
                        .iter()
                        .filter(|table| table.name().as_str() == "fct_orders")
                        .cloned()
                        .collect(),
                ))
            });
            let error = refuse_absent_tables(&bundle(), &engines).expect_err("a table that is not there stops the process");
            assert!(error.contains("fct_orders"), "the refusal must name the table: {error}");
            // **The whole clause and not the bare model name, which is a review correction to an
            // assertion that could not fail.** `"orders"` is a substring of `"fct_orders"`, so
            // `contains("orders")` was entailed by the line above and stayed green even with the model
            // set dropped entirely - coverage for the one property nothing checked.
            assert!(
                error.contains("named by model(s) [orders]"),
                "the refusal must name the model: {error}"
            );
            assert!(
                !error.contains("dim_customer"),
                "the refusal must not name a table the dataset holds: {error}"
            );
        }

        #[test]
        fn two_models_over_one_absent_table_are_both_named_in_the_refusal() {
            // **The reason the refusal names a SET of models**, and nothing exercised it before: a
            // bundle may declare several models over one fact table, and an operator told about only
            // one of them has half the edit in front of them. One clause for the table, both names
            // inside it, in reading order.
            let engines = opened(|asked| Ok(TablesPresent::of(asked.clone())));
            let error =
                refuse_absent_tables(&two_models_one_table(), &engines).expect_err("a table that is not there stops the process");
            assert!(
                error.contains("table fct_orders, named by model(s) [orders, returns]"),
                "both models that named the absent table must be in the refusal: {error}"
            );
        }

        #[test]
        fn the_whole_bundle_is_one_question_and_not_one_per_model() {
            // The control, and the cost argument the port is shaped around: two models behind two
            // tables are ONE call carrying both, so a bundle of forty costs one metadata read rather
            // than forty. This asserts it rather than claiming it.
            let engines = opened(|_asked| Ok(TablesPresent::All));
            refuse_absent_tables(&bundle(), &engines).expect("a bundle whose tables are all there serves");
            let engine = engines
                .get(&SourceName::parse("warehouse").expect("a test source is a source"))
                .expect("the fake is registered under that alias");
            assert_eq!(
                *engine.asked.borrow(),
                vec![2],
                "one call carrying both tables, not one call per model"
            );
        }

        #[test]
        fn a_dataset_that_could_not_be_reached_still_serves() {
            // The soft edge: a dataset that did not ANSWER is a condition that passes, so the surface
            // serves. Its twin below is what makes that defensible, and the test after this one is
            // what makes the *serving anyway* visible rather than merely argued.
            let engines = opened(|_asked| Err(CouldNotAsk));
            refuse_absent_tables(&bundle(), &engines)
                .expect("an endpoint that did not answer at startup is a process that still has to serve");
        }

        #[test]
        fn the_soft_outcome_is_emitted_and_says_what_went_unverified() {
            // **The mechanism the choice of sink rests on, held by a test rather than by review.**
            // The argument for standard error is *a check whose soft outcome is invisible is a silent
            // pass* - and while the line was printed from inside the decision, nothing could assert
            // it was printed at all. It is a value now: one line, naming the source, how many tables
            // went unverified, that the process is serving anyway, and the data system's own reason.
            let engines = opened(|_asked| Err(CouldNotAsk));
            let notices = absent_tables_notices(&bundle(), &engines).expect("an outage is not a refusal");
            assert_eq!(notices.len(), 1, "one line for the one data system asked: {notices:?}");
            let notice = &notices[0];
            assert!(notice.contains("could not verify"), "{notice}");
            assert!(notice.contains("warehouse"), "the line must name the source: {notice}");
            assert!(
                notice.contains("2 table(s)"),
                "the line must say how many went unverified: {notice}"
            );
            assert!(
                notice.contains("serving anyway"),
                "the line must say the process is serving: {notice}"
            );
            assert!(
                notice.contains("the dataset could not be listed"),
                "the data system's own reason must survive into the line: {notice}"
            );
        }

        #[test]
        fn a_verified_dataset_and_one_that_did_not_report_are_two_different_lines() {
            // The other two sinks, and why they are not one: `Present` is *this was checked* and
            // `NotReported` is *nothing checked this*. A root that printed the same words for both -
            // or nothing for the second - would make the port's default read as verification, which
            // is the property `TablesPresent` exists to keep. Serve says these at `info!`; here they
            // are standard error, decided rather than inherited, because this process installs no
            // subscriber.
            let verified = opened(|_asked| Ok(TablesPresent::All));
            let said = absent_tables_notices(&bundle(), &verified).expect("everything is there");
            assert_eq!(
                said,
                vec![String::from("every one of the 2 table(s) the catalog names is in warehouse")],
                "the verified line says the check happened and passed"
            );

            let silent = opened(|_asked| Ok(TablesPresent::NotAsked));
            let said = absent_tables_notices(&bundle(), &silent).expect("an adapter that did not look refuses nothing");
            assert_eq!(said.len(), 1, "{said:?}");
            assert!(
                said[0].contains("nothing here verified"),
                "the default must not read as verification: {}",
                said[0]
            );
        }

        #[test]
        fn a_credential_that_cannot_list_the_dataset_is_a_different_refusal_from_a_missing_table() {
            // **The split, and the two tests either side of it are what make it mean something:** the
            // same `Err` value, the same variant, and the only difference is what the dataset says
            // about its own failure. A refusal to LIST is one grant and fails identically on every
            // launch, so a soft line would hide the check being off - and the message an operator gets
            // names the grant rather than a table, because the two fixes are in different places.
            let engines = refusing(|_asked| Err(CouldNotAsk));
            let error = refuse_absent_tables(&bundle(), &engines)
                .expect_err("a dataset that refuses to be asked is a process that cannot verify itself");
            assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
            assert!(
                error.contains("bigquery.tables.list"),
                "the refusal must name the grant an operator has to add: {error}"
            );
            assert!(
                !error.contains("dim_customer") && !error.contains("fct_orders"),
                "a listing that never happened must not be reported as a table that is absent: {error}"
            );
        }

        #[test]
        fn an_adapter_that_reports_nothing_still_serves() {
            // Any adapter written before this port existed: `NotAsked` is not a claim that anything
            // was verified, and it is not a refusal either.
            //
            // **It does not exercise the port's DEFAULT**, which is worth saying because the name
            // invites the reading: this fake overrides `preflight` and returns `Ok(NotAsked)` by hand.
            // What exercises the real default is `sutura_domain::warehouse::preflight`'s own test,
            // whose fake omits the method entirely.
            let engines = opened(|_asked| Ok(TablesPresent::NotAsked));
            refuse_absent_tables(&bundle(), &engines).expect("an adapter that did not look refuses nothing");
        }
    }
}

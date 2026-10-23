//! The record `LocalService` writes for a delegated call: answered, refused or failed, it names the
//! principal each delegated leg was minted as, masked.

use sutura_domain::audit::{AuditSink, CallRecord, RecordedOutcome};
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::identity::{CredentialBroker, LegCredentials, Minted, Presented, PrincipalName, RequestContext, SourceSet};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{Grain, SourceName};
use sutura_domain::pinned::{CatalogKind, Contribution, ContributionManifest, PinnedDefinitions, SemanticCatalog};
use sutura_domain::plan::Executable;
use sutura_domain::query::Query;
use sutura_domain::raw::RawStatement;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, RawExecution, RawRows, ResultBatches, Value, Warehouse};

use super::{asked_by_a_person, bundle, certified, june, metric, source, test_deadline};
use crate::Warehouses;
use crate::surface::{LocalService, Surface as _, SurfaceFailure};
use crate::tests_support::{AdapterFailure, BrokerUnreachable, DriverFailure, canned};

/// Presents every source the address its deployment vouches for: `domain_delegation`'s shape.
struct Delegating;

impl CredentialBroker for Delegating {
    type Error = BrokerUnreachable;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let name = PrincipalName::parse("Analyst.One@Example.com").map_err(|_unparsable| BrokerUnreachable)?;
        let presented = sources
            .iter()
            .map(|at| (at.clone(), Presented::SubjectPrincipal { name: name.clone() }))
            .collect();
        let credentials = LegCredentials::minted(
            context.chain().subject().clone(),
            sutura_domain::identity::Expiry::NothingExpires,
            sources,
            presented,
        )
        .map_err(|_uncovered| BrokerUnreachable)?;
        Ok(Minted::Granted { credentials })
    }
}

/// An impersonating data system that answers every statement, or fails every one at the source.
struct Impersonating {
    source: SourceName,
    answers: bool,
}

impl Warehouse for Impersonating {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;
    const ACCEPTS_RAW_STATEMENTS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &SourcePosture::ImpersonationAtSource
    }

    fn dry_run(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        if self.answers {
            Ok(canned(&certified()))
        } else {
            Err(AdapterFailure::Statement { cause: DriverFailure })
        }
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(certified()))
    }

    /// Answers one row, or declines every statement - the one raw failure that is not a refusal.
    fn execute_raw(&self, _statement: &RawStatement, _presented: &Presented, _deadline: Deadline) -> RawExecution<Self::Error> {
        self.answers
            .then(|| Ok(RawRows::of(vec![String::from("n")], vec![vec![Value::Integer(1)]])))
    }
}

/// Each record's message and its delegated legs, as `source=principal`.
struct Recording(std::sync::mpsc::Sender<(&'static str, String)>);

impl AuditSink for Recording {
    fn record(&self, record: &CallRecord<'_>) {
        let message = match *record.outcome() {
            RecordedOutcome::Answered { .. } => "answered",
            RecordedOutcome::Refused { .. } => "refused",
            RecordedOutcome::RawAnswered { .. } => "raw_answered",
            RecordedOutcome::RawRefused { .. } => "raw_refused",
            RecordedOutcome::Failed { failure } | RecordedOutcome::RawFailed { failure, .. } => failure,
        };
        let delegated = record.delegated_as().map_or_else(String::new, |delegated| {
            delegated
                .legs()
                .map(|(at, principal)| format!("{at}={principal}"))
                .collect::<Vec<_>>()
                .join(",")
        });
        drop(self.0.send((message, delegated)));
    }
}

/// [`bundle`], from a catalog that declares everything it holds, so it composes.
struct Declared(PinnedDefinitions);

impl Declared {
    fn bundle() -> Self {
        let pinned = bundle();
        let produced = MetadataCapabilities::produced(pinned.definitions(), &Knowledge::none());
        Self(
            PinnedDefinitions::pin(
                pinned.version().clone(),
                pinned.definitions().clone(),
                Knowledge::none(),
                ContributionManifest::single(source(), Contribution::of(produced)),
            )
            .expect("the test definitions hash"),
        )
    }
}

impl SemanticCatalog for Declared {
    type Error = core::convert::Infallible;

    const KIND: CatalogKind = CatalogKind::Golden;

    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::everything()
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        Ok(self.0.clone())
    }
}

type Service = LocalService<Impersonating, Recording, Delegating, sutura_domain::plan::RefusingCombiner>;

/// Each record the service wrote, as [`Recording`] sends it.
type Records = std::sync::mpsc::Receiver<(&'static str, String)>;

fn served(answers: bool) -> (Service, Records) {
    let (sender, records) = std::sync::mpsc::channel();
    let service = LocalService::start(
        &Declared::bundle(),
        Warehouses::of(Impersonating {
            source: source(),
            answers,
        }),
        Recording(sender),
        Delegating,
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the anchor holds against the fake");
    (service, records)
}

const DELEGATED: &str = "local=A***.O***@E***.c***";

fn question() -> Query {
    Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new())
}

#[test]
fn a_delegated_answer_is_recorded_with_the_masked_principal() {
    let (service, records) = served(true);
    drop(
        service
            .answer(&asked_by_a_person(), &question(), test_deadline())
            .expect("the fake answers"),
    );
    assert_eq!(
        records.try_iter().collect::<Vec<_>>(),
        [("answered", String::from(DELEGATED))]
    );
}

#[test]
fn a_delegated_call_that_fails_at_the_source_is_recorded_as_failed() {
    let (service, records) = served(false);
    let failure = service
        .answer(&asked_by_a_person(), &question(), test_deadline())
        .expect_err("the source fails");
    assert!(matches!(failure, SurfaceFailure::Warehouse { .. }), "{failure:?}");
    assert_eq!(
        records.try_iter().collect::<Vec<_>>(),
        [("warehouse", String::from(DELEGATED))]
    );
}

#[test]
fn a_delegated_raw_call_is_recorded_whether_it_ends_or_fails() {
    let statement = RawStatement::parse("select 1").expect("a test statement is a statement");
    let (answering, answered) = served(true);
    drop(
        answering
            .run_sql(&asked_by_a_person(), &statement, test_deadline())
            .expect("the fake answers"),
    );
    assert_eq!(
        answered.try_iter().collect::<Vec<_>>(),
        [("raw_answered", String::from(DELEGATED))]
    );
    let (declining, failed) = served(false);
    drop(
        declining
            .run_sql(&asked_by_a_person(), &statement, test_deadline())
            .expect_err("a source that declines every statement is a wiring failure"),
    );
    assert_eq!(failed.try_iter().collect::<Vec<_>>(), [("miswired", String::from(DELEGATED))]);
}

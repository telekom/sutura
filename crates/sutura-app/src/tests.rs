//! This crate's own unit suite: the anchor pass, the query path, and the credential outcomes.
//!
//! Split out of `lib.rs` for the reason [`crate::tests_support`] is - the file reached the 1000-line
//! gate, and the fakes it uses live there.

/// The port's deadline, at this call site: `docs/adr/0029`'s two RED cells.
///
/// Its own module for the reason this file's own header gives - `tests.rs` was at the `max-lines`
/// cap this record needed to add two cells past - and not `#[cfg(test)]`, because this whole file
/// is already only ever compiled under `cfg(test)`.
mod deadline;

/// The counting fake for the two `docs/adr/0008` cells below. `#[cfg(test)]` for the reason
/// `tests_support.rs`'s `mod priced` carries it (`telekom/sutura#657`).
#[cfg(test)]
mod dry_run_counter;

use std::collections::BTreeSet;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Anchor, AnchorValue, Audience, Definitions, Description, Metric, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{
    AnchorCheck, Contribution, ContributionManifest, DefinitionVersion, NotExecutedReason, NotValidated, PinnedDefinitions,
};
use sutura_domain::plan::MAX_ROWS;
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::{RowSet, Value};

use sutura_domain::identity::{
    CredentialsDoNotCoverThePlan, CredentialsDoNotFitTheRequest, Expiry, PresentedDisagreesWithPosture, PrincipalChain,
    RequestContext, Subject,
};

use super::tests_support::{
    AdapterFailure, CountingBroker, DryRunOutcome, FixedBroker, FixedWarehouse, MonoPreflightWarehouse, RefusingSourceWarehouse,
    TransientlyBrokenWarehouse,
};
use super::{ServiceError, SpendLedger, Warehouses, exceeds_row_cap, verify_anchors, verify_and_validate};
use dry_run_counter::DryRunCountingWarehouse;

/// Bare `answer`, pinned to a 1 GiB working set, no spend ceiling - not imported as `super::answer`.
///
/// **Over `RefusingCombiner`, and that is a statement about these cells rather than a shortcut.**
/// Every question in this module is single-source, so the combiner is never reached; a fake that
/// answered would let a cell here pass over a federated answer nobody wrote. The two-source path's
/// own cells are in `crate::federated`, over the real `DataFusionCombiner`.
fn answer<W, B>(
    definitions: &super::Validated<PinnedDefinitions>,
    query: &sutura_domain::query::Query,
    context: &RequestContext,
    broker: &B,
    warehouses: &Warehouses<W>,
) -> super::Answering<W, B, sutura_domain::plan::RefusingCombiner>
where
    W: sutura_domain::warehouse::Warehouse + Sync,
    W::Error: Send,
    B: sutura_domain::identity::CredentialBroker,
    B::Error: Send,
{
    super::answer(
        definitions,
        query,
        context,
        broker,
        warehouses,
        &sutura_domain::plan::RefusingCombiner,
        1 << 30,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
}

/// A generous deadline for every unit answer in this crate's own tests that is not about the
/// deadline itself - shared by `crate::federated`'s test module too, so there is one fixture rather
/// than two copies of the same thirty seconds.
pub(crate) fn test_deadline() -> sutura_domain::warehouse::deadline::Deadline {
    sutura_domain::warehouse::deadline::Deadline::opened_at(
        std::time::Instant::now(),
        sutura_domain::warehouse::deadline::Budget::parse(std::time::Duration::from_secs(30))
            .expect("thirty seconds is a budget"),
    )
}

/// The context a question arrives with, naming a person the transport verified.
///
/// A verified subject rather than the deployment, because it is the case the whole port exists
/// for and because it must NOT change what a shared leg does: a deployment whose sources are all
/// shared answers a named caller exactly as it answered before, which is the behaviour the
/// `a_posture_is_recorded_in_provenance_per_leg` test above still asserts.
pub(crate) fn asked_by_a_person() -> RequestContext {
    RequestContext::of(PrincipalChain::of(
        Subject::verified("someone@example.com").expect("a test subject is a subject"),
    ))
}

pub(crate) fn metric() -> MetricName {
    MetricName::parse("revenue").expect("a test metric name is a name")
}

pub(crate) fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

/// The posture every fake warehouse here is opened with.
///
/// **One value rather than a sentence per call site, and `answer` is why.** The leg a broker mints is
/// now compared against the posture the adapter was opened with, witness included - so a test that
/// wrote its own acknowledgement prose would provoke that wiring defect rather than whatever it was
/// about. `tests_support` owns the one witness both halves read.
pub(crate) fn shared() -> SourcePosture {
    super::tests_support::shared_posture()
}

/// The June range the test bundle's anchor declares, which is also the only range a question
/// against it can ask for and get one row back.
pub(crate) fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range")
}

/// The certified number, in the shape the anchor check and a question both read it.
pub(crate) fn certified() -> RowSet {
    RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("one column and one cell is rectangular")
}

/// A one-metric bundle whose metric declares an anchor, so there is exactly one check to make.
pub(crate) fn bundle() -> PinnedDefinitions {
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date")]),
        Description::default(),
    );
    let range = june();
    let revenue = Metric::new(
        metric(),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        Some(Anchor::new(
            range,
            AnchorValue::parse("197122").expect("a test anchor value is a value"),
        )),
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
    // The real hasher, from the catalog adapter that owns the canonical form. `pin` applies it to
    // the definitions being pinned, so there is no digest here for the bundle not to describe.
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}

#[test]
fn a_failed_anchor_check_keeps_the_adapters_own_cause() {
    // THE BUG THIS EXISTS FOR. The failure used to be recorded as one formatted sentence, and
    // `Display` on a `thiserror` enum prints only the outermost message - so the driver's own
    // complaint, the half that names a table, a column or a file, was gone before the report was
    // built. Anchor verification is the readiness gate, so that sentence was the whole of what an
    // operator got when a deployment refused to serve.
    //
    // Asserted over the chain rather than over the message alone: the message was never the part
    // that went missing.
    let pinned = bundle();
    let report = verify_anchors(&pinned, &Warehouses::of(FixedWarehouse::new(source(), shared())));
    let check = report.checks().get(&metric()).expect("the anchored metric was checked");
    let AnchorCheck::NotExecuted {
        reason: NotExecutedReason::Failed { ref message, ref chain },
    } = *check
    else {
        panic!("a data system that fails every statement is a failed check, not {check:?}");
    };
    assert_eq!(message, "the data system rejected the statement");
    assert_eq!(chain, &vec![String::from("no such file: orders.csv")]);
}

#[test]
fn a_check_for_a_source_nobody_configured_says_so_rather_than_looking_like_an_outage() {
    // It was prose in a report field, and it is a governance condition: the plan names a data
    // system this process did not open. Typed, an operator can tell it apart from an outage
    // without reading a sentence, which is the difference that decides who gets paged.
    //
    // The registry is what sharpened it. The check used to COMPARE the plan's source against the
    // one warehouse it was handed, so this arm also fired for a bundle whose second source was
    // configured and open - the plan now SELECTS, so the only failure left is an unconfigured
    // name.
    let pinned = bundle();
    let elsewhere = Warehouses::of(FixedWarehouse::new(
        SourceName::parse("somewhere_else").expect("a test source is a source"),
        shared(),
    ));
    let report = verify_anchors(&pinned, &elsewhere);
    let check = report.checks().get(&metric()).expect("the anchored metric was checked");
    let AnchorCheck::NotExecuted {
        reason: NotExecutedReason::SourceNotConfigured { ref plan },
    } = *check
    else {
        panic!("a plan for a source nobody opened is not configured, not {check:?}");
    };
    assert_eq!(plan.as_str(), "local");
}

#[test]
fn an_anchor_runs_against_the_data_system_its_own_metric_names() {
    // What the registry buys the anchor pass, and it is not cosmetic: under one warehouse every
    // anchor on a second configured source came back as a source mismatch, so a two-source bundle
    // could not be validated for a reason that has nothing to do with its numbers.
    let pinned = bundle();
    let registry = Warehouses::of(FixedWarehouse::new(
        SourceName::parse("somewhere_else").expect("a test source is a source"),
        SourcePosture::ImpersonationAtSource,
    ))
    .and(FixedWarehouse::answering(source(), shared(), certified()))
    .expect("two sources");
    let report = verify_anchors(&pinned, &registry);
    let check = report.checks().get(&metric()).expect("the anchored metric was checked");
    assert_eq!(
        *check,
        AnchorCheck::Matched,
        "the metric reads `local`, so the anchor runs on the `local` adapter and not on whichever \
         one happens to be first"
    );
}

#[test]
fn a_posture_is_recorded_in_provenance_per_leg() {
    // The Done-when of the source registry: an answer says which mode produced it, read off the
    // adapter that executed rather than off a settings tree - nothing in this test holds one.
    //
    // **This test used to loop over BOTH postures and assert an answer for each, and a review was
    // right that the second half was blessing an unsafe pairing.** It handed an adapter declared
    // `impersonation-at-source` a leg carrying the deployment's own identity, and got an answer whose
    // provenance said `impersonation-at-source` - a leg that ran as the process, reported as the
    // asker's own access. That pairing is refused now, and the test below is what asserts it.
    //
    // So what is left here is the honest claim: the posture that CAN execute on this build is
    // recorded. The other one is untestable rather than untested - no adapter in this workspace can
    // carry a per-subject credential, so an `impersonation-at-source` leg cannot execute here at all.
    let question = Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let registry = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &registry).expect("the anchor reproduces its number");
    let outcome = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &registry,
    )
    .expect("the fake answers")
    .into_outcome();
    let ToolOutcome::Answer { ref provenance, .. } = outcome else {
        panic!("a certified question is answered, not {outcome:?}");
    };
    assert_eq!(
        provenance.executed_as().posture(&source()).map(SourcePosture::as_str),
        Some(shared().as_str()),
        "the answer records the posture the adapter that executed it was holding"
    );
    assert_eq!(provenance.executed_as().legs().count(), 1, "a mono-source answer has one leg");
    // And the two halves of provenance stay separable: the digest is over authored content, so it
    // does not move when the posture does.
    assert_eq!(provenance.digest(), bundle().digest());
}

#[test]
fn answer_calls_dry_run_even_when_the_preflight_accepts() {
    // `docs/adr/0008`: `answer` must call the source's `dry_run` and must not skip the check because
    // a value is already `Accepted` - the pre-flight's own answer is the data system's opinion at
    // pre-flight time, not an authorization decision.
    //
    // `DryRunCountingWarehouse` with `DryRunOutcome::Accepted` answers `PreFlight::Accepted` from
    // `dry_run`, so the answer proceeds past the pre-flight to `execute` - which is the path that
    // would silently skip `dry_run` if a future change read the `Accepted` as "already checked".
    // The `dry_runs()` counter on the fake proves the call was made; `executions()` proves the
    // answer completed, so the assertion is not passing against a path that refused early.
    let working = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &working).expect("the anchor reproduces its number");
    let warehouse = DryRunCountingWarehouse::new(source(), shared(), certified(), DryRunOutcome::Accepted);
    let warehouses = Warehouses::of(warehouse);
    let question = Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let outcome = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
    )
    .expect("the fake answers")
    .into_outcome();
    assert!(
        matches!(outcome, ToolOutcome::Answer { .. }),
        "the pre-flight accepted, so this answers"
    );
    assert_eq!(
        warehouses.get(&source()).expect("the source is registered").dry_runs(),
        1,
        "answer must call dry_run even when the pre-flight accepts"
    );
    assert_eq!(
        warehouses.get(&source()).expect("the source is registered").executions(),
        1,
        "the answer completed, so the dry-run count is not zero from an early refusal"
    );
}

#[test]
fn a_dry_run_refusal_stops_the_answer_before_execute() {
    // ADR 0008's other half: a dry-run that the source refuses must stop the answer, so `execute`
    // is never reached. The existing `a_source_that_refuses_the_preflight_is_refused_and_never_executed`
    // in `credentials.rs` pins the refusal outcome and zero executions but does not count `dry_runs`;
    // this cell adds the call count, proving the refusal came FROM `dry_run` and not from a skip.
    let working = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &working).expect("the anchor reproduces its number");
    let refusing = DryRunCountingWarehouse::new(source(), shared(), certified(), DryRunOutcome::SourceRefused);
    let warehouses = Warehouses::of(refusing);
    let question = Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let outcome = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
    )
    .expect("a pre-flight refusal is a governed answer")
    .into_outcome();
    let ToolOutcome::Refusal {
        reason: RefusalReason::SourceRefused { .. },
    } = outcome
    else {
        panic!("a dry-run refusal must come back as a SourceRefused refusal, not {outcome:?}");
    };
    assert_eq!(
        warehouses.get(&source()).expect("the source is registered").dry_runs(),
        1,
        "answer called dry_run once, and the refusal came from that call"
    );
    assert_eq!(
        warehouses.get(&source()).expect("the source is registered").executions(),
        0,
        "a dry-run refusal must stop the answer before execute"
    );
}

#[test]
fn a_leg_that_disagrees_with_the_adapters_posture_never_executes() {
    // **The port cannot make an adapter check this, so the application does.** Both shipped adapters
    // compare the leg they were handed against their own declared posture; `Warehouse` is a trait, so
    // an implementor can simply omit it - and this crate's own fake did. A review found what that
    // costs: pairing a `FixedWarehouse` declared `impersonation-at-source` with a broker granting the
    // deployment's own identity ANSWERED, and provenance then reported the leg as
    // `impersonation-at-source` because provenance is read off the adapter's posture. A question
    // answered as the process, recorded as the asker.
    //
    // Made in `answer`, the rule reaches every adapter this registry can hold, including the next one.
    let question = Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let impersonating = Warehouses::of(FixedWarehouse::answering(
        source(),
        SourcePosture::ImpersonationAtSource,
        certified(),
    ));
    let validated = verify_and_validate(bundle(), &impersonating).expect("the anchor reproduces its number");
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &impersonating,
    )
    .expect_err("a leg that is not the posture's shape is a wiring failure, not an answer");
    let ServiceError::Posture {
        cause:
            PresentedDisagreesWithPosture::ShapeIsNotThePosture {
                ref at,
                posture,
                presented,
            },
    } = failure
    else {
        panic!("the leg does not agree with the posture, and the failure says how: {failure:?}");
    };
    assert_eq!(at, &source());
    assert_eq!(posture, "impersonation-at-source");
    assert_eq!(presented, "the deployment's own identity for this source");

    // The second direction, which the shape check cannot see: both say shared, and the witness on the
    // leg is not this source's. The broker's witness is `tests_support`'s one acknowledgement, so a
    // posture built from a different sentence is another source's declaration.
    let elsewheres_witness = Warehouses::of(FixedWarehouse::answering(
        source(),
        SourcePosture::SharedServiceUser {
            declared: SharedIdentityDeclared::of(
                AcknowledgementReason::parse("a different operator's acknowledgement, for a different source")
                    .expect("a test reason is a reason"),
            ),
        },
        certified(),
    ));
    let validated = verify_and_validate(bundle(), &elsewheres_witness).expect("the anchor reproduces its number");
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &elsewheres_witness,
    )
    .expect_err("a shared leg carrying another acknowledgement is a wiring failure");
    assert!(
        matches!(
            failure,
            ServiceError::Posture {
                cause: PresentedDisagreesWithPosture::WitnessIsNotThisSources { .. }
            }
        ),
        "{failure:?}"
    );

    // And the agreeing pairing answers, so neither assertion above is passing against a check that
    // refuses everything - the test above this one is the same registry answering.
    let agreeing = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &agreeing).expect("the anchor reproduces its number");
    assert!(matches!(
        answer(
            &validated,
            &question,
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &agreeing
        )
        .expect("the fake answers")
        .into_outcome(),
        ToolOutcome::Answer { .. }
    ));
}

#[test]
fn a_question_for_a_source_nobody_configured_is_refused_rather_than_run_elsewhere() {
    // The query-path half of the same lookup. `SourceUnavailable` now means what its name says.
    let registry = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &registry).expect("the anchor reproduces its number");
    let elsewhere = Warehouses::of(FixedWarehouse::answering(
        SourceName::parse("somewhere_else").expect("a test source is a source"),
        shared(),
        certified(),
    ));
    let question = Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let outcome = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &elsewhere,
    )
    .expect("a refusal is an Ok")
    .into_outcome();
    let ToolOutcome::Refusal {
        reason: sutura_domain::query::RefusalReason::SourceUnavailable { ref source },
    } = outcome
    else {
        panic!("a plan for a source nobody opened is refused, not {outcome:?}");
    };
    assert_eq!(source.as_str(), "local");
}

/// A servable deployment: a validated bundle, the registry that validated it, and a question.
///
/// Named because the tuple is over this workspace's `type_complexity` threshold.
type Ready = (super::Validated<PinnedDefinitions>, Warehouses<FixedWarehouse>, Query);

/// A bundle, a registry and a question, for the credential tests below.
///
/// One helper because all four differ in exactly one thing - which broker answers - and that is
/// the property each of them is about.
fn ready() -> Ready {
    let registry = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &registry).expect("the anchor reproduces its number");
    (
        validated,
        registry,
        Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new()),
    )
}

/// What a minted credential has to agree with before an adapter sees it - split out for
/// `max-lines` along the seam `ready()`'s own doc named. `#[cfg(test)]` for `not_validated`'s
/// reason below.
#[cfg(test)]
mod credentials;

/// Split out for `max-lines`. `#[cfg(test)]` on the declaration itself is `telekom/sutura#657`'s
/// fix: a bare `mod` line reads as nothing to `xtask test-causality`'s scan, so a later diff would
/// silently orphan this module instead of failing loud; the attribute reads as `TestModule`.
#[cfg(test)]
mod not_validated;

/// `crate::answer` under a REAL, configured `SpendLedger` - the local `answer` wrapper above is
/// pinned to `SpendLedger::no_budget()`. `#[cfg(test)]` for `not_validated`'s reason above.
#[cfg(test)]
mod spend_test;

#[test]
fn the_row_cap_refuses_at_one_row_over_and_cannot_be_lifted_by_a_failed_conversion() {
    // The plan asks a data system for one row MORE than it will certify, so a result carrying
    // more than the cap is a result that was cut short - a wrong total under a certified name.
    // The boundary is the whole control: exactly the cap answers, one row over refuses.
    assert_eq!(MAX_ROWS, 10_000, "the boundary below is written in terms of the cap");
    assert!(!exceeds_row_cap(0, MAX_ROWS), "an empty result is not a truncated one");
    assert!(!exceeds_row_cap(9_999, MAX_ROWS), "under the cap answers");
    assert!(!exceeds_row_cap(10_000, MAX_ROWS), "exactly the cap answers");
    assert!(exceeds_row_cap(10_001, MAX_ROWS), "one row over the cap is a truncated total");
    // THE DIRECTION THIS FUNCTION EXISTS FOR. The comparison used to narrow the CAP to a
    // `usize` with `unwrap_or(usize::MAX)`, so a conversion that failed meant no cap at all and
    // the largest result set there is would have been certified as complete. A count that
    // cannot be carried is a count over every cap, and this asserts it refuses.
    assert!(
        exceeds_row_cap(usize::MAX, MAX_ROWS),
        "the largest count there is exceeds any cap"
    );
    assert!(
        exceeds_row_cap(usize::MAX, u32::MAX),
        "including against the widest cap a plan could carry"
    );
    // And a zero cap is a cap, not an absence: `QueryPlan::new` always sets `MAX_ROWS`, and this
    // function is not allowed to read a small number as permission.
    assert!(exceeds_row_cap(1, 0), "one row over a cap of zero is over the cap");
    assert!(!exceeds_row_cap(0, 0), "no rows is not over a cap of none");
    // The other end, so the comparison is not narrowing by accident: a cap no result could reach
    // admits an ordinary result.
    assert!(!exceeds_row_cap(1, u32::MAX), "one row is not over four billion");
}

#[test]
fn a_refused_question_never_reaches_the_broker() {
    // **`mint` is on the request path with no bound of its own**, so what it costs matters. It is
    // free for the implementor that ships - it reads a settings tree - and one authorization-server
    // round trip per question for the first broker that exchanges a token. This pins the one
    // structural property that keeps that bill honest: a question this deployment declines is
    // declined BEFORE anything is asked of the broker.
    //
    // **The limit, and it is the whole precision of the claim:** what holds is that the refusals
    // decided by COMPILATION and by the source lookup come first - an unknown metric, a grain the
    // metric does not declare, a range over the cap, a dimension outside the allowlist, a plan
    // spanning two sources, a source nobody opened. `ResultTooLarge`, `ResourcesExhausted` and
    // `CredentialUnavailable` are decided after minting and could not be otherwise: the first two
    // need a result, and the third IS the broker's answer.
    let (validated, registry, question) = ready();
    let broker = CountingBroker::default();

    let unknown = Query::single(
        MetricName::parse("no_such_metric").expect("a test metric name is a name"),
        Grain::Month,
        june(),
        Vec::new(),
        Vec::new(),
    );
    let outcome = answer(&validated, &unknown, &asked_by_a_person(), &broker, &registry)
        .expect("a refusal is an Ok")
        .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::MetricUnknown { .. }
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(
        broker.asked(),
        0,
        "a question this deployment declines cost an authorization-server round trip"
    );

    // And the count moves for a question that IS accepted, so the assertion above is not passing
    // against a broker nothing calls: one mint per answer, over the whole source set at once.
    let outcome = answer(&validated, &question, &asked_by_a_person(), &broker, &registry)
        .expect("the fake answers")
        .into_outcome();
    assert!(matches!(outcome, ToolOutcome::Answer { .. }), "{outcome:?}");
    assert_eq!(broker.asked(), 1, "one mint per accepted question, and not one per leg");
}

/// More metrics than `MAX_METRICS` is refused before any per-metric check runs - checked once every
/// name has resolved, so the same valid metric repeated past the cap is `TooManyMetrics`, not the
/// duplicate-name refusal the sibling test below provokes.
#[test]
fn more_metrics_than_the_cap_is_refused_before_the_broker_is_asked() {
    let (validated, registry, _question) = ready();
    let broker = CountingBroker::default();

    let over_the_cap = Query::new(
        sutura_domain::query::MetricNames::of(metric(), vec![metric(); sutura_domain::query::MAX_METRICS]),
        Grain::Month,
        june(),
        Vec::new(),
        Vec::new(),
    );
    let outcome = answer(&validated, &over_the_cap, &asked_by_a_person(), &broker, &registry)
        .expect("a refusal is an Ok")
        .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::TooManyMetrics {
                    requested,
                    limit: sutura_domain::query::MAX_METRICS
                }
            } if requested == sutura_domain::query::MAX_METRICS.saturating_add(1)
        ),
        "{outcome:?}"
    );
    assert_eq!(
        broker.asked(),
        0,
        "a question this deployment declines cost no broker round trip"
    );
}

/// The same metric named twice in one question is refused rather than de-duplicated.
#[test]
fn the_same_metric_named_twice_is_refused_rather_than_deduplicated() {
    let (validated, registry, _question) = ready();
    let broker = CountingBroker::default();

    let doubled = Query::new(
        sutura_domain::query::MetricNames::of(metric(), vec![metric()]),
        Grain::Month,
        june(),
        Vec::new(),
        Vec::new(),
    );
    let outcome = answer(&validated, &doubled, &asked_by_a_person(), &broker, &registry)
        .expect("a refusal is an Ok")
        .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::DuplicateMetricName { ref metric }
            } if *metric == self::metric()
        ),
        "{outcome:?}"
    );
    assert_eq!(
        broker.asked(),
        0,
        "a question this deployment declines cost no broker round trip"
    );
}

/// `top` names no metric to rank by, and this build's two rendering paths (SQL, the in-process
/// engine) disagree about which measure it would mean once more than one is named - refused at
/// compile time rather than ranking by whichever measure an adapter happens to read.
#[test]
fn top_with_more_than_one_metric_is_refused_before_a_plan_exists() {
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([
            ColumnName::parse("amount_cents").expect("a test column is a column"),
            column("order_date"),
        ]),
        Description::default(),
    );
    let revenue = Metric::new(
        MetricName::parse("revenue").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let orders = Metric::new(
        MetricName::parse("orders_count").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Count,
            column("amount_cents"),
        ))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue, orders]).expect("the test bundle is consistent");
    let pinned = PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash");

    let asked = Query::new(
        sutura_domain::query::MetricNames::of(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            vec![MetricName::parse("orders_count").expect("a test metric is a metric")],
        ),
        Grain::Month,
        june(),
        Vec::new(),
        Vec::new(),
    )
    .with_top(sutura_domain::query::Top::new(
        sutura_domain::query::TopN::parse(3).expect("a positive top.n"),
        sutura_domain::query::TopBy::Metric,
        sutura_domain::query::TopDirection::Desc,
    ));

    let compiled = sutura_semantic::compile(
        &asked,
        &sutura_domain::pinned::view::ScopedView::everything(&pinned),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a refusal is not a compile error");
    let sutura_semantic::Compiled::Refused { reason } = compiled else {
        panic!("expected a refusal, got {compiled:?}");
    };
    assert!(
        matches!(reason, RefusalReason::MultiMetricTopNotExecutable { ref metrics } if metrics.len() == 2),
        "{reason:?}"
    );
}

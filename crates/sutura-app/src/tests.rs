//! This crate's own unit suite: the anchor pass, the query path, and the credential outcomes.
//!
//! Split out of `lib.rs` for the reason [`crate::tests_support`] is - the file reached the 1000-line
//! gate, and the fakes it uses live there.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::{Anchor, Definitions, Description, Metric, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, Grain, ModelName, SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, NotValidated};
use sutura_domain::plan::MAX_ROWS;
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::Value;

use sutura_domain::identity::{PrincipalChain, RequestContext, Subject, SubjectId};

use super::tests_support::{AdapterFailure, CountingBroker, FixedBroker, FixedWarehouse};
use super::{
    AnchorCheck, MetricName, NotExecutedReason, PinnedDefinitions, RowSet, ServiceError, Warehouses, answer, exceeds_row_cap,
    verify_anchors, verify_and_validate,
};

/// The context a question arrives with, naming a person the transport verified.
///
/// A verified subject rather than the deployment, because it is the case the whole port exists
/// for and because it must NOT change what a shared leg does: a deployment whose sources are all
/// shared answers a named caller exactly as it answered before, which is the behaviour the
/// `a_posture_is_recorded_in_provenance_per_leg` test above still asserts.
fn asked_by_a_person() -> RequestContext {
    RequestContext::of(PrincipalChain::of(Subject::Verified {
        id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
    }))
}

fn metric() -> MetricName {
    MetricName::parse("revenue").expect("a test metric name is a name")
}

fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

fn shared(text: &str) -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(AcknowledgementReason::parse(text).expect("a test reason is a reason")),
    }
}

/// The June range the test bundle's anchor declares, which is also the only range a question
/// against it can ask for and get one row back.
fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range")
}

/// The certified number, in the shape the anchor check and a question both read it.
fn certified() -> RowSet {
    RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("one column and one cell is rectangular")
}

/// A one-metric bundle whose metric declares an anchor, so there is exactly one check to make.
fn bundle() -> PinnedDefinitions {
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
        BTreeMap::new(),
        Some(Anchor::new(range, String::from("197122"))),
        Description::default(),
    );
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
    // The real hasher, from the catalog adapter that owns the canonical form. `pin` applies it to
    // the definitions being pinned, so there is no digest here for the bundle not to describe.
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
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
    let report = verify_anchors(
        &pinned,
        &Warehouses::of(FixedWarehouse::new(source(), shared("a directory of CSVs"))),
    );
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
        shared("a directory of CSVs"),
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
    .and(FixedWarehouse::answering(
        source(),
        shared("a directory of CSVs"),
        certified(),
    ))
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
    // The Done-when of the source registry: an answer says which mode produced it. Asserted for
    // BOTH postures against the same bundle, the same question and the same rows, so the only
    // thing that moves is what the adapter was handed - which is the whole claim. Nothing in this
    // test holds a settings tree, so the value cannot have come from configuration.
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    for posture in [
        shared("a directory of CSVs this deployment owns"),
        SourcePosture::ImpersonationAtSource,
    ] {
        let expected = posture.as_str();
        let registry = Warehouses::of(FixedWarehouse::answering(source(), posture, certified()));
        let validated = verify_and_validate(bundle(), &registry).expect("the anchor reproduces its number");
        let outcome = answer(
            &validated,
            &question,
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &registry,
        )
        .expect("the fake answers");
        let ToolOutcome::Answer { ref provenance, .. } = outcome else {
            panic!("a certified question is answered, not {outcome:?}");
        };
        assert_eq!(
            provenance.executed_as().posture(&source()).map(SourcePosture::as_str),
            Some(expected),
            "the answer records the posture the adapter that executed it was holding"
        );
        assert_eq!(provenance.executed_as().legs().count(), 1, "a mono-source answer has one leg");
        // And the two halves of provenance stay separable: the digest is over authored content, so
        // it does not move when the posture does.
        assert_eq!(provenance.digest(), bundle().digest());
    }
}

#[test]
fn a_question_for_a_source_nobody_configured_is_refused_rather_than_run_elsewhere() {
    // The query-path half of the same lookup. `SourceUnavailable` now means what its name says.
    let registry = Warehouses::of(FixedWarehouse::answering(source(), shared("csv"), certified()));
    let validated = verify_and_validate(bundle(), &registry).expect("the anchor reproduces its number");
    let elsewhere = Warehouses::of(FixedWarehouse::answering(
        SourceName::parse("somewhere_else").expect("a test source is a source"),
        shared("csv"),
        certified(),
    ));
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let outcome = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &elsewhere,
    )
    .expect("a refusal is an Ok");
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
    let registry = Warehouses::of(FixedWarehouse::answering(
        source(),
        shared("a directory of CSVs this deployment owns"),
        certified(),
    ));
    let validated = verify_and_validate(bundle(), &registry).expect("the anchor reproduces its number");
    (
        validated,
        registry,
        Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new()),
    )
}

#[test]
fn a_subject_with_no_credential_is_refused_rather_than_downgraded() {
    // THE REFUSAL THIS STEP ADDS, at the query path. A broker with nothing to present for the
    // source the plan reads is not a reason to read it as this process: the question is refused,
    // it is refused as a RESULT rather than as an error, and it names the source so an operator
    // knows where the missing grant is.
    //
    // The alternative - answering under whatever identity the adapter holds - is what has no
    // signature any more, and this is the test that shows the refusal arriving instead of rows.
    let (validated, registry, question) = ready();
    let outcome = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::RefusesEverything,
        &registry,
    )
    .expect("a refusal is an Ok, so a client cannot retry it into an answer");
    let ToolOutcome::Refusal {
        reason: RefusalReason::CredentialUnavailable { ref source },
    } = outcome
    else {
        panic!("a subject with no credential is refused, not {outcome:?}");
    };
    assert_eq!(source, &self::source());
    // And no rows came back with it. A refusal that arrived after a successful read under
    // somebody else's identity would not be a refusal, and the same registry answers rows for
    // the broker that grants - which is what makes this assertion about the credential.
    assert!(!matches!(outcome, ToolOutcome::Answer { .. }));
    assert!(matches!(
        answer(
            &validated,
            &question,
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &registry
        )
        .expect("the fake answers"),
        ToolOutcome::Answer { .. }
    ));
}

#[test]
fn a_broker_that_could_not_be_reached_is_a_failure_and_not_a_refusal() {
    // The split `docs/adr/0008` part 6 draws, asserted rather than described: "this subject may
    // not reach that source" is a governance outcome in the `Ok`, and "the authorization server
    // did not answer" is an `Err`. Offering the second as a refusal would let a client library
    // retry an outage as though the question were wrong - and, worse, the two would be
    // indistinguishable to whoever is paged.
    let (validated, registry, question) = ready();
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::Unreachable,
        &registry,
    )
    .expect_err("a broker that cannot be reached is not a refusal");
    assert!(
        matches!(failure, ServiceError::Broker { .. }),
        "it is the broker's failure and not the data system's: {failure:?}"
    );
    // Its own variant rather than sharing the data system's, because the two are retried and
    // diagnosed differently - which is what the transports then turn into two different codes.
    assert!(!matches!(failure, ServiceError::Warehouse { .. }));
}

#[test]
fn an_adapter_that_receives_the_wrong_posture_returns_an_err_rather_than_a_refusal() {
    // The wiring defect between a broker and a source declaration. This adapter declares that it
    // has nowhere for a subject's own credential to arrive, and the broker handed it one anyway -
    // so nothing about the question was wrong and it must not come back as something a caller can
    // change.
    //
    // **This is the direction that is silent if it is not checked:** an adapter that quietly
    // ACCEPTED subject material it cannot use would answer, and provenance would report the leg
    // as impersonated when it ran as the process.
    let (validated, registry, question) = ready();
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsSubjectMaterial,
        &registry,
    )
    .expect_err("credential material an adapter cannot use is a failure, not a refusal");
    let ServiceError::Warehouse {
        cause: AdapterFailure::NoPlaceForASubject { ref at, presented },
    } = failure
    else {
        panic!("the adapter refuses what it was handed, and says what it was: {failure:?}");
    };
    assert_eq!(at, "local");
    assert_eq!(presented, "the asker's own credential");
}

#[test]
fn credentials_that_do_not_cover_the_plan_are_a_failure_rather_than_an_execution() {
    // A broker that answered about a different source than it was asked about. `LegCredentials`
    // cannot hold a set that fails to cover the sources it was minted FOR, so this is the case
    // where the broker and the plan disagree about what is being answered - and the alternative
    // to failing here is executing the leg with no credential, which is the one thing the port
    // exists to make unrepresentable.
    let (validated, registry, question) = ready();
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsTheWrongSource,
        &registry,
    )
    .expect_err("a grant that does not cover the plan is a failure");
    let ServiceError::Credentials { ref cause } = failure else {
        panic!("it is the credentials that are wrong, not the data system: {failure:?}");
    };
    assert_eq!(cause.at(), &source());
}

#[test]
fn a_bundle_whose_anchor_could_not_run_does_not_come_back_validated() {
    // The other half of the invariant, and the half a report could not carry: the ONLY way to a
    // `Validated` bundle runs the anchors, so a data system that answers nothing yields no
    // bundle at all. Before this operation existed, the same situation was a report a caller was
    // free to ignore - and `Validated::new` was happy to be handed a different one.
    let error = verify_and_validate(
        bundle(),
        &Warehouses::of(FixedWarehouse::new(source(), shared("a directory of CSVs"))),
    )
    .expect_err("a data system that fails every statement cannot validate a bundle");
    let NotValidated::AnchorNotExecuted { ref metric, .. } = error else {
        panic!("a failed anchor check is a not-executed verdict, not {error:?}");
    };
    assert_eq!(metric, &self::metric());
}

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

    let unknown = Query::new(
        MetricName::parse("no_such_metric").expect("a test metric name is a name"),
        Grain::Month,
        june(),
        Vec::new(),
        Vec::new(),
    );
    let outcome = answer(&validated, &unknown, &asked_by_a_person(), &broker, &registry).expect("a refusal is an Ok");
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
    let outcome = answer(&validated, &question, &asked_by_a_person(), &broker, &registry).expect("the fake answers");
    assert!(matches!(outcome, ToolOutcome::Answer { .. }), "{outcome:?}");
    assert_eq!(broker.asked(), 1, "one mint per accepted question, and not one per leg");
}

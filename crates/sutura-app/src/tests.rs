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

use sutura_domain::identity::{
    CredentialsDoNotCoverThePlan, CredentialsDoNotFitTheRequest, Expiry, PresentedDisagreesWithPosture, PrincipalChain,
    RequestContext, Subject, SubjectId,
};

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

/// The posture every fake warehouse here is opened with.
///
/// **One value rather than a sentence per call site, and `answer` is why.** The leg a broker mints is
/// now compared against the posture the adapter was opened with, witness included - so a test that
/// wrote its own acknowledgement prose would provoke that wiring defect rather than whatever it was
/// about. `tests_support` owns the one witness both halves read.
fn shared() -> SourcePosture {
    super::tests_support::shared_posture()
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
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
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
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
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
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
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
    .expect("a refusal is an Ok, so a client cannot retry it into an answer")
    .into_outcome();
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
        .expect("the fake answers")
        .into_outcome(),
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
    //
    // **The adapter is opened `impersonation-at-source` here, and it did not have to be before.**
    // `answer` now compares the leg against the adapter's posture before the adapter is called, so
    // subject material against a source declared SHARED is refused one step earlier - as
    // `ServiceError::Posture`, which the test above owns. What is left for the adapter is the case the
    // application cannot decide: a source the deployment declared impersonating, served by an adapter
    // whose CAPABILITY has nowhere for a subject to arrive. The composition root refuses that pairing
    // at boot; `answer` is not the boot path, so the adapter is the thing that stops it.
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let registry = Warehouses::of(FixedWarehouse::answering(
        source(),
        SourcePosture::ImpersonationAtSource,
        certified(),
    ));
    let validated = verify_and_validate(bundle(), &registry).expect("the anchor reproduces its number");
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
    let ServiceError::Credentials {
        cause:
            CredentialsDoNotFitTheRequest::Coverage {
                cause: CredentialsDoNotCoverThePlan::Missing { ref at },
            },
    } = failure
    else {
        panic!("it is the credentials that are wrong, not the data system: {failure:?}");
    };
    assert_eq!(at, &source());
}

#[test]
fn a_grant_minted_for_another_subject_never_reaches_an_adapter() {
    // **THE CONFUSED DEPUTY, and the one hole in the claim this whole port makes.**
    // `LegCredentials::minted` is `pub` and takes any `Subject` - it has to be, because a broker
    // adapter lives in another crate - so nothing but a comparison stops a broker from returning a
    // credential minted for somebody else. Reproduced by a review with exactly this fixture: the
    // mismatched grant reached execution and the question was answered.
    //
    // The consequence is worse than the label. The audit record takes the subject from the request
    // while the leg carries whatever the broker chose, so a bad mapping executes as one principal
    // and is recorded as another - which is the one question an incident asks first, answered
    // wrongly rather than not at all.
    let (validated, registry, question) = ready();
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsAnotherSubjectsCredential,
        &registry,
    )
    .expect_err("a grant for somebody else is a wiring failure, not an answer and not a refusal");
    let ServiceError::Credentials {
        cause: CredentialsDoNotFitTheRequest::AnotherSubject { ref asked, ref granted },
    } = failure
    else {
        panic!("the grant names another subject, and the failure says so: {failure:?}");
    };
    assert_eq!(
        asked,
        &Subject::Verified {
            id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
        }
    );
    assert_eq!(granted, &Subject::TheDeploymentItself);
    // And it is an `Err` rather than a refusal, because a caller can do nothing about it and being
    // told "you have no credential there" would be a false statement about their access.
    assert!(!matches!(failure, ServiceError::Warehouse { .. }));
    // The same registry, the same question and the same context answer for a broker that mints for
    // the asker - so this test is not passing against a deployment that refuses everything.
    assert!(matches!(
        answer(
            &validated,
            &question,
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &registry
        )
        .expect("the fake answers")
        .into_outcome(),
        ToolOutcome::Answer { .. }
    ));
}

#[test]
fn a_credential_whose_deadline_has_passed_never_reaches_an_adapter() {
    // **`Expiry` was dead metadata, and a review proved it by answering a question with a credential
    // that expired at the Unix epoch.** The earlier fix in this branch made `Expiry::earliest`
    // compute the deadline correctly; computing it correctly is worth nothing while nobody reads it.
    //
    // The deadline of zero is what makes this assertion independent of the clock `answer` reads:
    // there has never been a clock for which the epoch is in the future.
    //
    // **The data system this question is asked of FAILS every statement, and that is what makes the
    // test's name true rather than plausible.** The bundle is validated against a registry that
    // answers, and the question is asked of one that does not - so if the deadline were checked after
    // the pre-flight rather than before it, this would come back as the adapter's own complaint
    // instead. There is a second deadline check after the pre-flight, and this is how the two are told
    // apart.
    let (validated, answering, question) = ready();
    let refuses_every_statement = Warehouses::of(FixedWarehouse::new(source(), shared()));
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsSomethingAlreadyExpired,
        &refuses_every_statement,
    )
    .expect_err("a credential that has already expired is not something to execute with");
    let ServiceError::Credentials {
        cause: CredentialsDoNotFitTheRequest::Expired {
            deadline_unix_seconds,
            now_unix_seconds,
        },
    } = failure
    else {
        panic!("the deadline had passed, and the failure says when: {failure:?}");
    };
    assert_eq!(deadline_unix_seconds, 0, "the deadline the broker minted");
    assert!(
        now_unix_seconds > 0,
        "and the instant it was compared against, which is this machine's clock: {now_unix_seconds}"
    );
    // And nothing that does not expire is caught by it: the shipping broker mints
    // `Expiry::NothingExpires`, and the whole suite would be red if this guard refused that.
    assert!(matches!(
        answer(
            &validated,
            &question,
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &answering
        )
        .expect("the fake answers")
        .into_outcome(),
        ToolOutcome::Answer { .. }
    ));
}

#[test]
fn a_credential_that_expires_during_the_pre_flight_never_reaches_the_execution() {
    // **The second deadline check, and the one that can fire in production.** The check after minting
    // runs microseconds after the broker returned, so what it catches is a broker minting something
    // already dead. This one runs after the pre-flight - a round trip against a networked data system
    // - so a credential with seconds left when it was minted may have none when the statement would
    // run. A review asked for exactly this: enforcement "including after pre-flight / before
    // execution".
    //
    // Provoked without a clock a test can advance: the broker mints a deadline at the next whole
    // second, and this fake's pre-flight takes longer than that. The pre-flight itself succeeds, which
    // is what makes the failure about the second check rather than the first.
    let registry = Warehouses::of(FixedWarehouse::answering_after(
        source(),
        shared(),
        certified(),
        std::time::Duration::from_millis(1200),
    ));
    let validated = verify_and_validate(bundle(), &registry).expect("the anchor reproduces its number");
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsSomethingExpiringWithinTheSecond,
        &registry,
    )
    .expect_err("a credential that ran out during the pre-flight is not something to execute with");
    let ServiceError::Credentials {
        cause: CredentialsDoNotFitTheRequest::Expired {
            deadline_unix_seconds,
            now_unix_seconds,
        },
    } = failure
    else {
        panic!("the deadline passed between the pre-flight and the execution: {failure:?}");
    };
    assert!(
        now_unix_seconds >= deadline_unix_seconds,
        "the clock read after the pre-flight is at or past the deadline: {now_unix_seconds} vs {deadline_unix_seconds}"
    );
    // And the same grant answers against a data system whose pre-flight returns at once, so this test
    // is about the deadline passing rather than about the grant being refusable.
    let quick = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &quick).expect("the anchor reproduces its number");
    assert!(matches!(
        answer(
            &validated,
            &question,
            &asked_by_a_person(),
            &FixedBroker::GrantsSomethingExpiringWithinTheSecond,
            &quick
        )
        .expect("a credential with life left answers")
        .into_outcome(),
        ToolOutcome::Answer { .. }
    ));
}

#[test]
fn what_a_call_ran_under_travels_to_the_record_and_not_to_the_caller() {
    // `docs/adr/0008` fixes the audit record's content as the chain, the outcome, the posture per leg
    // AND the expiry the credentials carried. The last of those had nowhere to travel: `answer`
    // returned a `ToolOutcome` and the deadline was on a value it dropped. A review's objection was
    // sharper than "a missing field" - a deadline nothing reads is not a control, and one nothing
    // records cannot be reviewed afterwards either.
    //
    // It rides on `Answered` and not on `Provenance`, deliberately: provenance goes to the caller, and
    // how long this deployment's credential for a data system is good for is not the asker's business.
    let (validated, registry, question) = ready();
    let answered = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &registry,
    )
    .expect("the fake answers");
    assert!(matches!(answered.outcome(), &ToolOutcome::Answer { .. }));
    assert_eq!(
        answered.executed_until(),
        Some(Expiry::NothingExpires),
        "the shipping broker mints from a file, and `NothingExpires` is a case a record can carry"
    );

    // A question declined before the broker was asked has nothing to report, and `None` says so
    // rather than claiming a credential that never existed does not expire.
    let unknown = Query::new(
        MetricName::parse("no_such_metric").expect("a test metric name is a name"),
        Grain::Month,
        june(),
        Vec::new(),
        Vec::new(),
    );
    let declined = answer(
        &validated,
        &unknown,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &registry,
    )
    .expect("a refusal is an Ok");
    assert!(matches!(declined.outcome(), &ToolOutcome::Refusal { .. }));
    assert_eq!(
        declined.executed_until(),
        None,
        "nothing was minted for a question the bundle declined"
    );
}

#[test]
fn a_refusal_naming_a_source_nobody_asked_about_is_a_failure_rather_than_a_refusal() {
    // The refusal path used to trust an arbitrary `SourceName`. A review asked a broker for `local`,
    // had it refuse `elsewhere`, and got a caller-facing `CredentialUnavailable` naming a source the
    // caller had never asked about - a broker defect reported as the caller's own lack of access, with
    // an unrequested source alias disclosed in the process.
    let (validated, registry, question) = ready();
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::RefusesASourceNobodyAsked,
        &registry,
    )
    .expect_err("a refusal about a source nobody asked about is a wiring failure");
    let ServiceError::Credentials {
        cause: CredentialsDoNotFitTheRequest::RefusalNamesAnUnaskedSource { ref at },
    } = failure
    else {
        panic!("the refused source was never asked about, and the failure says which: {failure:?}");
    };
    assert_eq!(at.as_str(), "elsewhere");
    // The direction that makes this a security fix rather than a tidy-up: a refusal about the source
    // that WAS asked about is still a refusal, and still names it. One guard, and it does not refuse
    // the honest case.
    let refused = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::RefusesEverything,
        &registry,
    )
    .expect("a refusal about a source the request named is an Ok")
    .into_outcome();
    assert!(
        matches!(
            refused,
            ToolOutcome::Refusal {
                reason: RefusalReason::CredentialUnavailable { ref source }
            } if source == &self::source()
        ),
        "{refused:?}"
    );
}

#[test]
fn a_bundle_whose_anchor_could_not_run_does_not_come_back_validated() {
    // The other half of the invariant, and the half a report could not carry: the ONLY way to a
    // `Validated` bundle runs the anchors, so a data system that answers nothing yields no
    // bundle at all. Before this operation existed, the same situation was a report a caller was
    // free to ignore - and `Validated::new` was happy to be handed a different one.
    let error = verify_and_validate(bundle(), &Warehouses::of(FixedWarehouse::new(source(), shared())))
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

//! What a minted credential has to agree with before an adapter ever sees it.
//!
//! **One concept, and its own module because `tests.rs` reached the unexemptable `max-lines` cap**
//! on the commit that made `answer` generic in the combiner. The seam is the one the file already
//! named - `ready()`'s own doc said *for the credential tests below* - rather than wherever the
//! counter fell.
//!
//! Every cell here is about the same boundary: the broker answered, and what `answer` does with an
//! answer that does not fit the request. `use super::*` reaches every fixture, so the cells read
//! exactly as they did before the move.

use super::*;

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
fn a_source_that_refuses_the_statement_is_refused_not_a_transport_failure() {
    // THE CLASS THIS ISSUE ADDS, at the query path. The data system answered and answered no about
    // WHO asked: the identity the statement ran as may not read what it asks for. That is not the
    // `503` a dead data system produces - a refusal must not be silently retried as if it were
    // transient - so `answer` turns the adapter's `source_refused` predicate into
    // `RefusalReason::SourceRefused`, a governed answer in the `Ok`, carrying the source.
    //
    // The registry is validated (so the anchor numbers hold) and then the plan is answered against
    // a warehouse whose execute refuses at the identity/authorization level.
    let working = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &working).expect("the anchor reproduces its number");
    let refusing = Warehouses::of(RefusingSourceWarehouse::new(source(), shared()));
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let outcome = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &refusing,
    )
    .expect("a refusal is an Ok, so a client cannot retry it into an answer")
    .into_outcome();
    let ToolOutcome::Refusal {
        reason: RefusalReason::SourceRefused { ref source },
    } = outcome
    else {
        panic!("a source refusal must come back as a refusal, not {outcome:?}");
    };
    assert_eq!(source, &self::source());
    // And it never reaches a caller as the retryable class. The same question against the same
    // refusal must not surface as `ServiceError::Warehouse`, which is what a data system being
    // down looks like - the status that invites the very retry this refusal exists to prevent.
}

#[test]
fn a_source_that_refuses_the_preflight_is_refused_and_never_executed() {
    let working = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &working).expect("the anchor reproduces its number");
    let refusing = MonoPreflightWarehouse::new(source(), shared(), certified(), DryRunOutcome::SourceRefused);
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let warehouses = Warehouses::of(refusing);
    let outcome = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
    );
    let outcome = outcome.expect("a pre-flight refusal is a governed answer").into_outcome();
    let ToolOutcome::Refusal {
        reason: RefusalReason::SourceRefused { ref source },
    } = outcome
    else {
        panic!("a source refusal must come back as a refusal, not {outcome:?}");
    };
    assert_eq!(source, &self::source());
    assert_eq!(
        warehouses
            .get(&self::source())
            .expect("the source is registered")
            .executions(),
        0
    );
}

#[test]
fn a_failure_the_source_did_not_refuse_is_still_a_transport_failure() {
    // The reverse direction, and the more dangerous mistake: a transient failure told "do not
    // retry" has been told the wrong thing. A warehouse whose execute fails for a reason that is
    // NOT the data system refusing (the port's `source_refused` default is `false`) must still
    // leave as `ServiceError::Warehouse` - the retryable class - and never as a refusal.
    let working = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &working).expect("the anchor reproduces its number");
    let broken = Warehouses::of(TransientlyBrokenWarehouse::new(source(), shared()));
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let failure = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &broken,
    )
    .expect_err("a failure the source did not refuse is not a refusal");
    assert!(
        matches!(failure, ServiceError::Warehouse { .. }),
        "it stays in the retryable class, not {failure:?}"
    );
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
        &Subject::verified("someone@example.com").expect("a test subject is a subject")
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

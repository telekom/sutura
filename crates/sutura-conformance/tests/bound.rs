//! The harness held to itself: the macro binds, the declaration selects, and every fault fires.
//!
//! **Fakes over the port, not a data system.** A pack is a statement about the execution port, so
//! what proves the pack fires is an implementor whose answers are chosen - `sutura-exec-duckdb` and
//! `sutura-exec-datafusion` bind the same packs from their own crates, which is where the claim
//! about a real data system is made. Two halves, and they establish different things:
//!
//! 1. **The binding.** `mod conformance` invokes the macro twice, once per leg declaration, so a
//!    behaviour that lost its test or a declaration that disagrees with the adapter's own constant
//!    fails the build or the census rather than reducing the count quietly.
//! 2. **The faults.** All eight `Fault` variants are provoked - seven by a fake distorted in
//!    exactly one way, and `EmptyCorpus` through `execute::a_leg_is_refused_over`, the seam that
//!    variant needed to be reachable at all. The pack is called DIRECTLY, because through the macro
//!    a fault is a panic - so a variant that stopped being reachable would leave a pack that
//!    reports nothing. This is the half that would be missing if the bindings alone were green: a
//!    pack that returned `Ok` unconditionally passes every binding in this file.

use sutura_conformance::corpus;
use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::{AnchorRows, PreFlight, RowSet, Value, Warehouse};

/// How a fake's answer differs from the corpus's.
///
/// One axis with one variant per `Fault` the answer can produce, so each fault below is provoked
/// by exactly one distortion and a test cannot pass because two of them fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Distortion {
    /// None: the corpus's own answer, unchanged.
    Faithful,
    /// The right rows under the wrong column names.
    Relabelled,
    /// The right shape with one cell replaced.
    ANumberChanged,
    /// The right rows, back to front.
    Reversed,
    /// No answer at all.
    Silent,
}

/// What a fake's pre-flight says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    /// The port's own default: nobody looked.
    NotAsked,
    /// Asked and accepted.
    Accepted,
    /// Asked and refused, which for a plan the adapter is held to answer is a fault.
    Refused,
}

/// Never returned by anything a conformance run is about: this fake computes nothing.
#[derive(Debug, thiserror::Error)]
enum FakeFailure {
    #[error("the fake holds no answer for this executable")]
    NoSuchExecutable,
    #[error("this fake declines to execute a leg")]
    NoLeg,
    #[error("this fake was asked to be silent")]
    Silent,
    #[error("this fake refuses every pre-flight")]
    PreFlightRefused,
    #[error("the fake verifies no anchor")]
    NoAnchor,
    #[error("the fake built a result that is not rectangular")]
    Ragged(#[from] sutura_domain::warehouse::MalformedRowSet),
}

/// One implementor of the execution port, answering out of the corpus.
///
/// `LEGS` is a const parameter rather than two types, because `EXECUTES_LEGS` is an associated
/// constant and one distortion axis should not be written twice to vary it. `Fake<true>` and
/// `Fake<false>` are what the two bindings below are instantiated at.
struct Fake<const LEGS: bool> {
    source: SourceName,
    posture: SourcePosture,
    distortion: Distortion,
    check: Check,
    /// Answers a leg even though `LEGS` is `false`, which is the one thing the refusing pack exists
    /// to catch.
    answers_a_leg_anyway: bool,
}

impl<const LEGS: bool> Fake<LEGS> {
    /// A fake that answers the corpus faithfully, checks nothing, and honours its declaration.
    fn faithful() -> Self {
        Self {
            source: corpus::source(),
            posture: corpus::posture(),
            distortion: Distortion::Faithful,
            check: Check::NotAsked,
            answers_a_leg_anyway: false,
        }
    }

    fn distorted(distortion: Distortion) -> Self {
        Self {
            distortion,
            ..Self::faithful()
        }
    }

    fn checking(check: Check) -> Self {
        Self {
            check,
            ..Self::faithful()
        }
    }

    /// The corpus's own answer for whatever it was handed.
    fn faithful_answer(executable: Executable<'_>) -> Option<RowSet> {
        match executable {
            Executable::Query(plan) => corpus::cases()
                .into_iter()
                .find(|case| case.plan() == plan)
                .map(|case| case.expected().clone()),
            Executable::Leg(leg) => {
                let case = corpus::leg_case();
                (case.leg() == leg).then(|| case.expected().clone())
            }
        }
    }

    /// The answer, distorted the one way this fake was built to distort it.
    fn answer(&self, executable: Executable<'_>) -> Result<RowSet, FakeFailure> {
        if self.distortion == Distortion::Silent {
            return Err(FakeFailure::Silent);
        }
        let faithful = Self::faithful_answer(executable).ok_or(FakeFailure::NoSuchExecutable)?;
        let columns = faithful.columns().to_vec();
        let mut rows = faithful.rows().to_vec();
        match self.distortion {
            Distortion::Faithful | Distortion::Silent => {}
            Distortion::Relabelled => {
                let renamed = columns.iter().map(|label| format!("{label}_")).collect();
                return Ok(RowSet::new(renamed, rows)?);
            }
            Distortion::ANumberChanged => {
                if let Some(cell) = rows.first_mut().and_then(|row| row.last_mut()) {
                    *cell = Value::Integer(i64::MAX);
                }
            }
            Distortion::Reversed => rows.reverse(),
        }
        Ok(RowSet::new(columns, rows)?)
    }
}

impl<const LEGS: bool> Warehouse for Fake<LEGS> {
    type Error = FakeFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = LEGS;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<PreFlight, Self::Error> {
        match self.check {
            Check::NotAsked => Ok(PreFlight::NotAsked),
            Check::Accepted => Ok(PreFlight::Accepted),
            Check::Refused => Err(FakeFailure::PreFlightRefused),
        }
    }

    fn execute(&self, executable: Executable<'_>, presented: &Presented) -> Result<RowSet, Self::Error> {
        // Matched exhaustively, for the reason every real adapter does: a fake that accepted subject
        // material it cannot use would let a pack pass an adapter that reported a leg as
        // impersonated when it ran shared.
        match *presented {
            Presented::SharedServiceUser { .. } => {}
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => {
                return Err(FakeFailure::NoSuchExecutable);
            }
        }
        match executable {
            Executable::Query(_) => self.answer(executable),
            Executable::Leg(_) if LEGS || self.answers_a_leg_anyway => self.answer(executable),
            Executable::Leg(_) => Err(FakeFailure::NoLeg),
        }
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(FakeFailure::NoAnchor)
    }
}

/// The bindings: one macro invocation per leg declaration, over the same pack.
///
/// Wrapped in `mod conformance` so the emitted names are `conformance::<adapter>::<behaviour>`,
/// which is the tier selector the macro's own documentation states.
mod conformance {
    // A fake that executes a leg, checks its plans, and answers the corpus - so every behaviour in
    // the pack HOLDS, including the leg one, and the census counts six.
    sutura_conformance::execute_packs! {
        adapter: a_fake_that_executes_legs,
        warehouse: crate::Fake<true>,
        open: crate::an_engine,
        executes_legs,
    }

    // A fake that does not, and offers no pre-flight - so the leg behaviour holds in the REFUSING
    // direction under its own test name, and the pre-flight behaviour is declined rather than
    // counted as held.
    sutura_conformance::execute_packs! {
        adapter: a_fake_that_refuses_legs,
        warehouse: crate::Fake<false>,
        open: crate::a_source,
        refuses_legs,
    }
}

/// The faithful leg-executing fake, with a pre-flight that really answers.
fn an_engine() -> Fake<true> {
    Fake::<true>::checking(Check::Accepted)
}

/// The faithful non-federating fake, which offers no pre-flight.
fn a_source() -> Fake<false> {
    Fake::<false>::faithful()
}

// The fault half, in its own `#[cfg(test)]` module because `clippy::tests_outside_test_module`
// asks for one and the strict lints exempt what is inside it.
#[cfg(test)]
mod faults {
    use super::{Check, Distortion, Fake};
    use sutura_conformance::{Behaviour, Declination, Fault, Outcome, execute};

    /// A relabelled answer is a labels fault, and it is reported as one rather than as wrong rows.
    ///
    /// The reason [`Behaviour::Labels`] is its own behaviour: the content comparison would also refuse
    /// this, and would call it a column disagreement - true, and the less useful of the two readings.
    #[test]
    fn an_answer_under_the_wrong_labels_is_a_labels_fault() {
        let fake = Fake::<false>::distorted(Distortion::Relabelled);
        let fault = execute::labels_are_the_plans_own(&fake).expect_err("relabelled rows are a fault");
        assert!(matches!(fault, Fault::Labels { .. }), "{fault:?}");
    }

    /// One changed cell is a content fault.
    #[test]
    fn one_wrong_number_is_a_content_fault() {
        let fake = Fake::<false>::distorted(Distortion::ANumberChanged);
        let fault = execute::content_agrees_with_the_reference(&fake).expect_err("a changed cell is a fault");
        assert!(matches!(fault, Fault::Content { .. }), "{fault:?}");
    }

    /// The right rows in the wrong order are an ORDER fault and not a content one.
    ///
    /// Both halves asserted, because the value of the two behaviours being separate is exactly this: a
    /// reversed answer agrees on content, so a suite with one combined assertion would report a sort
    /// order as a wrong number or the other way round, depending on which way it was written.
    #[test]
    fn the_right_rows_reversed_agree_on_content_and_fault_on_order() {
        let fake = Fake::<false>::distorted(Distortion::Reversed);
        assert_eq!(
            execute::content_agrees_with_the_reference(&fake).expect("reversed rows are the same rows"),
            Outcome::Held
        );
        let fault = execute::order_agrees_with_the_reference(&fake).expect_err("a reversed answer is a fault");
        assert!(matches!(fault, Fault::Order { .. }), "{fault:?}");
    }

    /// An adapter that does not answer is reported as not answering, with its own error beneath.
    #[test]
    fn a_data_system_that_does_not_answer_is_a_not_answered_fault() {
        let fake = Fake::<false>::distorted(Distortion::Silent);
        let fault = execute::content_agrees_with_the_reference(&fake).expect_err("silence is a fault");
        assert!(matches!(fault, Fault::NotAnswered { .. }), "{fault:?}");
        assert!(
            core::error::Error::source(&fault).is_some(),
            "the adapter's own error is the cause: {fault:?}"
        );
    }

    /// A pre-flight that refuses a plan the adapter is held to answer is a fault, not a declination.
    #[test]
    fn a_pre_flight_that_refuses_is_a_fault() {
        let fake = Fake::<false>::checking(Check::Refused);
        let fault = execute::a_preflight_that_accepts_is_followed_by_an_answer(&fake).expect_err("a refused check is a fault");
        assert!(matches!(fault, Fault::PreFlightRefused { .. }), "{fault:?}");
    }

    /// A pre-flight that accepts and an execution that then fails is the other direction of the same
    /// contract: the two may not disagree about what this adapter accepts.
    #[test]
    fn a_pre_flight_that_accepts_and_then_no_answer_is_a_fault() {
        let fake = Fake::<false> {
            check: Check::Accepted,
            distortion: Distortion::Silent,
            ..Fake::<false>::faithful()
        };
        let fault =
            execute::a_preflight_that_accepts_is_followed_by_an_answer(&fake).expect_err("accepted then silent is a fault");
        assert!(matches!(fault, Fault::AcceptedThenDidNotAnswer { .. }), "{fault:?}");
    }

    /// **The declination, and that it is not a pass.**
    ///
    /// The distinction this whole harness turns on: an adapter that answers `NotAsked` has established
    /// nothing about its pre-flight, so the behaviour comes back `Declined` with a typed reason rather
    /// than `Held`. A boolean outcome would have made this indistinguishable from a green.
    #[test]
    fn an_adapter_with_no_pre_flight_declines_that_behaviour_rather_than_holding_it() {
        let fake = Fake::<false>::checking(Check::NotAsked);
        assert_eq!(
            execute::a_preflight_that_accepts_is_followed_by_an_answer(&fake).expect("no pre-flight is not a fault"),
            Outcome::Declined(Declination::OffersNoPreFlight)
        );
    }

    /// An adapter that declares it does not execute a leg, and executes one, is a fault.
    ///
    /// The declared-absence direction `docs/adr/0012` says is worth having: nothing upstream builds a
    /// leg today, so the adapter's own guard is exercised by this and by nothing else.
    #[test]
    fn an_adapter_that_answers_a_leg_it_declares_it_cannot_is_a_fault() {
        let fake = Fake::<false> {
            answers_a_leg_anyway: true,
            ..Fake::<false>::faithful()
        };
        let fault = execute::a_leg_is_refused(&fake).expect_err("answering an undeclared leg is a fault");
        assert!(matches!(fault, Fault::ALegWasAnswered { .. }), "{fault:?}");
    }

    /// The refusing pack's own precondition: it establishes the adapter is live before reading the
    /// refusal as evidence, so a fake that answers nothing at all does not pass it.
    #[test]
    fn a_leg_refusal_from_an_adapter_that_answers_nothing_is_not_evidence() {
        let fake = Fake::<false>::distorted(Distortion::Silent);
        let fault = execute::a_leg_is_refused(&fake).expect_err("a dead adapter proves nothing about its leg guard");
        assert!(matches!(fault, Fault::NotAnswered { .. }), "{fault:?}");
    }

    /// **The eighth fault**, and the reason `execute::a_leg_is_refused_over` exists.
    ///
    /// `Fault::EmptyCorpus` is what stops the refusing pack being green over nothing - it is the
    /// non-empty-corpus guard `census` gives every other behaviour, in the one place it is a
    /// `Fault` instead. Handed no cases, the pack refuses to read a refusal as evidence rather
    /// than reading it as one.
    #[test]
    fn a_leg_refusal_over_an_empty_corpus_is_a_fault() {
        let fake = Fake::<false>::faithful();
        let fault = execute::a_leg_is_refused_over(&fake, &[]).expect_err("no cases is a fault");
        assert!(matches!(fault, Fault::EmptyCorpus), "{fault:?}");
    }

    /// The census's own failure, called directly with a short list.
    ///
    /// Directly, because the macro can no longer EMIT a wrong one: since a review disproved the
    /// two-hand-written-lists version, the `#[test]`s and the array this compares are one
    /// repetition, so a behaviour without a test is a behaviour without a census element. What is
    /// asserted here is the comparison itself - that a list shorter than `Behaviour::EVERY` is a
    /// failure rather than a smaller green run. The mutation that proves the pairing is deleting an
    /// entry from the macro's list, which reddens this cell in every binding.
    #[test]
    #[should_panic(expected = "a behaviour without a test is coverage this run did not earn")]
    fn a_binding_that_skips_a_behaviour_fails_the_census() {
        sutura_conformance::census::<Fake<false>>("short", &[Behaviour::Content]);
    }
}

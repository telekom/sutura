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
//! 3. **The venue.** `Fixture::Absent` is the harness's answer to *no tier is up here*, and what is
//!    worth proving about it is what it STOPS: a cell over an absent fixture must not run the pack,
//!    and the same distorted fake standing up must fail. One fake, two venues, opposite verdicts -
//!    which is what makes an absence a decision rather than a swallowed failure. Either half alone
//!    is satisfied by a harness that swallows every fault.

use sutura_conformance::{Fixture, corpus};
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
///
/// `Fixture::standing` unconditionally, because a fake stands up wherever the test binary does -
/// which is what makes the ABSENT direction below a statement about the harness rather than about
/// any environment this file needs.
fn an_engine() -> Fixture<Fake<true>> {
    Fixture::standing(Fake::<true>::checking(Check::Accepted))
}

/// The faithful non-federating fake, which offers no pre-flight.
fn a_source() -> Fixture<Fake<false>> {
    Fixture::standing(Fake::<false>::faithful())
}

// Its own `#[cfg(test)]` module, because `clippy::tests_outside_test_module` asks for one and
// the strict lints exempt what is inside it - the same reason the three modules below have one.
#[cfg(test)]
mod location {
    use sutura_conformance::corpus;
    use sutura_dev::scope::Scope;

    /// **The corpus this harness materialises is THIS WORKTREE'S state, through the type that owns
    /// that answer.**
    ///
    /// `telekom/sutura#405`'s first instance: the corpus landed on
    /// `<temp_dir>/sutura-conformance/<table>.csv`, a name carrying no worktree and no key, so
    /// every checkout of this repository on the machine was a writer of one file. Reproduced with
    /// two worktrees and one differing row - the `DuckDB` binding failed two cases as content
    /// faults while the run that overwrote the file was green.
    ///
    /// **Why this cell is in `tests/` and not in `corpus.rs`.** `sutura-conformance` may reach
    /// `sutura-dev` only as a DEV-dependency - `xtask/src/boundaries/harness.rs` holds the packs to
    /// the interior - so `corpus.rs` spells the state directory a second time and this is where the
    /// two spellings are COMPARED, through `Scope` itself rather than against a literal. Exactly
    /// the arrangement `the_requirement_this_harness_reads_is_the_one_the_provisioner_writes` is in,
    /// for the same boundary and the same reason.
    ///
    /// The root is derived here independently, from this test's own manifest directory, so what is
    /// compared is two answers rather than one answer twice.
    #[test]
    fn the_corpus_is_this_worktrees_own_state_and_not_a_machine_shared_path() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .ancestors()
            .find(|dir| dir.join("flake.nix").is_file() && dir.join("Cargo.lock").is_file())
            .expect("this crate is inside a checkout of this repository");
        let scope = Scope::from_root(root).expect("the checkout root resolves");

        let written = std::fs::canonicalize(corpus::on_disk()).expect("the corpus was written");
        let declared = scope.state_dir();
        // EXISTENCE FIRST, and with the corpus's real location in the message: materialising the
        // corpus is what creates this directory, so an absent one means the rows went somewhere
        // else - and `canonicalize` on a missing path reports only `NotFound`, which sends a reader
        // looking for a filesystem problem instead of at the path they got.
        assert!(
            declared.is_dir(),
            "the corpus landed at {} and this worktree's state directory {} was never created - so \
             the rows are not under this checkout at all",
            written.display(),
            declared.display()
        );
        let state = std::fs::canonicalize(&declared).expect("it is a directory");
        assert!(
            written.starts_with(&state),
            "the corpus is at {} and this worktree's state is {} - a path outside it is reachable \
             from every other checkout on this machine",
            written.display(),
            state.display()
        );
    }
}

// The fault half, in its own `#[cfg(test)]` module because `clippy::tests_outside_test_module`
// asks for one and the strict lints exempt what is inside it.
#[cfg(test)]
mod faults {
    use super::{Check, Distortion, Fake};
    use sutura_conformance::{Behaviour, Declination, Fault, Outcome, Spent, execute};

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
    /// The declared-absence direction `docs/adr/0012` says is worth having. It used to rest on
    /// *nothing upstream builds a leg today*; a leg is built and executed on the shipped answer path
    /// now, so what this is worth is narrower and still real - it is the only thing that exercises
    /// the guard of an adapter with no leg venue of its own.
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
        sutura_conformance::census::<Fake<false>>("short", &[Behaviour::Content], None, crate::a_source);
    }

    /// **A cell's cost is measured, not stated**, which is the whole reason [`Spent`] is a type:
    /// `docs/adr/0012` claimed per-pack timings were reported from the start and nothing measured
    /// one (`telekom/sutura#353`). The fields are private and both constructors measure, so a
    /// caller cannot report a number nobody took.
    #[test]
    fn a_cells_cost_is_measured_around_the_fixture_and_the_behaviour_separately() {
        let slow = core::time::Duration::from_millis(15);
        let (conformed, spent) = Spent::measuring(
            || {
                std::thread::sleep(slow);
                Fake::<false>::faithful()
            },
            |fake| {
                std::thread::sleep(slow);
                execute::content_agrees_with_the_reference(fake)
            },
        );
        assert_eq!(conformed.expect("the faithful fake holds"), Outcome::Held);
        // Both halves are attributed to the work they timed, which a single total cannot do - and
        // that split is the point, because the fixture is rebuilt once per behaviour.
        assert!(spent.fixture() >= slow, "fixture: {:?}", spent.fixture());
        assert!(spent.pack() >= slow, "pack: {:?}", spent.pack());
    }

    /// The census's own measurement runs no behaviour, and says so by measuring only the fixture.
    #[test]
    fn the_census_measures_the_fixture_and_not_a_behaviour() {
        let spent = Spent::building(|| {
            std::thread::sleep(core::time::Duration::from_millis(15));
            Fake::<false>::faithful()
        });
        assert!(spent.fixture() >= core::time::Duration::from_millis(15), "{spent}");
        // Not "roughly zero": the census ran nothing, so the pack half is not a measurement of a
        // behaviour at all, and a floor computed from it would be about nothing.
        assert!(spent.pack() < spent.fixture(), "{spent}");
    }

    /// What a reader of a green run sees, and it carries both halves - so the per-pack aggregate
    /// `docs/adr/0012` asks for is a `grep` over a run whose behaviour names are on the same lines.
    #[test]
    fn the_reported_cost_names_both_halves_beside_the_total() {
        let printed = Spent::building(crate::a_source).to_string();
        assert!(printed.contains("(fixture "), "{printed}");
        assert!(printed.contains(" + pack "), "{printed}");
        // The total is FIRST, so a reader scanning a green run sorts on the number that matters
        // and reads the split only where it does.
        assert!(!printed.starts_with("(fixture"), "{printed}");
    }
}

// The VENUE half. Its own `#[cfg(test)]` module, for the reason `faults` has one.
#[cfg(test)]
mod venue {
    use super::{Distortion, Fake};
    use sutura_conformance::{Behaviour, Fixture, Missing, execute};

    /// The absence every cell here is written around.
    fn absent() -> Fixture<Fake<false>> {
        Fixture::Absent(Missing::tier("postgres", &"nothing in this worktree publishes an endpoint"))
    }

    /// **The absent direction stops the pack, and the proof is that a fake which FAULTS survives it.**
    ///
    /// `Distortion::ANumberChanged` makes [`execute::content_agrees_with_the_reference`] return
    /// `Fault::Content`, and `conduct` panics on a fault - so if an absent fixture ran the pack
    /// anyway this cell would fail. It passes, which says the behaviour never ran. The cell below
    /// asserts the same distortion in a venue where the fixture stands up, and that one must panic:
    /// either half alone is satisfied by a harness that swallows every fault.
    #[test]
    fn a_behaviour_over_an_absent_fixture_does_not_run_the_pack() {
        // `None` for the venue's declaration, and it is the seam rather than a shortcut: a fake
        // absence IS a fabricated one as a value, so a cell that let the environment answer was
        // refused inside `checks.nextest`, which provisions the tier and sets the variable.
        // `unsafe_code` is `forbid` here, so no test can unset it either.
        sutura_conformance::conduct(
            "absent",
            Behaviour::Content,
            None,
            absent,
            execute::content_agrees_with_the_reference,
        );
    }

    /// The other half: the SAME distortion, standing up, fails the cell.
    #[test]
    #[should_panic(expected = "the rows are not the reference's")]
    fn the_same_distortion_standing_up_fails_that_behaviour() {
        sutura_conformance::conduct(
            "standing",
            Behaviour::Content,
            None,
            || Fixture::standing(Fake::<false>::distorted(Distortion::ANumberChanged)),
            execute::content_agrees_with_the_reference,
        );
    }

    /// A census over an absent fixture still holds what it can.
    ///
    /// The assertions it makes - that the emitted behaviours are the pack's, and that the corpus is
    /// not empty - are facts about this TREE, so a venue with no tier does not excuse them. What it
    /// must not do is print a behaviour count, a case count and a floor beside cells that each
    /// reported `NOT RUN`; that suppression is a printed line and no test can read it, so what is
    /// asserted here is the half a test can reach.
    #[test]
    fn a_census_over_an_absent_fixture_reports_rather_than_failing() {
        sutura_conformance::census::<Fake<false>>("absent", Behaviour::EVERY, None, absent);
    }

    /// And an absent venue does NOT excuse a binding that lost a behaviour.
    #[test]
    #[should_panic(expected = "a behaviour without a test is coverage this run did not earn")]
    fn an_absent_fixture_does_not_excuse_a_short_binding() {
        sutura_conformance::census::<Fake<false>>("absent", &[Behaviour::Content], None, absent);
    }

    /// The absence names the service AND carries the provisioner's own diagnostic.
    ///
    /// Both halves, because a message with only the service is a skip nobody can act on and a
    /// message with only the diagnostic does not say which tier went missing. The remedy itself is
    /// `sutura_dev::provisioned::Absent`'s to derive, and three checks hold it there - so what this
    /// asserts is that the harness passes it through rather than composing a fourth copy beside it.
    #[test]
    fn an_absence_names_the_service_and_carries_the_provisioners_diagnostic() {
        let printed = Missing::tier("postgres", &"run `just postgres-tier start`").to_string();
        assert!(printed.contains("`postgres`"), "{printed}");
        assert!(printed.contains("run `just postgres-tier start`"), "{printed}");
        // And it says what the cell established, which is nothing - the sentence a reader of a
        // green run needs, because libtest has no outcome for it. It is deliberately NOT the
        // census's own wording: the two lines appear one after the other on an absent binding, and
        // a reader who saw the same clause twice would have to work out whether one of them was
        // about something else.
        assert!(printed.contains("nothing was asked of this adapter"), "{printed}");
    }

    /// **The requirement this harness reads is the one the provisioner writes.**
    ///
    /// `sutura_conformance` spells `SUTURA_DEV_REQUIRE_TIER` and its truthiness a second time,
    /// because `not_here` has to judge an absence and the packs crate may not reach the crate that
    /// publishes that decision through a normal dependency. Two statements about one fact can
    /// disagree - `nix/with-tier.sh` records that defect at a count of one, which is why the
    /// variable is exported there by the thing that STARTED the tier rather than asserted beside
    /// it. So this cell is the mechanism that keeps the copies equal rather than a comment claiming
    /// they are. Both halves, because a name that matches with a truthiness that does not is the
    /// worse of the two failures: it would read as agreement.
    #[test]
    fn the_requirement_this_harness_reads_is_the_one_the_provisioner_writes() {
        assert_eq!(sutura_conformance::REQUIRE_TIER, sutura_dev::requirement::FORCE);
        // **The falsy spellings are ITERATED from the owner, not listed here**, and that is the
        // correction review asked for: a fixed literal array is a SAMPLE, and the sample was
        // chosen in the crate that would not be the one changed. Add `"off"` to
        // `sutura_dev::requirement::NOT_REQUIRED` - the obvious next spelling for a variable people
        // set by hand - and `SUTURA_DEV_REQUIRE_TIER=off` would mean *optional* to
        // `provisioned::here`, which skips, and *required* to `a_tier_is_required`, which then
        // refuses the very absence that skip produced. Red on a machine that asked to skip, with a
        // message naming the one thing that did not happen. Iterating the constant makes that a
        // failure of this cell instead.
        let falsy = sutura_dev::requirement::NOT_REQUIRED
            .iter()
            .flat_map(|spelling| [(*spelling).to_owned(), format!("  {spelling}"), spelling.to_uppercase()]);
        // The truthy side stays a sample and has to: *anything not falsy* has no list to iterate,
        // so what is asserted is that both copies agree over the spellings a reader would try.
        let truthy = ["1", "true", "TRUE", "yes", "CI", "on"].into_iter().map(String::from);
        for value in falsy.chain(truthy) {
            assert_eq!(
                sutura_conformance::a_tier_is_required(Some(&value)),
                sutura_dev::requirement::decide(Some(&value)).is_required(),
                "`{value}`"
            );
        }
        // And the unset direction, which is the one a host with no tier binary is in.
        assert!(!sutura_conformance::a_tier_is_required(None));
        assert!(!sutura_dev::requirement::decide(None).is_required());
    }

    /// **A tier absence declared where a venue said it PROVISIONED one is a defect, not a skip.**
    ///
    /// The hole this closes was measured rather than imagined: with the tier UP and the variable
    /// set, a fixture answering `Fixture::Absent` unconditionally passed, and the only
    /// tell was the printed `NOT RUN` lines. Only the thing that brought a tier up sets that
    /// variable, so where it is set an absent tier is impossible.
    ///
    /// Both directions, and the second is not symmetry: a refusal that fired wherever an absence
    /// appeared would make the suite red on every machine without the tier binary, which is the
    /// fail-OPEN direction `sutura_dev::requirement` decides for this whole repository and that
    /// this crate does not get to re-decide.
    #[test]
    fn a_declared_tier_absence_is_impossible_where_a_venue_provisioned_one() {
        let absent = Missing::tier("postgres", &"a fixture that did not ask");
        assert!(sutura_conformance::absence_is_impossible(&absent, Some("1")));
        assert!(!sutura_conformance::absence_is_impossible(&absent, None));
        assert!(!sutura_conformance::absence_is_impossible(&absent, Some("0")));
    }

    /// **A fabricated absence fails the BEHAVIOUR cell where a venue provisioned a tier.**
    ///
    /// End to end through `conduct`, message included, which the seam is what makes possible: the
    /// venue's declaration is a parameter, so this cell states `Some("1")` instead of depending on
    /// which venue ran it. It is the same fixture the cell above passes `None` for, so the pair is
    /// one value judged two ways.
    #[test]
    #[should_panic(expected = "so it provisioned a tier and an absent one is not possible here")]
    fn a_fabricated_absence_fails_a_behaviour_where_a_venue_provisioned_a_tier() {
        sutura_conformance::conduct(
            "fabricated",
            Behaviour::Content,
            Some("1"),
            absent,
            execute::content_agrees_with_the_reference,
        );
    }

    /// **And the CENSUS cell too**, which is the half that was missing when the refusal lived in
    /// one ending only: measured at 6 of 7 failing, with the binding's own census the cell that
    /// passed over a fabricated absence.
    #[test]
    #[should_panic(expected = "so it provisioned a tier and an absent one is not possible here")]
    fn a_fabricated_absence_fails_the_census_where_a_venue_provisioned_a_tier() {
        sutura_conformance::census::<Fake<false>>("fabricated", Behaviour::EVERY, Some("1"), absent);
    }

    /// The two variants answer [`Fixture::missing`] apart, which is what `census` branches on.
    #[test]
    fn a_standing_fixture_reports_nothing_missing_and_an_absent_one_reports_why() {
        assert!(Fixture::standing(Fake::<false>::faithful()).missing().is_none());
        assert!(absent().missing().is_some());
    }
}

/// **What the corpus must CONTAIN for three of the packs' claims to say anything.**
///
/// Not facts about an adapter, so not [`Behaviour`]s: a behaviour would re-establish each of these
/// once per bound adapter, and what they are about is this tree. They sit here rather than in
/// `corpus.rs`'s own `mod tests` for a mechanical reason - `cargo xtask test-causality` restores
/// the base version of a changed file that added no test and HOLDS one that did, so an assertion
/// about the corpus written beside the corpus is measured against its own new data and reads as
/// green against base. Split across two files the pair is separable, and each test below is red
/// with `corpus.rs` at base.
///
/// Each guards something a count cannot see. [`sutura_conformance::census`] prints how many CASES
/// there are, so a deleted case is loud; a deleted ROW inside a case is silent, because a CSV line
/// and its expected row removed together leave every adapter cell green over a corpus that no
/// longer asks the question.
#[cfg(test)]
mod corpus_shape {
    use sutura_conformance::corpus;
    use sutura_domain::model::Aggregate;
    use sutura_domain::plan::{PlanMeasure, PlanTerm};
    use sutura_domain::warehouse::Value;

    /// A null sits in a GROUP-KEY position in the corpus, and every row carrying one sorts LAST.
    ///
    /// **The position is the assertion, and the first version of this test did not make it.** It
    /// asked `row.contains(&Value::Null)` - a null ANYWHERE in the row, the measure included - and
    /// a review moved the corpus's null out of the key and into the measure, leaving the test
    /// green over a corpus with no null group key at all. `QueryPlan::result_labels` projects the
    /// dimension keys first, then the time bucket, then the measure, so a key position is an index
    /// below the key count and nothing else is.
    ///
    /// What the row it guards buys is stated where the corpus states it, and it is NOT a check on
    /// our own null-placement rendering: for all three bound adapters the placement is their
    /// engine's own default, so deleting `sutura_sql`'s statement of it changes neither their SQL
    /// nor their answer. It is an upstream-default regression detector plus, through
    /// `Behaviour::Content`, a claim about our grouping - that a null key is a GROUP and not a row
    /// something dropped.
    #[test]
    fn a_null_group_key_is_expected_and_expected_last() {
        let mut null_keyed_rows = 0_usize;
        for case in corpus::cases() {
            // The key positions, and ONLY those: a null in the bucket or in the measure is a
            // different fact and must not satisfy this test.
            let keys = case.plan().keys().len();
            let mut after_a_null_key = false;
            for (index, row) in case.expected().rows().iter().enumerate() {
                let null_key = row.iter().take(keys).any(|cell| *cell == Value::Null);
                if null_key {
                    null_keyed_rows = null_keyed_rows.saturating_add(1);
                }
                assert!(
                    null_key || !after_a_null_key,
                    "case `{}`: row {index} has no null in a key position and follows one that does, so the null group is not last",
                    case.name()
                );
                after_a_null_key = after_a_null_key || null_key;
            }
        }
        assert!(
            null_keyed_rows > 0,
            "no expected answer in the corpus carries a null in a GROUP-KEY position, so \
             `Behaviour::Content` asserts nothing about a null key being a group rather than a row \
             something dropped, and `Behaviour::Order` asserts nothing about where such a group \
             sorts"
        );
    }

    /// An expected total reaches `i64::MAX`, which is the one integer no `f64` carries.
    ///
    /// `2^63 - 1` has 63 significant bits, so it rounds to `2^63` in a 64-bit float. Without a
    /// value there, every wide-integer arm in every bound adapter - a `HUGEINT`, a `NUMERIC` and an
    /// `Int64` - is interchangeable with a 32-bit read and with a float, and nothing says so.
    #[test]
    fn an_expected_total_reaches_the_i64_boundary() {
        let reached = corpus::cases().iter().any(|case| {
            case.expected()
                .rows()
                .iter()
                .flatten()
                .any(|cell| *cell == Value::Integer(i64::MAX))
        });
        assert!(
            reached,
            "no expected answer reaches `i64::MAX`, so the corpus cannot tell a wide-integer arm \
             from a narrow one or from an `f64`"
        );
    }

    /// A `SUM` answers a real number, which it can do only over a fractional column.
    ///
    /// The `AVG` case answers one too, and by a different route: its real number comes from an
    /// aggregate's division, and for Postgres from a cast `sutura_sql` applies to an `AVG`. This
    /// asserts the OTHER route - each adapter's own type inference over a fractional literal on the
    /// load path - so a corpus whose only fractional answer is the mean cannot satisfy it.
    #[test]
    fn a_sum_in_the_corpus_answers_a_real_number() {
        let found = corpus::cases().iter().any(|case| {
            let sums = matches!(
                *case.plan().measure(),
                PlanMeasure::Simple {
                    term: PlanTerm::Aggregate {
                        aggregate: Aggregate::Sum,
                        ..
                    }
                }
            );
            sums && case
                .expected()
                .rows()
                .iter()
                .flatten()
                .any(|cell| matches!(*cell, Value::Real(_)))
        });
        assert!(
            found,
            "no `SUM` in the corpus answers a real number, so no measure column is fractional and \
             the fractional class is only ever reached through a division"
        );
    }
}

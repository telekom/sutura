//! The conformance packs: one set of test bodies over the ports, bound to an adapter by a macro.
//!
//! `docs/adr/0012` is the construction and this crate is the first piece of it built. The
//! requirement it serves is that **a new data system proves itself by registering and declaring**
//! rather than by anybody editing a test - so the bodies live here, written once against the port,
//! and an adapter contributes a constructor.
//!
//! ```ignore
//! // In the adapter's own crate, in `tests/conformance.rs`.
//! #[cfg(test)]
//! mod conformance {
//!     fn open() -> Fixture<DuckDbWarehouse> { /* attach `corpus::on_disk()` */ }
//!
//!     sutura_conformance::execute_packs! {
//!         adapter: duckdb,
//!         warehouse: sutura_exec_duckdb::DuckDbWarehouse,
//!         open: crate::conformance::open,
//!         executes_legs,
//!     }
//! }
//! ```
//!
//! # Three properties, and each is the reason for a rule below
//!
//! 1. **The packs live in their own crate**, depending on `sutura-domain` and on no adapter - so the
//!    harness is a dependency an adapter's own crate can take rather than a directory another
//!    crate's tests reach into sideways. That is what `crates/sutura-exec-bigquery/tests/corpus.rs`
//!    could not do and had to hand-write instead.
//! 2. **Every behaviour keeps its own name per adapter.** A generic function per pack would give one
//!    test name per adapter, so a failure would say *the duckdb pack failed* and not which
//!    behaviour. [`execute_packs`] exists only to give each behaviour a name the runner reports and
//!    a filter selects.
//! 3. **A behaviour an adapter cannot satisfy, or that stopped existing, is never a silent pass.**
//!    Four mechanisms, because the cases are different and they are not equally strong - see below.
//!
//! # How an unsupported capability is kept out of the green count
//!
//! | Case | Mechanism |
//! | --- | --- |
//! | A capability with a **typed declaration** on the port (`EXECUTES_LEGS`) | The binding names the declaration, a `const` assertion tears a binding that disagrees with it, and the two directions get DIFFERENT test names - so which one ran is in the report rather than in a skip nobody reads |
//! | A capability with **no constant to declare** (`Warehouse::dry_run`) | The pack returns [`Outcome::Declined`] carrying a typed [`Declination`], which [`hold`] prints as `DECLINED` and `.config/nextest.toml`'s second override keeps on a green run. Stated limit: it is observed at RUN TIME, so it is weaker than the row above and would become that row the day the port carries the constant |
//! | A behaviour that loses its test | The `#[test]`s and the list [`census`] compares are ONE repetition inside [`execute_packs`], so a test cannot be deleted without deleting its census element, and the element is compared against [`Behaviour::EVERY`]. **This is the corrected version:** it was two hand-written lists 370 lines apart in this file, and a review deleted the content behaviour's test while leaving the variant in both - every census passed |
//! | The corpus being empty, which would make every behaviour vacuously green | [`census`] fails on a corpus with no cases, and prints the case count beside the behaviour count. Reachable as a [`Fault::EmptyCorpus`] too, through [`execute::a_leg_is_refused_over`] |
//! | The **environment** a networked adapter needs not being here, which is not a case above because it is not about the adapter at all | [`Fixture`] is what a binding's `open` returns, so an absence is a VALUE rather than a panic in a fixture; [`not_here`] prints `NOT RUN` under the behaviour's own name, and [`census`] prints no coverage line at all where the fixture did not stand up. The skip-or-fail DIRECTION is deliberately not this crate's - a provisioner that set `SUTURA_DEV_REQUIRE_TIER` gets a failure out of `sutura_dev::provisioned::here` before an absence can reach here at all |
//!
//! # What a green conformance run does NOT establish
//!
//! - **Ordering within a leg, and impersonation in any form.** [`execute`]'s header says which and
//!   why; `docs/adr/0012` decides the impersonation one.
//! - **Most of the port.** The packs call `execute` and `dry_run`. `verify_anchor`,
//!   `working_set_exhausted`, `result_did_not_fit`, `preflight` and `preflight_was_refused` are
//!   never called, so *held to the same test bodies* is a statement about two methods and not about
//!   `Warehouse`. Three of those five carry guarantees of their own in
//!   `.agents/skills/sutura/invariants`, held by other mechanisms.
//! - **That the corpus is hard.** It is two questions over one table - [`corpus`] lists by name the
//!   cases `docs/adr/0012` says nothing else finds, none of which is here yet.
//! - **That every adapter is held IS held now, and not by anything in this crate.** A pack is bound
//!   where an adapter's own crate binds it, so which adapters conform used to be a reading of which
//!   crates carry a `tests/conformance.rs` - deleting one left `just validate` green.
//!   `cargo xtask check-conformance-bindings` compares the golden matrix's `data_systems` registry
//!   against the crates holding a binding, with one declared exemption. **Its limit is the one this
//!   crate cannot help with:** it holds that a registered data system HAS a binding, never that a
//!   pack's body asserts anything - the four mechanisms above are what cover that, and a pack
//!   returning `Ok` unconditionally passes all of them and the gate.
//! - **That a binding which reports its fixture ABSENT actually asked anything.** [`Fixture`] is a
//!   type a binding fills in, and this crate cannot see a socket: `xtask/src/boundaries/harness.rs`
//!   holds it to `sutura-domain` alone, and that gate's own remedy assigns *reaching a provisioned
//!   tier* to the adapter's fixture. So a fixture that answered [`Fixture::Absent`] without looking
//!   would take its tier quiet and green, and the diff is where that one line is read. What the
//!   trust is worth in the venues that count is decided elsewhere and is not small: `just test`,
//!   `just gates`, `just causality`, `nix/run-gate.sh tests` and `checks.nextest` all provision the
//!   Postgres tier and export `SUTURA_DEV_REQUIRE_TIER`, and `sutura_dev::provisioned::here` PANICS
//!   in that direction - so a fixture that asks honestly cannot answer `Absent` there.
//! - **A COST, rather than a budget.** [`Spent`] reports what every cell and every fixture took,
//!   and [`census`] prints the per-adapter floor; nothing thresholds either, and nothing joins two
//!   adapters' numbers. `docs/adr/0012` carries what the remaining half would need.

/// The domain, re-exported so [`execute_packs`] can name the port without the consuming crate
/// having to depend on `sutura-domain` under that spelling.
///
/// A `macro_rules!` body resolves item paths at the EXPANSION site, so a bare `sutura_domain::` in
/// the expansion would compile only for a consumer that happens to have that dependency under that
/// name. `$crate::sutura_domain` always resolves.
pub use sutura_domain;

pub mod corpus;
pub mod execute;

use sutura_domain::warehouse::Warehouse;
use sutura_domain::warehouse::agreement::{ContentDisagreement, OrderDisagreement};

/// One behaviour in a pack: the unit a test name, a failure report and a CI filter all key on.
///
/// An enum rather than a string, so the pack's own list and the tests the macro emits are compared
/// by the compiler at one end and by [`census`] at the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// The answer's labels are the ones the plan projects.
    Labels,
    /// The rows are the reference's rows, as a multiset.
    Content,
    /// The rows are in the order the plan's `ORDER BY` claims.
    Order,
    /// One plan, asked twice, answered the same way twice.
    Determinism,
    /// A pre-flight that accepted the plan is followed by an answer.
    PreFlight,
    /// A leg is executed, or refused, as the adapter's declaration says.
    ///
    /// One behaviour and two directions rather than two behaviours: exactly one of them applies to
    /// any adapter, and which one is decided by `EXECUTES_LEGS`. The direction is in the emitted
    /// test's NAME, which is where a reviewer reads it.
    Leg,
}

impl Behaviour {
    /// Every behaviour a binding emits a test for, in the order it emits them.
    ///
    /// **The list [`census`] compares a binding against, and the pack's half of a pair that cannot
    /// be edited apart.** The other half is the one repetition inside [`execute_packs`] that emits
    /// the `#[test]`s: each entry there produces a test AND its census element, so a test cannot be
    /// deleted without deleting the element, and the element is compared against this list.
    ///
    /// **That pairing is the correction, and it was earned.** This started as two hand-written
    /// lists 370 lines apart in this file, and a review deleted the `#[test]` for [`Self::Content`],
    /// which [`execute`] calls *the* conformance claim, from the emitting arm while leaving the
    /// variant in both lists. Every census passed, in all four bindings, and the only signal was 38
    /// tests becoming 34. A list compared against a list says nothing about what ran.
    ///
    /// [`Self::Leg`] is in here, at the end, because exactly one of its two directions is emitted
    /// per adapter and both are emitted through the same repetition - so the count is derived
    /// rather than an increment somebody wrote next to it.
    pub const EVERY: &'static [Self] = &[
        Self::Labels,
        Self::Content,
        Self::Order,
        Self::Determinism,
        Self::PreFlight,
        Self::Leg,
    ];

    /// The name a report carries.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Labels => "the answer is labelled as the plan projects it",
            Self::Content => "the rows are the reference rows",
            Self::Order => "the rows are in the order the plan claims",
            Self::Determinism => "one plan asked twice is answered the same way",
            Self::PreFlight => "a pre-flight that accepts is followed by an answer",
            Self::Leg => "a leg is handled as the declaration says",
        }
    }
}

/// What running one behaviour against one adapter established.
///
/// Two variants and no third, because *this adapter cannot do that* and *this adapter did that* are
/// the only honest readings of a behaviour that did not fail. A boolean here would collapse them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The behaviour ran and holds.
    Held,
    /// The adapter cannot satisfy it, and said which way.
    Declined(Declination),
}

/// Why an adapter declined a behaviour.
///
/// Typed rather than a message, for the reason every refusal in this workspace is: a reader that
/// matched on the text would be depending on the text. One variant today; a second arrives with the
/// behaviour that can be declined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Declination {
    /// `dry_run` answered `NotAsked` for every case, so nothing was checked before the rows were
    /// read.
    ///
    /// The port's own default, and the honest answer for an adapter where checking costs what
    /// running costs. Nothing about the check has been established, which is why this is a
    /// declination and not a pass.
    #[error("the adapter offers no pre-flight: `dry_run` answered `NotAsked`, so nothing was checked")]
    OffersNoPreFlight,
}

/// An adapter's fixture, or the reason this venue could not stand one up.
///
/// **The type every binding's `open` path returns, and it is the mechanism rather than a
/// convention.** [`execute_packs`] used to call `open` for a `W`, so an adapter whose data system
/// may not be reachable here had exactly one option - panic in its fixture - and therefore could
/// not be bound at all: `sutura-exec-postgres` was registered in the golden matrix and carried the
/// one declared exemption in `cargo xtask check-conformance-bindings` for precisely that reason
/// (`telekom/sutura#348`). A RETURN TYPE asks the question, so a binding cannot omit it and a
/// reviewer reads which answer it gave.
///
/// # Why this is not an [`Outcome`], which is the distinction the design turns on
///
/// [`Outcome::Declined`] is a statement about the ADAPTER - *this adapter cannot do that*, carrying
/// a typed [`Declination`]. An absent tier is a statement about the ENVIRONMENT. Collapsing the two
/// would make a green run over an absent Postgres indistinguishable from a green run against one,
/// which is the failure mode the packs were built against. So the two are reported under different
/// words ([`hold`] prints `DECLINED`, [`not_here`] prints `NOT RUN`) and decided at different
/// levels: a declination comes out of a pack that RAN, and an absence stops the pack running.
///
/// # What it does NOT establish
///
/// See this module's header: nothing here can tell an absence that was DISCOVERED from one that was
/// merely declared, and the reason the harness cannot is a dependency rule that has its own gate.
pub enum Fixture<W> {
    /// It stood up. What an in-process or in-memory adapter always answers.
    Standing(W),
    /// The environment this adapter needs is not here, so nothing was asked of it.
    Absent(Missing),
}

impl<W> Fixture<W> {
    /// It stood up.
    ///
    /// Named rather than left to the variant, so an in-process binding's last line reads as the
    /// answer it is and the two answers are spelled at the same length.
    #[inline]
    pub const fn standing(warehouse: W) -> Self {
        Self::Standing(warehouse)
    }

    /// The reason it did not, where it did not.
    #[inline]
    pub const fn missing(&self) -> Option<&Missing> {
        match *self {
            Self::Standing(_) => None,
            Self::Absent(ref why) => Some(why),
        }
    }
}

/// Why this venue could not stand a fixture up. **About the environment, never about the adapter.**
///
/// Typed rather than a message, for the reason every refusal in this workspace is: a reader that
/// matched on the text would be depending on the text. One variant today - a second arrives with
/// the first adapter whose absence is not a tier, and cloud state a run cannot create is the shape
/// that asks for it. It arrives WITH that adapter rather than ahead of it, because a variant
/// nothing constructs is a claim nothing provokes, and this crate has paid for one of those already
/// ([`Fault::EmptyCorpus`], which needed a seam before it was reachable at all).
#[derive(Debug, Clone, thiserror::Error)]
pub enum Missing {
    /// A service this adapter reaches over a socket, which nothing has provisioned here.
    #[error("no `{service}` is provisioned in this venue, so nothing was asked of this adapter - {diagnostic}")]
    Tier {
        /// The service the provisioner was asked for.
        service: String,
        /// The provisioner's own diagnostic, which names the worktree, the file it read and the
        /// task that starts the tier.
        diagnostic: String,
    },
}

impl Missing {
    /// A tier this venue has not provisioned, carrying the provisioner's own diagnostic.
    ///
    /// The diagnostic arrives as a `Display` rather than as a `String`, so what reaches a reader is
    /// the sentence the provisioner wrote rather than one a binding composed beside it. That
    /// remedy is derived per venue and three checks hold it; a binding restating it would be a
    /// fourth copy with no mechanism.
    #[must_use]
    pub fn tier(service: &str, diagnostic: &impl core::fmt::Display) -> Self {
        Self::Tier {
            service: String::from(service),
            diagnostic: diagnostic.to_string(),
        }
    }
}

/// What one cell did once the environment had its say.
///
/// Private, and deliberately: the vocabulary a BINDING needs is [`Fixture`] and the vocabulary a
/// READER needs is the printed line, so a third public enum would be an API for a decision nobody
/// outside [`conduct`] makes.
enum Attempted<E>
where
    E: core::error::Error + 'static,
{
    /// The fixture stood up and the behaviour answered.
    Ran(Conformed<E>),
    /// It did not, so the behaviour never ran.
    NotHere(Missing),
}

/// What one cell of the matrix cost, measured rather than stated.
///
/// **`docs/adr/0012` said per-pack timings were reported *from the start* and nothing measured
/// one** (`telekom/sutura#353`). The record's own argument for having them is the one that governs
/// every number in this repository: a conformance matrix grows multiplicatively - adapters times
/// behaviours times cases - so *the tier that is supposed to be fast stops being fast quietly*, and
/// the fast tier is defended with a measurement or it is defended with a feeling.
///
/// # Two numbers, and the seam between them is where the multiplication is
///
/// [`execute_packs`] calls the binding's `open` once per BEHAVIOUR, so the fixture - opening the
/// adapter and attaching the corpus - is paid once per emitted test rather than once per binding.
/// That is deliberate, because no state may cross between tests, and it is also the term that
/// grows fastest - so one total would hide the thing a reader needs: whether a slow cell is a slow
/// behaviour or a slow fixture paid six times.
///
/// # It cannot be fabricated, which is why it is a type
///
/// The fields are private and both constructors MEASURE. A `Duration` parameter would have let a
/// caller report a number nobody took, which is the shape this repository has already paid for: a
/// count in a message is not a witness.
///
/// # What it does not reach, next to the claim
///
/// **Nothing joins two adapters' numbers.** A pack is a behaviour name shared across adapters, and
/// each binding is its own test binary in its own crate - under nextest each test is its own
/// PROCESS - so no value here can see another binding's. Aggregating per pack ACROSS adapters
/// needs a reader of a run's machine-readable output, which is a gate rather than a measurement;
/// `docs/adr/0012` carries that split. **And nothing thresholds any of this**: a budget with no
/// run beside it cannot be re-taken, so the report is the deliverable and a budget comes second
/// with its own measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spent {
    /// Building the fixture: the adapter opened and the corpus attached.
    fixture: core::time::Duration,
    /// Running the behaviour against it.
    pack: core::time::Duration,
}

impl Spent {
    /// Builds the fixture, runs the behaviour against it, and reports what each cost.
    ///
    /// Both halves are measured here rather than by the caller, so a cell's numbers and the work
    /// they are about cannot be paired wrongly and the split is the same split in every binding.
    pub fn measuring<W, T>(open: impl FnOnce() -> W, run: impl FnOnce(&W) -> T) -> (T, Self) {
        let building = std::time::Instant::now();
        let warehouse = open();
        let fixture = building.elapsed();
        let running = std::time::Instant::now();
        let answered = run(&warehouse);
        (
            answered,
            Self {
                fixture,
                pack: running.elapsed(),
            },
        )
    }

    /// The fixture alone: what [`census`] measures, because it runs no behaviour.
    #[must_use]
    pub fn building<W>(open: impl FnOnce() -> W) -> Self {
        let ((), spent) = Self::measuring(open, |_| ());
        spent
    }

    /// What the fixture cost.
    #[inline]
    #[must_use]
    pub const fn fixture(self) -> core::time::Duration {
        self.fixture
    }

    /// What the behaviour cost, once the fixture was standing.
    #[inline]
    #[must_use]
    pub const fn pack(self) -> core::time::Duration {
        self.pack
    }
}

impl core::fmt::Display for Spent {
    /// One decimal, through `Duration`'s own precision-aware formatter.
    ///
    /// **Not a hand-rolled millisecond conversion**, and the reason is a lint rather than taste:
    /// `clippy::float_arithmetic` is denied here, and an integer one needs a division that
    /// `clippy::integer_division` refuses - so the honest option is the one std already has, which
    /// also picks the unit rather than forcing a sub-millisecond cell to read `0.0`.
    ///
    /// A cheap honest number beside every behaviour is worth more than a precise one that never
    /// ships, and this shape is what makes the per-pack aggregate a `grep` over a green run: the
    /// behaviour's own name is on the same line.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let total = self.fixture.saturating_add(self.pack);
        write!(f, "{total:.1?} (fixture {:.1?} + pack {:.1?})", self.fixture, self.pack)
    }
}

/// Why a behaviour did not hold.
///
/// Generic in the adapter's own error, so a data system's typed failure survives to the report
/// instead of being flattened into a sentence at the pack boundary. Every variant names the case, so
/// a corpus of many says which one.
#[derive(Debug, thiserror::Error)]
pub enum Fault<E>
where
    E: core::error::Error + 'static,
{
    /// The data system did not answer at all.
    #[error("case `{case}`: the data system did not answer")]
    NotAnswered {
        case: &'static str,
        #[source]
        cause: E,
    },
    /// The answer is not labelled the way the plan projects it.
    #[error("case `{case}`: the plan projects {projected:?} and the answer is labelled {answered:?}")]
    Labels {
        case: &'static str,
        projected: Vec<String>,
        answered: Vec<String>,
    },
    /// The rows are not the reference's rows.
    // The disagreement is NOT interpolated here, and that is deliberate: `hold` walks the cause
    // chain, so a message that carried its own source printed it twice - measured on a real
    // failure, which read `... <- ...` with both halves identical.
    #[error("case `{case}`: the rows are not the reference's, compared as a multiset")]
    Content {
        case: &'static str,
        #[source]
        disagreement: Box<ContentDisagreement>,
    },
    /// The rows are not in the order the plan claims.
    #[error("case `{case}`: the rows agree and the order does not, and the plan's ORDER BY claims one")]
    Order {
        case: &'static str,
        #[source]
        disagreement: Box<OrderDisagreement>,
    },
    /// The pre-flight refused a plan the adapter is expected to be able to execute.
    #[error("case `{case}`: the pre-flight refused a plan this adapter is held to answer")]
    PreFlightRefused {
        case: &'static str,
        #[source]
        cause: E,
    },
    /// The pre-flight accepted the plan and the execution then failed.
    #[error("case `{case}`: the pre-flight accepted this plan and the execution did not answer")]
    AcceptedThenDidNotAnswer {
        case: &'static str,
        #[source]
        cause: E,
    },
    /// An adapter that declares it does not execute a leg executed one.
    #[error("case `{case}`: the adapter declares it does not execute a leg and answered one with {rows} row(s)")]
    ALegWasAnswered { case: &'static str, rows: usize },
    /// The corpus has no cases, so nothing could be asked.
    ///
    /// Reachable only from a pack that needs a case to establish the adapter is live. [`census`]
    /// fails on the same condition for every other behaviour.
    #[error("the corpus holds no cases, so this behaviour asserted nothing")]
    EmptyCorpus,
}

/// What one behaviour of one pack answers.
pub type Conformed<E> = Result<Outcome, Fault<E>>;

/// Reports one behaviour with what it cost, and fails the test if it did not hold.
///
/// The only place in this crate that ends a test, so what a failure prints is decided once: the
/// adapter, the behaviour, the cost, and the whole cause chain. `Display` on a `thiserror` enum
/// prints the outermost message and stops, and the outermost message here is the pack's - what
/// tells a rejected statement from an outage is one and two levels down.
///
/// **The cost is on the failing line too**, and that is not symmetry for its own sake: a cell that
/// failed in two milliseconds and one that failed after thirty seconds are different diagnoses, and
/// the second is the one `docs/adr/0012` says goes quiet.
#[expect(
    clippy::panic,
    reason = "this is the pack boundary's one assertion: a conformance failure IS a test failure, \
              and the panic message is what makes the report name the behaviour rather than only \
              the adapter"
)]
pub fn hold<E>(adapter: &str, behaviour: Behaviour, conformed: Conformed<E>, spent: Spent)
where
    E: core::error::Error + 'static,
{
    match conformed {
        Ok(Outcome::Held) => println!("conformance {adapter}: HELD - {} - {spent}", behaviour.as_str()),
        Ok(Outcome::Declined(why)) => {
            println!("conformance {adapter}: DECLINED - {} - {spent} - {why}", behaviour.as_str());
        }
        Err(fault) => panic!("conformance {adapter}: {} - {spent} - {}", behaviour.as_str(), chain(&fault)),
    }
}

/// Reports a behaviour that did not run, because this venue could not stand the fixture up.
///
/// **`NOT RUN` in the first column, and not `DECLINED`**, for the reason [`Fixture`]'s header
/// gives: one word is about the adapter and the other is about the environment, and a reader who
/// could not tell them apart would read a green run over an absent Postgres as a green run against
/// one. `.config/nextest.toml` already keeps this line on a green run - it scopes
/// `success-output` by BINARY, so `binary(conformance)` covers it with no second edit.
///
/// **It does not end the test, and that direction is deliberately not this crate's to choose.**
/// `sutura_dev::requirement` decides skip-or-fail once for every harness in this repository, from
/// `SUTURA_DEV_REQUIRE_TIER`, and only the thing that provisioned a tier may declare one - so where
/// a tier was required, `sutura_dev::provisioned::here` has already failed the run inside the
/// binding's fixture and nothing reaches here.
pub fn not_here(adapter: &str, behaviour: Behaviour, missing: &Missing, spent: Spent) {
    println!(
        "conformance {adapter}: NOT RUN - {} - {spent} - {missing}",
        behaviour.as_str()
    );
}

/// Builds the fixture, runs the behaviour where the environment stood one up, and reports either way.
///
/// **The one place a cell's endings are decided**, which is why the test [`execute_packs`] emits is
/// a single call to this: HELD or DECLINED through [`hold`], `NOT RUN` through [`not_here`], and a
/// [`Fault`] is [`hold`]'s panic.
///
/// Both halves of [`Spent`] are measured around the work they are about, so the pack half of an
/// absent fixture is a measurement of nothing rather than a number nobody took - and the fixture
/// half is still real, because asking a provisioner and being told no costs something.
///
/// The pack arrives as `impl FnOnce(&W) -> Conformed<E>` and every binding hands over a FUNCTION
/// ITEM, which is what keeps `clippy::result_large_err` off the adapter's own crate: that lint
/// inspects a closure's return type at its definition site, and the one closure this needs is
/// defined here, generic in the adapter's error, rather than six times per binding.
pub fn conduct<W, E>(
    adapter: &str,
    behaviour: Behaviour,
    open: impl FnOnce() -> Fixture<W>,
    pack: impl FnOnce(&W) -> Conformed<E>,
) where
    E: core::error::Error + 'static,
{
    let (attempted, spent) = Spent::measuring(open, |fixture| match *fixture {
        Fixture::Standing(ref warehouse) => Attempted::Ran(pack(warehouse)),
        Fixture::Absent(ref missing) => Attempted::NotHere(missing.clone()),
    });
    match attempted {
        Attempted::Ran(conformed) => hold(adapter, behaviour, conformed, spent),
        Attempted::NotHere(ref missing) => not_here(adapter, behaviour, missing, spent),
    }
}

/// What a binding actually covered, asserted and printed - with the cost of covering it.
///
/// Five things, and the first is the one a review had to correct:
///
/// 1. **the behaviours the binding actually emitted tests for are [`Behaviour::EVERY`]**. `bound`
///    is not a second hand-written list: [`execute_packs`] generates it from the same repetition
///    that generates the `#[test]`s, one element per emitted test, so a deleted test is a deleted
///    element and this comparison reddens. The version this replaced compared two lists nobody had
///    tied together, and deleting the content behaviour's test left every census green;
/// 2. the corpus is not empty, which is the state that would make every behaviour above vacuously
///    green;
/// 3. the counts are PRINTED - behaviours, cases, and which direction the leg declaration selected.
///    A suite that reports a ratio it has not earned is the failure this repository has already met
///    twice, and `.config/nextest.toml`'s second override is what makes this line survive a green
///    run instead of being captured and discarded;
/// 4. **the per-adapter FLOOR is printed, from a measurement.** `docs/adr/0012` asks for timings
///    aggregated per pack and per adapter (`telekom/sutura#353`); this is the per-adapter half that
///    a test process can actually take. [`execute_packs`] rebuilds the fixture once per behaviour,
///    so `behaviours x fixture` is the cost this binding pays before a single assertion runs - the
///    multiplicative term the record's *stops being fast quietly* is about. It is derived from the
///    same `bound` slice the comparison above uses, so the multiplier is the number of tests that
///    were actually emitted rather than a constant beside it;
/// 5. **a venue where the fixture did not stand up prints NO coverage line at all.** It takes the
///    `open` path rather than a [`Spent`] for exactly this: a census that printed *6 behaviour(s)
///    over 2 case(s)* beside six cells that each reported `NOT RUN` is the skip that reads as
///    coverage, which is the failure mode the packs were built against. What it prints instead
///    names the count as one that asserted nothing, and carries the provisioner's diagnostic. The
///    two assertions above it still run, because what a binding emitted and whether the corpus has
///    cases are facts about this tree rather than about this venue.
///
/// What it cannot do: know that a behaviour's BODY asserts anything. A pack that returned `Ok`
/// unconditionally passes every census, which is what `tests/bound.rs`'s fault half is for. And
/// the floor is a FLOOR: it is not the tier's cost, it says nothing about another adapter's cells,
/// and no gate reads it - see [`Spent`] for why each of those is deliberate.
pub fn census<W>(adapter: &str, bound: &[Behaviour], open: impl FnOnce() -> Fixture<W>)
where
    W: Warehouse,
{
    assert_eq!(
        bound,
        Behaviour::EVERY,
        "conformance {adapter}: the binding emitted tests for {bound:?} and the pack defines {:?} - \
         a behaviour without a test is coverage this run did not earn",
        Behaviour::EVERY
    );
    let cases = corpus::cases().len();
    assert!(
        cases > 0,
        "conformance {adapter}: the corpus holds no cases, so every behaviour above is green over \
         nothing"
    );
    // Both assertions ABOVE the fixture, and that order is the point: what a binding emitted and
    // whether the corpus has cases are facts about this tree, so a venue with no tier still holds
    // them. Only the FLOOR needs a fixture, and a floor is what an absent one cannot have.
    let (missing, fixture) = Spent::measuring(open, |built| built.missing().cloned());
    if let Some(why) = missing {
        // Deliberately NOT the coverage line below. Every behaviour in this binding reported
        // `NOT RUN`, so printing a behaviour count, a case count and a floor here would be the skip
        // that reads as coverage - which is the failure mode the packs were built against.
        println!(
            "conformance {adapter}: NOT RUN - the {} behaviour(s) this binding declares asserted \
             nothing, because no fixture stood up here - {why}",
            bound.len()
        );
        return;
    }
    let direction = if W::EXECUTES_LEGS {
        "EXECUTED (the adapter declares it executes legs)"
    } else {
        "REFUSED (the adapter declares it does not)"
    };
    let floor = fixture
        .fixture()
        .saturating_mul(u32::try_from(bound.len()).unwrap_or(u32::MAX));
    println!(
        "conformance {adapter}: {} behaviour(s) over {cases} case(s), and the leg one {direction} - \
         the fixture costs {:.1?} and every behaviour rebuilds it, so this binding's floor is \
         {floor:.1?}",
        bound.len(),
        fixture.fixture(),
    );
}

/// An error and every cause beneath it, as one string.
fn chain(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str(" <- ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

/// Binds the execute pack to one adapter, as one named `#[test]` per behaviour.
///
/// ```ignore
/// #[cfg(test)]
/// mod conformance {
///     fn open() -> Fixture<DuckDbWarehouse> { /* attach `corpus::on_disk()` */ }
///
///     sutura_conformance::execute_packs! {
///         adapter: duckdb,
///         warehouse: sutura_exec_duckdb::DuckDbWarehouse,
///         open: crate::conformance::open,
///         executes_legs,
///     }
/// }
/// ```
///
/// # The four arguments
///
/// - `adapter` is an **ident**, not a string, and that is the whole of what decides `macro_rules!`
///   over a proc-macro. A `literal` in `mod $name` position is `error: expected identifier, found
///   metavariable`, and getting from a string to a path segment needs a paste-style crate. With an
///   ident the expansion is `mod duckdb { #[test] fn .. }` and the naming scheme NESTS rather than
///   concatenates, which is the one thing `macro_rules!` cannot do and the only reason a proc-macro
///   would have been needed.
/// - `warehouse` is the adapter type. It is what the `const` assertion below reads the declaration
///   off, and what [`census`] is instantiated at.
/// - `open` is a **path** to an [`fn() -> Fixture<W>`](Fixture), called once per test so no state
///   crosses between them. A path rather than a closure because a `macro_rules!` body resolves
///   items at the expansion site: a closure naming a type the caller imported at file scope would
///   not resolve inside the generated module, and a `crate::`-rooted path always does. The fixture
///   lives in a `#[cfg(test)]` module because the strict lints exempt what is inside one, which is
///   why the path in the example names that module. **The return type is [`Fixture`] and not `W`**,
///   which is what lets an adapter needing a provisioned service be bound at all - see that type
///   for why an absence is a value here rather than a panic, and why it is not an
///   [`Outcome::Declined`].
/// - the last tag is the adapter's leg **declaration**, `executes_legs` or `refuses_legs`.
///
/// # Why the declaration is written at the binding as well as on the adapter
///
/// `macro_rules!` cannot read an associated constant, so a macro cannot branch on
/// `Warehouse::EXECUTES_LEGS` to choose which test to emit. The tag is this crate's routing copy of
/// it, and the arm's `const _: () = assert!(..)` is where the two are torn unless they agree: tag an
/// adapter against its own declaration and the binding does not build. That is the same mechanism
/// `sutura-app`'s catalog registry uses for `SemanticCatalog::KIND`, and it is what makes a
/// declaration a thing the compiler checks rather than a list somebody keeps in step.
///
/// # Selecting a tier
///
/// The emitted names are `<adapter>::<behaviour>` inside whatever module the invocation sits in, so
/// the convention above - `tests/conformance.rs`, wrapped in `mod conformance` - gives
/// `conformance::duckdb::the_rows_are_the_reference_rows`. One adapter's tier is then
/// `cargo nextest run --workspace --all-features -E 'test(conformance::duckdb)'` and every adapter's is
/// `cargo nextest run --workspace --all-features -E 'binary(conformance)'`, which is what `just test` runs as part of the
/// workspace. **Both are enforced now** - `cargo xtask check-conformance-bindings` refuses a
/// registered adapter whose binding is in another file or another module, with the selector
/// COMPUTED from where the invocation sits, because those two properties are what the filters above
/// rest on. What it still cannot see is an emitted test: the evidence is a written invocation and
/// its position.
#[macro_export]
macro_rules! execute_packs {
    (
        adapter: $name:ident,
        warehouse: $warehouse:ty,
        open: $open:path,
        executes_legs $(,)?
    ) => {
        // `#[cfg(test)]` on the generated module, and it is a lint that decides that rather
        // than taste: `clippy::tests_outside_test_module` refuses a `#[test]` that is not in
        // one, and a binding should not have to know. In an integration test target the
        // predicate is always on, so this changes nothing about what runs.
        #[cfg(test)]
        mod $name {
            const ADAPTER: &str = stringify!($name);

            const _: () = assert!(
                <$warehouse as $crate::sutura_domain::warehouse::Warehouse>::EXECUTES_LEGS,
                "an adapter bound with `executes_legs` must declare `EXECUTES_LEGS`"
            );

            $crate::execute_packs!(
                @bind $warehouse, $open,
                Leg => a_leg_is_executed_because_the_adapter_declares_it_executes_legs => a_leg_is_executed
            );
        }
    };

    (
        adapter: $name:ident,
        warehouse: $warehouse:ty,
        open: $open:path,
        refuses_legs $(,)?
    ) => {
        // `#[cfg(test)]`, for the reason the arm above gives.
        #[cfg(test)]
        mod $name {
            const ADAPTER: &str = stringify!($name);

            const _: () = assert!(
                !<$warehouse as $crate::sutura_domain::warehouse::Warehouse>::EXECUTES_LEGS,
                "an adapter bound with `refuses_legs` must not declare `EXECUTES_LEGS`"
            );

            $crate::execute_packs!(
                @bind $warehouse, $open,
                Leg => a_leg_is_refused_because_the_adapter_declares_it_does_not_execute_legs => a_leg_is_refused
            );
        }
    };

    // Every behaviour a binding is held to: the five that hold whatever an adapter declares, plus
    // the ONE leg direction its declaration selected, arriving as three tokens from the arm above.
    //
    // Internal, and one list rather than two. The leg cell is inside it rather than beside it,
    // which is what makes the census count derived: deleting the leg triple from either public arm
    // stops that arm matching this one, so it is a compile error rather than a smaller green run.
    (
        @bind $warehouse:ty, $open:path,
        $leg_variant:ident => $leg_name:ident => $leg_pack:ident
    ) => {
        $crate::execute_packs!(@behaviours $warehouse, $open, [
            Labels => the_answer_is_labelled_as_the_plan_projects_it => labels_are_the_plans_own,
            Content => the_rows_are_the_reference_rows => content_agrees_with_the_reference,
            Order => the_rows_are_in_the_order_the_plan_claims => order_agrees_with_the_reference,
            Determinism => one_plan_asked_twice_is_answered_the_same_way => one_plan_asked_twice_answers_the_same_way,
            PreFlight => a_pre_flight_that_accepts_is_followed_by_an_answer
                => a_preflight_that_accepts_is_followed_by_an_answer,
            $leg_variant => $leg_name => $leg_pack,
        ]);
    };

    // ONE repetition, expanded twice: once into the `#[test]`s and once into the array
    // [`census`](crate::census) compares against [`Behaviour::EVERY`](crate::Behaviour::EVERY).
    //
    // **That is the whole of the correction a review forced.** The two used to be hand-written
    // lists 370 lines apart, and deleting the content behaviour's `#[test]` while leaving the
    // variant in both left every census green - so the strongest claim this harness made was false.
    // With one repetition a test cannot be removed without removing its census element, and the
    // element is what the pack's own list is compared to.
    (
        @behaviours $warehouse:ty, $open:path,
        [$($variant:ident => $test_name:ident => $pack:ident),* $(,)?]
    ) => {
        $(
            #[test]
            fn $test_name() {
                // ONE call, because `conduct` is where a cell's endings are decided: it measures
                // the fixture and the behaviour separately (so the two numbers cannot be paired
                // with the wrong work), runs the behaviour only where the fixture stood up, and
                // reports HELD, DECLINED or NOT RUN. A binding that made that decision itself
                // would make it differently from the next binding, and one of them would make it
                // silently - which is the argument `sutura_dev::provisioned` makes one level down.
                //
                // The pack is handed over as a FUNCTION ITEM rather than wrapped in a closure, and
                // that is a lint rather than a style: `clippy::result_large_err` inspects a
                // closure's return type at its definition site, so `|w| pack(w)` reported every
                // adapter's own `Fault<E>` as too large to return - six errors per binding, in the
                // adapter's crate, about a type this crate owns.
                $crate::conduct(ADAPTER, $crate::Behaviour::$variant, $open, $crate::execute::$pack);
            }
        )*

        #[test]
        fn every_behaviour_this_declaration_selects_has_a_test_here() {
            // The fixture is opened HERE rather than passed in, so the number this binding's floor
            // is computed from is one this crate took, and so a venue that stood no fixture up
            // prints no coverage line. It costs one more `open` in a target that already pays one
            // per behaviour, which is the cheapest place to buy the multiplicative term
            // `docs/adr/0012` asks to have measured.
            $crate::census::<$warehouse>(ADAPTER, &[$($crate::Behaviour::$variant),*], $open);
        }
    };
}

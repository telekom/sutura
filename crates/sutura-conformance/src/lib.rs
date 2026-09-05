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
//!     fn open() -> DuckDbWarehouse { /* attach `corpus::on_disk()` */ }
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
//! 3. **A behaviour an adapter cannot satisfy is never a silent pass.** Two mechanisms, because the
//!    two cases are different - see below.
//!
//! # How an unsupported capability is kept out of the green count
//!
//! | Case | Mechanism |
//! | --- | --- |
//! | A capability with a **typed declaration** on the port (`EXECUTES_LEGS`) | The binding names the declaration, a `const` assertion tears a binding that disagrees with it, and the two directions get DIFFERENT test names - so which one ran is in the report rather than in a skip nobody reads |
//! | A capability with **no constant to declare** (a pre-flight) | The pack returns [`Outcome::Declined`] carrying a typed [`Declination`], which [`hold`] prints as `DECLINED`. Stated limit: this is observed at run time, so it is weaker than the row above and would become that row the day the port carries the constant |
//! | The corpus being empty, which would make every behaviour vacuously green | [`census`] fails on a corpus with no cases, and prints the case count beside the behaviour count so a run's ratio is read rather than assumed |
//!
//! # What a green conformance run does NOT establish
//!
//! - **Ordering within a leg, and impersonation in any form.** [`execute`]'s header says which and
//!   why; `docs/adr/0012` decides the impersonation one.
//! - **That the corpus is hard.** It is two questions over one table - [`corpus`] lists by name the
//!   cases `docs/adr/0012` says nothing else finds, none of which is here yet.
//! - **That every adapter is held.** A pack is bound where an adapter's own crate binds it, so which
//!   adapters conform is a question about which crates carry a `tests/conformance.rs`, and the
//!   answer is in those crates rather than here.

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
    /// The behaviours every adapter is held to, whatever it declares.
    ///
    /// **The list [`census`] compares a binding against.** A behaviour added to [`execute`] and not
    /// to [`execute_packs`] reddens the census rather than quietly reducing what a green run
    /// covered - which is the failure mode a suite reporting a ratio it had not earned is made of.
    pub const UNIVERSAL: &'static [Self] = &[Self::Labels, Self::Content, Self::Order, Self::Determinism, Self::PreFlight];

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

/// Reports one behaviour, and fails the test if it did not hold.
///
/// The only place in this crate that ends a test, so what a failure prints is decided once: the
/// adapter, the behaviour, and the whole cause chain. `Display` on a `thiserror` enum prints the
/// outermost message and stops, and the outermost message here is the pack's - what tells a rejected
/// statement from an outage is one and two levels down.
#[expect(
    clippy::panic,
    reason = "this is the pack boundary's one assertion: a conformance failure IS a test failure, \
              and the panic message is what makes the report name the behaviour rather than only \
              the adapter"
)]
pub fn hold<E>(adapter: &str, behaviour: Behaviour, conformed: Conformed<E>)
where
    E: core::error::Error + 'static,
{
    match conformed {
        Ok(Outcome::Held) => println!("conformance {adapter}: HELD - {}", behaviour.as_str()),
        Ok(Outcome::Declined(why)) => {
            println!("conformance {adapter}: DECLINED - {} - {why}", behaviour.as_str());
        }
        Err(fault) => panic!("conformance {adapter}: {} - {}", behaviour.as_str(), chain(&fault)),
    }
}

/// What a binding actually covered, asserted and printed.
///
/// Three things, and the third is why this exists rather than a comment:
///
/// 1. the universal behaviours the binding emitted tests for are the ones [`execute`] defines, so a
///    behaviour that lost its test reddens here;
/// 2. the corpus is not empty, which is the state that would make every behaviour above vacuously
///    green;
/// 3. the counts are PRINTED - behaviours, cases, and which direction the leg declaration selected -
///    because a suite that reports a ratio it has not earned is the failure this repository has
///    already met twice.
///
/// What it cannot do: know that a behaviour's body asserts anything. That is what a red-against-base
/// run is for.
pub fn census<W>(adapter: &str, bound: &[Behaviour])
where
    W: Warehouse,
{
    assert_eq!(
        bound,
        Behaviour::UNIVERSAL,
        "conformance {adapter}: the binding emits tests for {bound:?} and the pack defines {:?} - a \
         behaviour without a test is coverage this run did not earn",
        Behaviour::UNIVERSAL
    );
    let cases = corpus::cases().len();
    assert!(
        cases > 0,
        "conformance {adapter}: the corpus holds no cases, so every behaviour above is green over \
         nothing"
    );
    let direction = if W::EXECUTES_LEGS {
        "EXECUTED (the adapter declares it executes legs)"
    } else {
        "REFUSED (the adapter declares it does not)"
    };
    println!(
        "conformance {adapter}: {} behaviour(s) over {cases} case(s) - {} universal, and a leg {direction}",
        bound.len().saturating_add(1),
        bound.len(),
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
///     fn open() -> DuckDbWarehouse { /* attach `corpus::on_disk()` */ }
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
/// - `open` is a **path** to an `fn() -> W`, called once per test so no state crosses between them.
///   A path rather than a closure because a `macro_rules!` body resolves items at the expansion
///   site: a closure naming a type the caller imported at file scope would not resolve inside the
///   generated module, and a `crate::`-rooted path always does. The fixture lives in a
///   `#[cfg(test)]` module because the strict lints exempt what is inside one, which is why the
///   path in the example names that module.
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
/// workspace. **Nothing enforces the file name or the wrapper module**; they are a convention, and
/// an adapter that ignores them is still run by `just test` under a name a filter has to spell
/// differently.
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

            $crate::execute_packs!(@universal $warehouse, $open);

            #[test]
            fn a_leg_is_executed_because_the_adapter_declares_it_executes_legs() {
                $crate::hold(
                    ADAPTER,
                    $crate::Behaviour::Leg,
                    $crate::execute::a_leg_is_executed(&$open()),
                );
            }
        }
    };

    (
        adapter: $name:ident,
        warehouse: $warehouse:ty,
        open: $open:path,
        refuses_legs $(,)?
    ) => {
        // `#[cfg(test)]` on the generated module, and it is a lint that decides that rather
        // than taste: `clippy::tests_outside_test_module` refuses a `#[test]` that is not in
        // one, and a binding should not have to know. In an integration test target the
        // predicate is always on, so this changes nothing about what runs.
        #[cfg(test)]
        mod $name {
            const ADAPTER: &str = stringify!($name);

            const _: () = assert!(
                !<$warehouse as $crate::sutura_domain::warehouse::Warehouse>::EXECUTES_LEGS,
                "an adapter bound with `refuses_legs` must not declare `EXECUTES_LEGS`"
            );

            $crate::execute_packs!(@universal $warehouse, $open);

            #[test]
            fn a_leg_is_refused_because_the_adapter_declares_it_does_not_execute_legs() {
                $crate::hold(
                    ADAPTER,
                    $crate::Behaviour::Leg,
                    $crate::execute::a_leg_is_refused(&$open()),
                );
            }
        }
    };

    // The behaviours every adapter is held to, and the census over them. Internal: the list below
    // is the one `Behaviour::UNIVERSAL` is compared against, so it lives next to the tests it
    // enumerates rather than in either public arm.
    (@universal $warehouse:ty, $open:path) => {
        #[test]
        fn the_answer_is_labelled_as_the_plan_projects_it() {
            $crate::hold(
                ADAPTER,
                $crate::Behaviour::Labels,
                $crate::execute::labels_are_the_plans_own(&$open()),
            );
        }

        #[test]
        fn the_rows_are_the_reference_rows() {
            $crate::hold(
                ADAPTER,
                $crate::Behaviour::Content,
                $crate::execute::content_agrees_with_the_reference(&$open()),
            );
        }

        #[test]
        fn the_rows_are_in_the_order_the_plan_claims() {
            $crate::hold(
                ADAPTER,
                $crate::Behaviour::Order,
                $crate::execute::order_agrees_with_the_reference(&$open()),
            );
        }

        #[test]
        fn one_plan_asked_twice_is_answered_the_same_way() {
            $crate::hold(
                ADAPTER,
                $crate::Behaviour::Determinism,
                $crate::execute::one_plan_asked_twice_answers_the_same_way(&$open()),
            );
        }

        #[test]
        fn a_pre_flight_that_accepts_is_followed_by_an_answer() {
            $crate::hold(
                ADAPTER,
                $crate::Behaviour::PreFlight,
                $crate::execute::a_preflight_that_accepts_is_followed_by_an_answer(&$open()),
            );
        }

        #[test]
        fn every_behaviour_this_declaration_selects_has_a_test_here() {
            $crate::census::<$warehouse>(
                ADAPTER,
                &[
                    $crate::Behaviour::Labels,
                    $crate::Behaviour::Content,
                    $crate::Behaviour::Order,
                    $crate::Behaviour::Determinism,
                    $crate::Behaviour::PreFlight,
                ],
            );
        }
    };
}

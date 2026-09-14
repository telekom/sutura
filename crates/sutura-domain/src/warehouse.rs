//! The execution port: the plan that goes out, and the rows that come back.
//!
//! `Warehouse` names the port, not whether its implementation is a file or a cluster.
//!
//! **No statement appears in this module, and its absence is the decision rather than an omission.**
//! The port takes a [`crate::plan::QueryPlan`] - [`Warehouse`] says why that is what makes a second
//! kind of adapter possible - so nothing in the domain constructs or reads a statement, and the type
//! carrying one lives in `sutura-sql` beside the code that renders it: a domain holding a rendered
//! statement has acquired a concept no domain operation uses. What stays is [`ParamValue`], because
//! a [`crate::plan::QueryPlan`] carries a vector of them and because the rule lives there - a value
//! is a closed set of typed variants an adapter binds, never text somebody concatenated.
//! [`crate::query`] is the *tool* surface, where SQL must be unrepresentable because the text comes
//! from a caller; here there is no text for a value to reach at all.

use std::collections::BTreeSet;

use crate::calendar::Date;
use crate::identity::Presented;
use crate::model::{QualifiedTable, SourceName};
use crate::plan::{AnchorPlan, Executable};
use crate::source::{ImpersonationCapability, SourcePosture};

/// Shared typing for deliberately simple CSV fixtures.
#[cfg(any(test, feature = "fixtures"))]
pub mod csv;
pub mod estimate;
/// The pre-flight's own vocabulary: what a data system said about the tables a bundle names.
///
/// **`pub mod` with no re-export beside it, and that is a documentation decision rather than a
/// style one.** The domain's usual shape - a private submodule plus a `pub use` - did NOT inline
/// here: `just api` generated `### use None` stubs and no content, so the published reference would
/// have carried a port method returning a type it does not describe. A public module gets
/// documented.
pub mod preflight;
use estimate::EstimatedBytes;

/// What it takes for two answers to one plan to be the same answer, for the differential legs that
/// compare them.
///
/// Behind a default-off feature, and `cfg(test)` so this crate's own suite reaches it either way -
/// the shape `sutura_runtime::testing`'s `test-capture` established. Here rather than in each test
/// target because two copies of a comparison policy is how both of them came to erase the cell type;
/// its own header carries that story and the limits.
///
/// **That header links the module's OWN items by absolute `crate::` path, and that is not style.**
/// rustc merges this `///` block with the module's `//!` one and resolves it in THIS scope, where
/// `agreement`'s items are not - so a bare-name link there resolves to nothing, and no gate here
/// reads a rustdoc warning (#321). Four were shipped that way.
#[cfg(any(test, feature = "agreement"))]
pub mod agreement;

/// Whether a declared join key is really unique in the table it points at.
///
/// A module of its own for [`preflight`]'s reason - nothing in it is about a plan, a credential or a
/// row - and its header carries the measurement that made the check necessary: one violated
/// `many_to_one`, two topologies, two numbers, and a refusal from neither.
pub mod cardinality;

/// One cell of a result, and the checked real a cell may carry.
pub mod cell;
/// One absolute deadline per answer, and the budget it was opened from.
///
/// A module of its own rather than a type or two added here, for the reason [`cardinality`]
/// already gives - and because this file was at the `max-lines` cap the day the record needed
/// somewhere to grow (`docs/adr/0029`). `Deadline` and `Budget` are used unqualified below, the
/// same way [`cardinality`]'s two types are.
pub mod deadline;
/// A result set, and an anchor's rows.
pub mod rows;

pub use cell::{NotFinite, Real, Value};
pub use rows::{AnchorRows, MalformedRowSet, RowSet};

use crate::warehouse::cardinality::{DeclaredKey, KeyUniqueness};
use crate::warehouse::deadline::Deadline;
use crate::warehouse::preflight::TablesPresent;

/// A value bound to a placeholder.
///
/// A closed set rather than a string, because the whole point is that these never become text on
/// our side. An adapter binds them with whatever its driver offers, and the driver is what decides
/// how a date is written on the wire.
///
/// **There is no `Integer`, and its absence is the decision rather than an omission.** Nothing in
/// the workspace constructed one - every caller value and every required filter binds as
/// [`Text`](ParamValue::Text), because that is the type both of them are - while both adapters
/// carried an arm and the goldens carried a rendering, so it read as covered while no question could
/// reach it. The dead arm was the lesser half of the cost. The real half: a *numeric* definitional
/// filter cannot be expressed safely here. `equals: { column: amount_cents, value: "500" }` compares
/// an integer column against a text parameter, `DuckDB` casts it and answers, a driver sending an
/// explicitly-typed text parameter does not, and nothing refuses the definition because a
/// [`crate::catalog::Model`] declares only column NAMES - there is no column type to check against.
/// Adding the variant back without one would mean guessing the type from the value's own text, which
/// makes a text column whose allowed value is `"500"` compare as a number: the same wrong
/// comparison from the other side.
///
/// So it goes when a typed column model does, and not before - the reasoning
/// `sutura_exec_datafusion`'s `cell` gives for leaving `Date64` unmapped: an unreachable arm holding
/// a semantic choice nobody reviewed is worse than not having the arm.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum ParamValue {
    Text(String),
    Date(Date),
}

impl ParamValue {
    /// A human-readable form, for showing a plan to a person.
    ///
    /// **Display only, and deliberately not the SQL literal:** a function producing one is what
    /// somebody reaches for the day they want to inline a parameter, which is the one move this type
    /// exists to prevent. Text is quoted the way `Debug` quotes it, so an empty or space-padded value
    /// is visible rather than SQL-shaped.
    pub fn render(&self) -> String {
        match *self {
            Self::Text(ref v) => format!("{v:?}"),
            Self::Date(d) => d.to_iso(),
        }
    }
}

/// What a pre-flight established.
///
/// **[`Self::NotAsked`] is not [`Self::Accepted`], and no caller can read it as one.** With a
/// subject in `dry_run`'s signature, a default of `Ok(())` stops being honest: it is
/// indistinguishable from an adapter that asked the data system as that subject and was told yes, so
/// a defaulted pre-flight would read as *this subject may run this plan* for every adapter that
/// declined to implement one. The shape is the row cap's, where `row_limit()` is `max_rows + 1` so a
/// result *at* the cap is distinguishable from one cut off *by* it. `docs/adr/0008` part 1 decides.
///
/// **The limit, stated with the claim:** [`Self::Accepted`] is the data system's opinion at
/// pre-flight time, not a guarantee about `execute` and not an authorization decision. Nothing in
/// the plan path may treat it as one, and no mechanism would stop it - skipping a check on the
/// strength of `Accepted` is a review question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreFlight {
    /// The adapter did not ask. The default, and the honest answer for an adapter where checking costs what running costs.
    NotAsked,
    /// The data system was asked, as this subject, and accepted the plan - see [`estimate::EstimatedBytes`].
    Accepted { estimated_bytes: Option<EstimatedBytes> },
}

/// Where a plan runs.
///
/// **The port takes a [`crate::plan::QueryPlan`], not a statement, and that is what makes a second
/// kind of adapter possible.** Taking a rendered statement said that every data system speaks SQL.
/// An in-process engine does not: it executes a logical plan over Arrow and generates no SQL at all.
/// So the plan is the contract and rendering is one adapter's private business.
///
/// `dry_run` exists separately from `execute` because "would this be accepted" is worth being able
/// to ask before committing to the cost of an answer - **where asking is cheaper than answering.**
/// It is defaulted rather than required for exactly that reason: an adapter for which it is not
/// cheaper has no way to say so if the port demands an implementation, and the honest thing for it to
/// do is nothing.
///
/// # Nothing here executes without saying whose credential it holds
///
/// [`Self::execute`] takes a [`Presented`] and has no default, so there is no code path into a data
/// system that runs as whatever the process happens to be. **Today's signature IS the fallback:** an
/// adapter with no credential parameter runs as the process, and nothing anywhere had to decide
/// that. `docs/adr/0008` part 1 is the decision, and the mechanism is the absence of a signature
/// rather than a rule somebody follows.
///
/// The boot path is the other caller of this port and it has no subject, so it gets its own method:
/// [`Self::verify_anchor`] takes no credential and returns [`AnchorRows`] rather than a [`RowSet`].
/// **Which is narrower than the record asked for, deliberately.** `docs/adr/0008` gave that method a
/// `VerificationIdentity` parameter so the two credentials could not be confused at a call site, and
/// then named a `compile_fail` test asserting that answering a question cannot pass one. That test
/// could not have held: `crate::source::VerificationIdentity::parse` is `pub`, so any crate can
/// construct one. A method that takes NO credential has no parameter to pass one to, which is the
/// property the record wanted, reached by removing the argument instead of by typing it.
///
/// **What bounds a method with no credential is WHERE it is called from, and that is a lint here
/// rather than a type - which is a second review's correction to a claim this comment used to make.**
/// The first correction gave [`Self::verify_anchor`] an [`AnchorPlan`] instead of a bare
/// [`QueryPlan`](crate::plan::QueryPlan) and said the method could no longer be handed a question. A
/// reviewer disproved that in one function: the constructor is `pub`, every value it read was
/// publicly constructible, and a fabricated tuple passed all four guards. That is not a hole a fifth
/// guard closes - a shape check over caller-constructible values can only ever be a shape check, and
/// Rust has no cross-crate friend visibility to hide the constructor behind.
///
/// So the two mechanisms are named separately, because they do different jobs:
///
/// - `clippy.toml` bans `sutura_domain::warehouse::Warehouse::verify_anchor`, verified to resolve by
///   writing the call and watching clippy reject it. `sutura_app::verify_anchors` holds the single
///   `#[expect]`, so a second call site is an error under `-D warnings` until somebody writes a
///   second expectation a reviewer sees in the diff. **That is what makes the path boot-only**, and
///   its limit is that a lint reaches this workspace and an `#[allow]` walks past it.
/// - [`AnchorPlan`] checks that the boot path compiled the question it meant to, reading the metric's
///   definition, its anchor's range and its coarsest grain off the pinned bundle rather than taking
///   them as arguments. It is a **self-check on that one caller and not an authority**, and the type
///   says so at length.
///
/// # The two identity declarations, and why they are two
///
/// [`Self::IMPERSONATION`] is a property of the **code**: whether this adapter has anywhere for a
/// subject's own credential to arrive. It is an associated constant with no default, so an adapter
/// cannot omit it, and it cannot vary per instance - which is what lets a boot check mean anything.
///
/// [`Self::posture`] is a property of the **deployment**: which identity a query is to reach this
/// source as. It is a method, because the composition root hands it to the adapter at construction,
/// and it is required rather than defaulted because a default posture is a posture nobody chose.
///
/// The two are compared at boot by `SourcePosture::deliverable_by`. Conflating them was the tempting
/// mistake and it gives the mode two owners: an adapter cannot declare a mode it does not own,
/// because the same adapter is correct in either posture and only the deployment knows which one it
/// is being asked for.
///
/// **An adapter that declares no impersonation capability does not compile:**
///
/// ```compile_fail
/// use sutura_domain::identity::Presented;
/// use sutura_domain::model::SourceName;
/// use sutura_domain::plan::{AnchorPlan, Executable};
/// use sutura_domain::source::SourcePosture;
/// use sutura_domain::warehouse::deadline::Deadline;
/// use sutura_domain::warehouse::{AnchorRows, RowSet, Warehouse};
///
/// struct Undeclared {
///     source: SourceName,
///     posture: SourcePosture,
/// }
///
/// // No `const IMPERSONATION`, so this impl is incomplete: the trait declares it with no default.
/// impl Warehouse for Undeclared {
///     type Error = core::fmt::Error;
///
///     fn source(&self) -> &SourceName {
///         &self.source
///     }
///
///     fn posture(&self) -> &SourcePosture {
///         &self.posture
///     }
///
///     fn execute(&self, _executable: Executable<'_>, _presented: &Presented, _deadline: Deadline) -> Result<RowSet, Self::Error> {
///         Err(core::fmt::Error)
///     }
///
///     fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
///         Err(core::fmt::Error)
///     }
/// }
/// ```
///
/// The compiling twin, so the block above cannot be passing on a typo - the only difference between
/// the two is the one line that declares the capability:
///
/// ```
/// use sutura_domain::identity::Presented;
/// use sutura_domain::model::SourceName;
/// use sutura_domain::plan::{AnchorPlan, Executable};
/// use sutura_domain::source::{ImpersonationCapability, SourcePosture};
/// use sutura_domain::warehouse::deadline::Deadline;
/// use sutura_domain::warehouse::{AnchorRows, RowSet, Warehouse};
///
/// struct Declared {
///     source: SourceName,
///     posture: SourcePosture,
/// }
///
/// impl Warehouse for Declared {
///     type Error = core::fmt::Error;
///
///     const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
///
///     fn source(&self) -> &SourceName {
///         &self.source
///     }
///
///     fn posture(&self) -> &SourcePosture {
///         &self.posture
///     }
///
///     fn execute(&self, _executable: Executable<'_>, _presented: &Presented, _deadline: Deadline) -> Result<RowSet, Self::Error> {
///         Err(core::fmt::Error)
///     }
///
///     fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
///         Err(core::fmt::Error)
///     }
/// }
///
/// assert_eq!(
///     <Declared as Warehouse>::IMPERSONATION,
///     ImpersonationCapability::NoPlaceForASubject
/// );
/// ```
pub trait Warehouse {
    /// Why this data system could not answer. Typed per adapter: a connection failure, a rejected
    /// statement and a permission denial are not the same thing to whoever responds to them.
    type Error: core::error::Error + 'static;

    /// Whether this adapter can carry a per-subject credential **at all**.
    ///
    /// **Required, with no default, and that is the whole mechanism.** A defaulted capability would
    /// mean an adapter that said nothing got the benefit of the doubt in whichever direction the
    /// default pointed - and both directions are wrong. Defaulted to *can*, a file engine would
    /// silently satisfy an impersonation cross-check it cannot honour. Defaulted to *cannot*, a real
    /// network adapter that forgot the line would be refused for a capability it has, and somebody
    /// would fix that by deleting the check.
    ///
    /// A constant rather than a method because it is fixed for the life of the process: it is a fact
    /// about what was linked, nothing at run time can widen it, and a source that gains a capability
    /// is a deployment change.
    ///
    /// **The cost, stated where the decision is:** an associated constant makes this trait not
    /// object-safe. Nothing in the workspace holds a `dyn Warehouse` today, and a heterogeneous set
    /// of adapters behind one port wants a closed enum over the registered adapters rather than
    /// dynamic dispatch - which is the same *pluggable by declaration* argument this constant comes
    /// from. If that ever changes, it is an architecture decision and not a signature tweak.
    const IMPERSONATION: ImpersonationCapability;

    /// Whether this adapter can run a [`LegPlan`](crate::plan::LegPlan) - one half of a two-source
    /// [`FederatedPlan`](crate::plan::FederatedPlan) - rather than only a whole single-source plan.
    ///
    /// **Defaulted to `false`, and the default is the safe direction.** An adapter that forgets to
    /// declare itself is treated as unable to federate, so a two-source question is refused as
    /// [`RefusalReason::FederationNotExecutable`](crate::query::RefusalReason::FederationNotExecutable)
    /// rather than half-answered under a certified metric name. Only an adapter with a combiner
    /// above it to hand a leg's rows to opts in - `sutura_exec_duckdb` does, because the differential
    /// suite runs the combiner above its two sources. This is a missed-optimisation default rather
    /// than a missed-security one: the cost of being wrong is a refused question, never a wrong
    /// number.
    const EXECUTES_LEGS: bool = false;

    /// Whether this adapter can execute a metric whose computation is
    /// [`Computation::AuthoredSql`](crate::expression::Computation::AuthoredSql) - SQL a catalog
    /// author wrote, rather than a measure the generator composes.
    ///
    /// **Defaulted to `false`, and no adapter this workspace ships opts in.** An authored fragment
    /// is stored as written: the domain holds no SQL parser, a [`crate::plan::QueryPlan`] carries
    /// no SQL, and nothing published compiles the fragment - so an adapter that says nothing is
    /// treated as unable to execute one, and a bundle carrying an authored metric is refused at
    /// startup as
    /// [`NotValidated::AuthoredSqlNotExecutable`](crate::pinned::NotValidated::AuthoredSqlNotExecutable)
    /// rather than served with the metric skipped or a measure substituted. `docs/adr/0004` is the
    /// decision. Opting in is a claim that the adapter itself compiles the fragment against the
    /// model - which is where the compile belongs, beside the code that renders for that dialect -
    /// and executes it; the first adapter to make that claim brings the test that holds it.
    const EXECUTES_AUTHORED_SQL: bool = false;

    /// Whether an accepted [`PreFlight`] can carry a real [`estimate::EstimatedBytes`], rather than
    /// [`PreFlight::Accepted`]'s `estimated_bytes` always answering `None`.
    ///
    /// **Defaulted to `false`, the safe direction for the reason [`Self::EXECUTES_LEGS`]'s is**: an
    /// adapter that says nothing is held to answer `None` on every accepted pre-flight, so nothing
    /// downstream can read an adapter that never priced anything as though `Some(0)` meant "this
    /// will cost nothing" rather than "nobody asked". Only an adapter whose `dry_run` reads a real
    /// byte count off its data system - `BigQueryWarehouse` decodes `totalBytesProcessed` from the
    /// wire - declares `true`.
    ///
    /// This is the capability constant `crate::warehouse::PreFlight`'s own doc and
    /// `sutura_conformance::execute::a_preflight_that_accepts_is_followed_by_an_answer`'s once
    /// named as missing: without it, an adapter that returned `Some(0)` where it never priced
    /// anything, or `None` where it could, would be indistinguishable from one that got the
    /// distinction right. **The pack checks this constant against what `dry_run` actually returns,
    /// for every adapter it binds** - it is not itself a proof that `BigQuery`'s real endpoint
    /// prices correctly, since `BigQuery` has no `execute_packs!` binding to run the check against.
    const PRICES_DRY_RUN: bool = false;

    /// The name a plan uses to select this adapter.
    fn source(&self) -> &SourceName;

    /// Which identity a query is to reach this source as, as the deployment declared it.
    ///
    /// **Required, and what an answer's provenance is read off.** The composition root hands the
    /// posture to the adapter at construction and provenance asks the adapter, so the record says
    /// what the thing that executed was actually holding. A provenance field read from the settings
    /// tree instead would report a leg as impersonated on the strength of a file, which is the one
    /// thing that field exists to stop.
    ///
    /// The limit is worth naming with the claim: today this is still the value the root handed over,
    /// so what it proves is that configuration reached the adapter - not that the data system
    /// evaluated anybody's authorization. That becomes a stronger claim when the execution port takes
    /// a credential per leg and the adapter matches exhaustively on what it received.
    fn posture(&self) -> &SourcePosture;

    /// Checks the plan is executable here, without producing rows.
    ///
    /// Takes an [`Executable`] for [`execute`](Warehouse::execute)'s reason, so the two cannot
    /// disagree about what this adapter accepts.
    ///
    /// **Defaulted to doing nothing, and the default is a statement rather than a stub.** A data
    /// system across a network can prepare a statement for a fraction of what running it costs, so
    /// there the pre-flight is worth a round trip: a plan naming a column that is not there is
    /// rejected before any data is read. An in-process engine cannot make that trade - checking means
    /// building the logical plan and running the analyzer and the optimizer, which is most of
    /// executing it - so a required `dry_run` bought that guarantee at the price of planning every
    /// question twice. Not implementing this is how such an adapter says "checking is not cheaper
    /// than running here"; `execute` is then the only pass, and it still fails before returning rows
    /// for the same reasons the check would have.
    ///
    /// An adapter that overrides it must not read data: the contract is a plan that resolves, not a
    /// result.
    ///
    /// **It takes the credential too, and not for symmetry.** A pre-flight asks "would this be
    /// accepted", and the answer depends on who is asking: under the process identity it would report
    /// a plan as executable that the subject may not execute, or prepare a statement against tables
    /// the subject cannot see. The check has to be asked as the same principal as the question, or it
    /// answers a different question - which is also why the return type is [`PreFlight`] rather than
    /// `()`. See that type for what its two variants keep apart.
    ///
    /// **And it takes the deadline, for [`execute`](Warehouse::execute)'s reason.** Against a
    /// networked data system a pre-flight is a round trip that spends part of one answer's budget,
    /// so an adapter that honours it needs to know what is left before it starts one. `docs/adr/0029`
    /// is the record; every adapter in this slice accepts the parameter and ignores it - carried, not
    /// enforced here.
    fn dry_run(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    /// Runs the plan and returns its rows.
    ///
    /// **One method for both plan shapes, and the exhaustive match is why.** An
    /// [`Executable`] is a whole [`QueryPlan`](crate::plan::QueryPlan) or one
    /// [`LegPlan`](crate::plan::LegPlan) of a federated answer, so an adapter that reads what it was
    /// handed has to say what it does with each. A second port method for legs was considered and
    /// rejected: a second method invites a default body, a default that errors lets an adapter be
    /// silently non-federating, and *adding a data system is a registration* would then stop being
    /// true in the one direction nobody would notice.
    /// `docs/adr/0007-federating-across-different-data-systems.md` is the decision.
    ///
    /// **`sutura_app`'s federated path hands an adapter a leg, and a shipped binary reaches it** -
    /// the engine declares [`Self::EXECUTES_LEGS`], so `sutura` and `sutura-serve` answer a
    /// two-source question rather than refusing one. This paragraph twice said the opposite: first
    /// that nothing handed any adapter a leg, then that no shipped binary did.
    ///
    /// [`Self::EXECUTES_LEGS`] still DEFAULTS to `false`, which is what makes the refusal the safe
    /// direction for an adapter that has no leg venue - `answer_federated` reads it and refuses
    /// before it splits. An adapter that cannot execute one says so with a typed error of its own
    /// rather than with a default it inherited.
    ///
    /// # The credential is a parameter, and it cannot be omitted
    ///
    /// `presented` is what this leg executes as, minted for THIS source by a
    /// [`CredentialBroker`](crate::identity::CredentialBroker). An adapter matches on it
    /// exhaustively and returns its own typed error for a shape it is not configured for -
    /// `docs/adr/0008` part 4 states both directions and says which is the dangerous one: an adapter
    /// that quietly accepted subject material it cannot use would report a leg as impersonated that
    /// ran shared.
    ///
    /// Calling it without one does not compile:
    ///
    /// ```compile_fail
    /// use sutura_domain::plan::Executable;
    /// use sutura_domain::warehouse::{RowSet, Warehouse};
    ///
    /// fn _as_the_process<W: Warehouse>(warehouse: &W, executable: Executable<'_>) -> Result<RowSet, W::Error> {
    ///     warehouse.execute(executable)
    /// }
    /// ```
    ///
    /// The compiling twin, so the block above cannot be passing for a typo - the only difference is
    /// the two arguments that say whose credential this runs under and by when it has to be done:
    ///
    /// ```
    /// use sutura_domain::identity::Presented;
    /// use sutura_domain::plan::Executable;
    /// use sutura_domain::warehouse::deadline::Deadline;
    /// use sutura_domain::warehouse::{RowSet, Warehouse};
    ///
    /// fn _as_the_asker<W: Warehouse>(
    ///     warehouse: &W,
    ///     executable: Executable<'_>,
    ///     presented: &Presented,
    ///     deadline: Deadline,
    /// ) -> Result<RowSet, W::Error> {
    ///     warehouse.execute(executable, presented, deadline)
    /// }
    /// ```
    ///
    /// # The deadline is a parameter too, opened by the transport before this call was ever reached
    ///
    /// One absolute [`Deadline`] per answer, shared by the pre-flight, this call, and every leg of a
    /// federated answer - never re-derived, never divided. `docs/adr/0029` is the record: what an
    /// adapter does with it is the adapter's own business, because the interrupt that actually stops
    /// a data system is that data system's, and this port cannot make one uniform. [`Self::deadline_exceeded`]
    /// is how an adapter reports that its own failure WAS the deadline, for a caller above this port
    /// that has no way to inspect [`Self::Error`] itself.
    ///
    /// **Every adapter in this slice accepts the parameter and ignores it.** Carried, not enforced
    /// here - the engine, `BigQuery` and Postgres each stop their own data system with it in a later
    /// change behind `telekom/sutura#160`, and `docs/adr/0029`'s table says which mechanism per
    /// adapter. An adapter that ignores the deadline mid-call is not caught until its own `Result`
    /// comes back; what IS caught here, before this call is ever made, is a budget already spent -
    /// `sutura_app::answer` and `sutura_app::federated`'s leg functions both ask before every call.
    ///
    /// The call a caller who remembered `presented` but not `deadline` would actually write - the
    /// shape this parameter's own addition produced - does not compile either:
    ///
    /// ```compile_fail
    /// use sutura_domain::identity::Presented;
    /// use sutura_domain::plan::Executable;
    /// use sutura_domain::warehouse::{RowSet, Warehouse};
    ///
    /// fn _forgot_the_deadline<W: Warehouse>(
    ///     warehouse: &W,
    ///     executable: Executable<'_>,
    ///     presented: &Presented,
    /// ) -> Result<RowSet, W::Error> {
    ///     warehouse.execute(executable, presented)
    /// }
    /// ```
    ///
    /// Its compiling twin is `_as_the_asker` above - three arguments, not two - which is the whole
    /// point: nothing here checks the ARITY, the compiler already does, so this pair is honest about
    /// proving only that the third argument exists and is a `Deadline`, not that a reviewer needs to
    /// remember to ask for it.
    fn execute(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<RowSet, Self::Error>;

    /// Re-runs one anchor's plan, under the identity this adapter was configured with.
    ///
    /// **Separate from [`execute`](Warehouse::execute) because the boot path has no caller.** Every
    /// anchor in a bundle is executed against the data system before a listener is bound, so the one
    /// thing this method cannot be handed is an asking subject's credential - there is none in
    /// scope, and inventing one is the service-identity fallback arriving through the back door.
    /// `docs/adr/0008` part 1 decides that it gets its own method, and the trait's own header says
    /// where this shape is narrower than that record asked for.
    ///
    /// Required, with no default. A defaulted body could not execute anything - it has no credential
    /// to pass `execute` - so the only default available is one that lies about having verified
    /// something, which is the shape [`PreFlight`] exists to avoid one level up.
    ///
    /// It takes an [`AnchorPlan`] rather than an [`Executable`]: an anchor is asked with no dimensions
    /// and resolves to one model on one source, so there is no leg for it to be - and the plan is
    /// checked against the pinned bundle as a declared anchor's own rather than taken on trust, which
    /// catches a boot path that compiled a question where an anchor was meant. **It does not make the
    /// method unreachable, and the trait header says why**: what keeps this method to the boot path is
    /// the `clippy.toml` ban on it. Returns [`AnchorRows`], which is what stops a boot result being
    /// handed back as an answer.
    ///
    /// **What an executed anchor proves, precisely:** that these statements reproduced the numbers
    /// their author certified *for the identity this adapter holds*. Under row-level security that is
    /// not necessarily any caller's - a per-subject anchor is a function rather than a number, and
    /// there is no subject at boot to evaluate it at.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error>;

    /// Was this failure the working-set ceiling refusing a reservation, and what was the ceiling?
    ///
    /// **The domain naming what it needs, because it cannot look.** `Self::Error` is the adapter's
    /// own type, so nothing above this port can tell "the pool would not grow" from "the connection
    /// dropped" - and those two are a governance refusal and a transport failure respectively. This
    /// is the one question the domain has to be able to ask about an adapter's error, and it is a
    /// predicate rather than a conversion so that an adapter cannot mint an arbitrary
    /// [`RefusalReason`](crate::query::RefusalReason) from a failure of its own.
    ///
    /// `Some(bytes)` is the ceiling in bytes that the reservation was refused against, which is a
    /// number an operator configured; `None` is every other failure, including a failure whose cause
    /// happens to mention memory. An adapter that cannot tell the difference must answer `None`,
    /// because the cost of the two mistakes is not symmetric: a transport failure reported as
    /// exhaustion tells a caller not to retry something a retry would have answered.
    ///
    /// Defaulted to `None`, which is the honest answer for an adapter with no pool to bound - and
    /// for one whose engine has an unbounded one, since a ceiling that cannot be exceeded cannot be
    /// the thing that refused.
    ///
    /// Takes `&self` because the ceiling belongs to the adapter rather than to the error, and the
    /// error's own text is not where a bound belongs.
    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        None
    }

    /// Was this failure the data system declining to hand back a result this large?
    ///
    /// **The sibling of [`working_set_exhausted`](Warehouse::working_set_exhausted), added against
    /// the same defect one place further out.** That method exists because exhaustion used to leave
    /// as a transport failure and reach a caller as `503`, which is what a data system being down
    /// looks like - so a caller was told to retry against a bound that fires again in the same place.
    /// A result the data system will not deliver in one piece is that defect exactly: a networked
    /// endpoint caps a reply by size, and a result INSIDE the row cap can still be over it. Retrying
    /// returns the same reply.
    ///
    /// `true` leaves as [`RefusalReason::ResultTooLarge`](crate::query::RefusalReason::ResultTooLarge)
    /// carrying [`ResultBound::Volume`](crate::query::ResultBound::Volume) - the same refusal, code
    /// and status the row cap produces, because *too much data* is one answer to a caller whichever
    /// side measured it. `false` is every other failure.
    ///
    /// **A predicate rather than a conversion**, for the reason `working_set_exhausted` gives: the
    /// refusal vocabulary belongs to the domain, and an adapter that could return a
    /// [`RefusalReason`](crate::query::RefusalReason) could mint any of them from a failure of its
    /// own. And a `bool` rather than an `Option<u64>` for a reason of its own, stated where the
    /// signature is: **there is no honest number to carry.** The bound is the data system's, and the
    /// endpoint this was built for reports neither the cap nor the size of the reply that hit it - so
    /// a numeric return would be a field every adapter had to fill with something, and something is
    /// how a certified-looking figure gets attached to a bound nobody measured. If an adapter ever
    /// does know its bound, widening this is a diff with that adapter's evidence in it.
    ///
    /// An adapter that cannot tell the difference must answer `false`, because the two mistakes do
    /// not cost the same: a transport failure reported as a governance refusal tells a caller not to
    /// retry something a retry would have answered.
    ///
    /// Defaulted to `false`, which is the honest answer for an adapter with no such bound - an
    /// in-process engine hands back whatever it computed, and a local driver reads a whole result.
    ///
    /// **The limit, stated with the claim:** this is asked only of a failure from
    /// [`execute`](Warehouse::execute). `dry_run`'s contract is that a check reads no data, so there
    /// is no reply for a size bound to refuse there, and the boot path's `verify_anchor` runs one
    /// certified scalar.
    fn result_did_not_fit(&self, _error: &Self::Error) -> bool {
        false
    }

    /// Was this [`dry_run`](Warehouse::dry_run) or [`execute`](Warehouse::execute) failure the data
    /// system refusing the statement at the identity/authorization level, rather than failing to
    /// answer? The query-time sibling of
    /// [`preflight_was_refused`](Warehouse::preflight_was_refused). A statement can be refused
    /// because the identity it ran as may not read what it asks for; that refusal returns forever
    /// until a grant changes, unlike a transient connection failure where a retry may answer. A
    /// caller told `true` receives
    /// [`RefusalReason::SourceRefused`](crate::query::RefusalReason::SourceRefused), not the `503`
    /// a data system being down produces - a refusal is never silently retried as if transient. A
    /// predicate rather than a conversion, for [`result_did_not_fit`](Warehouse::result_did_not_fit)'s
    /// reasons; which permission or identity is the data system's to say. Defaulted to `false`.
    fn source_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    /// Was this [`dry_run`](Warehouse::dry_run) or [`execute`](Warehouse::execute) failure the
    /// deadline: fired at the data system, or found already spent before the statement was sent?
    ///
    /// **The fourth predicate beside [`working_set_exhausted`](Warehouse::working_set_exhausted),
    /// [`result_did_not_fit`](Warehouse::result_did_not_fit) and
    /// [`source_refused`](Warehouse::source_refused), for their exact reason: `Self::Error` is the
    /// adapter's own type, so nothing above this port can tell a stopped question from a dropped
    /// connection, and a predicate is what lets the domain ask without an adapter minting its own
    /// [`RefusalReason`](crate::query::RefusalReason).** `true` leaves as
    /// [`RefusalReason::DeadlineExceeded`](crate::query::RefusalReason::DeadlineExceeded), audited
    /// and answered `422` rather than the retryable failure a data system being down produces -
    /// `docs/adr/0029` argues both directions once.
    ///
    /// Defaulted to `false`, which is the honest answer for an adapter that does not yet read the
    /// deadline at all: every adapter in this slice takes the default, because carrying the
    /// parameter and stopping the data system with it are two different changes and this one is the
    /// first. An adapter that does read it and cannot tell its own timeout from another failure must
    /// still answer `false`, for [`result_did_not_fit`](Warehouse::result_did_not_fit)'s reason - the
    /// two mistakes do not cost the same, and a transport failure reported as a stopped deadline
    /// tells a caller not to retry something a retry might answer.
    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }

    /// Does this data system hold the tables the bundle names?
    ///
    /// **Asked once, at boot, before a listener is bound**, and it exists to close an asymmetry
    /// between two kinds of deployment rather than to add a check. A file engine is GIVEN one file
    /// per model, so a catalog naming a table with nothing behind it is already a refusal naming the
    /// model. A networked data system has no such step: the tables live in the dataset, and without
    /// this the process learns a table is absent when a question reaches it - a green startup, a
    /// healthy liveness probe, and a failure for whoever asked first.
    ///
    /// **Defaulted to [`TablesPresent::NotAsked`], and the default is the whole reason this is a
    /// defaulted method rather than a required one.** An adapter that has no cheap way to ask must
    /// not be forced to answer, and the only answers available to one that cannot look are *nothing
    /// to report* and a lie. That is `working_set_exhausted`'s and `result_did_not_fit`'s precedent
    /// pointed at a boot check: the default is the reading that costs least when it is wrong.
    /// [`TablesPresent`] says at length why the default is not readable as *verified*.
    ///
    /// **A SET rather than a table, deliberately.** A networked data system can usually answer this
    /// for a whole dataset in one call, and a method taking one table would make that a call per
    /// model - which is the cost that kept this check from existing. An adapter reading more than
    /// one dataset makes one call per dataset, which is still a set rather than a model.
    ///
    /// It takes no credential, for [`verify_anchor`](Warehouse::verify_anchor)'s reason: there is no
    /// caller at boot. So what it establishes is what the identity this adapter was configured with
    /// can see, which is the same limit an anchor carries.
    ///
    /// # Errors
    ///
    /// **A data system that could not be ASKED is an `Err` and never a variant of the answer** - a
    /// credential with no permission to list, a dataset that is not there, an endpoint that did not
    /// reply. That separation is the contract: *could not verify* and *this table is absent* must
    /// reach an operator as two different sentences, because the fix for each is in a different
    /// place. A composition root is free to treat the first as a warning and the second as a refusal,
    /// and it cannot make that choice if the adapter collapsed them.
    fn preflight(&self, _tables: &BTreeSet<QualifiedTable>) -> Result<TablesPresent, Self::Error> {
        Ok(TablesPresent::NotAsked)
    }

    /// Was this pre-flight failure the data system REFUSING, rather than failing to answer?
    ///
    /// **Added because the first version of the pre-flight could not tell those apart, and a review
    /// found what that cost.** [`preflight`](Warehouse::preflight)'s `Err` is *could not verify*, and
    /// a composition root's reasonable response to that is a warning rather than a refusal - a
    /// deployment whose data system is briefly unreachable at boot still has to be able to serve. But
    /// a data system that refused because the identity lacks the permission to LIST will refuse again
    /// on every boot, forever, and the fix is one grant - collapsed into the warning, the check
    /// silently does nothing in exactly the deployment least likely to read a startup log.
    ///
    /// `true` means *this identity may not ask*, and a composition root is expected to refuse and
    /// name the grant. `false` is every other failure, including one whose text happens to mention
    /// permissions.
    ///
    /// **A predicate rather than a conversion, and a `bool` rather than a reason**, for
    /// [`result_did_not_fit`](Warehouse::result_did_not_fit)'s reasons exactly: `Self::Error` is the
    /// adapter's own type so nothing above this port can read it, and the refusal vocabulary stays the
    /// domain's - the adapter's error already carries the detail an operator needs, travelling as the cause.
    ///
    /// Defaulted to `false`, honest for an adapter that cannot tell the two apart or whose
    /// [`preflight`](Warehouse::preflight) never fails. **The safe direction, and the opposite of
    /// this trait's other two predicates:** a refusal reported as a hiccup leaves a deployment
    /// serving unverified - where this check started; a hiccup reported as a refusal stops a
    /// deployment that would have worked. `false` picks the status quo over a new failure mode.
    ///
    /// **WHAT THE DEFAULT COSTS**, per review: this trait's own rule is *required with no default
    /// where the absence changes what a caller may believe*, and here it does - an adapter that
    /// overrides [`preflight`](Warehouse::preflight) so it really asks, and forgets this predicate,
    /// gets *never a refusal*, silently, the permanent-`WARN` collapse this pair exists to remove.
    /// Nothing catches that; a defaulted method has no `compile_fail` twin. Defaulted anyway, because
    /// three adapters that cannot fail a pre-flight at all would each have to write `false` - so this
    /// is a JUDGEMENT held by review, not a checked property, and pairing the two is what a reviewer
    /// of an adapter overriding one of them has to look for.
    fn preflight_was_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    /// Is a declared join key really unique in the table it points at?
    ///
    /// **Asked once, at boot**, for every relationship whose [`JoinType`](crate::model::JoinType)
    /// promises that its target column identifies at most one row. The whole join path spends that
    /// promise and spends it two different ways - a rendered `JOIN` on one data system, a lookup
    /// leg's `GROUP BY` on two - so a table that contradicts it answers one question with two
    /// numbers and refuses neither. [`cardinality`] carries the measurement and the arithmetic.
    ///
    /// **Defaulted to [`KeyUniqueness::NotAsked`], for [`preflight`](Warehouse::preflight)'s
    /// reason**: an adapter with no cheap way to count must not be forced to answer, and the only
    /// answers available to one that cannot look are nothing-to-report and a lie. So this is a
    /// per-adapter capability rather than a guarantee of the port, and [`KeyUniqueness`] is shaped so
    /// that no caller can read the default as verified.
    ///
    /// It takes no credential, for [`verify_anchor`](Warehouse::verify_anchor)'s reason: there is no
    /// caller at boot. What it establishes is what the identity this adapter was configured with can
    /// see, which is the same limit an anchor and a pre-flight each carry.
    ///
    /// # Errors
    ///
    /// A data system that could not be asked is an `Err`, never a clean count - [`TablesPresent`]'s
    /// separation applied to this question, and for its reason.
    fn declared_key(&self, _key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        Ok(KeyUniqueness::NotAsked)
    }

    /// Whether this adapter accepts a raw statement - `false` by default; see [`crate::raw`], `docs/adr/0013`.
    const ACCEPTS_RAW_STATEMENTS: bool = false;

    /// Runs one literal statement for the raw SQL tool - not what [`execute`](Warehouse::execute) uses; see [`crate::raw`].
    fn execute_raw(&self, _statement: &crate::raw::RawStatement, _presented: &Presented) -> RawExecution<Self::Error> {
        None
    }
}

pub mod raw; // `docs/adr/0013`'s raw types - carved out: this file hit the thousand-line limit.
pub use raw::{RawColumnsAndRows, RawExecution, RawRows};

#[cfg(test)]
mod tests;

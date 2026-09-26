//! The second driven port: what joins and re-aggregates two legs, above every adapter.
//!
//! # Why this is a port and not a function
//!
//! `docs/adr/0007` designed it here and recorded that it was not what got built: *"the combine
//! needs `DataFusion`, `sutura-app` may not name a framework, so the domain declares a port beside
//! `Warehouse` and `SemanticCatalog` and a crate above it implements the combine over
//! `DataFusion`"*. What landed instead was `FederatedPlan::combine`, a pure domain function that
//! walked rows by hand. `docs/adr/0039-arrow-and-datafusion-override-the-hand-written-combiner.md`
//! step 3 reverses that by owner instruction - *no hand row handling: `DataFusion`, Arrow, Arrow
//! Flight or ADBC* - so this is the port arriving, not a new invention.
//!
//! A path in backticks rather than a Markdown link, and that is not style: `just api` republishes
//! this header at `docs/api/sutura-domain.md`, where `mkdocs --strict` resolves a relative link
//! against the PUBLISHED page's directory rather than this file's - and fails the build.
//!
//! **The domain names no engine, and that is the constraint the trait is shaped by.**
//! `xtask/src/boundaries/edges.rs`'s `ALLOWED_IN_DOMAIN` is an allowlist over this crate's whole
//! transitive tree and its line is *no runtime, no client, no engine*. `DataFusion` is an engine, so
//! the trait lives here and its implementor lives in an adapter. Arrow is a data FORMAT, which is
//! why [`ResultBatches`](crate::warehouse::ResultBatches) may be the currency on both sides of this
//! signature - the argument is
//! `crate::warehouse::arrow`'s and it was paid there.
//!
//! # What the shape buys, which is that the two legs cannot be swapped
//!
//! A combiner taking `(&ResultBatches, &ResultBatches)` has two arguments of one type whose swap
//! compiles and answers a wrong number under a certified metric name - the fact leg's measure
//! grouped by the lookup leg's keys. [`Legs`] makes the swap unrepresentable rather than reviewed:
//! each leg's result is tagged with the side by [`LegResult::of`], which reads it off the
//! [`LegPlan`] the leg was executed from and takes no side from its caller, and [`Legs::of`]
//! assigns by that tag rather than by argument position. So the two can be handed over in either
//! order and a pair that is not one of each does not build a value.

use crate::plan::federated::{FederatedAnswerRefusal, FederatedPlan, LegSide};
use crate::plan::leg::LegPlan;
use crate::warehouse::ResultBatches;

/// One leg's result, tagged with the side the [`LegPlan`] it was executed from named.
///
/// **The tag is parsed, not passed.** [`Self::of`] takes the plan and reads the side off the
/// variant, so a caller cannot label a lookup leg's rows as the fact leg's - which is the mistake
/// [`Legs`] exists to make unrepresentable one level up.
#[derive(Debug, Clone)]
pub struct LegResult {
    side: LegSide,
    batches: ResultBatches,
}

impl LegResult {
    /// One executed leg's result, under the side its plan names.
    #[must_use]
    pub const fn of(plan: &LegPlan, batches: ResultBatches) -> Self {
        let side = match *plan {
            LegPlan::Fact { .. } => LegSide::Fact,
            LegPlan::Lookup { .. } => LegSide::Lookup,
        };
        Self { side, batches }
    }

    /// Which leg this result came from.
    #[inline]
    #[must_use]
    pub const fn side(&self) -> LegSide {
        self.side
    }

    /// The batches, as the leg's adapter handed them over.
    #[inline]
    #[must_use]
    pub const fn batches(&self) -> &ResultBatches {
        &self.batches
    }
}

/// Both legs' results, which cannot hold two of one side and cannot be built with them swapped.
///
/// A two-fact plan (`telekom/sutura#780`) carries an optional second fact leg: a third result
/// from a second fact model, joined above on the link and the time bucket. The combiner reads it
/// through [`Self::second_fact`] and routes each `Carried` leaf to the fact leg its model names. No
/// question produces a second fact yet: `plan()` refuses a cross-model ratio before dispatching.
///
/// Borrowed rather than owned, because a combiner reads the batches and the caller still holds them
/// for the refusal it may have to build - and because Arrow batches are reference-counted buffers,
/// so an owned pair would say *moved* about something that is shared either way.
#[derive(Debug, Clone, Copy)]
pub struct Legs<'a> {
    fact: &'a ResultBatches,
    second_fact: Option<&'a ResultBatches>,
    lookup: &'a ResultBatches,
}

impl<'a> Legs<'a> {
    /// The pair, assigned by each result's own tag rather than by the order they arrive in.
    ///
    /// # Errors
    ///
    /// [`LegsAreNotOneOfEach`] when both results name the same side.
    pub const fn of(one: &'a LegResult, other: &'a LegResult) -> Result<Self, LegsAreNotOneOfEach> {
        match (one.side(), other.side()) {
            (LegSide::Fact, LegSide::Lookup) => Ok(Self {
                fact: one.batches(),
                second_fact: None,
                lookup: other.batches(),
            }),
            (LegSide::Lookup, LegSide::Fact) => Ok(Self {
                fact: other.batches(),
                second_fact: None,
                lookup: one.batches(),
            }),
            (side, _) => Err(LegsAreNotOneOfEach { both: side }),
        }
    }

    /// The third leg: the second fact's batches, when the plan carries two fact models.
    ///
    /// # Errors
    ///
    /// [`LegsAreNotOneOfEach`] naming [`LegSide::Lookup`] when `second` is a lookup's result, so a
    /// second lookup cannot be combined as if it were a fact.
    #[inline]
    pub const fn with_second_fact(self, second: Option<&'a LegResult>) -> Result<Self, LegsAreNotOneOfEach> {
        match second {
            None => Ok(self),
            Some(second) => match second.side() {
                LegSide::Fact => Ok(Self {
                    second_fact: Some(second.batches()),
                    ..self
                }),
                LegSide::Lookup => Err(LegsAreNotOneOfEach { both: LegSide::Lookup }),
            },
        }
    }

    /// The metric's own leg: its keys, its time bucket and its carried leaves.
    #[inline]
    #[must_use]
    pub const fn fact(self) -> &'a ResultBatches {
        self.fact
    }

    /// The second fact leg, when the plan carries two fact models.
    #[inline]
    #[must_use]
    pub const fn second_fact(self) -> Option<&'a ResultBatches> {
        self.second_fact
    }

    /// The second data system's leg: the remote keys the answer groups by.
    #[inline]
    #[must_use]
    pub const fn lookup(self) -> &'a ResultBatches {
        self.lookup
    }
}

/// Two leg results that name the same side, so there is no pair to combine.
///
/// Unreachable through `sutura_app`'s federated path, which builds one [`LegResult`] per
/// [`FederatedPlan::legs`] entry. Typed anyway rather than assumed away: it is the one thing
/// [`Legs::of`] cannot answer, and a silent choice between two facts would combine a leg with itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("both leg results name the {both:?} leg, so there is no pair to combine")]
pub struct LegsAreNotOneOfEach {
    both: LegSide,
}

impl LegsAreNotOneOfEach {
    /// The side both results claimed.
    ///
    /// An accessor rather than a `pub` field, which `xtask check-boundaries`'s API-shape rule holds
    /// for every library type here: a public field lets a struct literal build the value the
    /// constructor would have rejected, and this one is only ever minted by [`Legs::of`].
    #[inline]
    #[must_use]
    pub const fn both(self) -> LegSide {
        self.both
    }
}

/// Joins two legs' results and re-aggregates the answer above them.
///
/// **The second driven port, declared beside [`Warehouse`](crate::warehouse::Warehouse) and
/// [`SemanticCatalog`](crate::pinned::SemanticCatalog)**, and the module header is why it is a port.
/// `sutura_app::federated::answer_federated` is the driver; a crate above this one implements it.
/// An adapter never calls another adapter and this does not change that: the combine is called from
/// the application, above every adapter, exactly where `FederatedPlan::combine` was called from.
///
/// # Every method is required, and that is the mechanism
///
/// [`Warehouse`](crate::warehouse::Warehouse) defaults its four classifying predicates, because an
/// adapter with no memory pool has an honest `None` to give. A combiner does not: there is one
/// implementation, the working-set ceiling is the whole of what bounds it, and an implementor that
/// inherited `None` would turn a ceiling an operator configured into the `503` a dead data system
/// produces - with nothing in a diff to see. So [`Self::working_set_exhausted`] and
/// [`Self::answer_not_well_formed`] have no default body: a second implementor has to write both
/// arms where a reviewer reads them.
///
/// # What the domain may ask about an implementor's error, and why it is two predicates
///
/// `Self::Error` is the implementor's own type, so nothing above this port can tell *the pool would
/// not grow* from *the plan would not build*. The two questions split the same way the query path's
/// already do:
///
/// * [`Self::working_set_exhausted`] is the governance bound -
///   [`RefusalReason::ResourcesExhausted`](crate::query::RefusalReason::ResourcesExhausted), which
///   carries the ceiling an operator configured.
/// * [`Self::answer_not_well_formed`] is a deterministic refusal about the DATA the legs returned -
///   [`RefusalReason::FederatedAnswerNotWellFormed`](crate::query::RefusalReason::FederatedAnswerNotWellFormed).
///   The same plan against the same rows refuses again, which is what makes it a refusal rather
///   than a retryable failure.
///
/// Both are predicates rather than conversions, for the reason
/// [`Warehouse::working_set_exhausted`](crate::warehouse::Warehouse::working_set_exhausted) gives:
/// an implementor that could return a [`RefusalReason`](crate::query::RefusalReason) could mint any
/// of them from a failure of its own. Everything else leaves as the implementor's typed error and
/// reaches a caller as this workspace's own defect.
pub trait FederationCombiner {
    /// Why the combine could not be assembled. Typed per implementor: a refused reservation, a plan
    /// that would not build and a leg schema that does not carry a label the plan named are not the
    /// same thing to whoever responds to them.
    type Error: core::error::Error + 'static;

    /// One answer's rows from two legs' results.
    ///
    /// The fact and lookup results are joined on the link column both legs project under
    /// [`InternalLabel::Link`](crate::plan::InternalLabel::Link), grouped by the answer's keys in
    /// the order [`FederatedPlan::keys`] gives them and by the fact leg's time bucket,
    /// re-aggregated by each carried leaf's own
    /// [`Carried::combine`](crate::federation::Carried::combine), and divided through the
    /// [`Above`](crate::federation::Above) tree once, above every leg. The answer's columns are the
    /// keys in question order, then the bucket, then the measure under the metric's own certified
    /// name - the same order the mono path emits, which is what makes the two comparable.
    ///
    /// `working_set_bytes` is `docs/adr/0009`'s working-set ceiling. **What it has to bound is the
    /// combine's own working set, and an implementor that cannot bound it must refuse rather than
    /// answer** - the bound is the reason a two-source question is executable at all, so a combiner
    /// that quietly ignored it would be an overstated control rather than a missing one.
    ///
    /// # Errors
    ///
    /// [`Self::Error`], which the two predicates above classify.
    fn combine(&self, plan: &FederatedPlan, legs: Legs<'_>, working_set_bytes: u64) -> Result<ResultBatches, Self::Error>;

    /// Was this failure the working-set ceiling refusing a reservation, and what was the ceiling?
    ///
    /// `Some(bytes)` is the ceiling the refusal was measured against, which is a number an operator
    /// configured; `None` is every other failure, including one whose cause happens to mention
    /// memory. Required rather than defaulted - see the trait's own header.
    fn working_set_exhausted(&self, error: &Self::Error) -> Option<u64>;

    /// Was this failure a deterministic refusal about what the legs returned?
    ///
    /// `None` is this workspace's own wiring defect: a leg result that does not carry a label the
    /// plan named, a plan that would not build. Those leave as [`Self::Error`], because no caller
    /// caused them and none can fix them. Required rather than defaulted - see the trait's own
    /// header.
    fn answer_not_well_formed(&self, error: &Self::Error) -> Option<FederatedAnswerRefusal>;
}

/// A combiner that answers nothing, for a transport's own tests.
///
/// **A fake over the port, which is this workspace's rule for one** - never a mocked engine. It
/// exists because a transport's suite composes a whole `sutura_app::LocalService` to exercise
/// routing, credentials and refusal shapes, and a service takes a combiner: without this, each
/// transport would write its own, and two copies of a fake are two things to keep in step.
///
/// **Refusing rather than answering, and that is the honest fake for its callers.** No transport
/// test asks a two-source question - the adapters those suites register are mono fakes - so a
/// combiner that produced rows would be inventing an answer nothing reads. A combine that is
/// reached through this fake is a test that has drifted into the federated path, and it fails
/// loudly rather than passing over a fabricated number.
///
/// Behind `fixtures`, so nothing published holds it.
#[cfg(any(test, feature = "fixtures"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct RefusingCombiner;

/// What [`RefusingCombiner`] answers with.
#[cfg(any(test, feature = "fixtures"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("this fixture combiner assembles no federated answer")]
pub struct NothingCombined;

#[cfg(any(test, feature = "fixtures"))]
impl FederationCombiner for RefusingCombiner {
    type Error = NothingCombined;

    fn combine(&self, _plan: &FederatedPlan, _legs: Legs<'_>, _working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        Err(NothingCombined)
    }

    /// Never the ceiling: this fake reserves nothing, so answering `Some` would name a bound that
    /// did not fire - which is the mistake
    /// [`Warehouse::working_set_exhausted`](crate::warehouse::Warehouse::working_set_exhausted)'s
    /// own doc says costs a caller a retry they cannot win.
    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        None
    }

    /// Never a refusal about the data either: nothing here read any. A caller reaching this gets
    /// the internal failure it is, which is what makes a drifted test visible.
    fn answer_not_well_formed(&self, _error: &Self::Error) -> Option<FederatedAnswerRefusal> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{LegResult, Legs};
    use crate::model::{QualifiedTable, SourceName, TableName};
    use crate::plan::federated::LegSide;
    use crate::plan::leg::LegPlan;
    use crate::plan::{PlanBindings, ResultLabel};
    use crate::warehouse::ResultBatches;
    use crate::warehouse::arrow::of_rows;

    /// Inline rather than in a `combiner/tests.rs`, and the reason is the causality gate: this file
    /// is new, so a tree with the production files reverted loses the `mod combiner;` that declares
    /// it, and a test module that is never compiled reports a proof it did not make.
    fn batches(label: &str) -> ResultBatches {
        of_rows(&[String::from(label)], &[]).expect("an empty result of one column is well formed")
    }

    fn source(name: &str) -> SourceName {
        SourceName::parse(name).expect("a test source is a source")
    }

    fn table(name: &str) -> QualifiedTable {
        TableName::parse(name).expect("a test table is a table").into()
    }

    /// A lookup leg, which is the cheaper of the two to build: no bucket, no terms, no range.
    fn lookup_plan() -> LegPlan {
        LegPlan::Lookup {
            source: source("geo"),
            table: table("dim_region"),
            keys: Vec::new(),
            bindings: PlanBindings::none(),
        }
    }

    /// A fact leg. Every field is a placeholder: nothing here reads a plan's contents, only which
    /// VARIANT it is, which is the whole of what [`LegResult::of`] parses.
    fn fact_plan() -> LegPlan {
        LegPlan::Fact {
            source: source("facts"),
            metric: crate::model::MetricName::parse("revenue").expect("a test metric is a metric"),
            tables: crate::plan::StatementTables::only(TableName::parse("fct").expect("a test table is a table")),
            bucket: crate::plan::PlanBucket::new(
                ResultLabel::bucket(),
                crate::model::Grain::Month,
                crate::plan::PlanColumn::new(
                    TableName::parse("fct").expect("a test table is a table"),
                    crate::model::ColumnName::parse("month").expect("a test column is a column"),
                ),
            ),
            keys: Vec::new(),
            terms: Vec::new(),
            bindings: PlanBindings::none(),
            range: crate::calendar::TimeRange::new(
                crate::calendar::Date::parse("2026-01-01").expect("a test date is a date"),
                crate::calendar::Date::parse("2026-02-01").expect("a test date is a date"),
            )
            .expect("a test range is a range"),
        }
    }

    /// **The swap, which is what this pair of types exists for.** Handed over in either order the
    /// pair resolves the same way, because the side comes off each result's own plan rather than
    /// off the argument position - so the mistake that would group the fact leg's measure by the
    /// lookup leg's keys is not expressible rather than caught in review.
    #[test]
    fn the_pair_resolves_the_same_way_in_either_order() {
        let fact = LegResult::of(&fact_plan(), batches("0_leaf_0"));
        let lookup = LegResult::of(&lookup_plan(), batches("region"));
        let forwards = Legs::of(&fact, &lookup).expect("one of each is a pair");
        let backwards = Legs::of(&lookup, &fact).expect("one of each is a pair, in either order");
        assert_eq!(forwards.fact().schema(), backwards.fact().schema());
        assert_eq!(forwards.lookup().schema(), backwards.lookup().schema());
        assert_ne!(
            forwards.fact().schema(),
            forwards.lookup().schema(),
            "the two fixture results differ, so the assertion above is not vacuous"
        );
    }

    /// The side is read off the plan and is never taken from a caller.
    #[test]
    fn a_leg_result_takes_its_side_from_the_plan_it_was_executed_from() {
        assert_eq!(LegResult::of(&fact_plan(), batches("x")).side(), LegSide::Fact);
        assert_eq!(LegResult::of(&lookup_plan(), batches("x")).side(), LegSide::Lookup);
    }

    /// Two of one side is the one thing the pair cannot answer, and it refuses rather than
    /// combining a leg with itself.
    #[test]
    fn two_results_of_one_side_are_not_a_pair() {
        let one = LegResult::of(&fact_plan(), batches("a"));
        let other = LegResult::of(&fact_plan(), batches("b"));
        let refused = Legs::of(&one, &other).expect_err("two facts are not a pair");
        assert_eq!(refused.both(), LegSide::Fact);
    }
}

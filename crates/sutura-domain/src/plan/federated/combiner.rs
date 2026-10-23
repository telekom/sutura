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
//! A path in backticks rather than a Markdown link, and that is not style: the API reference republishes
//! this header on `sutura-domain`'s generated page, where `mkdocs --strict` resolves a relative link
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
//! # What the shape buys, which is that a fact and a lookup result cannot be swapped
//!
//! A combiner taking `(&ResultBatches, &ResultBatches)` has two arguments of one type whose swap
//! compiles and answers a wrong number under a certified metric name - the fact leg's measure
//! grouped by the lookup leg's keys. [`Legs`] refuses the swap rather than leaving it to review:
//! each leg's result is tagged with the side by [`LegResult::of`], which reads it off the
//! [`LegPlan`] the leg was executed from and takes no side from its caller, and [`Legs::assign`]
//! checks every tag against the side of the plan's leg at its position and the count of results
//! against the plan's count of legs. **The limit:** two lookup results swapped with each other
//! carry one tag, and so do the fact and the second fact ([`LegSide::Fact`]), so [`Legs::assign`]
//! sees neither swap. The order of results within one side is held by
//! [`FederationCombiner::execute`], which orders its results as [`FederatedPlan::legs`] does, and by
//! nothing for a caller that builds [`Legs`] itself.

use crate::plan::federated::{FederatedAnswerRefusal, FederatedPlan, LegSide, LookupLeg};
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
        Self {
            side: Self::side_of(plan),
            batches,
        }
    }

    const fn side_of(plan: &LegPlan) -> LegSide {
        match *plan {
            LegPlan::Fact { .. } => LegSide::Fact,
            LegPlan::Lookup { .. } => LegSide::Lookup,
        }
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

/// Every leg's result, assigned to the plan's legs, which cannot be built without every result.
///
/// A two-fact plan (`telekom/sutura#780`) carries an optional second fact leg: a result from a
/// second fact model, joined above on the link and the time bucket. The combiner reads it through
/// [`Self::second_fact`] and routes each `Carried` leaf to the fact leg its model names. Each lookup
/// result arrives beside its [`LookupLeg`], which names the leg it joins onto.
///
/// Borrowed rather than owned, because a combiner reads the batches and the caller still holds them
/// for the refusal it may have to build - and because Arrow batches are reference-counted buffers,
/// so an owned set would say *moved* about something that is shared either way.
#[derive(Debug, Clone)]
pub struct Legs<'a> {
    fact: &'a ResultBatches,
    second_fact: Option<&'a ResultBatches>,
    lookups: Vec<(&'a LookupLeg, &'a ResultBatches)>,
}

impl<'a> Legs<'a> {
    /// The results, one per [`FederatedPlan::legs`] entry and in that order, each checked against
    /// the side of the leg it stands for.
    ///
    /// # Errors
    ///
    /// [`LegsMiswired`] for a count of results that is not the plan's count of legs, and for a
    /// result whose side is not its leg's.
    pub fn assign(plan: &'a FederatedPlan, results: &'a [LegResult]) -> Result<Self, LegsMiswired> {
        let legs = plan.legs();
        if legs.len() != results.len() {
            return Err(LegsMiswired::Count {
                legs: legs.len(),
                results: results.len(),
            });
        }
        if let Some((leg, result)) = legs
            .iter()
            .zip(results)
            .find(|&(leg, result)| LegResult::side_of(leg) != result.side())
        {
            return Err(LegsMiswired::WrongSide {
                expected: LegResult::side_of(leg),
                found: result.side(),
            });
        }
        let (facts, lookups) = results.split_at(results.len() - plan.lookups().legs().len());
        let [fact, second_fact @ ..] = facts else {
            return Err(LegsMiswired::Count {
                legs: legs.len(),
                results: results.len(),
            });
        };
        Ok(Self {
            fact: fact.batches(),
            second_fact: second_fact.first().map(LegResult::batches),
            lookups: plan
                .lookups()
                .legs()
                .iter()
                .zip(lookups.iter().map(LegResult::batches))
                .collect(),
        })
    }

    /// The metric's own leg: its keys, its time bucket and its carried leaves.
    #[inline]
    #[must_use]
    pub const fn fact(&self) -> &'a ResultBatches {
        self.fact
    }

    /// The second fact leg, when the plan carries two fact models.
    #[inline]
    #[must_use]
    pub const fn second_fact(&self) -> Option<&'a ResultBatches> {
        self.second_fact
    }

    /// Each lookup leg beside its result, in plan order.
    #[inline]
    #[must_use]
    pub fn lookups(&self) -> &[(&'a LookupLeg, &'a ResultBatches)] {
        &self.lookups
    }
}

/// Leg results that do not stand for the plan's legs, so there is nothing to combine.
///
/// Unreachable through [`FederationCombiner::execute`], which builds one [`LegResult`] per
/// [`FederatedPlan::legs`] entry. Typed anyway rather than assumed away: a silent choice would
/// combine a leg with the wrong rows, or read a missing leg as a leg with no match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LegsMiswired {
    /// The plan runs `legs` legs and `results` results arrived.
    #[error("the plan runs {legs} legs and {results} results arrived")]
    Count { legs: usize, results: usize },
    /// A result stands where the plan runs a leg of the other side.
    #[error("a {found:?} leg's result stands where the plan runs a {expected:?} leg")]
    WrongSide { expected: LegSide, found: LegSide },
}

/// The legs a federated plan runs at their sources, and the plan the combine runs above them.
///
/// [`FederationCombiner::execute`] takes it by value and only this crate builds one, so the value
/// it takes came from [`FederationCombiner::plan`]. The type does not hold which legs run.
#[derive(Debug)]
pub struct Cut<'p> {
    plan: &'p FederatedPlan,
}

impl<'p> Cut<'p> {
    /// Every leg of the cut, each once, in [`FederatedPlan::legs`] order.
    pub fn legs(&self) -> impl Iterator<Item = &'p LegPlan> + use<'p> {
        self.plan.legs().into_iter()
    }
}

/// Runs one leg at its source, under every guard the application holds for a leg.
///
/// **The driven port a combiner calls back into.** The combiner runs the cut's
/// legs through it, so the credential per leg, the deadline and the refusal mapping stay in the
/// application, and an adapter still never calls another adapter. `sutura_app` implements it.
pub trait LegRunner {
    /// Why a leg did not answer: the application's own type, which a combiner carries out of
    /// [`FederationCombiner::execute`] unchanged and never maps.
    type Error;

    /// The batches one leg returned.
    ///
    /// # Errors
    ///
    /// [`Self::Error`].
    fn run(&self, leg: &LegPlan) -> Result<ResultBatches, Self::Error>;
}

/// Why [`FederationCombiner::execute`] or `DrillAcrossCombiner::execute_across` gave no answer.
///
/// The combine runs only after every leg answered, so a leg's error wins over the combine's.
#[derive(Debug)]
pub enum ExecuteError<L, C> {
    /// A leg did not answer. The runner's own error, unchanged.
    Leg(L),
    /// The leg results did not stand for the plan's legs: this workspace's own defect.
    Miswired(LegsMiswired),
    /// The combine failed, which [`FederationCombiner`]'s two predicates classify.
    Combine(C),
}

/// What [`FederationCombiner::execute`] and `DrillAcrossCombiner::execute_across` return, named so
/// their signatures are not a `type_complexity` finding.
pub type Executed<R, C> = Result<ResultBatches, ExecuteError<<R as LegRunner>::Error, <C as FederationCombiner>::Error>>;

/// Joins two legs' results and re-aggregates the answer above them.
///
/// **The second driven port, declared beside [`Warehouse`](crate::warehouse::Warehouse) and
/// [`SemanticCatalog`](crate::pinned::SemanticCatalog)**, and the module header is why it is a port.
/// `sutura_app::federated::answer_federated` is the driver; a crate above this one implements it.
/// An adapter never calls another adapter and this does not change that: the combine is called from
/// the application, above every adapter, exactly where `FederatedPlan::combine` was called from.
///
/// # Every method but the cut's two is required, and that is the mechanism
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

    /// One answer's rows from every leg's result.
    ///
    /// Each lookup result is joined onto the result of the leg it names, on every link column both
    /// project under that lookup leg's [`InternalLabel::Link`](crate::plan::InternalLabel::Link)
    /// (one per key part, [`FederatedPlan::link_labels`]),
    /// in the direction its [`LookupLeg::include_unmatched`] names, then grouped by the answer's keys in
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

    /// The cut of `plan`: the legs that run at their sources. Executes nothing.
    ///
    /// Provided, unlike the methods above, because today's cut is the splitter's own legs and this
    /// crate holds it: every implementor cuts the same way until an engine cuts the plan itself.
    fn plan<'p>(&self, plan: &'p FederatedPlan) -> Cut<'p> {
        Cut { plan }
    }

    /// Runs the cut's legs through `runner`, then [`Self::combine`]s their results.
    ///
    /// The fact leg runs on this thread while each lookup leg runs beside it on a scoped thread of
    /// its own, and a second fact runs after all of them. Scoped, so the runner and the plan are
    /// borrowed and nothing is cloned. If the OS refuses a thread, that lookup leg runs after the
    /// fact leg on this thread. The fact leg's error wins over every lookup leg's, an earlier lookup
    /// leg's over a later one's, all of them over the second fact's, and every leg's over the
    /// combine's. A leg that did not answer ends the answer, so the combine never reads a missing
    /// leg as a leg with no match.
    ///
    /// Provided, for the reason [`Self::plan`] is.
    ///
    /// # Errors
    ///
    /// [`ExecuteError`].
    fn execute<R>(&self, cut: Cut<'_>, runner: &R, working_set_bytes: u64) -> Executed<R, Self>
    where
        R: LegRunner + Sync,
        R::Error: Send,
    {
        let plan = cut.plan;
        let lookups: Vec<&LegPlan> = plan.lookups().legs().iter().map(LookupLeg::leg).collect();
        let (fact, looked_up) = std::thread::scope(|scope| {
            #[expect(clippy::needless_collect, reason = "every lookup leg starts before the fact leg runs")]
            let started: Vec<_> = lookups
                .iter()
                .map(|&leg| {
                    std::thread::Builder::new()
                        .name(String::from("federated-lookup-leg"))
                        .spawn_scoped(scope, move || runner.run(leg))
                        .map_err(|_refused| leg)
                })
                .collect();
            let fact = runner.run(plan.fact());
            let looked_up: Vec<_> = started
                .into_iter()
                .map(|thread| match thread {
                    Ok(thread) => match thread.join() {
                        Ok(lookup) => lookup,
                        // A panicking leg aborts the process under `panic = "abort"`, so this arm is
                        // reachable only in an unwinding test build. There is no leg outcome to return.
                        Err(panic) => std::panic::resume_unwind(panic),
                    },
                    Err(leg) => runner.run(leg),
                })
                .collect();
            (fact, looked_up)
        });
        let fact = LegResult::of(plan.fact(), fact.map_err(ExecuteError::Leg)?);
        let looked_up = lookups
            .into_iter()
            .zip(looked_up)
            .map(|(leg, answered)| answered.map(|batches| LegResult::of(leg, batches)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(ExecuteError::Leg)?;
        let second_fact = plan
            .second_fact()
            .map(|leg| runner.run(leg).map(|batches| LegResult::of(leg, batches)))
            .transpose()
            .map_err(ExecuteError::Leg)?;
        let results: Vec<LegResult> = core::iter::once(fact).chain(second_fact).chain(looked_up).collect();
        let legs = Legs::assign(plan, &results).map_err(ExecuteError::Miswired)?;
        self.combine(plan, legs, working_set_bytes).map_err(ExecuteError::Combine)
    }
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
    use super::LegResult;
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
            table: table("dim_region").into(),
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

    /// The side is read off the plan and is never taken from a caller.
    #[test]
    fn a_leg_result_takes_its_side_from_the_plan_it_was_executed_from() {
        assert_eq!(LegResult::of(&fact_plan(), batches("x")).side(), LegSide::Fact);
        assert_eq!(LegResult::of(&lookup_plan(), batches("x")).side(), LegSide::Lookup);
    }
}

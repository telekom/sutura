//! Plan: the resolved question becomes a [`QueryPlan`] or a [`FederatedPlan`], and three things are
//! settled here and nowhere else.
//!
//! **The plan names its data systems.** A question whose joins reach a data system other than the
//! metric's own is refused unless there is exactly one such system - two sources are served by
//! splitting the question into a fact leg and a lookup leg and combining them above; three or more
//! are refused before anything runs, because each source is a separate identity to satisfy.
//!
//! **Every value becomes a bind parameter, in the order the statement will refer to them.** That
//! order is this module's contract with whatever executes the plan: for a dialect that writes `?`,
//! position in the statement *is* a parameter's identity, so the predicate list and the parameter
//! list are built together, in one pass, and cannot drift apart.
//!
//! **A definitional predicate is applied whether the caller asked or not.** A metric's required
//! filters go in before anything the caller chose, so `mrr` cannot be answered without
//! `status = 'active'`. They are marked [`PredicateOrigin::Definition`], which is what lets a golden
//! assert they are always present and a reader tell them from what was requested.
//!
//! The plan types live in `sutura-domain`, because the execution port speaks them. This module only
//! decides what goes into one.
//!
//! **The splitter lives here because it reads a [`Resolution`].** It turns a two-source question
//! into the fact and lookup [`LegPlan`](sutura_domain::plan::LegPlan)s the combiner in
//! `sutura_domain::plan::federated` joins, and refuses a measure that cannot decompose. The
//! labelling of each leg's terms is `sutura_domain`'s one function,
//! [`labels`](sutura_domain::plan::labels) - named by the splitter and read back by the combiner,
//! so the two can never disagree.

use std::collections::BTreeSet;

use sutura_domain::measure::Measure;
use sutura_domain::model::{DimensionName, MetricName, ModelName, SourceName, TableName};
use sutura_domain::plan::{
    FederatedPlan, FederatedPlanError, IncoherentBindings, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey,
    PlanPredicate, PredicateOrigin, QueryPlan, ResultLabel, StatementTables, plan_measure, plan_required_filter,
};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::ParamValue;

use crate::resolve::{Resolution, ResolvedFilter, ResolvedFilterValue};

mod chain;
mod federated;

use chain::{chain_joins, chain_leaving_its_source, column_of, every_remote_dimension};
use federated::federated_plan;

/// What the plan stage decided to execute.
///
/// The plans are held behind pointers on purpose: both are large values, and this enum is handed
/// around (matched, returned, stashed in a `Compiled`) far more often than it is reconstructed. An
/// enum sized to the larger of the two would copy a whole plan every time it moved.
pub(crate) enum Plan {
    /// One whole answer from one data system.
    Mono(Box<QueryPlan>),
    /// Two legs from two data systems, combined above.
    Federated(Box<FederatedPlan>),
}

/// Refused, or this workspace could not assemble the plan it had just decided on.
///
/// **The two go to different places, and that is `telekom/sutura#338`.** A refusal becomes a result
/// the caller reads; a plan the splitter built and [`FederatedPlan::new`] then rejected is a defect
/// in this workspace's own wiring, which no caller can act on and none should be told to retry.
/// They used to be one value: the assembly failure was flattened into
/// [`RefusalReason::FederationNotExecutable`], which is ALSO the answer a build gets when its
/// adapter type does not declare `Warehouse::EXECUTES_LEGS` - so a wiring defect here and a
/// deployment that cannot run a leg were indistinguishable at the surface, and a refusal a caller
/// can only tell apart by comparing two answers is not a refusal. **That was sharper before
/// `telekom/sutura#441`**, when every published build took the default and the two were literally
/// the same answer everywhere; it is narrower and still true now that the shipped engine executes a
/// leg, because a build whose adapter takes the default - `BigQuery`, or a fake - still gets that
/// refusal, and this failure must not look like it.
///
/// The same two arms `crate::resolve::ResolveError` already has, for the same reason. **The limit,
/// next to the claim:** every [`FederatedPlanError`] variant is structurally unreachable from
/// `federated_plan` as it stands - the splitter builds a `Fact` beside a `Lookup` on a source
/// `is_remote` has already established is not its own, projects `InternalLabel::Link` onto both legs
/// unconditionally, derives each answer key's side from the same predicate that filled that leg's
/// keys, and refuses a `Carried::Keys` leaf before this point, so no `combine` other than `Sum`,
/// `Min` or `Max` reaches the re-aggregation check. So this arm carries no test that can provoke it,
/// and what it buys is that a future edit which makes one of those reachable produces an error
/// rather than a governance refusal. `crates/sutura-app/tests/differential/federated.rs` is the
/// venue that would see such an edit today: it asserts that the only compile-side refusal a
/// two-source corpus question may get is `MeasureDoesNotFederate`.
///
/// [`NotBound`](PlanError::NotBound) is the fourth arm and carries the same argument for the same
/// reason. [`predicates_and_params`] and `federated::requested_for` mint every parameter index from the
/// position the value was pushed to, so neither can build a set
/// [`PlanBindings::parse`](sutura_domain::plan::PlanBindings::parse) refuses, and no test provokes
/// this arm either. What it buys is that a producer which stops minting - a hand-written index, a
/// reordered push - surfaces as a failure rather than as a statement that renders correctly on a
/// numbered dialect and binds the wrong values on a positional one.
/// [`sutura_domain::plan::bindings`] argues why that is a wrong number rather than an error, and it
/// is why this is not a [`RefusalReason`]: a caller cannot narrow their question out of our own
/// arithmetic.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PlanError {
    #[error("the question was refused")]
    Refused(RefusalReason),
    #[error("metric {metric} uses authored SQL, which this plan shape does not carry")]
    AuthoredSqlNotPlanned { metric: MetricName },
    #[error(transparent)]
    NotAssembled(#[from] FederatedPlanError),
    #[error(transparent)]
    NotBound(#[from] IncoherentBindings),
    /// `federated_plan` ran with no remote dimension it could join through.
    ///
    /// **A2: the same argument [`NotAssembled`](Self::NotAssembled) and [`NotBound`](Self::NotBound)
    /// already carry, applied to a third invariant.** `plan` calls `federated_plan` only when exactly
    /// one remote source exists, and a remote dimension has a join by construction - so no test
    /// provokes this arm either. It used to be reported as
    /// `RefusalReason::PlanSpansTooManySources { sources: 1, limit: 2 }`, a fabricated count with no
    /// relationship to anything a caller asked; a caller cannot narrow their way out of our own
    /// wiring, which is why this is not a [`RefusalReason`] at all.
    #[error("the federated splitter found no remote dimension to join the fact leg through")]
    NoRemoteJoin,
    /// A resolved chain that leaves the metric's data system anywhere but at its first hop.
    ///
    /// **The plan-time half of a refusal that used to exist only at load, and the reason for two is
    /// that the load one was wrong in a way nothing downstream could see.** It compared each hop's
    /// TARGET against the metric's source, so a chain crossing at hop 1 and returning at hop 2 was
    /// accepted; [`chain::is_remote`] then read that chain off its last hop, called it local, and
    /// [`mono_plan`] rendered the other system's table into one statement under a certified metric
    /// name. `sutura_domain::catalog::Definitions::assemble` compares both ends of every later hop
    /// now, which is the mechanism - and this is what holds if a bundle ever reaches the plan stage
    /// without having been through it.
    ///
    /// A [`PlanError`] rather than a [`RefusalReason`], for [`NoRemoteJoin`](Self::NoRemoteJoin)'s
    /// reason: a caller cannot narrow their question out of a catalog this workspace admitted, so
    /// telling them to ask differently would be telling them to retry our defect. `hop` is 1-based,
    /// matching the number the load-time report gives a catalog author.
    ///
    /// **Unlike the three arms above it, this one is provoked** - `a_chain_that_leaves_its_source`
    /// in this module's tests builds the resolution by hand, which is the only way past the load
    /// check, and that is also the negative control for the load check being the real mechanism.
    #[error("dimension {dimension} of metric {metric} leaves its data system at hop {hop} of its chain")]
    ChainLeavesItsSource {
        metric: MetricName,
        dimension: DimensionName,
        hop: usize,
    },
}

impl From<RefusalReason> for PlanError {
    fn from(reason: RefusalReason) -> Self {
        Self::Refused(reason)
    }
}

/// Turns a resolution into a plan, or refuses it.
///
/// **Two refusals are produced here and nowhere else, and both are about the SHAPE of the statement
/// rather than about anything a caller wrote:** a plan that would read from more data systems than
/// the deployment serves, and a statement whose tables could not be told apart inside it. Everything
/// a caller could have got wrong was already checked when their names were looked up.
///
/// The second one is asked TWICE, once per plan shape, and that is the type's doing rather than this
/// function's discipline: a whole-answer plan and a fact leg each take their tables as a
/// [`StatementTables`], so neither can be built without the answer.
///
/// **A third thing is checked before either, and it is not a refusal:** a chain that leaves the
/// metric's data system after its first hop cannot be rendered by either plan shape, and neither
/// shape would notice - see [`PlanError::ChainLeavesItsSource`].
pub(crate) fn plan(resolution: &Resolution<'_>) -> Result<Plan, PlanError> {
    // `resolve` has already checked a multi-metric question exactly as it checks a single one -
    // every metric's grain, every dimension against every metric, every filter value against
    // every metric's own allowlist. What it has NOT done is decide how more than one metric's
    // measure becomes one statement's select list, which this stage does not do yet. See
    // `RefusalReason::MultiMetricNotExecutable`'s own doc comment for the boundary.
    if resolution.metrics.len() > 1 {
        return Err(PlanError::Refused(RefusalReason::MultiMetricNotExecutable {
            requested: resolution.metrics.len(),
        }));
    }
    let Some(measure) = resolution.metric.measure() else {
        return Err(PlanError::AuthoredSqlNotPlanned {
            metric: resolution.metric.name().clone(),
        });
    };
    // `telekom/sutura#780`: neither plan shape builds a second FACT leg, so a term naming a model
    // other than the metric's own is refused here rather than resolved against the metric's own
    // table under a certified name - the catalog already proved the reference, not the plan shape.
    if let Some(model) = cross_model_term(resolution, measure) {
        return Err(PlanError::Refused(RefusalReason::CrossModelRatioNotExecutable {
            metric: resolution.metric.name().clone(),
            model: model.clone(),
        }));
    }
    if let Some((dimension, hop)) = chain_leaving_its_source(resolution) {
        return Err(PlanError::ChainLeavesItsSource {
            metric: resolution.metric.name().clone(),
            dimension: dimension.clone(),
            hop,
        });
    }
    // Every source besides the metric's own that a chain reaches. The LAST hop decides which source
    // a dimension reads from (`is_remote`'s note), so the filter cannot drop a source here: a remote
    // dimension's last hop sits on that source by construction.
    let remote: BTreeSet<&SourceName> = every_remote_dimension(resolution)
        .filter_map(|dim| dim.join.as_ref().and_then(|hops| hops.last()).map(|hop| hop.model.source()))
        .collect();

    match remote.len() {
        0 => Ok(Plan::Mono(Box::new(mono_plan(resolution, measure)?))),
        1 => Ok(Plan::Federated(Box::new(federated_plan(resolution, measure)?))),
        // Two are served; three or more refused, because each source is a separate identity.
        _ => Err(PlanError::Refused(RefusalReason::PlanSpansTooManySources {
            sources: 1 + remote.len(),
            limit: 2,
        })),
    }
}

/// The first term whose `model` names one other than the metric's own, if the measure has one.
///
/// `None` for every measure written before `telekom/sutura#780`'s vocabulary existed, and for a
/// term that names the metric's own model explicitly - the two are the same question to a plan,
/// because both resolve their column against the metric's own table.
fn cross_model_term<'a>(resolution: &Resolution<'a>, measure: &'a Measure) -> Option<&'a ModelName> {
    let own = resolution.metric.model();
    measure.models().into_iter().flatten().find(|model| *model != own)
}

/// A question confined to one data system: exactly the plan this module already built.
fn mono_plan(resolution: &Resolution<'_>, closed: &Measure) -> Result<QueryPlan, PlanError> {
    let metric = resolution.metric;
    let model = resolution.model;
    // Two readings of "the table", and both are used below. `own_path` is what the `FROM` names -
    // dataset and project included, where the model declares them. `own_table` is the bare name, which
    // is what every column is qualified by: `FROM a.b.c` gives the reference an implicit alias of `c`
    // in all five dialects rendered for, so a `PlanColumn` holds `c` and never the path.
    let own_path = model.table();
    let own_table = model.table_name();

    // Every hop of every chain the question reaches - see `chain_joins`. `mono_plan` is dispatched
    // only for a question with no remote dimension at all, so no chain stops short here.
    let joins = chain_joins(resolution, own_table, model.source());

    let time_column = PlanColumn::new(own_table.clone(), metric.time_column().clone());

    let requested: Vec<&ResolvedFilter> = resolution.filters.iter().collect();
    let bindings = predicates_and_params(resolution, &requested, own_table, &time_column)?;

    let keys: Vec<PlanKey> = resolution
        .keys
        .iter()
        .map(|key| PlanKey::new(ResultLabel::dimension(key.dimension.name()), column_of(key, own_table)))
        .collect();

    let measure = plan_measure(closed, |column| PlanColumn::new(own_table.clone(), column.clone()));

    // **Where the statement's tables stop being a list and become a checked set.** Two tables whose
    // paths end in the same name render under one implicit alias, so a column qualified by it names
    // neither and the `ON` clause compares one table with itself - reproduced, and
    // `sutura_domain::plan::tables` holds the measurement and the argument for refusing rather than
    // aliasing. The refusal is here rather than at load because a physical table name is not
    // something a catalog author can rename, so the metric stays authorable and only the question
    // that actually puts both in one statement is declined.
    let tables =
        StatementTables::parse(own_path.clone(), joins).map_err(|ambiguous| RefusalReason::PlanTablesShareAnIdentifier {
            table: ambiguous.alias().clone(),
        })?;

    let plan = QueryPlan::new(
        model.source().clone(),
        metric.name().clone(),
        tables,
        PlanBucket::new(ResultLabel::bucket(), resolution.grain, time_column),
        keys,
        measure,
        ResultLabel::measure(metric.name()),
        bindings,
        resolution.range,
    );
    Ok(match resolution.top {
        Some(top) => plan.with_top(top),
        None => plan,
    })
}

/// Every predicate a fact leg will carry, and the parameters they bind, built together.
///
/// The range bounds, then the metric's required filters, then `requested` - the caller's own filters
/// that constrain this leg's columns. `requested` is passed in (rather than read off the resolution)
/// so the splitter can hand the local half here and the remote half to the lookup leg.
///
/// **Shared by both plan shapes**, which is why it stays here rather than moving into
/// `plan::federated` with the rest of the splitter: [`mono_plan`] above calls it directly, and
/// `plan::federated::federated_plan` reaches it as `super::predicates_and_params`.
fn predicates_and_params(
    resolution: &Resolution<'_>,
    requested: &[&ResolvedFilter<'_>],
    own_table: &TableName,
    time_column: &PlanColumn,
) -> Result<PlanBindings, IncoherentBindings> {
    let metric = resolution.metric;
    let mut params: Vec<ParamValue> = Vec::new();
    let mut filters: Vec<PlanFilter> = Vec::new();

    // A function-shaped closure, so it never borrows `params` for its own lifetime: each call takes
    // the list as an argument and borrows it only for the statement.
    let bind = |params: &mut Vec<ParamValue>, value: ParamValue| {
        params.push(value);
        params.len().saturating_sub(1)
    };

    let start = bind(&mut params, ParamValue::Date(resolution.range.start()));
    filters.push(PlanFilter::new(
        PredicateOrigin::Definition,
        PlanPredicate::AtOrAfter {
            column: time_column.clone(),
            param: start,
        },
    ));
    let end = bind(&mut params, ParamValue::Date(resolution.range.end()));
    filters.push(PlanFilter::new(
        PredicateOrigin::Definition,
        PlanPredicate::Before {
            column: time_column.clone(),
            param: end,
        },
    ));

    for required in metric.required_filters() {
        let column = PlanColumn::new(own_table.clone(), required.column().clone());
        let predicate = plan_required_filter(required, column, |value| {
            params.push(ParamValue::Text(value));
            params.len().saturating_sub(1)
        });
        filters.push(PlanFilter::new(PredicateOrigin::Definition, predicate));
    }

    for filter in requested {
        let column = column_of(&filter.dimension, own_table);
        let predicate = requested_predicate(filter, column, &mut params);
        filters.push(PlanFilter::new(PredicateOrigin::Requested, predicate));
    }

    PlanBindings::parse(filters, params)
}

/// One requested filter's predicate, and the parameter(s) it binds - one for `Eq`, one per value
/// for `In`/`NotIn`, each pushed in placeholder order so [`PlanBindings::parse`] sees them
/// consecutive.
fn requested_predicate(filter: &ResolvedFilter<'_>, column: PlanColumn, params: &mut Vec<ParamValue>) -> PlanPredicate {
    match filter.value {
        ResolvedFilterValue::Eq(ref value) => {
            let param = params.len();
            params.push(ParamValue::Text(value.clone()));
            PlanPredicate::Equals { column, param }
        }
        ResolvedFilterValue::In(ref values) => {
            let start = params.len();
            params.extend(values.iter().cloned().map(ParamValue::Text));
            PlanPredicate::In {
                column,
                params: index_range(start, values.len()),
            }
        }
        ResolvedFilterValue::NotIn(ref values) => {
            let start = params.len();
            params.extend(values.iter().cloned().map(ParamValue::Text));
            PlanPredicate::NotIn {
                column,
                params: index_range(start, values.len()),
            }
        }
    }
}

/// `count` consecutive placeholder indices from `start`, as the `NonEmpty` [`PlanPredicate::In`]/
/// [`PlanPredicate::NotIn`] carry - infallible because `count` is a [`sutura_domain::nonempty::NonEmpty`]'s
/// own [`sutura_domain::nonempty::NonEmpty::len`], never zero, so there is no [`sutura_domain::nonempty::EmptySet`]
/// this could produce and no `expect`/`unwrap` to reach for.
fn index_range(start: usize, count: usize) -> sutura_domain::nonempty::NonEmpty<usize> {
    sutura_domain::nonempty::NonEmpty::of(start, ((start.saturating_add(1))..start.saturating_add(count)).collect())
}

/// The predicates a lookup leg carries: only the caller's own remote filters, bound on the remote
/// table.
fn requested_for(requested: &[&ResolvedFilter<'_>], remote_table: &TableName) -> Result<PlanBindings, IncoherentBindings> {
    let mut params: Vec<ParamValue> = Vec::new();
    let mut filters: Vec<PlanFilter> = Vec::new();
    for filter in requested {
        let column = PlanColumn::new(remote_table.clone(), filter.dimension.dimension.column().clone());
        let predicate = requested_predicate(filter, column, &mut params);
        filters.push(PlanFilter::new(PredicateOrigin::Requested, predicate));
    }
    PlanBindings::parse(filters, params)
}
/// The three properties of a chain a plan is a function of: which table qualifies hop N's origin
/// column, that a chain which left its data system is refused here as well as at load, and that the
/// join order does not depend on the order the caller listed their dimensions.
///
/// Inline rather than a `plan/tests.rs`, and the reason is the causality gate rather than taste: it
/// reconstructs a base tree by reverting every changed file that added no test, so a `mod tests;`
/// line in a reverted `plan.rs` would orphan the file it declares and the base run would have no
/// test to be red.
///
/// A [`Resolution`] is built by hand here, and that is the ONLY venue two of these can be measured
/// in. `sutura_domain::catalog::Definitions::assemble` refuses a cross-source chain at load, so no
/// pinned bundle can carry one this far, and a question has no field that names a join - so the
/// plan-time refusal is provokable from a hand-built resolution and from nothing else. Reaching
/// past the load check is what makes that cell honest about being the SECOND reader of one rule.
/// The end-to-end evidence for the first property is the two-hop dimension in
/// `examples/single-player`, which every rendering golden and the executed corpus read.
#[cfg(test)]
mod tests;

//! Plan: the resolved question becomes a [`QueryPlan`] or a [`FederatedPlan`], and three things are
//! settled here and nowhere else.
//!
//! **The plan names its lookup legs.** A question whose joins cross onto a data system other than
//! the metric's own is split into a fact leg and one lookup leg per crossing relationship, combined
//! above; more lookup legs than the deployment's [`LookupLegCeiling`] are refused before anything
//! runs, because each leg's source is a separate identity to satisfy.
//!
//! [`LookupLegCeiling`]: sutura_domain::plan::LookupLegCeiling
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

use sutura_domain::calendar::TimeRange;
use sutura_domain::catalog::Metric;
use sutura_domain::measure::Measure;
use sutura_domain::model::{MetricName, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::nonempty::NonEmpty;
use sutura_domain::plan::{
    DrillAcrossPlan, DrillAcrossPlanError, FederatedPlan, FederatedPlanError, IncoherentBindings, LegCeiling, PlanBindings,
    PlanBucket, PlanCeilings, PlanColumn, PlanFilter, PlanKey, PlanPredicate, PlannedMeasure, PredicateOrigin, QueryPlan,
    ResultLabel, SourceCeiling, StatementTables, plan_measure, plan_required_filter,
};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::ParamValue;

use crate::resolve::{Resolution, Resolved, ResolvedFilter, ResolvedFilterValue};

mod chain;
mod drill_across;
mod federated;

use chain::{Lookups, chain_joins, column_of, every_dimension};
use federated::federated_plan;

/// What the plan stage decided to execute.
///
/// The plans are held behind pointers on purpose: both are large values, and this enum is handed
/// around (matched, returned, stashed in a `Compiled`) far more often than it is reconstructed. An
/// enum sized to the larger of the two would copy a whole plan every time it moved.
pub(crate) enum Plan {
    /// One whole answer from one data system.
    Mono(Box<QueryPlan>),
    /// A fact leg and one to the configured number of lookup legs on other data systems, combined above.
    Federated(Box<FederatedPlan>),
    /// One arm per metric, each answered on its own and joined above.
    DrillAcross(Box<DrillAcrossPlan>),
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
/// `Lookups` has already established is not its own, projects each lookup leg's `InternalLabel::link`
/// onto that leg and the leg it joins onto unconditionally, derives each answer key's side from the same predicate that filled that leg's
/// keys, and projects a `Carried::Keys` leaf as a fact key under its own label. So this arm carries
/// no test that can provoke it, and what it buys is that a future edit which makes one of those
/// reachable produces an error rather than a governance refusal.
/// `crates/sutura-app/tests/differential/federated.rs` is the venue that would see such an edit
/// today: it asserts that no two-source corpus question is refused at compile.
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
    /// The planner built arms that [`DrillAcrossPlan::new`] then rejected.
    ///
    /// [`NotAssembled`](Self::NotAssembled)'s argument again: every arm is built from the same
    /// question, so its keys, bucket and range agree, no metric is named twice, and a `top` is refused
    /// when the question resolves. The one variant a caller could cause, a question over more legs than
    /// [`MAX_LEGS`](sutura_domain::plan::MAX_LEGS), is refused against the deployment's
    /// [`LegCeiling`](sutura_domain::plan::LegCeiling) - at most `MAX_LEGS` - before the plan is built.
    #[error(transparent)]
    NotAssembledAcross(#[from] DrillAcrossPlanError),
    /// `federated_plan` ran with no lookup leg it could join through.
    ///
    /// **A2: the same argument [`NotAssembled`](Self::NotAssembled) and [`NotBound`](Self::NotBound)
    /// already carry, applied to a third invariant.** `plan` calls `federated_plan` only when at
    /// least one lookup leg exists, and a remote dimension has a join by construction - so no test
    /// provokes this arm either. A caller cannot narrow their way out of our own wiring, which is
    /// why this is not a [`RefusalReason`] at all.
    #[error("the federated splitter found no remote dimension to join the fact leg through")]
    NoRemoteJoin,
    /// A cross-model ratio reached the splitter with no `resolution.cross` to build its second
    /// fact leg from.
    ///
    /// **Unreachable by construction, [`NoRemoteJoin`](Self::NoRemoteJoin)'s reason again.**
    /// `resolve` fills `cross` whenever the first metric names another model and declares a
    /// calendar, `plan` refuses a ratio with no calendar before this,
    /// and `Federation::of_metric` erases the metric's own model, so a leaf left naming a model is
    /// one `cross` was built for. A `PlanError` rather than a `RefusalReason`, for the same reason:
    /// a caller cannot narrow their way out of our own wiring.
    #[error("the federated splitter carries a second fact's leaves with no second fact model or calendar model")]
    NoSecondFactModel,
}

impl From<RefusalReason> for PlanError {
    fn from(reason: RefusalReason) -> Self {
        Self::Refused(reason)
    }
}

/// Turns a resolution into a plan, or refuses it.
///
/// **The refusals produced here are about the SHAPE of the plan rather than about anything a caller
/// wrote:** a plan that would read more lookup legs, legs or data systems than `ceilings` allows, and
/// a statement whose tables could not be told apart inside it. Everything
/// a caller could have got wrong was already checked when their names were looked up.
///
/// The second one is asked TWICE, once per plan shape, and that is the type's doing rather than this
/// function's discipline: a whole-answer plan and a fact leg each take their tables as a
/// [`StatementTables`], so neither can be built without the answer.
pub(crate) fn plan(resolution: &Resolution<'_>, ceilings: PlanCeilings) -> Result<Plan, PlanError> {
    // The one-statement path builds one `PlannedMeasure` per metric, each guarded by its own required
    // filters so none of them leaks into another's column; the metrics share a model, a time column
    // and every dimension's path (`one_statement`). No authored-SQL metric can be part of a question
    // this plan shape carries: the plan holds no SQL. Checked over every named metric, so a
    // multi-metric question naming one beside ordinary ones is refused by name rather than silently
    // dropped from the select list.
    for named in &resolution.metrics {
        if named.measure().is_none() {
            return Err(PlanError::AuthoredSqlNotPlanned {
                metric: named.name().clone(),
            });
        }
    }
    let (measure, lookups) = prechecked(resolution)?;
    match lookups.len() {
        0 => Ok(Plan::Mono(Box::new(mono_plan(resolution, measure)?))),
        legs if legs > ceilings.lookup_legs().get() => Err(too_many_lookup_legs(legs, ceilings)),
        _ => {
            let planned = federated_plan(resolution, measure, &lookups)?;
            bounded_legs(planned.legs().len(), ceilings.legs())?;
            bounded_sources(planned.sources(), ceilings.sources())?;
            Ok(Plan::Federated(Box::new(planned)))
        }
    }
}

/// The refusal for one metric's plan over `legs` lookup legs, past the deployment's ceiling.
pub(super) const fn too_many_lookup_legs(legs: usize, ceilings: PlanCeilings) -> PlanError {
    PlanError::Refused(RefusalReason::PlanNeedsTooManyLookupLegs {
        legs,
        limit: ceilings.lookup_legs().get(),
    })
}

/// Refuses an answer that runs more legs than the deployment's `ceiling`, every leg of every metric
/// counted.
pub(super) const fn bounded_legs(legs: usize, ceiling: LegCeiling) -> Result<(), PlanError> {
    if legs > ceiling.get() {
        return Err(PlanError::Refused(RefusalReason::QuestionNeedsTooManyLegs {
            legs,
            limit: ceiling.get(),
        }));
    }
    Ok(())
}

/// Refuses a plan that reads more distinct data systems than the deployment's `ceiling`.
///
/// The one count for the one-statement path and for a drill-across: the sources are the set the broker
/// mints credentials for, so a source two legs read counts once.
pub(super) fn bounded_sources<'a>(
    sources: impl Iterator<Item = &'a SourceName>,
    ceiling: SourceCeiling,
) -> Result<(), PlanError> {
    let sources = sources.collect::<BTreeSet<_>>().len();
    if sources > ceiling.get() {
        return Err(PlanError::Refused(RefusalReason::PlanSpansTooManySources {
            sources,
            limit: ceiling.get(),
        }));
    }
    Ok(())
}

/// Turns a resolved question into a plan: the one statement when every metric reads the same
/// dimensions the same way, an arm per metric otherwise.
pub(crate) fn plan_resolved(resolved: &Resolved<'_>, ceilings: PlanCeilings) -> Result<Plan, PlanError> {
    match resolved.per_metric.as_slice() {
        [first, second, rest @ ..] if !one_statement(resolved) => drill_across::plan(first, second, rest, ceilings),
        _ => plan(&resolved.whole, ceilings),
    }
}

/// Whether every metric of a multi-metric question can share one grouped statement: one model, one
/// time column, no remote dimension, no cross-model term, and every dimension reached through the
/// same relationships. Anything else is an arm per metric, which reads each metric's own path.
fn one_statement(resolved: &Resolved<'_>) -> bool {
    let whole = &resolved.whole;
    let first = paths(whole);
    Lookups::of(whole).is_empty()
        && cross_model_term(whole).is_none()
        && resolved.per_metric.iter().all(|each| {
            each.model.name() == whole.model.name()
                && each.metric.time_column() == whole.metric.time_column()
                && paths(each) == first
        })
}

/// The relationships each dimension of `resolution` is reached through, in the order it lists them.
fn paths<'a>(resolution: &Resolution<'a>) -> Vec<Vec<&'a RelationshipName>> {
    every_dimension(resolution)
        .map(|dimension| dimension.join.iter().flatten().map(|hop| hop.relationship.name()).collect())
        .collect()
}

/// A metric's measure and its lookup legs, or the refusal.
type Prechecked<'r, 'a> = Result<(&'a Measure, Lookups<'r, 'a>), PlanError>;

/// What one metric's resolution reads, refused where it cannot be planned: its measure, and the
/// lookup legs its dimensions' crossings open.
///
/// Shared by the one-statement path and by each arm of a drill-across, which are each one metric's
/// plan.
fn prechecked<'r, 'a>(resolution: &'r Resolution<'a>) -> Prechecked<'r, 'a> {
    let Some(measure) = resolution.metric.measure() else {
        return Err(PlanError::AuthoredSqlNotPlanned {
            metric: resolution.metric.name().clone(),
        });
    };
    let lookups = Lookups::of(resolution);
    // `telekom/sutura#780`: a cross-model ratio plans as two fact statements joined above, both
    // bucketed through the metric's shared calendar - a conformed time dimension each fact joins
    // inside its own statement. Every shape that cannot be built that way is refused here by name,
    // never planned against the metric's own table.
    if let Some((cross_metric, model)) = cross_model_term(resolution) {
        if cross_metric.shared_calendars().is_none() {
            return Err(PlanError::Refused(RefusalReason::CrossModelRatioWithoutSharedCalendar {
                metric: cross_metric.name().clone(),
                model: model.clone(),
            }));
        }
        let Some(cross) = resolution.cross.as_ref() else {
            return Err(PlanError::NoSecondFactModel);
        };
        // Each fact leg joins the calendar inside its own statement, and a statement reads one
        // source - so the second fact sits on the metric's. The calendar is the one listed on the
        // metric's own source, so it does by construction.
        let own = resolution.model.source();
        if cross.second.source() != own {
            return Err(PlanError::Refused(RefusalReason::CrossModelRatioSpansSources {
                metric: cross_metric.name().clone(),
                model: cross.second.name().clone(),
            }));
        }
        // The two facts are combined above on the link into a remote dimension, so the question
        // groups and filters by that dimension's model and nothing else: a same-source dimension
        // would group the first fact alone, and with no remote dimension there is no link.
        if lookups.is_empty() || every_dimension(resolution).any(|dimension| lookups.index_of(dimension).is_none()) {
            return Err(PlanError::Refused(RefusalReason::CrossModelRatioWithoutSharedDimension {
                metric: cross_metric.name().clone(),
                model: cross.second.name().clone(),
            }));
        }
    }
    Ok((measure, lookups))
}

/// The first metric (and the first term within it) whose `model` names one other than that metric's
/// own, if any metric's measure has one.
///
/// `None` for every measure written before `telekom/sutura#780`'s vocabulary existed, and for a
/// term that names the metric's own model explicitly - the two are the same question to a plan,
/// because both resolve their column against the metric's own table. Scans every named metric,
/// because a multi-metric question can name one metric with a cross-model ratio beside others
/// without one.
fn cross_model_term<'a>(resolution: &Resolution<'a>) -> Option<(&'a Metric, &'a ModelName)> {
    resolution.metrics.iter().copied().find_map(|metric| {
        let own = metric.model();
        metric
            .measure()?
            .models()
            .into_iter()
            .flatten()
            .find(|model| *model != own)
            .map(|model| (metric, model))
    })
}

/// A question confined to one data system.
///
/// One grouped statement, with one certified column per named metric. A single-metric question is
/// exactly the plan this module already built - the metric's required filters stay in the shared
/// `WHERE`, where there is only one column for them to constrain. A multi-metric question folds
/// each metric's OWN required filters into that metric's conditional aggregate (its guard), so one
/// metric's filter cannot prune a row another metric's column must count.
fn mono_plan(resolution: &Resolution<'_>, closed: &Measure) -> Result<QueryPlan, PlanError> {
    let model = resolution.model;
    // Two readings of "the table", and both are used below. `own_path` is what the `FROM` names -
    // dataset and project included, where the model declares them. `own_table` is the bare name, which
    // is what every column is qualified by: `FROM a.b.c` gives the reference an implicit alias of `c`
    // in all five dialects rendered for, so a `PlanColumn` holds `c` and never the path.
    let own_path = model.table();
    let own_table = model.table_name();

    // Every hop of every chain the question reaches - see `chain_joins`. `mono_plan` is dispatched
    // only for a question with no remote dimension at all, so no chain stops short here.
    let joins = chain_joins(resolution, own_table);

    let time_column = PlanColumn::new(own_table.clone(), resolution.metric.time_column().clone());

    let requested: Vec<&ResolvedFilter> = resolution.filters.iter().collect();

    let keys: Vec<PlanKey> = resolution
        .keys
        .iter()
        .map(|key| PlanKey::new(ResultLabel::dimension(key.dimension.name()), column_of(key, own_table)))
        .collect();

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

    let bucket = PlanBucket::new(ResultLabel::bucket(), resolution.grain, time_column.clone());

    let plan = if resolution.metrics.len() == 1 {
        // Single metric: required filters in the shared WHERE, exactly as before - there is one
        // column for them to constrain, so nothing can leak.
        let bindings = predicates_and_params(resolution, &requested, own_table, &time_column)?;
        let metric = resolution.metric;
        let measure = plan_measure(closed, |column| PlanColumn::new(own_table.clone(), column.clone()));
        QueryPlan::new(
            model.source().clone(),
            metric.name().clone(),
            tables,
            bucket,
            keys,
            measure,
            ResultLabel::measure(metric.name()),
            bindings,
            resolution.range,
        )
    } else {
        // Multi metric: one column per metric, each guarded by that metric's own required filters
        // so none of them restricts another's column. The single shared WHERE carries only the
        // range and the question's own filters.
        let (measures, bindings) = guards_and_shared(resolution, &requested, own_table, &time_column)?;
        QueryPlan::with_measures(
            model.source().clone(),
            measures,
            tables,
            bucket,
            keys,
            bindings,
            resolution.range,
        )
    };

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

    filters.extend(range_filters(resolution.range, time_column, &mut params));

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

/// Multi-metric: one [`PlannedMeasure`] per named metric (each guarded by that metric's OWN
/// required filters) plus the single shared `WHERE` bindings, as one union.
///
/// **The parameter order is the statement's text order, guards first.** A metric's guard renders
/// inside its measure column, which appears in the `SELECT` before any `WHERE` predicate, so on a
/// positional dialect its placeholders must come before the range and requested ones. Hence every
/// metric's guard parameters are pushed first, and `predicates_and_params`' shared half (range +
/// requested, no metric's required filters) is appended after, producing the exact list a renderer
/// emits. [`sutura_domain::plan::QueryPlan::with_measures`] splits this union back into the shared
/// tail it stores as `filters` and keeps the guards on their own measures.
///
/// Returns a plain `Vec` rather than a [`NonEmpty`], because the caller constructs that when it has
/// the whole list in hand; that construction happens only in the multi-metric branch, so it never
/// fails.
type MeasuresAndBindings = (NonEmpty<PlannedMeasure>, PlanBindings);

fn guards_and_shared(
    resolution: &Resolution<'_>,
    requested: &[&ResolvedFilter<'_>],
    own_table: &TableName,
    time_column: &PlanColumn,
) -> Result<MeasuresAndBindings, PlanError> {
    let mut measures: Vec<PlannedMeasure> = Vec::with_capacity(resolution.metrics.len());
    let mut params: Vec<ParamValue> = Vec::new();
    // The whole statement's union of WHERE-or-guard filters. Guards (the measure columns) come
    // first, in the order the metrics were asked; range then requested follow as the shared tail.
    let mut all_filters: Vec<PlanFilter> = Vec::new();

    for metric in &resolution.metrics {
        // `plan()` has already refused any authored-SQL metric (over every named one), so every
        // measure here resolves; the `ok_or_else` arm is the defensive spelling of a fact the caller
        // established, kept as an error rather than a panic because a catalog file must not be able
        // to reach one.
        let measure = metric
            .measure()
            .ok_or_else(|| PlanError::AuthoredSqlNotPlanned {
                metric: metric.name().clone(),
            })
            .map(|measure| plan_measure(measure, |column| PlanColumn::new(own_table.clone(), column.clone())))?;
        // This metric's own required filters, kept both as the guard the measure renders against
        // and as the filter head the union validates.
        let mut guard: Vec<PlanPredicate> = Vec::new();
        for required in metric.required_filters() {
            let column = PlanColumn::new(own_table.clone(), required.column().clone());
            let predicate = plan_required_filter(required, column, |value| {
                params.push(ParamValue::Text(value));
                params.len().saturating_sub(1)
            });
            guard.push(predicate.clone());
            all_filters.push(PlanFilter::new(PredicateOrigin::Definition, predicate));
        }
        measures.push(PlannedMeasure::new(
            metric.name().clone(),
            ResultLabel::measure(metric.name()),
            measure,
            guard,
        ));
    }

    // The shared tail: the two range bounds, then the question's own filters. None of the metrics'
    // required filters appear here - each is already a guard above, and putting it in the WHERE
    // would let it prune a row a sibling metric's column must count.
    all_filters.extend(range_filters(resolution.range, time_column, &mut params));
    for filter in requested {
        let column = column_of(&filter.dimension, own_table);
        let predicate = requested_predicate(filter, column, &mut params);
        all_filters.push(PlanFilter::new(PredicateOrigin::Requested, predicate));
    }

    // `resolution.metrics` is what `Query.metrics()` (a `MetricNames`, i.e. a `NonEmpty`) resolved
    // to, so it is never empty and `measures` - one entry per metric, built above - is never empty
    // either. `remove(0)` moves the head out to seed the `NonEmpty` without cloning and, on the
    // empty case that cannot happen here, would panic rather than silently mis-build.
    let head = measures.remove(0);
    Ok((NonEmpty::of(head, measures), PlanBindings::parse(all_filters, params)?))
}
/// The range's two bounds on `time_column`, each binding the next parameter - the head of every
/// fact leg's predicates, and the whole of a second fact leg's (`telekom/sutura#780`).
fn range_filters(range: TimeRange, time_column: &PlanColumn, params: &mut Vec<ParamValue>) -> [PlanFilter; 2] {
    let start = params.len();
    params.push(ParamValue::Date(range.start()));
    params.push(ParamValue::Date(range.end()));
    [
        PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::AtOrAfter {
                column: time_column.clone(),
                param: start,
            },
        ),
        PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::Before {
                column: time_column.clone(),
                param: start.saturating_add(1),
            },
        ),
    ]
}

/// A second fact leg's bindings: the range on the shared calendar and nothing else - the metric's
/// required filters constrain its own model's columns, which the second fact's statement does not read.
fn range_only(range: TimeRange, time_column: &PlanColumn) -> Result<PlanBindings, IncoherentBindings> {
    let mut params = Vec::with_capacity(2);
    let filters = range_filters(range, time_column, &mut params);
    PlanBindings::parse(filters.into(), params)
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
        let column = column_of(&filter.dimension, remote_table);
        let predicate = requested_predicate(filter, column, &mut params);
        filters.push(PlanFilter::new(PredicateOrigin::Requested, predicate));
    }
    PlanBindings::parse(filters, params)
}
/// The three properties of a chain a plan is a function of: which table qualifies hop N's origin
/// column, that a chain which crosses data systems is planned as lookup legs, and that the join
/// order and the legs do not depend on the order the caller listed their dimensions.
///
/// Inline rather than a `plan/tests.rs`, and the reason is the causality gate rather than taste: it
/// reconstructs a base tree by reverting every changed file that added no test, so a `mod tests;`
/// line in a reverted `plan.rs` would orphan the file it declares and the base run would have no
/// test to be red.
///
/// A [`Resolution`] is built by hand here, because a question has no field that names a join and a
/// chain placed on several data systems is cheaper to state than to assemble. The end-to-end evidence for the first property is the two-hop dimension in
/// `examples/single-player`, which every rendering golden and the executed corpus read.
#[cfg(test)]
mod tests;

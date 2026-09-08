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
//! into the fact and lookup [`LegPlan`]s the combiner in `sutura_domain::plan::federated` joins, and
//! refuses a measure that cannot decompose. The labelling of each leg's terms is `sutura_domain`'s
//! one function, [`labels`](sutura_domain::plan::labels) - named by the splitter and read back by the
//! combiner, so the two can never disagree.

use std::collections::BTreeSet;

use sutura_domain::federation::{Carried, Federation};
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::plan::leg::LegTerm;
use sutura_domain::plan::{
    FederatedPlan, FederatedPlanError, InternalLabel, PlanBucket, PlanColumn, PlanFilter, PlanJoin, PlanKey, PlanPredicate,
    PlanTerm, PredicateOrigin, QueryPlan, ResultLabel, StatementTables, labels, plan_measure, plan_required_filter,
};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::ParamValue;

use crate::resolve::{Resolution, ResolvedDimension, ResolvedFilter};

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
#[derive(Debug, thiserror::Error)]
pub(crate) enum PlanError {
    #[error("the question was refused")]
    Refused(RefusalReason),
    #[error(transparent)]
    NotAssembled(#[from] FederatedPlanError),
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
pub(crate) fn plan(resolution: &Resolution<'_>) -> Result<Plan, PlanError> {
    // Every source besides the metric's own that a join reaches. A `RemoteDimension` that this
    // iterator yields has a join by construction (`is_remote` requires one), so the filter cannot
    // drop a source here.
    let remote: BTreeSet<&SourceName> = every_remote_dimension(resolution)
        .filter_map(|dim| dim.join.as_ref().map(|join| join.model.source()))
        .collect();

    match remote.len() {
        0 => Ok(Plan::Mono(Box::new(mono_plan(resolution)?))),
        1 => Ok(Plan::Federated(Box::new(federated_plan(resolution)?))),
        // Two are served; three or more refused, because each source is a separate identity.
        _ => Err(PlanError::Refused(RefusalReason::PlanSpansTooManySources {
            sources: 1 + remote.len(),
            limit: 2,
        })),
    }
}

/// Every dimension that sits on a data system other than the metric's own.
fn every_remote_dimension<'a, 'r>(resolution: &'r Resolution<'a>) -> impl Iterator<Item = &'r ResolvedDimension<'a>> {
    let own = resolution.model.source();
    every_dimension(resolution).filter(move |dim| is_remote(dim, own))
}

/// Whether a dimension sits on a data system other than `own`.
fn is_remote(dimension: &ResolvedDimension<'_>, own: &SourceName) -> bool {
    dimension.join.as_ref().is_some_and(|join| join.model.source() != own)
}

/// A question confined to one data system: exactly the plan this module already built.
fn mono_plan(resolution: &Resolution<'_>) -> Result<QueryPlan, RefusalReason> {
    let metric = resolution.metric;
    let model = resolution.model;
    // Two readings of "the table", and both are used below. `own_path` is what the `FROM` names -
    // dataset and project included, where the model declares them. `own_table` is the bare name, which
    // is what every column is qualified by: `FROM a.b.c` gives the reference an implicit alias of `c`
    // in all four dialects rendered for, so a `PlanColumn` holds `c` and never the path.
    let own_path = model.table();
    let own_table = model.table_name();

    // Deduplicated by relationship and then sorted, so two dimensions reached through one
    // relationship produce one join, and the join order is a function of the plan rather than of the
    // order the caller happened to list their dimensions in. A statement whose text depends on
    // argument order has no stable golden.
    let mut joins: Vec<PlanJoin> = Vec::new();
    for resolved in every_dimension(resolution) {
        if let Some(ref join) = resolved.join {
            let name = join.relationship.name().clone();
            if joins.iter().any(|existing| *existing.relationship() == name) {
                continue;
            }
            joins.push(PlanJoin::new(
                name,
                // The joined model's own path: a dimension table in another dataset is still one
                // statement. Its columns are qualified by the bare name beside it, for the reason
                // `own_table` above gives.
                join.model.table().clone(),
                join.relationship.join_type(),
                PlanColumn::new(own_table.clone(), join.relationship.origin_column().clone()),
                PlanColumn::new(join.model.table_name().clone(), join.relationship.target_column().clone()),
            ));
        }
    }
    joins.sort_by(|a, b| a.relationship().cmp(b.relationship()));

    let time_column = PlanColumn::new(own_table.clone(), metric.time_column().clone());

    let requested: Vec<&ResolvedFilter> = resolution.filters.iter().collect();
    let (filters, params) = predicates_and_params(resolution, &requested, own_table, &time_column);

    let keys: Vec<PlanKey> = resolution
        .keys
        .iter()
        .map(|key| PlanKey::new(ResultLabel::dimension(key.dimension.name()), column_of(key, own_table)))
        .collect();

    let measure = plan_measure(metric.measure(), |column| PlanColumn::new(own_table.clone(), column.clone()));

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

    Ok(QueryPlan::new(
        model.source().clone(),
        metric.name().clone(),
        tables,
        PlanBucket::new(ResultLabel::bucket(), resolution.grain, time_column),
        keys,
        measure,
        ResultLabel::measure(metric.name()),
        filters,
        params,
        resolution.range,
    ))
}

/// Splits a two-source question into a fact leg and a lookup leg.
///
/// The metric's own rows (and any same-source dimension) form the fact leg; everything on the one
/// remote data system forms the lookup leg. The link between them is the relationship's join column,
/// grouped into the fact leg and projected from the lookup leg under the same label - so the combiner
/// can find it. A measure that cannot decompose is refused rather than pulled up.
// The splitter builds both legs, their keys, their filters and the link in one pass over the
// resolution; it is a single act of splitting a resolved question, and it returns Err from several
// places that far apart to make a reviewer see the splitter's refusals together.
fn federated_plan(resolution: &Resolution<'_>) -> Result<FederatedPlan, PlanError> {
    let metric = resolution.metric;
    let model = resolution.model;
    // Two readings of "the table", for the reason `mono_plan` gives: the path is what a leg's `FROM`
    // names, the bare name is what every column in that leg is qualified by.
    let own_path = model.table();
    let own_table = model.table_name();

    let federation = Federation::of(metric.measure());
    // The combiner cannot re-count a distinct aggregate, so a measure that needs that is refused.
    if let Some(keys) = federation.carried().iter().find_map(|leaf| match **leaf {
        Carried::Keys { pulled, .. } => Some(pulled.above()),
        _ => None,
    }) {
        return Err(PlanError::Refused(RefusalReason::MeasureDoesNotFederate {
            metric: metric.name().clone(),
            aggregate: keys,
        }));
    }

    // One remote data system (more are refused upstream); its dimensions must all join through one
    // relationship, because the combiner links the legs on a single column. The two guards below are
    // unreachable - `plan` calls this only for exactly one remote source, and a remote dimension has
    // a join by construction - but a panic here would be reachable from a catalog plus a question, so
    // they refuse instead.
    let Some(first_remote) = every_remote_dimension(resolution).next() else {
        return Err(RefusalReason::PlanSpansTooManySources { sources: 1, limit: 2 }.into());
    };
    let Some(first_join) = first_remote.join.as_ref() else {
        return Err(RefusalReason::PlanSpansTooManySources { sources: 1, limit: 2 }.into());
    };
    let relationship = first_join.relationship;
    let remote_path = first_join.model.table();
    let remote_table = first_join.model.table_name();
    let remote_source = first_join.model.source();
    for dim in every_remote_dimension(resolution) {
        let Some(join) = dim.join.as_ref() else {
            continue;
        };
        if join.relationship.name() != relationship.name() {
            return Err(PlanError::Refused(RefusalReason::FederationLinkAmbiguous {
                source: remote_source.clone(),
            }));
        }
    }

    // **The link's label is reserved, and it used to be the physical join column's text.** That text
    // is a legal dimension name, and it sat in the same result namespace as the public dimension
    // labels beside it - so a metric with a legal dimension named `customer_key`, backed by a
    // different column, produced two fact columns under one label and the combiner refused the
    // answer. `InternalLabel` is a namespace a question cannot spell into; the dimension stays legal.
    let link_label = ResultLabel::internal(InternalLabel::Link);

    // The fact leg groups by its local dimension keys plus the join origin, so the lookup leg can be
    // joined to it above.
    let mut fact_keys: Vec<PlanKey> = Vec::new();
    for key in resolution.keys.iter().filter(|key| !is_remote(key, model.source())) {
        fact_keys.push(PlanKey::new(
            ResultLabel::dimension(key.dimension.name()),
            column_of(key, own_table),
        ));
    }
    fact_keys.push(PlanKey::new(
        link_label.clone(),
        PlanColumn::new(own_table.clone(), relationship.origin_column().clone()),
    ));

    // The lookup leg projects the join target plus the remote dimension keys.
    let mut lookup_keys: Vec<PlanKey> = Vec::new();
    lookup_keys.push(PlanKey::new(
        link_label,
        PlanColumn::new(remote_table.clone(), relationship.target_column().clone()),
    ));
    for key in resolution.keys.iter().filter(|key| is_remote(key, model.source())) {
        lookup_keys.push(PlanKey::new(
            ResultLabel::dimension(key.dimension.name()),
            PlanColumn::new(remote_table.clone(), key.dimension.column().clone()),
        ));
    }

    // Filters split by which leg owns the column they constrain.
    let local_filters: Vec<&ResolvedFilter> = resolution
        .filters
        .iter()
        .filter(|filter| !is_remote(&filter.dimension, model.source()))
        .collect();
    let remote_filters: Vec<&ResolvedFilter> = resolution
        .filters
        .iter()
        .filter(|filter| is_remote(&filter.dimension, model.source()))
        .collect();

    let time_column = PlanColumn::new(own_table.clone(), metric.time_column().clone());
    let (fact_filters, fact_params) = predicates_and_params(resolution, &local_filters, own_table, &time_column);
    let (lookup_filters, lookup_params) = requested_for(&remote_filters, remote_table);

    // The fact leg's terms, projected under the one labelling rule the combiner reads back - in the
    // same reserved namespace as the link, for the same reason: `metric__{n}` is a legal dimension
    // name too, and over a 63-character metric name it also crossed the identifier limit a data
    // system truncates silently.
    let leaf_labels = labels(&federation);
    let mut terms: Vec<LegTerm> = Vec::with_capacity(leaf_labels.len());
    for (leaf, &label) in federation.carried().iter().zip(leaf_labels.iter()) {
        let plan_term = match **leaf {
            Carried::Aggregated { pushed, ref column } => PlanTerm::Aggregate {
                aggregate: pushed.push(),
                column: PlanColumn::new(own_table.clone(), column.clone()),
            },
            Carried::CountIf { ref column } => PlanTerm::CountIf {
                column: PlanColumn::new(own_table.clone(), column.clone()),
            },
            // Unreachable: the refusal above returned for any Keys leaf.
            Carried::Keys { .. } => {
                return Err(PlanError::Refused(RefusalReason::MeasureDoesNotFederate {
                    metric: metric.name().clone(),
                    aggregate: sutura_domain::model::Aggregate::CountDistinct,
                }));
            }
        };
        terms.push(LegTerm::new(plan_term, ResultLabel::internal(label)));
    }

    // Same-source hops (dimensions on the metric's own system) stay joins on the fact leg.
    let mut joins: Vec<PlanJoin> = Vec::new();
    for resolved in every_dimension(resolution) {
        if let Some(ref join) = resolved.join {
            if join.model.source() != model.source() {
                continue;
            }
            let name = join.relationship.name().clone();
            if joins.iter().any(|existing| *existing.relationship() == name) {
                continue;
            }
            joins.push(PlanJoin::new(
                name,
                // The joined model's own path, for the reason `mono_plan`'s loop gives.
                join.model.table().clone(),
                join.relationship.join_type(),
                PlanColumn::new(own_table.clone(), join.relationship.origin_column().clone()),
                PlanColumn::new(join.model.table_name().clone(), join.relationship.target_column().clone()),
            ));
        }
    }
    joins.sort_by(|a, b| a.relationship().cmp(b.relationship()));

    let bucket = PlanBucket::new(ResultLabel::bucket(), resolution.grain, time_column);

    // **Where the fact leg's tables stop being a list and become a checked set - the same guard the
    // whole-answer path goes through, reached from the other plan shape.** A leg keeps every
    // same-source hop as a `JOIN` of its own, so two paths ending in one name render under one
    // implicit alias inside ONE leg's statement: reproduced, and worse there than on the whole-answer
    // path, because a leg's rows are combined above it and nothing downstream sees the statement.
    // `sutura_domain::plan::tables` holds the measurement and the argument for refusing rather than
    // aliasing; `LegPlan::Fact` takes the checked set as a field, so this call is not something a
    // future leg producer can forget.
    let fact_tables =
        StatementTables::parse(own_path.clone(), joins).map_err(|ambiguous| RefusalReason::PlanTablesShareAnIdentifier {
            table: ambiguous.alias().clone(),
        })?;

    let fact = sutura_domain::plan::LegPlan::Fact {
        source: model.source().clone(),
        metric: metric.name().clone(),
        tables: fact_tables,
        bucket: bucket.clone(),
        keys: fact_keys,
        terms,
        filters: fact_filters,
        params: fact_params,
        range: resolution.range,
    };

    let lookup = sutura_domain::plan::LegPlan::Lookup {
        source: remote_source.clone(),
        table: remote_path.clone(),
        keys: lookup_keys,
        filters: lookup_filters,
        params: lookup_params,
    };

    // The answer's group-by keys in question order, each naming which leg's result it is read from.
    // Question order is the mono path's column order too, so a federated answer aligns with a
    // single-source one (and with a future federated differential that reads rows by position).
    let answer_keys: Vec<sutura_domain::plan::AnswerKey> = resolution
        .keys
        .iter()
        .map(|key| {
            let label = ResultLabel::dimension(key.dimension.name());
            if is_remote(key, model.source()) {
                sutura_domain::plan::AnswerKey::lookup(label)
            } else {
                sutura_domain::plan::AnswerKey::fact(label)
            }
        })
        .collect();

    FederatedPlan::new(
        metric.name().clone(),
        ResultLabel::measure(metric.name()),
        bucket,
        fact,
        lookup,
        // LEFT when the lookup carries no filter (an unmatched fact row survives), INNER when it
        // does. `docs/adr/0009` decides the direction.
        remote_filters.is_empty(),
        federation,
        answer_keys,
    )
    // **The cause is no longer erased, and this is the whole of `telekom/sutura#338`.** It used to
    // become `RefusalReason::FederationNotExecutable`, which is ALSO what a build whose adapter does
    // not declare `EXECUTES_LEGS` gets - so a plan this workspace could not assemble read exactly
    // like the deployment simply not being able to execute a leg. It leaves as an error now, keeping
    // the typed cause, because a defect in our own wiring is not a governance answer and a caller
    // must not be handed one it could retry. `PlanError` carries what a reader needs, including
    // which variants are reachable from here today, and that is none of them.
    .map_err(PlanError::NotAssembled)
}

/// The predicates a statement carries, paired with the parameters they bind.
///
/// A named alias because the tuple is over the complexity threshold in `clippy.toml`, and naming it
/// says what the pairing means: neither half is usable without the other.
type PredicatesAndParams = (Vec<PlanFilter>, Vec<ParamValue>);

/// Every predicate a fact leg will carry, and the parameters they bind, built together.
///
/// The range bounds, then the metric's required filters, then `requested` - the caller's own filters
/// that constrain this leg's columns. `requested` is passed in (rather than read off the resolution)
/// so the splitter can hand the local half here and the remote half to the lookup leg.
fn predicates_and_params(
    resolution: &Resolution<'_>,
    requested: &[&ResolvedFilter<'_>],
    own_table: &TableName,
    time_column: &PlanColumn,
) -> PredicatesAndParams {
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
        let param = bind(&mut params, ParamValue::Text(filter.value.clone()));
        filters.push(PlanFilter::new(
            PredicateOrigin::Requested,
            PlanPredicate::Equals {
                column: column_of(&filter.dimension, own_table),
                param,
            },
        ));
    }

    (filters, params)
}

/// The predicates a lookup leg carries: only the caller's own remote filters, bound on the remote
/// table.
fn requested_for(requested: &[&ResolvedFilter<'_>], remote_table: &TableName) -> PredicatesAndParams {
    let mut params: Vec<ParamValue> = Vec::new();
    let mut filters: Vec<PlanFilter> = Vec::new();
    for filter in requested {
        let bind = params.len();
        params.push(ParamValue::Text(filter.value.clone()));
        filters.push(PlanFilter::new(
            PredicateOrigin::Requested,
            PlanPredicate::Equals {
                column: PlanColumn::new(remote_table.clone(), filter.dimension.dimension.column().clone()),
                param: bind,
            },
        ));
    }
    (filters, params)
}

/// Every dimension the question mentions, grouped by or filtered on.
///
/// One iterator, so the source check and the join collection walk the same set. They read it for
/// different reasons, and a dimension missing from one of them would be a join that is planned and
/// not counted, or counted and not planned.
fn every_dimension<'a, 'r>(resolution: &'r Resolution<'a>) -> impl Iterator<Item = &'r ResolvedDimension<'a>> {
    resolution
        .keys
        .iter()
        .chain(resolution.filters.iter().map(|filter| &filter.dimension))
}

/// Which table a dimension's column is read from: the joined one when there is a join, the metric's
/// own otherwise.
fn column_of(resolved: &ResolvedDimension<'_>, own_table: &TableName) -> PlanColumn {
    let table = resolved
        .join
        .as_ref()
        .map_or_else(|| own_table.clone(), |join| join.model.table_name().clone());
    PlanColumn::new(table, resolved.dimension.column().clone())
}

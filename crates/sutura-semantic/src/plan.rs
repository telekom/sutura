//! Plan: the resolved question becomes a [`QueryPlan`], and three things are settled here and
//! nowhere else.
//!
//! **The plan names exactly one data system.** A question whose join would reach a second one is
//! refused before anything runs, because a second data system is a second identity to satisfy.
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
//! The plan type itself lives in `sutura-domain`, because the execution port speaks it. This module
//! only decides what goes into one.

use std::collections::BTreeSet;

use sutura_domain::catalog::TIME_BUCKET_LABEL;
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::plan::{
    PlanBucket, PlanColumn, PlanFilter, PlanJoin, PlanKey, PlanPredicate, PredicateOrigin, QueryPlan, StatementTables,
    plan_measure, plan_required_filter,
};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::ParamValue;

use crate::resolve::{Resolution, ResolvedDimension};

/// Turns a resolution into a plan, or refuses it.
///
/// **Two refusals are produced here and nowhere else, and both are about the SHAPE of the statement
/// rather than about anything a caller wrote:** a plan that would read from two data systems, and a
/// plan whose tables could not be told apart inside one statement. Everything a caller could have got
/// wrong was already checked when their names were looked up.
pub(crate) fn plan(resolution: &Resolution<'_>) -> Result<QueryPlan, RefusalReason> {
    let metric = resolution.metric;
    let model = resolution.model;
    // Two readings of "the table", and both are used below. `own_path` is what the `FROM` names -
    // dataset and project included, where the model declares them. `own_table` is the bare name, which
    // is what every column is qualified by: `FROM a.b.c` gives the reference an implicit alias of `c`
    // in all four dialects rendered for, so a `PlanColumn` holds `c` and never the path.
    let own_path = model.table();
    let own_table = model.table_name();

    // Every table the statement will read, which is the definition of "one data system" a join
    // cannot get around.
    //
    // **A source is a credential plus a billing project, not a project - so two datasets or two
    // projects reached by ONE credential in one statement is ONE source, and this refusal must not
    // fire on it.** That is why what is collected here is `SourceName` and never any part of a table
    // path: cross-project is not federation. `BigQuery` joins across projects natively, in one job, and
    // pushes the join down; routing that through a splitter and a client-side combiner would replace a
    // pushed-down join with a slower one and discard exactly the pushdown that makes the adapter worth
    // having. Two SOURCES is a second credential, and that is the case this refuses.
    // `sutura_domain::model::qualified`'s header carries the same boundary from the naming side.
    let mut sources: BTreeSet<&SourceName> = BTreeSet::new();
    sources.insert(model.source());
    for resolved in every_dimension(resolution) {
        if let Some(ref join) = resolved.join {
            sources.insert(join.model.source());
        }
    }
    if sources.len() != 1 {
        return Err(RefusalReason::PlanSpansTwoSources { sources: sources.len() });
    }

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

    // Predicates and their parameters, built together: see `predicates_and_params`.
    let (filters, params) = predicates_and_params(resolution, own_table, &time_column);

    let keys: Vec<PlanKey> = resolution
        .keys
        .iter()
        .map(|key| PlanKey::new(String::from(key.dimension.name().as_str()), column_of(key, own_table)))
        .collect();

    // Every column a measure reads is on the metric's own model. A measure may not reach through a
    // relationship, because a joined measure is the row-duplication problem the catalog already
    // refuses a dimension for - so resolving them needs no lookup.
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
        PlanBucket::new(String::from(TIME_BUCKET_LABEL), resolution.grain, time_column),
        keys,
        measure,
        String::from(metric.name().as_str()),
        filters,
        params,
        resolution.range,
    ))
}

/// The predicates a statement carries, paired with the parameters they bind.
///
/// A named alias because the tuple is over the complexity threshold in `clippy.toml`, and naming it
/// says what the pairing means: neither half is usable without the other.
type PredicatesAndParams = (Vec<PlanFilter>, Vec<ParamValue>);

/// Every predicate the statement will carry, and the parameters they bind, built together.
///
/// One function because the two lists are one decision: for a dialect that writes `?`, a
/// parameter's position in the statement is its identity, so a predicate appended without its
/// value - or in a different order - is a statement that reads the wrong parameter and still runs.
/// Returning them as a pair is what makes it impossible to build one without the other.
///
/// Order: the two range bounds, then the metric's required filters, then the caller's. The
/// definitional ones come first so that reading a generated statement shows what the metric always
/// does before what this caller asked for.
fn predicates_and_params(resolution: &Resolution<'_>, own_table: &TableName, time_column: &PlanColumn) -> PredicatesAndParams {
    let metric = resolution.metric;
    let mut params: Vec<ParamValue> = Vec::new();
    let mut filters: Vec<PlanFilter> = Vec::new();

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

    for filter in &resolution.filters {
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

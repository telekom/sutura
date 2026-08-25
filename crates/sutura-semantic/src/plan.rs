//! Plan: the resolved question becomes something owned, and two things are settled here and
//! nowhere else.
//!
//! **The plan names exactly one data system.** A question whose join would reach a second one is
//! refused before anything runs, because a second data system is a second identity to satisfy, and a
//! plan that runs partly as somebody else is the failure this design exists to prevent.
//!
//! **Every value from the question becomes a bind parameter.** They are collected here, in the order
//! the generated statement will refer to them, so no caller-supplied value reaches the generator as
//! text. The generator has no access to the question at all.
//!
//! A plan holds no SQL. Its public contract is its serialized form: it is the artifact a golden
//! snapshot pins, so a change to what we plan shows up as a reviewable diff rather than as a
//! different number.

use std::collections::BTreeSet;

use sutura_domain::model::{Aggregate, ColumnName, Grain, JoinType, MetricName, RelationshipName, SourceName, TableName};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::ParamValue;

use crate::resolve::{Resolution, ResolvedDimension};

/// The label the truncated time column is projected under.
///
/// Re-exported from the domain rather than defined here, because it is part of the result schema and
/// `Definitions::assemble` has to refuse a dimension by this name for the same reason: two columns
/// with one label is a result a caller cannot read.
pub use sutura_domain::catalog::TIME_BUCKET_LABEL;

/// The most rows any generated statement may return.
///
/// A hard cap rather than a budget, for now. It exists because a bounded range and a bounded set of
/// group-by keys still permit a large result, and the cost of that lands on a shared data system.
/// When there is a real budget this becomes its floor.
pub const MAX_ROWS: u32 = 10_000;

/// A column, qualified by the table it is read from.
///
/// Qualified always, even when there is only one table. An unqualified column in a statement that
/// later grows a join silently binds to whichever table has it, and that is a wrong number rather
/// than an error.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PlanColumn {
    pub(crate) table: TableName,
    pub(crate) column: ColumnName,
}

/// One join, as the statement will make it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PlanJoin {
    pub(crate) relationship: RelationshipName,
    pub(crate) table: TableName,
    pub(crate) join_type: JoinType,
    pub(crate) origin: PlanColumn,
    pub(crate) target: PlanColumn,
}

/// What is measured, and under what label.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PlanMeasure {
    pub(crate) label: String,
    pub(crate) aggregate: Aggregate,
    pub(crate) column: PlanColumn,
}

/// The truncated time column.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PlanBucket {
    pub(crate) label: String,
    pub(crate) grain: Grain,
    pub(crate) column: PlanColumn,
}

/// One group-by key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PlanKey {
    pub(crate) label: String,
    pub(crate) column: PlanColumn,
}

/// One equality predicate, and which parameter carries its value.
///
/// The index is recorded rather than implied by position, so a reader of a plan can see which
/// parameter goes where without reconstructing the generator's ordering in their head.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PlanFilter {
    pub(crate) column: PlanColumn,
    pub(crate) param: usize,
}

/// One statement's worth of decisions, and no SQL.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Plan {
    source: SourceName,
    metric: MetricName,
    table: TableName,
    joins: Vec<PlanJoin>,
    bucket: PlanBucket,
    keys: Vec<PlanKey>,
    measure: PlanMeasure,
    time_column: PlanColumn,
    start_param: usize,
    end_param: usize,
    filters: Vec<PlanFilter>,
    params: Vec<ParamValue>,
    row_limit: u32,
}

impl Plan {
    pub(crate) const fn source(&self) -> &SourceName {
        &self.source
    }

    pub(crate) const fn table(&self) -> &TableName {
        &self.table
    }

    pub(crate) fn joins(&self) -> &[PlanJoin] {
        &self.joins
    }

    pub(crate) const fn bucket(&self) -> &PlanBucket {
        &self.bucket
    }

    pub(crate) fn keys(&self) -> &[PlanKey] {
        &self.keys
    }

    pub(crate) const fn measure(&self) -> &PlanMeasure {
        &self.measure
    }

    pub(crate) const fn time_column(&self) -> &PlanColumn {
        &self.time_column
    }

    pub(crate) fn filters(&self) -> &[PlanFilter] {
        &self.filters
    }

    pub(crate) fn params(&self) -> &[ParamValue] {
        &self.params
    }

    pub(crate) const fn row_limit(&self) -> u32 {
        self.row_limit
    }

    /// The metric this plan answers about. Public because a caller that gets a plan back wants to
    /// know what it is a plan for without deserializing it.
    #[inline]
    pub const fn metric(&self) -> &MetricName {
        &self.metric
    }
}

/// Turns a resolution into a plan, or refuses it.
///
/// The only refusal this stage can produce is the one about data systems, because everything a
/// caller could have got wrong was already checked when their names were looked up.
pub(crate) fn plan(resolution: &Resolution<'_>) -> Result<Plan, RefusalReason> {
    let metric = resolution.metric;
    let model = resolution.model;

    // Collected as a set over every table the statement will read, which is the definition of "one
    // data system" that a join cannot get around.
    let mut sources: BTreeSet<&SourceName> = BTreeSet::new();
    sources.insert(model.source());
    for key in resolution
        .keys
        .iter()
        .chain(resolution.filters.iter().map(|filter| &filter.dimension))
    {
        if let Some(ref join) = key.join {
            sources.insert(join.model.source());
        }
    }
    if sources.len() != 1 {
        return Err(RefusalReason::PlanSpansTwoSources { sources: sources.len() });
    }

    // Sorted and deduplicated by relationship name, so two dimensions reached through the same
    // relationship produce one join and the join order is a function of the plan rather than of the
    // order the caller happened to list their dimensions in. A statement whose text depends on
    // argument order has no stable golden.
    let mut joins: Vec<PlanJoin> = Vec::new();
    for resolved in resolution
        .keys
        .iter()
        .chain(resolution.filters.iter().map(|filter| &filter.dimension))
    {
        if let Some(ref join) = resolved.join {
            let name = join.relationship.name().clone();
            if joins.iter().any(|existing| existing.relationship == name) {
                continue;
            }
            joins.push(PlanJoin {
                relationship: name,
                table: join.model.table().clone(),
                join_type: join.relationship.join_type(),
                origin: PlanColumn {
                    table: model.table().clone(),
                    column: join.relationship.origin_column().clone(),
                },
                target: PlanColumn {
                    table: join.model.table().clone(),
                    column: join.relationship.target_column().clone(),
                },
            });
        }
    }
    joins.sort_by(|a, b| a.relationship.cmp(&b.relationship));

    let time_column = PlanColumn {
        table: model.table().clone(),
        column: metric.time_column().clone(),
    };

    // Parameter order is the order the statement refers to them in: the two bounds, then one per
    // filter. Recorded as indices so the generator does not have to agree with this by convention.
    let mut params = vec![
        ParamValue::Date(resolution.range.start()),
        ParamValue::Date(resolution.range.end()),
    ];
    let mut filters = Vec::with_capacity(resolution.filters.len());
    for filter in &resolution.filters {
        params.push(ParamValue::Text(filter.value.clone()));
        filters.push(PlanFilter {
            column: column_of(&filter.dimension, model.table()),
            param: params.len().saturating_sub(1),
        });
    }

    Ok(Plan {
        source: model.source().clone(),
        metric: metric.name().clone(),
        table: model.table().clone(),
        joins,
        bucket: PlanBucket {
            label: String::from(TIME_BUCKET_LABEL),
            grain: resolution.grain,
            column: time_column.clone(),
        },
        keys: resolution
            .keys
            .iter()
            .map(|key| PlanKey {
                label: String::from(key.dimension.name().as_str()),
                column: column_of(key, model.table()),
            })
            .collect(),
        measure: PlanMeasure {
            label: String::from(metric.name().as_str()),
            aggregate: metric.measure().aggregate(),
            column: PlanColumn {
                table: model.table().clone(),
                column: metric.measure().column().clone(),
            },
        },
        time_column,
        start_param: 0,
        end_param: 1,
        filters,
        params,
        row_limit: MAX_ROWS,
    })
}

/// Which table a dimension's column is read from: the joined one when there is a join, and the
/// metric's own otherwise.
fn column_of(resolved: &ResolvedDimension<'_>, own_table: &TableName) -> PlanColumn {
    let table = resolved
        .join
        .as_ref()
        .map_or_else(|| own_table.clone(), |join| join.model.table().clone());
    PlanColumn {
        table,
        column: resolved.dimension.column().clone(),
    }
}

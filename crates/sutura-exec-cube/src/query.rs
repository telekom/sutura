//! A [`QueryPlan`] as a Cube JSON query: which members it names, and what each filter becomes.
//!
//! | In the plan                    | In the Cube query                                           |
//! | ------------------------------ | ----------------------------------------------------------- |
//! | a metric                       | the measure `<table>.<metric>`                              |
//! | a key                          | the dimension `<table>.<column>`                            |
//! | the bucket                     | a time dimension with the grain as its `granularity`        |
//! | `>=` / `<` a date              | `afterOrOnDate` / `beforeDate`                              |
//! | `=` / `IN`                     | `equals` with one or more values                            |
//! | `<>` / `NOT IN`                | `notEquals`, and `set` - Cube's `notEquals` keeps a `NULL`  |
//! | `IS TRUE` / `IS NOT NULL`      | `equals` `true` / `set`                                     |
//! | `top`                          | `order` and `limit`                                         |
//!
//! Every value is a JSON string in `values`, never query text. A required filter of the metric is
//! sent like every other filter, so Cube's own measure filter is never the only one. A plan of
//! several metrics carries a required filter as one metric's guard, which one Cube query cannot
//! scope to that metric alone, so [`CubeQuery`] splits such a plan rather than drop the filter.

use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::Aggregate;
use sutura_domain::plan::{PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, QueryPlan};
use sutura_domain::query::{TopBy, TopDirection};
use sutura_domain::warehouse::ParamValue;

/// Why a plan cannot be asked of Cube as one query.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotExpressible {
    #[error("the plan binds a parameter it does not carry")]
    UnboundParameter,
    #[error(
        "a top over several metrics, one of them under its own required filter, ranks the joined answer, \
         which no one Cube query holds"
    )]
    TopAcrossMetricQueries,
}

/// What one column of the answer is, for the decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Read {
    Key,
    Bucket,
    /// A measure: `real` when its value is never an integer, `fails` when it names a metric whose
    /// zero denominator fails rather than answering no value, and `count` when no row under its
    /// guard is a zero rather than no value - what the plan's own SQL answers for that group.
    Measure {
        real: bool,
        fails: Option<String>,
        count: bool,
    },
}

/// One column of the answer: its label in sutura, the key it has in Cube's answer, and how to read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Column {
    pub(crate) label: String,
    pub(crate) key: String,
    pub(crate) read: Read,
}

/// A plan as Cube queries, and the columns their answers come back as.
///
/// One query when no metric carries a guard. Otherwise the plan's own SQL would fold each guard
/// into that metric's aggregate, so the plan becomes one query for the groups - the dimensions and
/// the bucket under the shared filters, no measure - and one query per metric under its own guard
/// and the shared filters. [`crate::rows`] joins them back on the groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CubeQuery {
    queries: Vec<serde_json::Value>,
    split: bool,
    members: Vec<String>,
    columns: Vec<Column>,
}

impl CubeQuery {
    /// The queries for `plan`, each asking for at most `limit` rows unless the plan's `top` sets fewer.
    ///
    /// # Errors
    ///
    /// The plan is one [`NotExpressible`] names.
    pub fn of(plan: &QueryPlan, limit: u32) -> Result<Self, NotExpressible> {
        let mut members = Vec::new();
        let mut columns = Vec::new();
        let mut dimensions = Vec::new();
        for key in plan.keys() {
            let name = member(key.column());
            dimensions.push(serde_json::Value::String(name.clone()));
            columns.push(Column {
                label: String::from(key.label()),
                key: name.clone(),
                read: Read::Key,
            });
            members.push(name);
        }
        let bucket = plan.bucket();
        let time = member(bucket.column());
        let grain = bucket.grain().as_str();
        columns.push(Column {
            label: String::from(bucket.label()),
            key: format!("{time}.{grain}"),
            read: Read::Bucket,
        });
        members.push(time.clone());
        let mut measures = Vec::new();
        let mut guards = Vec::new();
        for planned in plan.measures().iter() {
            let name = format!("{}.{}", plan.table_name().as_str(), planned.metric().as_str());
            measures.push(name.clone());
            columns.push(Column {
                label: String::from(planned.label()),
                key: name.clone(),
                read: measure_read(planned.measure(), planned.metric().as_str()),
            });
            members.push(name);
            guards.push(filters_of(planned.guard().iter(), plan.params(), &mut members)?);
        }
        let shared = filters_of(plan.filters().iter().map(PlanFilter::predicate), plan.params(), &mut members)?;
        let split = guards.iter().any(|guard| !guard.is_empty());
        if split && plan.top().is_some() {
            return Err(NotExpressible::TopAcrossMetricQueries);
        }
        let mut order: Vec<serde_json::Value> = Vec::new();
        let mut rows = limit;
        if let Some(top) = plan.top() {
            let first = match top.by() {
                TopBy::Metric => measures.first().cloned(),
                TopBy::Period => Some(time.clone()),
            };
            let direction = match top.direction() {
                TopDirection::Desc => "desc",
                TopDirection::Asc => "asc",
            };
            order.extend(first.map(|first| serde_json::json!([first, direction])));
            rows = rows.min(top.n().get());
        }
        order.extend(plan.keys().iter().map(|key| serde_json::json!([member(key.column()), "asc"])));
        order.push(serde_json::json!([time, "asc"]));
        let query = |measures: &[String], filters: Vec<serde_json::Value>| {
            serde_json::json!({
                "measures": measures,
                "dimensions": dimensions,
                "timeDimensions": [{ "dimension": time, "granularity": grain }],
                "filters": filters,
                "order": order,
                "limit": rows,
            })
        };
        let queries = if split {
            let mut queries = vec![query(&[], shared.clone())];
            for (name, guard) in measures.iter().zip(guards) {
                let mut filters = guard;
                filters.extend(shared.iter().cloned());
                queries.push(query(core::slice::from_ref(name), filters));
            }
            queries
        } else {
            vec![query(&measures, shared)]
        };
        members.sort_unstable();
        members.dedup();
        Ok(Self {
            queries,
            split,
            members,
            columns,
        })
    }

    /// The queries, each as `/cubejs-api/v1/load` takes it under `query`. With a guard, the first
    /// asks for the groups and each other one for one metric, in the plan's order.
    #[must_use]
    pub fn queries(&self) -> &[serde_json::Value] {
        &self.queries
    }

    /// Every member the queries name, each once.
    #[must_use]
    pub fn members(&self) -> &[String] {
        &self.members
    }

    pub(crate) const fn split(&self) -> bool {
        self.split
    }

    pub(crate) fn columns(&self) -> &[Column] {
        &self.columns
    }
}

fn filters_of<'plan>(
    predicates: impl Iterator<Item = &'plan PlanPredicate>,
    params: &[ParamValue],
    members: &mut Vec<String>,
) -> Result<Vec<serde_json::Value>, NotExpressible> {
    let mut filters = Vec::new();
    for predicate in predicates {
        let name = member(predicate.column());
        filters.extend(cube_filters(predicate, &name, params)?);
        members.push(name);
    }
    Ok(filters)
}

fn member(column: &PlanColumn) -> String {
    format!("{}.{}", column.table().as_str(), column.column().as_str())
}

fn measure_read(measure: &PlanMeasure, metric: &str) -> Read {
    match *measure {
        PlanMeasure::Simple { ref term } => Read::Measure {
            real: matches!(
                *term,
                PlanTerm::Aggregate {
                    aggregate: Aggregate::Avg,
                    ..
                }
            ),
            fails: None,
            count: matches!(
                *term,
                PlanTerm::CountIf { .. }
                    | PlanTerm::Aggregate {
                        aggregate: Aggregate::Count | Aggregate::CountDistinct,
                        ..
                    }
            ),
        },
        PlanMeasure::Ratio { zero_denominator, .. } => Read::Measure {
            real: true,
            fails: matches!(zero_denominator, ZeroDenominator::Fail).then(|| String::from(metric)),
            count: false,
        },
    }
}

fn cube_filters(
    predicate: &PlanPredicate,
    member: &str,
    params: &[ParamValue],
) -> Result<Vec<serde_json::Value>, NotExpressible> {
    let value = |index: usize| params.get(index).map(text).ok_or(NotExpressible::UnboundParameter);
    let values = |indices: &mut dyn Iterator<Item = &usize>| indices.map(|&index| value(index)).collect::<Result<Vec<_>, _>>();
    let one =
        |operator: &str, values: Vec<String>| serde_json::json!({ "member": member, "operator": operator, "values": values });
    let set = serde_json::json!({ "member": member, "operator": "set" });
    Ok(match *predicate {
        PlanPredicate::AtOrAfter { param, .. } => vec![one("afterOrOnDate", vec![value(param)?])],
        PlanPredicate::Before { param, .. } => vec![one("beforeDate", vec![value(param)?])],
        PlanPredicate::Equals { param, .. } => vec![one("equals", vec![value(param)?])],
        PlanPredicate::In { ref params, .. } => vec![one("equals", values(&mut params.iter())?)],
        PlanPredicate::NotEquals { param, .. } => vec![one("notEquals", vec![value(param)?]), set],
        PlanPredicate::NotIn { ref params, .. } => vec![one("notEquals", values(&mut params.iter())?), set],
        PlanPredicate::IsTrue { .. } => vec![one("equals", vec![String::from("true")])],
        PlanPredicate::IsNotNull { .. } => vec![set],
    })
}

fn text(value: &ParamValue) -> String {
    match *value {
        ParamValue::Text(ref text) => text.clone(),
        ParamValue::Date(date) => date.to_iso(),
    }
}

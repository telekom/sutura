//! The federated question: two legs, and the combine that happens above them.
//!
//! **This module gives [`crate::federation`] its caller.** Until now that module was "a
//! classification and a rule, nothing executes it": `Descent::of` and `Federation::of` were total but
//! nothing produced a [`crate::plan::LegPlan`] and nothing consumed the rows one returns. This module
//! is the other half - the small, closed contract that a splitter fills with facts and this module's
//! own [`FederatedPlan::combine`] turns back into rows.
//!
//! **What the splitter and the combiner agree on, and it is one function.** A fact leg's terms are
//! projected under labels, and the combiner has to find each term's column *by* its label - the
//! mistake this design refuses to make is the two halves agreeing by review. [`labels`] is that one
//! function: the splitter names the fact leg's terms with it and the combiner re-derives the same
//! names from the same [`Federation`] and looks them up in the fact leg's result. There is no second
//! copy of the naming rule to drift.
//!
//! **The division cannot happen in a leg, and [`combine`](FederatedPlan::combine) is where it
//! happens instead.** The [`Above`] tree already carries the only
//! [`ZeroDenominator`](crate::measure::ZeroDenominator) in the federated path; this module walks it
//! above the legs, after every leg's rows have been re-aggregated. Applying a guard inside a leg is
//! the wrong number this shape exists to prevent.
//!
//! **What this module will not do, because `combine` cannot express it.** The re-aggregation
//! `combine` performs covers the leaves a *decomposable* measure produces - a re-aggregating
//! [`Sum`](crate::model::Aggregate::Sum), [`Min`](crate::model::Aggregate::Min) or
//! [`Max`](crate::model::Aggregate::Max) over already-aggregated leg columns. A measure that does not
//! decompose at all (a distinct count) has no re-aggregating function, and the honest answer for this
//! slice is to refuse it in the splitter rather than pull its rows up through a combiner that would
//! have to re-count. The refusal names the aggregate.

use std::collections::BTreeMap;

use crate::federation::{Above, Federation};
use crate::measure::ZeroDenominator;
use crate::model::{Aggregate, MetricName};
use crate::plan::PlanBucket;
use crate::plan::leg::LegPlan;
use crate::warehouse::{Real, RowSet, Value};

/// The one definition of what a carried leaf is projected under.
///
/// The splitter and the combiner both call this, so the column the combiner reads a leaf from and
/// the label the splitter projected it under cannot disagree - there is no second copy of the rule.
///
/// **A single leaf is the answer's own name; several leaves disambiguate by position.** A plain sum
/// travels as the metric's own label, and the halves of a decomposition travel as `metric__{n}`,
/// where `n` is the leaf's position in carried order. Position cannot collide: a ratio of two sums -
/// `sum(a) / sum(b)` - is one aggregating function twice, so naming by aggregate would give both
/// leaves the same label and a combine that divides a column by itself. Whatever makes the labels
/// unique within one plan is enough - the final measure comes back under the metric's own name - and
/// this rule is that minimum.
pub fn labels(federation: &Federation, metric: &MetricName) -> Vec<String> {
    let leaves = federation.carried();
    let single = leaves.len() == 1;
    leaves
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if single {
                String::from(metric.as_str())
            } else {
                format!("{}__{index}", metric.as_str())
            }
        })
        .collect()
}

/// The one federated shape this workspace combines: a fact leg on one source and a lookup leg on
/// another, linked by a single column.
///
/// **Two legs and no more, recorded as a vector because a match over [`LegPlan`] is exhaustive.**
/// The shape is deliberately the one [`crate::plan::leg`] pins in its goldens: the metric's own rows
/// (and any same-source dimension) form the [`Fact`](LegPlan::Fact) leg, and a dimension on a second
/// data system forms the [`Lookup`](LegPlan::Lookup) leg. The final answer groups by the local keys
/// from the fact leg and the remote keys from the lookup leg, bucketed and measured under the
/// metric's own name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FederatedPlan {
    metric: MetricName,
    measure_label: String,
    bucket: PlanBucket,
    /// The answer's group-by keys, local first then remote.
    ///
    /// Labels into the fact result for the local half and the lookup result for the remote half,
    /// kept apart so the combiner knows which rowset to read each from.
    fact_keys: Vec<String>,
    lookup_keys: Vec<String>,
    /// Exactly two: the fact leg followed by the lookup leg.
    legs: Vec<LegPlan>,
    /// The label of the column that links the two legs, in each leg's own result.
    fact_join: String,
    lookup_join: String,
    /// Whether an unmatched fact row survives with null remote keys.
    ///
    /// INNER for a lookup carrying a filter, LEFT for one that does not - the splitter's decision,
    /// recorded here so the combiner does not have to guess. `docs/adr/0009` decides the direction.
    include_unmatched: bool,
    /// The combine tree above the legs, and the metric that names its leaves.
    federation: Federation,
}

impl FederatedPlan {
    /// Constructs a federated plan from its parts.
    #[expect(clippy::too_many_arguments, reason = "a plan is what the splitter decided, in one place")]
    pub const fn new(
        metric: MetricName,
        measure_label: String,
        bucket: PlanBucket,
        fact_keys: Vec<String>,
        lookup_keys: Vec<String>,
        legs: Vec<LegPlan>,
        fact_join: String,
        lookup_join: String,
        include_unmatched: bool,
        federation: Federation,
    ) -> Self {
        Self {
            metric,
            measure_label,
            bucket,
            fact_keys,
            lookup_keys,
            legs,
            fact_join,
            lookup_join,
            include_unmatched,
            federation,
        }
    }

    /// Every leg, in execution order: the fact leg, then the lookup leg.
    pub fn legs(&self) -> &[LegPlan] {
        &self.legs
    }

    /// The metric this answer is measured in.
    pub const fn metric(&self) -> &MetricName {
        &self.metric
    }

    /// Every data system this plan reads from.
    pub fn sources(&self) -> impl Iterator<Item = &crate::model::SourceName> {
        self.legs.iter().map(LegPlan::source)
    }
}

/// Why a federated answer could not be assembled.
///
/// The shape failures are defects in this workspace's own wiring - a leg result missing a column
/// [`labels`] named, or a count of legs that is not two. The [`NonFinite`](FederatedFailure::NonFinite)
/// variant is a `fails` guard meeting a zero denominator, which no divide-tree node can produce a
/// value for.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FederatedFailure {
    /// The two legs this plan claims do not both render one rowset each.
    #[error("a federated question needs one result per leg, and {legs} were combined")]
    LegCount { legs: usize },
    /// A column `combine` reached for by label was absent from a leg's result.
    ///
    /// The labelling contract is one function - the splitter and the combiner both call [`labels`] -
    /// so this is a wiring defect between the two halves rather than a choice either side made.
    #[error("the {side} result has no column `{label}`")]
    MissingColumn { side: &'static str, label: String },
    /// A division happened by a zero denominator while the measure declared `fails`.
    ///
    /// On the mono-source path a non-finite cell is refused at the port; this is this slice's port,
    /// so the guard landing here is an error naming the metric it could not certify.
    #[error("a non-finite value reached the answer for `{metric}`")]
    NonFinite { metric: MetricName },
    /// A leg result had two columns under one label, so the combiner could not tell which of them
    /// a leaf or key names.
    #[error("the {side} result labels two columns `{label}`")]
    DuplicateLabels { side: &'static str, label: String },
    /// A link cell carried a floating-point key, which the ADR's float-key rule forbids.
    #[error("a link column carried a floating-point key ({value})")]
    FloatLinkKey { value: f64 },
    /// A link value had more than one lookup row, which would double every measure.
    #[error("the link value `{key}` maps to more than one lookup row")]
    AmbiguousLink { key: String },
    /// A leaf cell that was not a number reached a re-aggregating aggregate.
    ///
    /// The DuckDB adapter deliberately returns `DECIMAL` and wide integer columns as
    /// [`Value::Text`] to keep them exact; a sum reaching such a cell cannot certify a number, so
    /// it is refused rather than counted as zero.
    #[error("a `{aggregate:?}` re-aggregation met a non-numeric leaf cell (`{value:?}`)")]
    NonNumericLeaf { aggregate: Aggregate, value: Value },
    /// A leaf total overflowed a 64-bit integer.
    #[error("a `{aggregate:?}` re-aggregation overflowed a 64-bit integer")]
    Overflow { aggregate: Aggregate },
    /// An aggregate the combiner does not know how to re-aggregate with.
    ///
    /// The splitter refuses such a measure, so this is a wiring defect rather than a choice - a
    /// caller must receive a failure, not silent data.
    #[error("the combiner does not re-aggregate with `{aggregate:?}`")]
    UnsupportedAggregate { aggregate: Aggregate },
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the accessors and the combiner span one type, kept apart for readability"
)]
impl FederatedPlan {
    /// Turns one result per leg into one answer's rows.
    ///
    /// The fact and lookup results are joined on the recorded link column, grouped by the answer's
    /// keys and the bucket, re-aggregated by each leaf's own [`Carried::combine`], and only then
    /// divided through the [`Above`] tree.
    pub fn combine(&self, leg_results: &[RowSet]) -> Result<RowSet, FederatedFailure> {
        let fact = leg_results
            .first()
            .ok_or(FederatedFailure::LegCount { legs: leg_results.len() })?;
        let lookup = leg_results
            .get(1)
            .ok_or(FederatedFailure::LegCount { legs: leg_results.len() })?;

        distinct_columns(fact, "fact")?;
        distinct_columns(lookup, "lookup")?;

        let fact_join_index = column_index(fact, &self.fact_join, "fact")?;
        let lookup_join_index = column_index(lookup, &self.lookup_join, "lookup")?;
        let fact_key_indexes = self.indexes(fact, &self.fact_keys, "fact")?;
        let lookup_key_indexes = self.indexes(lookup, &self.lookup_keys, "lookup")?;
        let bucket_index = column_index(fact, self.bucket.label(), "fact")?;
        let leaf_indexes = self.indexes(fact, &labels(&self.federation, &self.metric), "fact")?;

        // The fact leg already grouped by its keys, so one fact row per (local keys, link, bucket);
        // several rows can share a link value (one per local-key group), so each link maps to a list.
        // A null link never joins, and a real link is refused by the float-key rule.
        let mut fact_by_link: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (row_index, row) in fact.rows().iter().enumerate() {
            let Some(link) = row.get(fact_join_index) else {
                continue;
            };
            let Some(key) = link_key(link)? else {
                continue;
            };
            fact_by_link.entry(key).or_default().push(row_index);
        }

        // The lookup result maps a link value to the remote keys that share it. More than one lookup
        // row for one link would double every measure, so it is refused rather than certified.
        let mut lookup_by_link: RemoteByLink = BTreeMap::new();
        for row in lookup.rows() {
            let Some(link) = row.get(lookup_join_index) else {
                continue;
            };
            let Some(key) = link_key(link)? else {
                continue;
            };
            let remote: Option<Vec<Value>> = lookup_key_indexes.iter().map(|index| row.get(*index).cloned()).collect();
            let Some(remote) = remote else {
                continue;
            };
            let entry = lookup_by_link.entry(key.clone()).or_default();
            if !entry.is_empty() {
                return Err(FederatedFailure::AmbiguousLink { key });
            }
            entry.push(remote);
        }

        // A final answer's group is identified by its key cells in answer order (plan keys, then the
        // bucket) and collects the leaf cells of every fact row it joined to.
        let mut groups: BTreeMap<Vec<String>, Group> = BTreeMap::new();
        for (link_key, fact_rows) in &fact_by_link {
            let keys = match lookup_by_link.get(link_key) {
                Some(remote_rows) => remote_rows.clone(),
                None if self.include_unmatched => vec![vec![Value::Null; self.lookup_keys.len()]],
                None => continue,
            };
            for &row_index in fact_rows {
                let Some(fact_row) = fact.rows().get(row_index) else {
                    return Err(FederatedFailure::LegCount { legs: leg_results.len() });
                };
                let local: Option<Vec<Value>> = fact_key_indexes.iter().map(|index| fact_row.get(*index).cloned()).collect();
                let Some(local) = local else {
                    return Err(FederatedFailure::LegCount { legs: leg_results.len() });
                };
                let Some(bucket_cell) = fact_row.get(bucket_index) else {
                    return Err(FederatedFailure::LegCount { legs: leg_results.len() });
                };
                let leaves: Option<Vec<Value>> = leaf_indexes.iter().map(|index| fact_row.get(*index).cloned()).collect();
                let Some(leaves) = leaves else {
                    return Err(FederatedFailure::LegCount { legs: leg_results.len() });
                };
                for remote in &keys {
                    let mut cells = local.clone();
                    cells.extend(remote.iter().cloned());
                    cells.push(bucket_cell.clone());
                    let map_key: Vec<String> = cells.iter().map(key_cell_str).collect();
                    groups
                        .entry(map_key)
                        .or_insert_with(|| Group {
                            cells: cells.clone(),
                            leaves: Vec::new(),
                        })
                        .leaves
                        .push(leaves.clone());
                }
            }
        }

        // Re-aggregate each leaf across its group, then walk the divide tree.
        let mut rows: Vec<Vec<Value>> = Vec::new();
        for group in groups.into_values() {
            let aggregated = leaf_values(&self.federation, &group.leaves, &self.metric)?;
            let measure = apply_above(self.federation.above(), &aggregated, &mut 0, &self.metric)?;
            let mut row = group.cells;
            row.push(measure);
            rows.push(row);
        }

        let mut columns: Vec<String> = self.fact_keys.clone();
        columns.extend(self.lookup_keys.iter().cloned());
        columns.push(String::from(self.bucket.label()));
        columns.push(self.measure_label.clone());

        // Deterministic order: an answer's rows should not depend on hash iteration or on the order a
        // data system happened to return. Compared by the rendered key cells a caller sees.
        let key_width = columns.len().saturating_sub(1);
        rows.sort_by(|a, b| {
            let a_key: Vec<String> = a.iter().take(key_width).map(Value::render).collect();
            let b_key: Vec<String> = b.iter().take(key_width).map(Value::render).collect();
            a_key.cmp(&b_key)
        });

        RowSet::new(columns, rows).map_err(|_malformed| FederatedFailure::LegCount { legs: leg_results.len() })
    }

    fn indexes(&self, rows: &RowSet, labels: &[String], side: &'static str) -> Result<Vec<usize>, FederatedFailure> {
        labels.iter().map(|label| column_index(rows, label, side)).collect()
    }
}

/// The index of the column with `label` in `rows`.
fn column_index(rows: &RowSet, label: &str, side: &'static str) -> Result<usize, FederatedFailure> {
    rows.column_index(label).ok_or_else(|| FederatedFailure::MissingColumn {
        side,
        label: String::from(label),
    })
}

/// Refuse a leg result whose columns are not all distinctly labelled.
///
/// A duplicate label is the one shape the combiner cannot disambiguate - two leaf columns under one
/// name, or a key colliding with a leaf - so it is caught at the boundary rather than allowed to
/// answer a wrong number.
fn distinct_columns(rows: &RowSet, side: &'static str) -> Result<(), FederatedFailure> {
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for name in rows.columns() {
        if !seen.insert(name.as_str()) {
            return Err(FederatedFailure::DuplicateLabels {
                side,
                label: name.clone(),
            });
        }
    }
    Ok(())
}

/// The key a link cell joins on, or `None` for a null link.
///
/// A `Null` link never joins (`NULL = NULL` is not true in SQL), and a real link is refused - the
/// ADR's float-key rule, because formatting a float into equality lets distinct values collide.
/// Text and integer links are keyed by their typed text, so `Integer(1001)` and `Text("1001")` do
/// not false-match across two sources.
fn link_key(value: &Value) -> Result<Option<String>, FederatedFailure> {
    match value {
        Value::Null => Ok(None),
        Value::Real(v) => Err(FederatedFailure::FloatLinkKey { value: v.get() }),
        Value::Integer(_) | Value::Text(_) => Ok(Some(key_cell_str(value))),
    }
}

/// One link value's remote-key rows.
type RemoteByLink = BTreeMap<String, Vec<Vec<Value>>>;

/// One final answer's group: its key cells (as they should appear in the answer) and every fact
/// row's leaf values that joined to it.
struct Group {
    cells: Vec<Value>,
    leaves: Vec<Vec<Value>>,
}

/// A group-key cell ordered and distinguished by type.
///
/// The map is keyed on this rather than on [`Value::render`], because a null and a text cell that
/// happens to read "null" would otherwise be one group - and they are not the same answer.
fn key_cell_str(value: &Value) -> String {
    match value {
        Value::Null => String::from("N"),
        Value::Integer(v) => format!("I:{v}"),
        Value::Real(v) => format!("R:{}", v.get()),
        Value::Text(v) => format!("T:{v}"),
    }
}

/// Re-aggregates every leaf in carried order, one value per leaf.
fn leaf_values(federation: &Federation, leaf_rows: &[Vec<Value>], metric: &MetricName) -> Result<Vec<Value>, FederatedFailure> {
    federation
        .carried()
        .iter()
        .enumerate()
        .map(|(column, leaf)| aggregate(leaf.combine(), leaf_rows.iter().filter_map(|row| row.get(column)), metric))
        .collect()
}

/// Re-aggregates one leaf's already-aggregated values across a group.
///
/// **The only aggregates that arrive here are the ones a decomposable measure re-aggregates with.** A
/// `Count` leaf re-aggregates with a sum and is itself an integer; the splitter refuses a `Carried::Keys`
/// leaf entirely, so `combine` is a total, minimum or maximum over a list of numbers. A group with no
/// non-null value contributes null; a cell that is not a number, an overflow, or a non-finite total is
/// a refusal, never a silent zero or null.
#[expect(
    clippy::float_arithmetic,
    reason = "the re-aggregation of a leg column sums real numbers by design"
)]
fn aggregate<'a>(
    aggregate: Aggregate,
    values: impl Iterator<Item = &'a Value>,
    metric: &MetricName,
) -> Result<Value, FederatedFailure> {
    let numeric: Vec<&Value> = values.filter(|v| !matches!(*v, Value::Null)).collect();
    if numeric.is_empty() {
        return Ok(Value::Null);
    }
    match aggregate {
        Aggregate::Sum => {
            let mut sum_i: i64 = 0;
            let mut sum_r: f64 = 0.0;
            let mut has_real = false;
            for value in numeric {
                match value {
                    Value::Integer(v) => {
                        sum_i = sum_i.checked_add(*v).ok_or(FederatedFailure::Overflow {
                            aggregate: Aggregate::Sum,
                        })?
                    }
                    Value::Real(v) => {
                        has_real = true;
                        sum_r += v.get();
                    }
                    other => {
                        return Err(FederatedFailure::NonNumericLeaf {
                            aggregate: Aggregate::Sum,
                            value: other.clone(),
                        });
                    }
                }
            }
            if has_real {
                return Real::parse(sum_r).map_or_else(
                    |_| Err(FederatedFailure::NonFinite { metric: metric.clone() }),
                    |real| Ok(Value::Real(real)),
                );
            }
            Ok(Value::Integer(sum_i))
        }
        Aggregate::Min => minmax(numeric, false),
        Aggregate::Max => minmax(numeric, true),
        other => Err(FederatedFailure::UnsupportedAggregate { aggregate: other }),
    }
}

/// The minimum or maximum of a non-empty numeric list, preserving the winning cell's own type.
fn minmax(values: Vec<&Value>, max: bool) -> Result<Value, FederatedFailure> {
    let mut best: Option<Value> = None;
    for value in values {
        let candidate = (*value).clone();
        best = Some(match best {
            None => candidate,
            Some(current) => {
                let candidate_is_better = match value {
                    Value::Integer(_) | Value::Real(_) => match (to_f64(&current), to_f64(&candidate)) {
                        (Some(a), Some(b)) => {
                            if max {
                                b > a
                            } else {
                                b < a
                            }
                        }
                        _ => false,
                    },
                    other => {
                        return Err(FederatedFailure::NonNumericLeaf {
                            aggregate: if max { Aggregate::Max } else { Aggregate::Min },
                            value: other.clone(),
                        });
                    }
                };
                if candidate_is_better { candidate } else { current }
            }
        });
    }
    Ok(best.unwrap_or(Value::Null))
}

/// Applies the divide tree above a group's re-aggregated leaves, returning the measure.
///
/// `cursor` walks the tree in the same order [`Federation::carried`] collects its leaves, so each
/// [`Above::Total`] node reads the leaf [`leaf_values`] aggregated for it.
fn apply_above(above: &Above, aggregated: &[Value], cursor: &mut usize, metric: &MetricName) -> Result<Value, FederatedFailure> {
    match *above {
        Above::Total(_) => {
            let value = aggregated.get(*cursor).cloned();
            *cursor = cursor.saturating_add(1);
            Ok(value.unwrap_or(Value::Null))
        }
        Above::Quotient {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => {
            let numerator = apply_above(numerator, aggregated, cursor, metric)?;
            let denominator = apply_above(denominator, aggregated, cursor, metric)?;
            divide(&numerator, &denominator, zero_denominator, metric)
        }
    }
}

/// One division, with the guard the definition asked for applied to the final denominator.
#[expect(clippy::float_arithmetic, reason = "a division of leg totals is float arithmetic by design")]
fn divide(
    numerator: &Value,
    denominator: &Value,
    zero_denominator: ZeroDenominator,
    metric: &MetricName,
) -> Result<Value, FederatedFailure> {
    let Some(num) = to_f64(numerator) else {
        return Ok(Value::Null);
    };
    let Some(den) = to_f64(denominator) else {
        return Ok(Value::Null);
    };
    if den == 0.0 {
        return match zero_denominator {
            ZeroDenominator::Null => Ok(Value::Null),
            ZeroDenominator::Fail => Err(FederatedFailure::NonFinite { metric: metric.clone() }),
        };
    }
    let real = Real::parse(num / den).map_err(|_not_finite| FederatedFailure::NonFinite { metric: metric.clone() })?;
    Ok(Value::Real(real))
}

/// A numeric cell as `f64`, or `None` for a null.
///
/// [`expect`](macro@expect)-bounded: casting a wide integer to `f64` can lose precision, which is
/// accepted here because a ratio over leg totals is inherently floating-point and the divide tree
/// only ever reads these as `f64`.
#[expect(clippy::cast_precision_loss, reason = "a division reads leg totals as f64 by design")]
const fn to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(v) => Some(*v as f64),
        Value::Real(v) => Some(v.get()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::catalog::TIME_BUCKET_LABEL;
    use crate::federation::Federation;
    use crate::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
    use crate::model::{Aggregate, ColumnName, Grain, MetricName, TableName};
    use crate::plan::{FederatedFailure, FederatedPlan, PlanBucket, PlanColumn};
    use crate::warehouse::{Real, RowSet, Value};

    const FACT: &str = "fct_subscription_monthly";

    fn metric(name: &str) -> MetricName {
        MetricName::parse(name).expect("a test metric is a metric")
    }

    fn column(name: &str) -> ColumnName {
        ColumnName::parse(name).expect("a test column is a column")
    }

    fn term(aggregate: Aggregate, name: &str) -> Term {
        Term::Aggregate(AggregatedColumn::new(aggregate, column(name)))
    }

    fn bucket() -> PlanBucket {
        PlanBucket::new(
            String::from(TIME_BUCKET_LABEL),
            Grain::Month,
            PlanColumn::new(TableName::parse(FACT).expect("a table"), column("month")),
        )
    }

    fn plan_for(measure_name: &str, measure: Measure, include_unmatched: bool) -> FederatedPlan {
        let name = metric(measure_name);
        let federation = Federation::of(&measure);
        FederatedPlan::new(
            name,
            String::from(measure_name),
            bucket(),
            vec![String::from("product_family")],
            vec![String::from("region")],
            Vec::new(),
            String::from("customer_key"),
            String::from("customer_key"),
            include_unmatched,
            federation,
        )
    }

    fn sum_plan(include_unmatched: bool) -> FederatedPlan {
        plan_for(
            "revenue",
            Measure::Simple(term(Aggregate::Sum, "mrr_cents")),
            include_unmatched,
        )
    }

    fn avg_plan() -> FederatedPlan {
        plan_for(
            "mean_subscription_mrr",
            Measure::Simple(term(Aggregate::Avg, "mrr_cents")),
            true,
        )
    }

    fn failing_ratio_plan() -> FederatedPlan {
        plan_for(
            "mean_subscription_mrr",
            Measure::Ratio {
                numerator: term(Aggregate::Sum, "mrr_cents"),
                denominator: term(Aggregate::Count, "mrr_cents"),
                zero_denominator: ZeroDenominator::Fail,
            },
            true,
        )
    }

    fn fact(rows: Vec<Vec<Value>>) -> RowSet {
        RowSet::new(
            vec![
                String::from("product_family"),
                String::from("customer_key"),
                String::from(TIME_BUCKET_LABEL),
                String::from("revenue"),
            ],
            rows,
        )
        .expect("a test fact result is well formed")
    }

    fn lookup(rows: Vec<Vec<Value>>) -> RowSet {
        RowSet::new(vec![String::from("customer_key"), String::from("region")], rows)
            .expect("a test lookup result is well formed")
    }

    #[test]
    fn joins_two_legs_and_reaggregates_by_remote_key() {
        let plan = sum_plan(true);
        let fact = fact(vec![
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ],
            vec![
                Value::Text("A".into()),
                Value::Text("c2".into()),
                Value::Text("2026-06".into()),
                Value::Integer(200),
            ],
            vec![
                Value::Text("B".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(50),
            ],
        ]);
        let lookup = lookup(vec![
            vec![Value::Text("c1".into()), Value::Text("north".into())],
            vec![Value::Text("c2".into()), Value::Text("north".into())],
        ]);

        let combined = plan.combine(&[fact, lookup]).expect("a two-leg question combines");
        assert_eq!(combined.columns(), &["product_family", "region", "period", "revenue"]);
        assert_eq!(
            combined.rows(),
            &[
                vec![
                    Value::Text("A".into()),
                    Value::Text("north".into()),
                    Value::Text("2026-06".into()),
                    Value::Integer(300)
                ],
                vec![
                    Value::Text("B".into()),
                    Value::Text("north".into()),
                    Value::Text("2026-06".into()),
                    Value::Integer(50)
                ],
            ]
        );
    }

    #[test]
    fn an_inner_join_drops_an_unmatched_fact_row() {
        let plan = sum_plan(false);
        let fact = fact(vec![vec![
            Value::Text("A".into()),
            Value::Text("c9".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]]);
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
        let combined = plan.combine(&[fact, lookup]).expect("combines");
        assert!(
            combined.rows().is_empty(),
            "an unmatched fact row is dropped by an inner join"
        );
    }

    #[test]
    fn a_left_join_keeps_an_unmatched_fact_row_with_null_remote() {
        let plan = sum_plan(true);
        let fact = fact(vec![vec![
            Value::Text("A".into()),
            Value::Text("c9".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]]);
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
        let combined = plan.combine(&[fact, lookup]).expect("combines");
        assert_eq!(
            combined.rows(),
            &[vec![
                Value::Text("A".into()),
                Value::Null,
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ]]
        );
    }

    fn avg_fact(rows: Vec<Vec<Value>>) -> RowSet {
        RowSet::new(
            vec![
                String::from("product_family"),
                String::from("customer_key"),
                String::from(TIME_BUCKET_LABEL),
                String::from("mean_subscription_mrr__0"),
                String::from("mean_subscription_mrr__1"),
            ],
            rows,
        )
        .expect("an average fact result is well formed")
    }

    #[test]
    fn an_average_is_undivided_in_the_leg_and_divided_above() {
        let plan = avg_plan();
        let fact = avg_fact(vec![vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(300),
            Value::Integer(3),
        ]]);
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);

        let combined = plan.combine(&[fact, lookup]).expect("an average combines");
        assert_eq!(
            combined.columns(),
            &["product_family", "region", "period", "mean_subscription_mrr"]
        );
        match &combined.rows()[0][3] {
            Value::Real(r) => assert_eq!(r.get(), 100.0),
            other => panic!("an average answers a real number, got {other:?}"),
        }
    }

    #[test]
    fn a_failing_ratio_guard_errors_on_a_zero_denominator() {
        let plan = failing_ratio_plan();
        let fact = avg_fact(vec![vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(300),
            Value::Integer(0),
        ]]);
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
        assert!(matches!(
            plan.combine(&[fact, lookup]),
            Err(FederatedFailure::NonFinite { .. })
        ));
    }

    #[test]
    fn a_decomposed_average_with_zero_over_zero_is_null() {
        let plan = avg_plan();
        let fact = avg_fact(vec![vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(0),
            Value::Integer(0),
        ]]);
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
        let combined = plan.combine(&[fact, lookup]).expect("a null guard answers");
        assert!(matches!(combined.rows()[0][3], Value::Null));
    }

    #[test]
    fn a_missing_leaf_label_is_an_error() {
        let plan = sum_plan(true);
        let fact = RowSet::new(
            vec![
                String::from("product_family"),
                String::from("customer_key"),
                String::from(TIME_BUCKET_LABEL),
            ],
            vec![vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
            ]],
        )
        .expect("a fact result missing the measure column");
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
        assert!(matches!(
            plan.combine(&[fact, lookup]),
            Err(FederatedFailure::MissingColumn { .. })
        ));
    }

    #[test]
    fn a_non_numeric_leaf_is_refused_not_counted_as_zero() {
        // The DuckDB adapter returns a DECIMAL money column as Text to keep it exact; a sum that
        // meets it must refuse rather than certify a zero.
        let plan = sum_plan(true);
        let fact = fact(vec![vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Text("1234.56".into()),
        ]]);
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
        assert!(matches!(
            plan.combine(&[fact, lookup]),
            Err(FederatedFailure::NonNumericLeaf {
                aggregate: Aggregate::Sum,
                ..
            })
        ));
    }

    #[test]
    fn a_float_link_key_is_refused() {
        let plan = sum_plan(true);
        let fact = fact(vec![vec![
            Value::Text("A".into()),
            Value::Real(Real::parse(1001.0).expect("a finite real")),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]]);
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
        assert!(matches!(
            plan.combine(&[fact, lookup]),
            Err(FederatedFailure::FloatLinkKey { .. })
        ));
    }

    #[test]
    fn an_integer_link_and_a_text_link_do_not_false_match() {
        // Integer(1001) and Text("1001") are different cells; comparing them as rendered text would
        // join them, which is the false match the typed link key refuses. Under an inner join the
        // non-matching fact row is dropped.
        let plan = sum_plan(false);
        let fact = fact(vec![vec![
            Value::Text("A".into()),
            Value::Integer(1001),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]]);
        // The lookup holds the same digits as text.
        let lookup = lookup(vec![vec![Value::Text("1001".into()), Value::Text("north".into())]]);
        let combined = plan.combine(&[fact, lookup]).expect("combines");
        assert!(
            combined.rows().is_empty(),
            "an integer link must not join to a text link with the same digits"
        );
    }

    #[test]
    fn a_null_link_never_joins() {
        let plan = sum_plan(false);
        let fact = fact(vec![vec![
            Value::Text("A".into()),
            Value::Null,
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]]);
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
        let combined = plan.combine(&[fact, lookup]).expect("combines");
        assert!(
            combined.rows().is_empty(),
            "a null link value never joins, not even to itself"
        );
    }

    #[test]
    fn a_duplicate_leaf_label_is_refused() {
        // Two columns under one name would be traced to one of them arbitrarily, so the boundary
        // refuses the result rather than answer a wrong number.
        let plan = sum_plan(true);
        let fact = RowSet::new(
            vec![
                String::from("product_family"),
                String::from("customer_key"),
                String::from(TIME_BUCKET_LABEL),
                String::from("revenue"),
                String::from("revenue"),
            ],
            vec![vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
                Value::Integer(200),
            ]],
        )
        .expect("a fact result with a duplicated label");
        let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
        assert!(matches!(
            plan.combine(&[fact, lookup]),
            Err(FederatedFailure::DuplicateLabels { .. })
        ));
    }

    #[test]
    fn an_ambiguous_lookup_link_is_refused() {
        let plan = sum_plan(true);
        let fact = fact(vec![vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]]);
        // Two lookup rows for one link would double the measure.
        let lookup = RowSet::new(
            vec![String::from("customer_key"), String::from("region")],
            vec![
                vec![Value::Text("c1".into()), Value::Text("north".into())],
                vec![Value::Text("c1".into()), Value::Text("south".into())],
            ],
        )
        .expect("a lookup result with two rows for one link");
        assert!(matches!(
            plan.combine(&[fact, lookup]),
            Err(FederatedFailure::AmbiguousLink { .. })
        ));
    }
}

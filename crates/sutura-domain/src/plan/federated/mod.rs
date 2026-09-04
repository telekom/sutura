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

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::federation::{Above, Federation};
use crate::measure::ZeroDenominator;
use crate::model::{Aggregate, MetricName, SourceName};
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
/// **Two legs, as two named fields.** A match over [`LegPlan`] is exhaustive, so the fact leg *is*
/// the [`Fact`](LegPlan::Fact) variant and the lookup leg the [`Lookup`](LegPlan::Lookup) one, and
/// a plan that had anything other than exactly these two is a type that does not exist rather than a
/// count a caller checks. The shape is deliberately the one [`crate::plan::leg`] pins in its goldens:
/// the metric's own rows (and any same-source dimension) form the fact leg, and a dimension on a
/// second data system forms the lookup leg. The final answer groups by the answer's keys - each
/// named by which leg's result it is read from, in question order - bucketed and measured under the
/// metric's own name.
///
/// **The [`serde::Serialize`] derive exists for the CLI's plan dump and nothing else.** A plan is
/// serialized to be printed; nothing in the workspace gains [`serde::Deserialize`], so a plan cannot
/// be reconstructed from its serialized form and no field here is a request a caller writes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FederatedPlan {
    metric: MetricName,
    measure_label: String,
    bucket: PlanBucket,
    /// The metric's own share of the question: the same-source rows and leaves.
    fact: LegPlan,
    /// The second data system's share: the remote dimensions the answer groups by.
    lookup: LegPlan,
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
    /// The answer's group-by keys in question order, each naming which leg's result it is read from.
    ///
    /// This is the one honest statement of the answer's column order, matching the mono path which
    /// emits dimensions as the question ordered them. Fact keys are read from the fact result,
    /// lookup keys from the lookup result, and the two never overlap because a dimension belongs to
    /// exactly one leg.
    keys: Vec<AnswerKey>,
}

/// Which leg's result an answer key is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum LegSide {
    /// The metric's own leg.
    Fact,
    /// The second data system's leg.
    Lookup,
}

/// One group-by key of the answer: which leg owns it, and the label it carries in that leg's result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AnswerKey {
    side: LegSide,
    label: String,
}

impl AnswerKey {
    /// A key read from the fact leg's result, under `label`.
    #[inline]
    pub const fn fact(label: String) -> Self {
        Self {
            side: LegSide::Fact,
            label,
        }
    }

    /// A key read from the lookup leg's result, under `label`.
    #[inline]
    pub const fn lookup(label: String) -> Self {
        Self {
            side: LegSide::Lookup,
            label,
        }
    }

    /// Which leg this key is read from.
    #[inline]
    pub const fn side(&self) -> LegSide {
        self.side
    }

    /// The label this key carries in its leg's result.
    #[inline]
    pub fn label(&self) -> &str {
        &self.label
    }
}

impl FederatedPlan {
    /// Constructs a federated plan from its two legs and the answer's key order.
    ///
    /// A `Result` constructor is this workspace's convention for a value with an invariant: a plan
    /// that is not a fact leg beside a lookup leg, or that names one data system on both legs, is not
    /// a plan and cannot be built.
    // The constructor takes the shape of the question as the splitter decided it; a bundle of named
    // fields is the alternative, and a `Vec` would let a caller omit or duplicate a leg - the two
    // instantiations it exists to forbid.
    pub fn new(
        metric: MetricName,
        measure_label: String,
        bucket: PlanBucket,
        fact: LegPlan,
        lookup: LegPlan,
        fact_join: String,
        lookup_join: String,
        include_unmatched: bool,
        federation: Federation,
        keys: Vec<AnswerKey>,
    ) -> Result<Self, FederatedPlanError> {
        let is_fact = matches!(fact, LegPlan::Fact { .. });
        let is_lookup = matches!(lookup, LegPlan::Lookup { .. });
        if !is_fact {
            return Err(FederatedPlanError::NotFact {
                source_name: fact.source().clone(),
            });
        }
        if !is_lookup {
            return Err(FederatedPlanError::NotLookup {
                source_name: lookup.source().clone(),
            });
        }
        if fact.source() == lookup.source() {
            return Err(FederatedPlanError::SameSource {
                source_name: fact.source().clone(),
            });
        }
        for key in &keys {
            match key.side() {
                LegSide::Fact => leg_has_key(&fact, &key.label).map_err(|label| FederatedPlanError::KeyNotOnLeg {
                    side: LegSide::Fact,
                    label: String::from(label),
                })?,
                LegSide::Lookup => leg_has_key(&lookup, &key.label).map_err(|label| FederatedPlanError::KeyNotOnLeg {
                    side: LegSide::Lookup,
                    label: String::from(label),
                })?,
            }
        }
        for leaf in federation.carried() {
            let aggregate = leaf.combine();
            if Reduction::of(aggregate).is_none() {
                return Err(FederatedPlanError::LeafDoesNotReaggregate { aggregate });
            }
        }
        Ok(Self {
            metric,
            measure_label,
            bucket,
            fact,
            lookup,
            fact_join,
            lookup_join,
            include_unmatched,
            federation,
            keys,
        })
    }

    /// Every leg, in execution order: the fact leg, then the lookup leg.
    pub const fn legs(&self) -> [&LegPlan; 2] {
        [&self.fact, &self.lookup]
    }

    /// The fact leg.
    pub const fn fact(&self) -> &LegPlan {
        &self.fact
    }

    /// The lookup leg.
    pub const fn lookup(&self) -> &LegPlan {
        &self.lookup
    }

    /// The metric this answer is measured in.
    pub const fn metric(&self) -> &MetricName {
        &self.metric
    }

    /// Every data system this plan reads from, in execution order.
    pub fn sources(&self) -> impl Iterator<Item = &crate::model::SourceName> + '_ {
        [&self.fact, &self.lookup].into_iter().map(LegPlan::source)
    }

    /// The answer's group-by keys, in question order.
    pub fn keys(&self) -> &[AnswerKey] {
        &self.keys
    }
}

/// Why a federated plan could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FederatedPlanError {
    /// The leg meant to be the fact leg is not a [`LegPlan::Fact`].
    #[error("the fact leg reads `{source_name}`, which is not a fact leg")]
    NotFact { source_name: SourceName },
    /// The leg meant to be the lookup leg is not a [`LegPlan::Lookup`].
    #[error("the lookup leg reads `{source_name}`, which is not a lookup leg")]
    NotLookup { source_name: SourceName },
    /// Both legs name the same data system, which is a single-source question, not a federated one.
    #[error("both legs read from `{source_name}`, which is not a federated question")]
    SameSource { source_name: SourceName },
    /// An answer key names a column the leg it belongs to does not project.
    #[error("the {side:?} leg projects no key `{label}`")]
    KeyNotOnLeg { side: LegSide, label: String },
    /// A carried leaf names an aggregate the combine has no re-aggregating function for.
    ///
    /// Refused before a plan exists rather than when a group is reduced: it is a defect in this
    /// workspace's own wiring, and reduced, the same plan refused a group holding a value and
    /// answered `Null` for a group of nulls, under the metric's own certified name.
    #[error("a carried leaf re-aggregates with `{aggregate}`, which the combine cannot apply")]
    LeafDoesNotReaggregate { aggregate: Aggregate },
}

/// Whether a [`LegPlan`] projects a key under `label`.
fn leg_has_key<'a>(leg: &LegPlan, label: &'a str) -> Result<(), &'a str> {
    leg.keys().iter().any(|key| key.label() == label).then_some(()).ok_or(label)
}

/// Why a federated answer could not be assembled.
///
/// The shape failures are defects in this workspace's own wiring - a leg result missing a column
/// [`labels`] named, or a row narrower than its result's own columns. The [`NonFinite`](FederatedFailure::NonFinite)
/// variant is a `fails` guard meeting a zero denominator, which no divide-tree node can produce a
/// value for.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FederatedFailure {
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
    /// The `DuckDB` adapter deliberately returns `DECIMAL` and wide integer columns as
    /// [`Value::Text`] to keep them exact; a sum reaching such a cell cannot certify a number, so
    /// it is refused rather than counted as zero.
    #[error("a `{aggregate:?}` re-aggregation met a non-numeric leaf cell (`{value:?}`)")]
    NonNumericLeaf { aggregate: Aggregate, value: Value },
    /// A leaf column carried two numeric types, so no total or comparison over it is exact.
    ///
    /// A result column in a data system has one logical type. [`RowSet`] constrains a row's width and
    /// nothing about its cells, so a column mixing [`Value::Integer`] and [`Value::Real`] cells is
    /// representable here, and the two ways to answer one are both wrong numbers: dropping either
    /// subtotal loses it outright, and folding the integer one into the real one is an `i64 as f64`
    /// widening - the same silent widening `DuckDB`'s own conversion refuses for a 32-bit float and
    /// for a wide integer that does not fit an `i64`. Refused instead, which is also what leaves the
    /// aggregates above comparing and adding one type.
    #[error("a `{aggregate:?}` re-aggregation met a leaf column mixing integer and real cells")]
    MixedNumericLeaf { aggregate: Aggregate },
    /// A leaf total overflowed a 64-bit integer.
    #[error("a `{aggregate:?}` re-aggregation overflowed a 64-bit integer")]
    Overflow { aggregate: Aggregate },
    /// An aggregate the combiner does not know how to re-aggregate with.
    ///
    /// Unreachable through a plan [`FederatedPlan::new`] built, which refuses such a federation
    /// before any leg runs. **The limit:** that guarantee is module-scoped - code in this file can
    /// write the struct literal - so this stays a refusal rather than becoming a panic.
    #[error("the combiner does not re-aggregate with `{aggregate:?}`")]
    UnsupportedAggregate { aggregate: Aggregate },
    /// Materialising the answer crossed the byte budget `docs/adr/0009` applies at the conversion
    /// boundary.
    ///
    /// The legs have no row cap - that measured key cardinality rather than bytes, which is exactly
    /// what 0009 retired - so this is the bound on the answer `combine` builds. A refusal is honest
    /// in the way a truncated one is not: the caller sees a `federation_not_executable`-adjacent
    /// refusal rather than a row set that stopped early.
    #[error("the federated answer exceeds the {ceiling_bytes}-byte working-set ceiling")]
    ResourcesExhausted { ceiling_bytes: u64 },
    /// A row whose width contradicts the result's own column count.
    ///
    /// Unreachable by construction on both halves: a leg result is built by [`RowSet::new`], which
    /// refuses a ragged row up front, and the answer is projected from a single fixed key list. It is
    /// this slice's defensive arm - the named, reachable-if-the-type-lying shape the old `LegCount`
    /// catch-all used to swallow.
    #[error("a row of the {side} result had the wrong number of cells")]
    MalformedRow { side: &'static str },
}

/// The column positions [`FederatedPlan::combine`] needs, resolved once.
///
/// Resolved together so every label lookup happens in one place and the rest of the combine reads by
/// position. The maps are owned, so this struct does not borrow from either result set.
struct LegIndexes {
    fact_join: usize,
    lookup_join: usize,
    bucket: usize,
    fact_index: BTreeMap<String, usize>,
    lookup_columns: Vec<(String, usize)>,
    lookup_pos: BTreeMap<String, usize>,
    leaf_indexes: Vec<usize>,
    leaf_labels: Vec<String>,
}

impl LegIndexes {
    fn resolve(plan: &FederatedPlan, fact: &RowSet, lookup: &RowSet) -> Result<Self, FederatedFailure> {
        let mut fact_index: BTreeMap<String, usize> = BTreeMap::new();
        let mut lookup_columns: Vec<(String, usize)> = Vec::new();
        for key in &plan.keys {
            match key.side() {
                LegSide::Fact => {
                    fact_index.insert(String::from(key.label()), column_index(fact, key.label(), "fact")?);
                }
                LegSide::Lookup => lookup_columns.push((String::from(key.label()), column_index(lookup, key.label(), "lookup")?)),
            }
        }
        let lookup_pos: BTreeMap<String, usize> = lookup_columns
            .iter()
            .enumerate()
            .map(|(index, (label, _))| (label.clone(), index))
            .collect();
        let leaf_labels = labels(&plan.federation, &plan.metric);
        let leaf_indexes: Vec<usize> = leaf_labels
            .iter()
            .map(|label| column_index(fact, label, "fact"))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            fact_join: column_index(fact, &plan.fact_join, "fact")?,
            lookup_join: column_index(lookup, &plan.lookup_join, "lookup")?,
            bucket: column_index(fact, plan.bucket.label(), "fact")?,
            fact_index,
            lookup_columns,
            lookup_pos,
            leaf_indexes,
            leaf_labels,
        })
    }

    /// One answer key's cell, read from whichever leg's result owns it.
    fn read_cell(&self, key: &AnswerKey, fact_row: &[Value], remote: &[Value]) -> Result<Value, FederatedFailure> {
        match key.side() {
            LegSide::Fact => {
                let index = self
                    .fact_index
                    .get(key.label())
                    .copied()
                    .ok_or_else(|| FederatedFailure::MissingColumn {
                        side: "fact",
                        label: String::from(key.label()),
                    })?;
                cell(fact_row, index, "fact", key.label()).cloned()
            }
            LegSide::Lookup => {
                let index = self
                    .lookup_pos
                    .get(key.label())
                    .copied()
                    .ok_or_else(|| FederatedFailure::MissingColumn {
                        side: "lookup",
                        label: String::from(key.label()),
                    })?;
                cell(remote, index, "lookup", key.label()).cloned()
            }
        }
    }
}

/// Every fact row that carries a link value, keyed by that value.
///
/// A `Null` link never joins and a real link is refused by the float-key rule; both fall through.
fn facts_by_link<'a>(fact: &'a RowSet, fact_join: usize) -> Result<FactByLink<'a>, FederatedFailure> {
    let mut by_link: FactByLink<'a> = BTreeMap::new();
    for row in fact.rows() {
        let Some(link) = row.get(fact_join) else {
            continue;
        };
        let Some(key) = link_key(link)? else {
            continue;
        };
        by_link.entry(key).or_default().push(row);
    }
    Ok(by_link)
}

/// The remote keys each link value maps to, refusing a link with more than one lookup row.
///
/// More than one row for one link would double every measure, so it is refused rather than certified.
fn lookups_by_link(
    lookup: &RowSet,
    lookup_join: usize,
    lookup_columns: &[(String, usize)],
) -> Result<RemoteByLink, FederatedFailure> {
    let mut by_link: RemoteByLink = BTreeMap::new();
    for row in lookup.rows() {
        let Some(link) = row.get(lookup_join) else {
            continue;
        };
        let Some(key) = link_key(link)? else {
            continue;
        };
        let remote: Option<Vec<Value>> = lookup_columns
            .iter()
            .map(|(label, index)| cell(row, *index, "lookup", label).cloned().ok())
            .collect();
        let Some(remote) = remote else {
            continue;
        };
        let entry = by_link.entry(key.clone()).or_default();
        if !entry.is_empty() {
            return Err(FederatedFailure::AmbiguousLink { key });
        }
        entry.push(remote);
    }
    Ok(by_link)
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the accessor impl and the combiner impl span one type, kept apart for readability"
)]
impl FederatedPlan {
    /// Turns one result per leg into one answer's rows.
    ///
    /// The fact and lookup results are joined on the recorded link column, grouped by the answer's
    /// keys - in the order the question asked them, matching the mono path - and the bucket,
    /// re-aggregated by each leaf's own [`Carried::combine`], and only then divided through the
    /// [`Above`] tree.
    ///
    /// `byte_budget` is the working-set ceiling `docs/adr/0009` applies at the conversion boundary:
    /// the answer materialised here is counted as it is built, and a question that would cross it is
    /// refused as [`FederatedFailure::ResourcesExhausted`] rather than truncated, so a caller never
    /// reads a result that stopped early as a result that returned.
    pub fn combine(&self, fact: &RowSet, lookup: &RowSet, byte_budget: u64) -> Result<RowSet, FederatedFailure> {
        distinct_columns(fact, "fact")?;
        distinct_columns(lookup, "lookup")?;

        let indexes = LegIndexes::resolve(self, fact, lookup)?;
        let fact_by_link = facts_by_link(fact, indexes.fact_join)?;
        let lookup_by_link = lookups_by_link(lookup, indexes.lookup_join, &indexes.lookup_columns)?;

        let mut budget = ByteBudget::new(byte_budget);
        let column_bytes: u64 = self.keys.iter().map(|key| key.label().len() as u64).sum::<u64>()
            + self.bucket.label().len() as u64
            + self.measure_label.len() as u64;
        budget.add(column_bytes, byte_budget)?;

        let groups = self.group_facts(&fact_by_link, &lookup_by_link, &indexes, &mut budget, byte_budget)?;

        // Re-aggregate each leaf across its group, then walk the divide tree.
        let mut rows: Vec<Vec<Value>> = Vec::with_capacity(groups.len());
        for group in groups.into_values() {
            let aggregated = leaf_values(&self.federation, &group.leaves, &self.metric)?;
            let measure = apply_above(self.federation.above(), &aggregated, &mut 0, &self.metric)?;
            let measure_bytes = value_bytes(&measure);
            let mut row = group.cells;
            row.push(measure);
            budget.add(measure_bytes, byte_budget)?;
            rows.push(row);
        }

        let mut columns: Vec<String> = self.keys.iter().map(|key| String::from(key.label())).collect();
        columns.push(String::from(self.bucket.label()));
        columns.push(self.measure_label.clone());

        // Deterministic order. The answer's rows are ordered by their key cells **typed** - a null
        // before a number, integers by value, reals by value - and not by their rendered text, so an
        // integer key `10` orders after `9` the way the mono path's ORDER BY would, rather than
        // before it because `"10" < "9"`.
        let key_width = columns.len().saturating_sub(1);
        rows.sort_by(|a, b| {
            for (a_cell, b_cell) in a.iter().zip(b).take(key_width) {
                let order = compare_cells(a_cell, b_cell);
                if order != std::cmp::Ordering::Equal {
                    return order;
                }
            }
            std::cmp::Ordering::Equal
        });

        RowSet::new(columns, rows).map_err(|_malformed| FederatedFailure::MalformedRow { side: "answer" })
    }

    /// Project every joined fact row into answer groups, counting the working set as it goes.
    ///
    /// This is the join and the grouping, kept out of [`FederatedPlan::combine`] so one function does
    /// not carry both the whole loop and the budget.
    fn group_facts(
        &self,
        fact_by_link: &FactByLink<'_>,
        lookup_by_link: &RemoteByLink,
        indexes: &LegIndexes,
        budget: &mut ByteBudget,
        byte_budget: u64,
    ) -> Result<GroupMap, FederatedFailure> {
        let mut groups: BTreeMap<Vec<String>, Group> = BTreeMap::new();
        for (link_key, fact_rows) in fact_by_link {
            let remote_rows: Vec<Vec<Value>> = match lookup_by_link.get(link_key) {
                Some(rows) => rows.clone(),
                None if self.include_unmatched => vec![vec![Value::Null; indexes.lookup_columns.len()]],
                None => continue,
            };
            for fact_row in fact_rows {
                let bucket_cell = cell(fact_row, indexes.bucket, "fact", self.bucket.label())?.clone();
                let leaves: Vec<Value> = indexes
                    .leaf_indexes
                    .iter()
                    .zip(&indexes.leaf_labels)
                    .map(|(&index, label)| cell(fact_row, index, "fact", label).cloned().unwrap_or(Value::Null))
                    .collect();
                for remote in &remote_rows {
                    let mut cells = Vec::with_capacity(self.keys.len() + 1);
                    for key in &self.keys {
                        cells.push(indexes.read_cell(key, fact_row, remote)?);
                    }
                    cells.push(bucket_cell.clone());
                    budget.add(cells.iter().map(value_bytes).sum(), byte_budget)?;
                    budget.add(leaves.iter().map(value_bytes).sum(), byte_budget)?;
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
        Ok(groups)
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

/// The fact rows that share one link value, addressed by reference so the join clones nothing.
///
/// A link value is shared by several fact rows (one per local-key group), each still owned by the
/// fact result this function borrows for its own duration.
type FactByLink<'a> = BTreeMap<String, Vec<&'a Vec<Value>>>;

/// One final answer's group: its key cells (as they should appear in the answer) and every fact
/// row's leaf values that joined to it.
struct Group {
    cells: Vec<Value>,
    leaves: Vec<Vec<Value>>,
}

/// All of a combine's groups, keyed by the typed form of their key cells.
type GroupMap = BTreeMap<Vec<String>, Group>;

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

/// One cell by column index, with a named error for a read that cannot happen.
///
/// `column_index` validates that the column exists, and [`RowSet::new`] guarantees every row has as
/// many cells as columns, so this is always `Some`; the `MissingColumn` fallback is how a typed arm
/// stands in for the case the type has already ruled out, rather than an `index` panicking.
fn cell<'a>(row: &'a [Value], index: usize, side: &'static str, label: &str) -> Result<&'a Value, FederatedFailure> {
    row.get(index).ok_or_else(|| FederatedFailure::MissingColumn {
        side,
        label: String::from(label),
    })
}

/// `docs/adr/0009`'s byte budget over the answer `combine` materialises.
///
/// A running total with a ceiling: the budget never goes backward, and an overflow of the total (or
/// a total that passes the ceiling) is refused as [`FederatedFailure::ResourcesExhausted`] rather
/// than saturated. The ceiling is carried to the error so a caller can report the number that fired.
struct ByteBudget {
    /// What a row set may not exceed.
    ceiling: u64,
    /// The bytes counted so far.
    used: u64,
}

impl ByteBudget {
    const fn new(ceiling: u64) -> Self {
        Self { ceiling, used: 0 }
    }

    const fn add(&mut self, bytes: u64, ceiling: u64) -> Result<(), FederatedFailure> {
        self.used = match self.used.checked_add(bytes) {
            Some(total) => total,
            None => return Err(FederatedFailure::ResourcesExhausted { ceiling_bytes: ceiling }),
        };
        if self.used > self.ceiling {
            return Err(FederatedFailure::ResourcesExhausted { ceiling_bytes: ceiling });
        }
        Ok(())
    }
}

/// A conservative estimate of one cell's size in memory, for the working-set budget.
const fn value_bytes(value: &Value) -> u64 {
    match value {
        Value::Null => 1,
        Value::Integer(_) | Value::Real(_) => 8,
        Value::Text(v) => v.len() as u64,
    }
}

/// A total order over key cells, matching the mono path's `ORDER BY` rather than rendered text.
///
/// Nulls sort first, then integers by value, then reals by value, then text lexicographically, so a
/// numeric column is ordered numerically (`9` before `10`) and not by its string form (`"10"` before
/// `"9"`). Cells of different scalar types never compare equal.
fn compare_cells(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    const fn rank(value: &Value) -> u8 {
        match value {
            Value::Null => 0,
            Value::Integer(_) => 1,
            Value::Real(_) => 2,
            Value::Text(_) => 3,
        }
    }
    let order = rank(a).cmp(&rank(b));
    if order != Equal {
        return order;
    }
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
        (Value::Real(x), Value::Real(y)) => x.get().total_cmp(&y.get()),
        (Value::Text(x), Value::Text(y)) => x.cmp(y),
        // The sole same-rank pair not caught above is Null/Null, and different ranks returned early.
        _ => Equal,
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
/// non-null value contributes null; a cell that is not a number, a column that is not one kind of
/// number, an overflow, or a non-finite total is a refusal, never a silent zero or null.
///
/// **The column is read before the aggregate is applied**, so each reduction below sees one numeric
/// type - see [`LeafColumn`], which is where that decision and its reason live.
fn aggregate<'a>(
    aggregate: Aggregate,
    values: impl Iterator<Item = &'a Value>,
    metric: &MetricName,
) -> Result<Value, FederatedFailure> {
    let reduction = Reduction::of(aggregate).ok_or(FederatedFailure::UnsupportedAggregate { aggregate })?;
    let Some(column) = LeafColumn::parse(values, aggregate)? else {
        return Ok(Value::Null);
    };
    match reduction {
        Reduction::Total => column.total(metric),
        Reduction::Least => Ok(column.extreme(Ordering::Less)),
        Reduction::Greatest => Ok(column.extreme(Ordering::Greater)),
    }
}

/// What a leaf column is reduced to, named rather than left as the aggregate it came from.
///
/// [`Reduction::of`] is the one definition of which aggregates the combine re-aggregates with, and
/// [`FederatedPlan::new`] is where a leaf naming any other one is refused - so which reduction a
/// column gets is settled by the plan, before a single cell of it is read.
#[derive(Clone, Copy)]
enum Reduction {
    /// [`Aggregate::Sum`], which a `Count` leaf also re-aggregates with.
    Total,
    /// [`Aggregate::Min`].
    Least,
    /// [`Aggregate::Max`].
    Greatest,
}

impl Reduction {
    /// The reduction an aggregate re-aggregates with, or `None` for one that has none.
    ///
    /// Named arms rather than a wildcard, so a seventh [`Aggregate`] has to answer here.
    const fn of(aggregate: Aggregate) -> Option<Self> {
        match aggregate {
            Aggregate::Sum => Some(Self::Total),
            Aggregate::Min => Some(Self::Least),
            Aggregate::Max => Some(Self::Greatest),
            Aggregate::Count | Aggregate::Avg | Aggregate::CountDistinct => None,
        }
    }
}

/// One leaf column's non-null cells, once their single numeric type is established.
///
/// **Reading the whole column before any aggregate touches it is what makes the arithmetic exact
/// rather than checked**, and it removes two wrong numbers at once: `Sum` accumulated an integer
/// subtotal and a real one and returned only the real one, and `Min`/`Max` compared every cell as an
/// `f64`, so two integers a data system tells apart read as equal above `2^53` and the answer was
/// whichever arrived first. Now each aggregate sees one type and nothing widens an `i64` to add it or
/// to compare it.
///
/// A cell that is no kind of number is refused for **every** aggregate rather than inside two of
/// them: a lone `Text` cell used to be accepted as its own minimum without being read as a number,
/// and `DuckDB` returns a `DECIMAL` money column as one. A column carrying both numeric types is
/// [`FederatedFailure::MixedNumericLeaf`], which carries that reasoning.
///
/// Both variants are non-empty by construction: [`parse`](LeafColumn::parse) answers `None` for a
/// column with no non-null cell, because a group contributing nothing is a null and not a zero.
enum LeafColumn {
    /// Every non-null cell was a [`Value::Integer`].
    Integers(Vec<i64>),
    /// Every non-null cell was a [`Value::Real`], and so is already finite.
    Reals(Vec<Real>),
}

impl LeafColumn {
    /// One leaf column's cells, `None` for a column of nulls, or the reason it is neither.
    fn parse<'a>(values: impl Iterator<Item = &'a Value>, aggregate: Aggregate) -> Result<Option<Self>, FederatedFailure> {
        let mut integers: Vec<i64> = Vec::new();
        let mut reals: Vec<Real> = Vec::new();
        for value in values {
            match *value {
                Value::Null => {}
                Value::Integer(cell) => integers.push(cell),
                Value::Real(cell) => reals.push(cell),
                Value::Text(_) => {
                    return Err(FederatedFailure::NonNumericLeaf {
                        aggregate,
                        value: value.clone(),
                    });
                }
            }
        }
        match (integers.is_empty(), reals.is_empty()) {
            (false, true) => Ok(Some(Self::Integers(integers))),
            (true, false) => Ok(Some(Self::Reals(reals))),
            (true, true) => Ok(None),
            (false, false) => Err(FederatedFailure::MixedNumericLeaf { aggregate }),
        }
    }

    /// The column's total, in the column's own type.
    ///
    /// An integer column totals as `i64` and overflow is a refusal; a real column totals as `f64`,
    /// which is the float addition the mono path's own `SUM` performs, and a total that leaves the
    /// finite range is a refusal because [`Real`] cannot hold it.
    #[expect(
        clippy::float_arithmetic,
        reason = "the re-aggregation of a real-valued leg column sums real numbers by design"
    )]
    fn total(&self, metric: &MetricName) -> Result<Value, FederatedFailure> {
        match *self {
            Self::Integers(ref cells) => cells
                .iter()
                .try_fold(0_i64, |total, cell| total.checked_add(*cell))
                .map(Value::Integer)
                .ok_or(FederatedFailure::Overflow {
                    aggregate: Aggregate::Sum,
                }),
            Self::Reals(ref cells) => Real::parse(cells.iter().fold(0.0_f64, |total, cell| total + cell.get()))
                .map(Value::Real)
                .map_err(|_not_finite| FederatedFailure::NonFinite { metric: metric.clone() }),
        }
    }

    /// The column's least or greatest cell, in the column's own type.
    ///
    /// `wanted` is the ordering a cell must have against the incumbent to replace it: [`Ordering::Less`]
    /// for a minimum, [`Ordering::Greater`] for a maximum. Both comparisons are exact - an `i64` against
    /// an `i64`, and `total_cmp` over reals that [`Real`] has already established are finite.
    ///
    /// `reduce` answers `None` only for an empty column, which [`parse`](LeafColumn::parse) answers
    /// `None` for instead - so the `Value::Null` below is unreachable rather than a case.
    fn extreme(&self, wanted: Ordering) -> Value {
        match *self {
            Self::Integers(ref cells) => cells
                .iter()
                .copied()
                .reduce(|best, cell| if cell.cmp(&best) == wanted { cell } else { best })
                .map_or(Value::Null, Value::Integer),
            Self::Reals(ref cells) => cells
                .iter()
                .copied()
                .reduce(|best, cell| {
                    if cell.get().total_cmp(&best.get()) == wanted {
                        cell
                    } else {
                        best
                    }
                })
                .map_or(Value::Null, Value::Real),
        }
    }
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

/// A numeric cell as `f64`, or `None` for a cell no ratio can be taken over.
///
/// [`expect`](macro@expect)-bounded: casting a wide integer to `f64` loses precision above `2^53`,
/// which is accepted **here and only here** because a ratio over leg totals is inherently
/// floating-point and [`divide`] is the one caller. It is not accepted for a total or a comparison -
/// see [`FederatedFailure::MixedNumericLeaf`] for the widening this path refuses instead.
///
/// Every variant is named rather than left to a wildcard, so a fifth [`Value`] has to answer here.
/// [`Value::Text`] is one of the two `None`s and is unreachable through [`apply_above`]: every value
/// it reads came from [`aggregate`], which refuses a text cell as [`FederatedFailure::NonNumericLeaf`].
#[expect(clippy::cast_precision_loss, reason = "a division reads leg totals as f64 by design")]
const fn to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(cell) => Some(*cell as f64),
        Value::Real(cell) => Some(cell.get()),
        Value::Null | Value::Text(_) => None,
    }
}

#[cfg(test)]
mod tests;

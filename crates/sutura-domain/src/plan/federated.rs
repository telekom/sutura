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
//! **Those labels live in a namespace a question cannot reach, which is [`label`]'s job.** A leg's
//! result carries public dimension labels beside the internal ones, so an internal label spelled as
//! an identifier is a label a legal dimension name can collide with - reproduced. [`InternalLabel`]
//! is the type that cannot be spelled by one.
//!
//! **The division cannot happen in a leg, and [`combine`](FederatedPlan::combine) is where it
//! happens instead.** The [`Above`](crate::federation::Above) tree already carries the only
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

/// Everything above the legs: how a leaf column is re-aggregated, and the division that follows.
///
/// Split out when this file reached the unexemptable 1000-line gate, along the seam the module
/// already had rather than wherever the counter fell. It declares no test module of its own and
/// took none with it - every assertion this module ever made is still in `tests.rs` - because a
/// test module declared from a file `test-causality` reverts is never compiled, and the proof it
/// then reports is vacuous.
mod reaggregate;

/// The reserved label namespace, and the one function that assigns it.
///
/// Its own module because it is what the splitter, the combiner and the leg goldens all read the
/// spelling from, and because a namespace is a thing to reason about on its own. It declares no test
/// module: a test module declared from a file `test-causality` reverts is never compiled, and the
/// proof it then reports is vacuous - every assertion about it is in `tests.rs`.
pub mod label;

use std::collections::BTreeMap;

use crate::federation::Federation;
use crate::model::{Aggregate, MetricName, SourceName};
use crate::plan::PlanBucket;
use crate::plan::ResultLabel;
use crate::plan::leg::LegPlan;
use crate::warehouse::{RowSet, Value};

pub use label::{InternalLabel, labels};
use reaggregate::{Leaves, reaggregates};

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
    measure_label: ResultLabel,
    bucket: PlanBucket,
    /// The metric's own share of the question: the same-source rows and leaves.
    fact: LegPlan,
    /// The second data system's share: the remote dimensions the answer groups by.
    lookup: LegPlan,
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
    label: ResultLabel,
}

impl AnswerKey {
    /// A key read from the fact leg's result, under `label`.
    #[inline]
    pub const fn fact(label: ResultLabel) -> Self {
        Self {
            side: LegSide::Fact,
            label,
        }
    }

    /// A key read from the lookup leg's result, under `label`.
    #[inline]
    pub const fn lookup(label: ResultLabel) -> Self {
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

    /// The text this key carries in its leg's result.
    #[inline]
    pub fn label(&self) -> &str {
        self.label.as_str()
    }
}

impl FederatedPlan {
    /// Constructs a federated plan from its two legs and the answer's key order.
    ///
    /// A `Result` constructor is this workspace's convention for a value with an invariant: a plan
    /// that is not a fact leg beside a lookup leg, or that names one data system on both legs, is not
    /// a plan and cannot be built.
    ///
    /// **There is no link-label parameter, and that is the F2 fix's structural half.** The label the
    /// legs are joined under used to be two `String` arguments, and the splitter filled both with the
    /// physical remote join column's text - which is a legal dimension name, so a legal question
    /// produced two fact columns under one label. It is now [`InternalLabel::Link`], a constant of
    /// the scheme rather than data on the plan: there is no argument for a caller to spell, nothing
    /// for the two legs to disagree about, and the constructor requires both legs to project it.
    // The constructor takes the shape of the question as the splitter decided it; a bundle of named
    // fields is the alternative, and a `Vec` would let a caller omit or duplicate a leg - the two
    // instantiations it exists to forbid.
    pub fn new(
        metric: MetricName,
        measure_label: ResultLabel,
        bucket: PlanBucket,
        fact: LegPlan,
        lookup: LegPlan,
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
                LegSide::Fact => leg_has_key(&fact, key.label()).map_err(|label| FederatedPlanError::KeyNotOnLeg {
                    side: LegSide::Fact,
                    label: String::from(label),
                })?,
                LegSide::Lookup => leg_has_key(&lookup, key.label()).map_err(|label| FederatedPlanError::KeyNotOnLeg {
                    side: LegSide::Lookup,
                    label: String::from(label),
                })?,
            }
        }
        // The link column, which is not an answer key and used to be checked by nothing: the
        // combiner looked it up in each leg's result and reported a missing column when a leg had
        // not projected it. Asked here instead, so a plan that cannot be joined does not exist.
        let link = InternalLabel::Link.label();
        leg_has_key(&fact, &link).map_err(|label| FederatedPlanError::KeyNotOnLeg {
            side: LegSide::Fact,
            label: String::from(label),
        })?;
        leg_has_key(&lookup, &link).map_err(|label| FederatedPlanError::KeyNotOnLeg {
            side: LegSide::Lookup,
            label: String::from(label),
        })?;
        for leaf in federation.carried() {
            let aggregate = leaf.combine();
            if !reaggregates(aggregate) {
                return Err(FederatedPlanError::LeafDoesNotReaggregate { aggregate });
            }
        }
        Ok(Self {
            metric,
            measure_label,
            bucket,
            fact,
            lookup,
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
    /// The one path [`FederatedPlan::new`] closes is a carried leaf naming an aggregate
    /// `reaggregate::reaggregates` answers `false` for - it refuses such a federation before any
    /// leg runs, so no plan that constructor built carries this value. **The limit: nothing else
    /// closes it, and construction is not restricted to this module.** `FederatedFailure` is `pub`
    /// and re-exported, and the application's federated execution already writes a sibling
    /// variant's literal from outside the crate. So any caller can build this value directly; it
    /// stays a refusal rather than becoming a panic because a value that claims a re-aggregation
    /// which does not exist would answer wrongly, not because the type seals the variant.
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
    /// One all-null remote row, as the one-element slice a LEFT-retained fact row is projected
    /// against - the stand-in for a lookup row that is not there.
    ///
    /// **Resolved here rather than built in [`FederatedPlan::project`], which runs once per distinct
    /// link value.** Only the unmatched arm reads it, so building it there allocated one throwaway
    /// `Vec<Value>` per matched link value and one per INNER miss as well. `docs/adr/0009`'s
    /// working-set ceiling makes link cardinality the axis that matters, which is what makes an
    /// allocation per link value a correctness question here rather than a style one.
    unmatched: [Vec<Value>; 1],
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
        let leaf_labels: Vec<String> = labels(&plan.federation).into_iter().map(InternalLabel::label).collect();
        let leaf_indexes: Vec<usize> = leaf_labels
            .iter()
            .map(|label| column_index(fact, label, "fact"))
            .collect::<Result<_, _>>()?;
        // Both legs project the link under the one internal label, so this is the scheme's constant
        // rather than a field either leg could have spelled differently.
        let link = InternalLabel::Link.label();
        let unmatched = [vec![Value::Null; lookup_columns.len()]];
        Ok(Self {
            fact_join: column_index(fact, &link, "fact")?,
            lookup_join: column_index(lookup, &link, "lookup")?,
            bucket: column_index(fact, plan.bucket.label(), "fact")?,
            fact_index,
            lookup_columns,
            lookup_pos,
            leaf_indexes,
            leaf_labels,
            unmatched,
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

/// Every fact row, split by whether its link value can match a lookup row at all.
///
/// A `Null` link matches nothing - `NULL = NULL` is not true in SQL - so a null-keyed fact row is
/// **unmatched by construction** rather than unmatched by lookup, and the join kind decides it
/// exactly as it decides an unmatched non-null key: retained with null remote keys under LEFT,
/// dropped under INNER. Dropping it here instead lost the row and its measure under BOTH kinds,
/// which is the defect this shape exists so that no caller can reintroduce - a row that reaches
/// neither half does not exist. A real link is refused by the float-key rule before either.
fn fact_rows(fact: &RowSet, fact_join: usize) -> Result<FactRows<'_>, FederatedFailure> {
    let mut rows = FactRows {
        linked: BTreeMap::new(),
        unlinkable: Vec::new(),
    };
    for row in fact.rows() {
        let Some(link) = row.get(fact_join) else {
            continue;
        };
        match link_key(link)? {
            Some(key) => rows.linked.entry(key).or_default().push(row),
            None => rows.unlinkable.push(row),
        }
    }
    Ok(rows)
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
    /// re-aggregated by each leaf's own [`Carried::combine`](crate::federation::Carried::combine),
    /// and only then divided through the [`Above`](crate::federation::Above) tree. Those last two
    /// steps belong to `reaggregate`, reached as `Leaves::of` and `Leaves::measure`; the join, the
    /// grouping and the budget are this file's.
    ///
    /// `byte_budget` is the working-set ceiling `docs/adr/0009` applies at the conversion boundary:
    /// the answer materialised here is counted as it is built, and a question that would cross it is
    /// refused as [`FederatedFailure::ResourcesExhausted`] rather than truncated, so a caller never
    /// reads a result that stopped early as a result that returned.
    pub fn combine(&self, fact: &RowSet, lookup: &RowSet, byte_budget: u64) -> Result<RowSet, FederatedFailure> {
        distinct_columns(fact, "fact")?;
        distinct_columns(lookup, "lookup")?;

        let indexes = LegIndexes::resolve(self, fact, lookup)?;
        let facts = fact_rows(fact, indexes.fact_join)?;
        let lookup_by_link = lookups_by_link(lookup, indexes.lookup_join, &indexes.lookup_columns)?;

        let mut budget = ByteBudget::new(byte_budget);
        let column_bytes: u64 = self.keys.iter().map(|key| key.label().len() as u64).sum::<u64>()
            + self.bucket.label().len() as u64
            + self.measure_label.as_str().len() as u64;
        budget.add(column_bytes, byte_budget)?;

        let groups = self.group_facts(&facts, &lookup_by_link, &indexes, &mut budget, byte_budget)?;

        // Re-aggregate each leaf across its group, then walk the divide tree.
        let mut rows: Vec<Vec<Value>> = Vec::with_capacity(groups.len());
        for group in groups.into_values() {
            let leaves = Leaves::of(&self.federation, &group.leaves, &self.metric)?;
            let measure = leaves.measure(&self.metric)?;
            let measure_bytes = value_bytes(&measure);
            let mut row = group.cells;
            row.push(measure);
            budget.add(measure_bytes, byte_budget)?;
            rows.push(row);
        }

        let mut columns: Vec<String> = self.keys.iter().map(|key| String::from(key.label())).collect();
        columns.push(String::from(self.bucket.label()));
        columns.push(String::from(self.measure_label.as_str()));

        // The mono path's ordered-result contract, applied above the legs: ascending by each key
        // cell **typed**, nulls last, in key order. Not by rendered text, so an integer key `10`
        // orders after `9` rather than before it because `"10" < "9"`. `compare_cells` carries why
        // the null placement is a contract, where the other half of it is written, and the half it
        // does NOT reach - a text key is compared by bytes and the mono path's text order is the
        // serving source's collation.
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
    ///
    /// **Both halves of [`FactRows`] are walked, and the second is why.** A keyed row with no lookup
    /// row and a null-keyed row that could never have one are the same unmatched outcome, so both go
    /// through [`FederatedPlan::project`] with no remote rows and `include_unmatched` decides them
    /// together. A null-keyed row is never MATCHED to a null-linked lookup row: `lookups_by_link`
    /// keys nothing under a null, so the lookup side of that pair does not exist to be found.
    fn group_facts(
        &self,
        facts: &FactRows<'_>,
        lookup_by_link: &RemoteByLink,
        indexes: &LegIndexes,
        budget: &mut ByteBudget,
        byte_budget: u64,
    ) -> Result<GroupMap, FederatedFailure> {
        let mut groups: GroupMap = BTreeMap::new();
        for (link_key, rows) in &facts.linked {
            self.project(rows, lookup_by_link.get(link_key), indexes, budget, byte_budget, &mut groups)?;
        }
        self.project(&facts.unlinkable, None, indexes, budget, byte_budget, &mut groups)?;
        Ok(groups)
    }

    /// Project one link value's fact rows against the remote rows they joined to.
    ///
    /// `matched` is `None` for a fact row with no lookup row, whether because its key found none or
    /// because it had no key: LEFT projects it once against null remote keys, INNER drops it. The
    /// matched rows are BORROWED - the join copies a remote row into an answer group and nowhere
    /// else, which is the only place `docs/adr/0009`'s working set may grow.
    fn project(
        &self,
        fact_rows: &[&Vec<Value>],
        matched: Option<&RemoteRows>,
        indexes: &LegIndexes,
        budget: &mut ByteBudget,
        byte_budget: u64,
        groups: &mut GroupMap,
    ) -> Result<(), FederatedFailure> {
        let remote_rows: &[Vec<Value>] = match matched {
            Some(rows) => rows,
            // Built once by `LegIndexes::resolve`, not here: this function runs once per distinct
            // link value and the other two arms never read it. See the field.
            None if self.include_unmatched => &indexes.unmatched,
            None => return Ok(()),
        };
        for fact_row in fact_rows {
            let bucket_cell = cell(fact_row, indexes.bucket, "fact", self.bucket.label())?.clone();
            let leaves: Vec<Value> = indexes
                .leaf_indexes
                .iter()
                .zip(&indexes.leaf_labels)
                .map(|(&index, label)| cell(fact_row, index, "fact", label).cloned().unwrap_or(Value::Null))
                .collect();
            for remote in remote_rows {
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
        Ok(())
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

/// The remote-key rows one link value maps to.
///
/// A named alias because it appears in a signature the complexity threshold in `clippy.toml`
/// rejects spelled out, and naming it says which of the two `Vec`s is the row.
type RemoteRows = Vec<Vec<Value>>;

/// One link value's remote-key rows.
type RemoteByLink = BTreeMap<String, RemoteRows>;

/// The fact rows that share one link value, addressed by reference so the join clones nothing.
///
/// A link value is shared by several fact rows (one per local-key group), each still owned by the
/// fact result this function borrows for its own duration.
type FactByLink<'a> = BTreeMap<String, Vec<&'a Vec<Value>>>;

/// Every fact row of a combine, in the two groups the join treats differently.
///
/// Two fields rather than one map, because *no lookup row for this key* and *no key at all* are
/// the same OUTCOME reached two ways, and a map keyed by link value cannot hold the second. See
/// [`fact_rows`] for why the second is unmatched rather than absent.
struct FactRows<'a> {
    /// Rows whose link value is a key, grouped by it.
    linked: FactByLink<'a>,
    /// Rows whose link value is null, so no key exists to look up.
    unlinkable: Vec<&'a Vec<Value>>,
}

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

/// A total order over key cells: the mono path's null placement and its numeric order, never its
/// rendered text.
///
/// **What "matching the mono path" reaches, stated before the argument for it.** The null placement
/// and the by-value numeric order are the contract, and both are asserted. **Text is not:** this
/// compares `&str` by bytes, while the mono path's text order is whatever **collation** the serving
/// data system applies - `docs/adr/0012` records that collation as *unstated by the plan* and
/// *differing per system*, which is why its own conformance packs re-sort text by bytes rather than
/// trusting a source's locale. So for a text key on a target whose collation is not byte order -
/// `Postgres` under a non-`C` locale orders `Business` after `business`, byte order puts it before -
/// one certified metric still comes back in one order from one data system and another from two.
/// That is the class of defect the null half of this comparator closes, surviving for text keys.
/// `telekom/sutura#92` scoped itself out of it explicitly - its *not in scope* is *"ordering
/// stability where the question itself does not determine an order. This is only about null placement
/// within an order the plan already asks for."* - so it is a limit rather than a regression, and
/// closing it means declaring a text collation per dialect the way `Dialect::identifier_case` is
/// declared.
///
/// **`ASC NULLS LAST`, which is the whole of the ordered-result contract and not this file's
/// choice.** A whole-answer plan emits `ORDER BY <key> ASC NULLS LAST` -
/// `sutura_sql::generate::ordered_nulls_last`, which `telekom/sutura#92` decided after a live run
/// found the four dialects disagreeing about null placement, and which makes every target converge
/// on the engine's own order. This comparator ranked a null FIRST, so one certified metric came back
/// in one order from one data system and in another order from two, with no golden able to see it -
/// a golden pins statement text, and this path emits none. The placement is stated in both places
/// for the same reason it is stated in the SQL: a default is not a contract.
///
/// Integers order by value, then reals by value, then text by bytes, and a null after all of them -
/// so a numeric column is ordered numerically (`9` before `10`) and not by its string form
/// (`"10"` before `"9"`). Cells of different scalar types never compare equal. A result column in a
/// data system has one logical type, so the cross-type ranks decide nothing an `ORDER BY` decides;
/// what they buy is a TOTAL order, which is what makes the sort deterministic for a column
/// [`RowSet`] permits to be mixed.
fn compare_cells(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    const fn rank(value: &Value) -> u8 {
        match value {
            Value::Integer(_) => 0,
            Value::Real(_) => 1,
            Value::Text(_) => 2,
            // Last, and the one rank that is a contract rather than a tie-break. See above.
            Value::Null => 3,
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

#[cfg(test)]
mod tests;

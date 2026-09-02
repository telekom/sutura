//! One source's share of a federated question, and the only thing the port can be handed.
//!
//! **The shapes, their closure, and nothing that produces or executes one.** There is no splitter
//! and no combiner in this workspace, so no production code constructs a [`LegPlan`]: what is here
//! is the vocabulary a splitter will emit and `sutura-sql` already renders, pinned per dialect
//! before anything runs it. `.agents/skills/sutura/query-surface` carries that state, and this
//! module says it rather than leaving it to be discovered.
//! `docs/adr/0007-federating-across-different-data-systems.md` decides the shape and
//! `docs/adr/0009-the-plan-from-one-source-to-many.md` Decision 2 decides what a leg may compute.
//!
//! **Why a leg is not a [`QueryPlan`], which is the whole reason this module exists.** A
//! `QueryPlan` requires a [`PlanBucket`], a [`PlanMeasure`](crate::plan::PlanMeasure) and a measure
//! label, and a dimension lookup has none of the three: a dimension table has no time column, no
//! measure and no metric name. Reaching for `Option` on those three fields would make *a dimension
//! leg carrying a time range* constructible, which is the checked shape where this one is the
//! unrepresentable one.
//!
//! And the measure is worse than absent. [`PlanMeasure`](crate::plan::PlanMeasure) has exactly two
//! variants and the only one carrying two terms is the one that **divides** them -
//! `sutura_sql::generate` renders a `Ratio` as `CAST(numerator AS DOUBLE) / NULLIF(denominator, 0)`.
//! So a decomposed `Avg` travelling as a sum beside a count, and a ratio travelling as an undivided
//! numerator and denominator, are not expressible by `PlanMeasure` at all - and the division per leg
//! that 0009's Decision 2 forbids is exactly what it *would* express. [`LegTerm`] is the answer: a
//! [`PlanTerm`] and a label, with no shape that divides and no field a
//! [`ZeroDenominator`](crate::measure::ZeroDenominator) fits in, at any depth. That is the same
//! argument [`crate::federation::Carried`] makes one level up, and the two are deliberately built
//! the same way.
//!
//! **Two variants and not three.** There are three shapes a leg can be - an aggregate fact leg, a
//! distinct-key fact leg, and a dimension lookup - and only one of the two axes they split along is
//! worth a variant. Splitting by *which model is read* moves four fields together: a dimension
//! model has no time column, so no bucket and no range; it is not the metric's own model, so no
//! metric name; and a one-hop dimension join does not start from it, so no joins. Splitting by
//! *whether an aggregate is applied* moves one bit, and both halves render identically - project the
//! key list, group by the key list. So the distinct-key leg is a [`LegPlan::Fact`] whose `terms` are
//! empty, and it needs no variant of its own.
//!
//! **No leg carries a row cap.** A leg is not an answer, and [`MAX_ROWS`](crate::plan::MAX_ROWS)
//! caps one answer's rows; `sutura_sql::generate_leg` emits no `LIMIT` for the same reason.

use crate::calendar::TimeRange;
use crate::model::{MetricName, QualifiedTable, SourceName, TableName};
use crate::plan::{PlanBucket, PlanFilter, PlanKey, PlanTerm, QueryPlan, StatementTables};
use crate::warehouse::ParamValue;

/// One number a leg computes, and the label it is projected under.
///
/// **A [`PlanTerm`] and not a [`PlanMeasure`](crate::plan::PlanMeasure), and that is a type rather
/// than a convention.** There is no shape here that divides, so a leg's statement cannot carry a
/// `NULLIF` guard and a per-leg quotient is unrepresentable rather than discouraged. The bug that
/// closes is specific: applied *inside* a leg, `ZeroDenominator::Null` turns a subgroup with a zero
/// denominator into a null, the re-aggregating `SUM` above skips nulls, and that subgroup's
/// numerator is silently dropped from the answer instead of nulling it.
///
/// A leg's terms come off [`crate::federation::Federation::carried`], which is built purely from the
/// term level for the same reason.
///
/// The division a leg cannot express does not compile:
///
/// ```compile_fail
/// use sutura_domain::measure::ZeroDenominator;
/// use sutura_domain::plan::{LegTerm, PlanMeasure, PlanTerm};
///
/// // `LegTerm::new` takes a term, and a ratio is not one.
/// fn _divided(numerator: PlanTerm, denominator: PlanTerm, zero_denominator: ZeroDenominator) -> LegTerm {
///     LegTerm::new(
///         PlanMeasure::Ratio {
///             numerator,
///             denominator,
///             zero_denominator,
///         },
///         String::from("ratio"),
///     )
/// }
/// ```
///
/// And the twin, so a rename cannot make that block pass vacuously: the two halves travel as two
/// terms, and the division happens above every leg.
///
/// ```
/// use sutura_domain::plan::{LegTerm, PlanTerm};
///
/// fn _undivided(numerator: PlanTerm, denominator: PlanTerm) -> Vec<LegTerm> {
///     vec![
///         LegTerm::new(numerator, String::from("numerator")),
///         LegTerm::new(denominator, String::from("denominator")),
///     ]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LegTerm {
    term: PlanTerm,
    label: String,
}

impl LegTerm {
    #[inline]
    pub const fn new(term: PlanTerm, label: String) -> Self {
        Self { term, label }
    }

    #[inline]
    pub const fn term(&self) -> &PlanTerm {
        &self.term
    }

    #[inline]
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// One source's share of a federated question.
///
/// An enum rather than a struct with four `Option`s, which is this repository's *prefer
/// unrepresentable to checked* applied to the one place it buys the most: there are exactly two
/// legal shapes, and the illegal combinations - a measure with no bucket, a time range on a table
/// with no time column - are not constructible.
///
/// A third shape cannot arrive without a rendering arm, because a match over this enum is
/// exhaustive:
///
/// ```compile_fail
/// use sutura_domain::plan::LegPlan;
///
/// // Non-exhaustive: `Lookup` renders differently and has to say so.
/// fn _render(leg: &LegPlan) -> &str {
///     match *leg {
///         LegPlan::Fact { .. } => "fact",
///     }
/// }
/// ```
///
/// ```
/// use sutura_domain::plan::LegPlan;
///
/// fn _render(leg: &LegPlan) -> &str {
///     match *leg {
///         LegPlan::Fact { .. } => "fact",
///         LegPlan::Lookup { .. } => "lookup",
///     }
/// }
/// ```
///
/// A lookup leg has no bucket, no terms, no range and no metric, and that is the type rather than a
/// check somebody runs:
///
/// ```compile_fail
/// use sutura_domain::calendar::TimeRange;
/// use sutura_domain::model::{QualifiedTable, SourceName};
/// use sutura_domain::plan::LegPlan;
///
/// fn _dated(source: SourceName, table: QualifiedTable, range: TimeRange) -> LegPlan {
///     LegPlan::Lookup {
///         source,
///         table,
///         keys: Vec::new(),
///         filters: Vec::new(),
///         params: Vec::new(),
///         range,
///     }
/// }
/// ```
///
/// ```
/// use sutura_domain::model::{QualifiedTable, SourceName};
/// use sutura_domain::plan::LegPlan;
///
/// fn _undated(source: SourceName, table: QualifiedTable) -> LegPlan {
///     LegPlan::Lookup {
///         source,
///         table,
///         keys: Vec::new(),
///         filters: Vec::new(),
///         params: Vec::new(),
///     }
/// }
/// ```
///
/// A fact leg's tables arrive as a checked set, so a producer cannot state a table beside a vector
/// of joins and skip the ambiguity guard - which is exactly what the first producer of a leg did:
///
/// ```compile_fail
/// use sutura_domain::calendar::TimeRange;
/// use sutura_domain::model::{MetricName, QualifiedTable, SourceName};
/// use sutura_domain::plan::{LegPlan, PlanBucket, PlanJoin};
///
/// fn _unchecked(
///     source: SourceName,
///     metric: MetricName,
///     table: QualifiedTable,
///     joins: Vec<PlanJoin>,
///     bucket: PlanBucket,
///     range: TimeRange,
/// ) -> LegPlan {
///     LegPlan::Fact {
///         source,
///         metric,
///         table,
///         joins,
///         bucket,
///         keys: Vec::new(),
///         terms: Vec::new(),
///         filters: Vec::new(),
///         params: Vec::new(),
///         range,
///     }
/// }
/// ```
///
/// ```
/// use sutura_domain::calendar::TimeRange;
/// use sutura_domain::model::{MetricName, QualifiedTable, SourceName};
/// use sutura_domain::plan::{LegPlan, PlanBucket, PlanJoin, StatementTables};
///
/// fn _checked(
///     source: SourceName,
///     metric: MetricName,
///     table: QualifiedTable,
///     joins: Vec<PlanJoin>,
///     bucket: PlanBucket,
///     range: TimeRange,
/// ) -> Result<LegPlan, sutura_domain::plan::AmbiguousTables> {
///     Ok(LegPlan::Fact {
///         source,
///         metric,
///         tables: StatementTables::parse(table, joins)?,
///         bucket,
///         keys: Vec::new(),
///         terms: Vec::new(),
///         filters: Vec::new(),
///         params: Vec::new(),
///         range,
///     })
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegPlan {
    /// Read off the metric's own model, grouped by the bucket and by every key the answer or a
    /// non-descending term needs.
    ///
    /// `keys` holds three kinds of column and this type does not distinguish them - the answer's
    /// local dimension keys, every remote dimension's join key, and every distinct key a
    /// non-descending term needs. It does not, because the rendering does not care: all three are
    /// grouped by and projected. Which is which is read from the federated plan beside the legs.
    /// Bounded by arithmetic that already exists rather than by a budget:
    /// [`MAX_DIMENSIONS`](crate::query::MAX_DIMENSIONS) dimension keys, plus one bucket, plus at
    /// most two distinct keys because a [`Measure`](crate::measure::Measure) is one term or a ratio
    /// of two. Seven grouping columns, worst case.
    ///
    /// **`terms` may be EMPTY, and the empty case is the whole of the distinct-key leg.** A fact leg
    /// with no terms groups by its key list and projects it, which is a distinct set of keys - the
    /// exact answer an exact `CountDistinct` needs above. At most four entries, because a measure is
    /// one term or a ratio of two and `Avg` expands one term into two.
    ///
    /// **`tables` is a [`StatementTables`] and not a table beside a vector of joins, and that is
    /// this variant's own guard rather than a tidier signature.** A fact leg keeps every SAME-SOURCE
    /// hop as a `JOIN` of its own, so two tables whose paths end in one name render under one
    /// implicit alias inside this leg's statement - `crate::plan::tables` holds the reproduction and
    /// the argument for refusing rather than aliasing. Taking the checked set as the field is what
    /// makes the ambiguous leg unrepresentable rather than something a producer has to remember to
    /// ask about: it did not, and the statement it rendered compared one table with itself. It
    /// serializes `#[serde(flatten)]`, so the pinned form is `table` beside `joins` exactly as
    /// before - a leg plan's serialized shape is a function of the split, not of where the guard
    /// lives.
    ///
    /// Same-source hops only. A dimension on another data system is a
    /// [`Lookup`](LegPlan::Lookup) leg, not a join.
    Fact {
        source: SourceName,
        metric: MetricName,
        #[serde(flatten)]
        tables: StatementTables,
        bucket: PlanBucket,
        keys: Vec<PlanKey>,
        terms: Vec<LegTerm>,
        filters: Vec<PlanFilter>,
        params: Vec<ParamValue>,
        range: TimeRange,
    },
    /// Read off one remote dimension model: its join key, the columns the answer groups by, and
    /// whatever filters went with it.
    ///
    /// No bucket, no terms, no range and no metric. A dimension table has no time column, so a range
    /// would be a predicate on a column that is not there; nothing is aggregated, so there is no
    /// term and no measure label; and the metric belongs to the fact leg.
    ///
    /// `filters` may be empty, and whether it is decides the join kind above - INNER for a remote
    /// dimension carrying a filter, LEFT for one that does not. That derivation belongs to the
    /// splitter and is deliberately not a field here.
    Lookup {
        source: SourceName,
        table: QualifiedTable,
        keys: Vec<PlanKey>,
        filters: Vec<PlanFilter>,
        params: Vec<ParamValue>,
    },
}

impl LegPlan {
    /// The one data system this leg runs against.
    ///
    /// Every leg is mono-source, so *a plan cannot silently span two sources* applies per leg
    /// unchanged: neither variant has a second [`SourceName`] to disagree with this one.
    #[inline]
    pub const fn source(&self) -> &SourceName {
        match *self {
            Self::Fact { ref source, .. } | Self::Lookup { ref source, .. } => source,
        }
    }

    /// Where the table this leg reads lives: the whole path, which is what its `FROM` names.
    ///
    /// A leg qualifies exactly as a whole-answer plan does, and it shares `generate`'s rendering to
    /// make sure of it - two `FROM`-building paths would be two places for identifier quoting to
    /// differ, which is the drift `sutura_sql::generate`'s own header is written against.
    #[inline]
    pub const fn table(&self) -> &QualifiedTable {
        match *self {
            Self::Fact { ref tables, .. } => tables.table(),
            Self::Lookup { ref table, .. } => table,
        }
    }

    /// The table's own name, which is what this leg's columns are qualified by.
    #[inline]
    pub const fn table_name(&self) -> &TableName {
        self.table().name()
    }

    /// The columns this leg groups by and projects.
    #[inline]
    pub fn keys(&self) -> &[PlanKey] {
        match *self {
            Self::Fact { ref keys, .. } | Self::Lookup { ref keys, .. } => keys,
        }
    }

    /// The predicates this leg applies.
    #[inline]
    pub fn filters(&self) -> &[PlanFilter] {
        match *self {
            Self::Fact { ref filters, .. } | Self::Lookup { ref filters, .. } => filters,
        }
    }

    /// The values bound to this leg's placeholders, in placeholder order.
    #[inline]
    pub fn params(&self) -> &[ParamValue] {
        match *self {
            Self::Fact { ref params, .. } | Self::Lookup { ref params, .. } => params,
        }
    }

    /// The labels this leg's result will carry, in the order it projects them.
    ///
    /// One definition, for [`QueryPlan::result_labels`]'s reason: an adapter that builds a schema
    /// and an adapter that renders a projection cannot disagree about it. Keys, then the bucket if
    /// there is one, then one label per term.
    pub fn result_labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self.keys().iter().map(|key| String::from(key.label())).collect();
        match *self {
            Self::Fact {
                ref bucket, ref terms, ..
            } => {
                labels.push(String::from(bucket.label()));
                labels.extend(terms.iter().map(|term| String::from(term.label())));
            }
            Self::Lookup { .. } => {}
        }
        labels
    }
}

/// What the execution port can be handed.
///
/// **One method rather than two, and the exhaustive match is the reason.** A second port method for
/// legs is a smaller diff and worse where it matters: a second method invites a default body, a
/// default that errors lets an adapter be silently non-federating, and *adding a data system is a
/// registration* then stops being true in the one direction nobody would notice. With one method
/// taking this enum, a third leg shape cannot be added without every adapter stating what it does
/// with it.
///
/// **It borrows.** A plan is built once and executed once, and the federated path multiplies the
/// working set against a memory bound that refuses - so a copy per leg would be a correctness
/// question rather than a style one.
///
/// An adapter that does not answer for every shape does not compile:
///
/// ```compile_fail
/// use sutura_domain::plan::Executable;
///
/// fn _dispatch(executable: Executable<'_>) -> &'static str {
///     match executable {
///         Executable::Query(_) => "a whole answer",
///     }
/// }
/// ```
///
/// ```
/// use sutura_domain::plan::Executable;
///
/// fn _dispatch(executable: Executable<'_>) -> &'static str {
///     match executable {
///         Executable::Query(_) => "a whole answer",
///         Executable::Leg(_) => "one source's share of one",
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Executable<'plan> {
    /// A whole answer from one data system.
    Query(&'plan QueryPlan),
    /// One data system's share of an answer assembled above it.
    Leg(&'plan LegPlan),
}

impl<'plan> Executable<'plan> {
    /// The one data system this runs against, whichever shape it is.
    #[inline]
    pub const fn source(self) -> &'plan SourceName {
        match self {
            Self::Query(plan) => plan.source(),
            Self::Leg(leg) => leg.source(),
        }
    }

    /// The values bound to this statement's placeholders, in placeholder order.
    #[inline]
    pub fn params(self) -> &'plan [ParamValue] {
        match self {
            Self::Query(plan) => plan.params(),
            Self::Leg(leg) => leg.params(),
        }
    }

    /// The labels the result will carry, in the order it projects them.
    #[inline]
    pub fn result_labels(self) -> Vec<String> {
        match self {
            Self::Query(plan) => plan.result_labels(),
            Self::Leg(leg) => leg.result_labels(),
        }
    }
}

impl<'plan> From<&'plan QueryPlan> for Executable<'plan> {
    #[inline]
    fn from(plan: &'plan QueryPlan) -> Self {
        Self::Query(plan)
    }
}

impl<'plan> From<&'plan LegPlan> for Executable<'plan> {
    #[inline]
    fn from(leg: &'plan LegPlan) -> Self {
        Self::Leg(leg)
    }
}

#[cfg(test)]
mod tests;

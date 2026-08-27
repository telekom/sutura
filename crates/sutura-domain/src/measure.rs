//! What a metric measures, and the filters that are part of its definition rather than of a
//! question.
//!
//! **A closed vocabulary, and the axis it is closed along is the term rather than the shape.** The
//! first version of this crate allowed exactly one aggregate over one column, which was safe and
//! could not express the metrics people actually certify: a revenue that means "active subscriptions
//! only", an average revenue per user that is one aggregate divided by another, a churn rate that is
//! a conditional count over a distinct count. Two of seven real metrics fitted; five did not.
//!
//! The second version bought most of it back with three sibling shapes - `simple`, `count_if`,
//! `ratio` - and left the seventh metric unsayable, for a reason that was structural rather than
//! incidental. A conditional count was a *shape*, so it could not be a *half* of a ratio, and
//! `count_if(churned) / count_distinct(subscription)` had every ingredient present and no way to
//! write it. Adding `count_if_over_x` shapes would have been the same mistake once per numerator.
//!
//! So the vocabulary is two levels: a [`Term`] is what one number is computed from, and a
//! [`Measure`] is one term or a ratio of two. Widening it is adding a `Term`, once, and every shape
//! gets the new term for free.
//!
//! The security property was never "one aggregate". It was **no free-text SQL**: every leaf is a
//! column the model declares, every operation is a variant the generator has an arm for, and there
//! is no string anywhere that reaches a statement unexamined. Two shapes, two terms and four
//! predicates buy back the expressiveness while keeping exactly that.
//!
//! What is still unrepresentable, deliberately: an expression over two columns
//! (`sum(price * quantity)`), a window function, a three-table join. Those need an expression
//! language, and an expression language on this path is the escape hatch
//! `docs/adr/0001-first-party-semantic-models.md` argues against. They belong to a definition
//! rendered upstream and taken as given.

use crate::model::{Aggregate, ColumnName};

/// One aggregate applied to one declared column.
///
/// `Count` is the case where the column is not read and still has to be named: a `COUNT(*)` over a
/// joined result counts join products rather than facts, so naming the column is what makes the
/// generated count count the thing the model says it counts.
///
/// Carries no `serde` derive. A term's on-disk shape belongs to [`TermRepr`] and to nothing else, so
/// there is exactly one place where the format of `{ aggregate: sum, column: x }` is decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregatedColumn {
    aggregate: Aggregate,
    column: ColumnName,
}

impl AggregatedColumn {
    #[inline]
    pub const fn new(aggregate: Aggregate, column: ColumnName) -> Self {
        Self { aggregate, column }
    }

    #[inline]
    pub const fn aggregate(&self) -> Aggregate {
        self.aggregate
    }

    #[inline]
    pub const fn column(&self) -> &ColumnName {
        &self.column
    }
}

/// One number a measure is computed from.
///
/// **The extensible axis.** A shape says how terms combine; a term says what one of them is. That
/// split is what makes a conditional count usable as a ratio's numerator, which is the metric the
/// previous vocabulary could not say with every one of its ingredients already present.
///
/// **Flat on disk, and read through [`TermRepr`] rather than by an external tag.** The one-key
/// mapping the rest of this format uses would spell the common half of a ratio
/// `numerator: { aggregate: { aggregate: sum, column: mrr_cents } }`: the tag word and the field
/// word are the same word, so the nesting says nothing and every existing `simple:` document in
/// every catalog would have to be rewritten to gain it. `#[serde(untagged)]` is not the way out
/// either - it reports "data did not match any variant", which names nothing, and `document.rs`
/// carries a test that exists to keep that error out of this format.
///
/// So a term is one flat mapping with a `deny_unknown_fields` struct behind it, and the word that
/// says which term it is - `aggregate` or `count_if` - is a key the author writes rather than a
/// shape inferred from an absence. A misspelled key is still an error naming the typo, an
/// unrecognised term is an error naming the word that was written, and a document that writes half
/// of one or both of them gets a [`InvalidTerm`] that says which.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "TermRepr", into = "TermRepr")]
pub enum Term {
    /// One aggregate over one column: `SUM(amount_cents)`.
    Aggregate(AggregatedColumn),
    /// How many rows have this boolean column true.
    ///
    /// Its own term rather than `Aggregate(count, column)`, because `COUNT(col)` counts non-null
    /// rows and would count the `false` ones too. It is spelled `COUNTIF` in one dialect and
    /// `SUM(CASE WHEN .. THEN 1 ELSE 0 END)` in another, which is the generator's problem and
    /// exactly the kind of thing that should not be in a catalog document.
    ///
    /// **And it is not a variant of [`Aggregate`], which was the obvious alternative.** `Aggregate`
    /// is a pure function-name set: `as_str` returns the word for a refusal, and each of the two
    /// generators has one `match` over it that maps a name to a call. A conditional term inside it
    /// would mean `{ aggregate: count_if, column: x }` and `{ count_if: x }` were two catalog
    /// spellings of one measure, compiling to two plan shapes and two digests for a definition
    /// that is identical - the "two paths" this module argues against everywhere else. A disk-only
    /// mirror of `Aggregate` with one extra word avoids the two spellings and buys the other half
    /// of the problem: a set that has to be kept in step with the domain's, whose failure mode is
    /// an aggregate no document can write and nothing anywhere failing to say so.
    CountIf { column: ColumnName },
}

impl Term {
    /// The column this term reads.
    #[inline]
    pub const fn column(&self) -> &ColumnName {
        match *self {
            Self::Aggregate(ref inner) => inner.column(),
            Self::CountIf { ref column } => column,
        }
    }

    /// The name of this term, for a refusal or a description.
    #[inline]
    pub const fn kind(&self) -> &'static str {
        match *self {
            Self::Aggregate(_) => "aggregate",
            Self::CountIf { .. } => "count_if",
        }
    }
}

/// The on-disk shape of a [`Term`], and the only place that shape is decided.
///
/// It exists to be converted, the way `calendar::TimeRangeInput` does. `deny_unknown_fields` is the
/// most useful line in it: without it `count_iff:` is dropped in silence and the failure becomes
/// "a term must say what it measures" printed next to a line that plainly says something.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TermRepr {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    aggregate: Option<Aggregate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    column: Option<ColumnName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    count_if: Option<ColumnName>,
}

/// Why a term was rejected.
///
/// Four variants rather than one message, because each of them is a different mistake and the
/// variant is what says which. A single "invalid term" would send an author back to compare their
/// line against a grammar.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidTerm {
    /// Nothing was written. The mapping parsed and named no term at all.
    #[error("a term must say what it measures: write `aggregate` and `column`, or `count_if`")]
    Empty,
    /// Both terms at once. Refused rather than resolved by precedence: a document that writes both
    /// means one of them, and picking one would certify a number the author did not ask for.
    #[error("a term is one thing, and this one writes two: `count_if` cannot appear beside `aggregate` or `column`")]
    TwoTerms,
    #[error("an aggregate term needs a column: `aggregate: {aggregate}` has no `column` beside it")]
    NoColumn { aggregate: Aggregate },
    #[error("an aggregate term needs an aggregate: `column: {column}` has no `aggregate` beside it")]
    NoAggregate { column: ColumnName },
}

impl TryFrom<TermRepr> for Term {
    type Error = InvalidTerm;

    fn try_from(repr: TermRepr) -> Result<Self, Self::Error> {
        match (repr.aggregate, repr.column, repr.count_if) {
            (Some(aggregate), Some(column), None) => Ok(Self::Aggregate(AggregatedColumn::new(aggregate, column))),
            (None, None, Some(column)) => Ok(Self::CountIf { column }),
            (Some(aggregate), None, None) => Err(InvalidTerm::NoColumn { aggregate }),
            (None, Some(column), None) => Err(InvalidTerm::NoAggregate { column }),
            (None, None, None) => Err(InvalidTerm::Empty),
            (_, _, Some(_)) => Err(InvalidTerm::TwoTerms),
        }
    }
}

/// The other direction, so what a digest is taken over and what a catalog wrote are the same text.
///
/// Not a convenience: [`Measure`] is serialized into the canonical form the definition digest hashes
/// and into the snapshot a reviewer reads, and a serialized shape that differed from the on-disk one
/// would make that snapshot a description of serde rather than of the catalog.
impl From<Term> for TermRepr {
    fn from(term: Term) -> Self {
        match term {
            Term::Aggregate(AggregatedColumn { aggregate, column }) => Self {
                aggregate: Some(aggregate),
                column: Some(column),
                count_if: None,
            },
            Term::CountIf { column } => Self {
                aggregate: None,
                column: None,
                count_if: Some(column),
            },
        }
    }
}

/// What a zero denominator means.
///
/// An enum and not a boolean, because `zero_safe: true` records that somebody thought about it and
/// not what they decided. Both behaviours are defensible - a rate over an empty period is arguably
/// null and arguably an error - the difference shows up only in the periods where it matters, and a
/// definition should say which one it means in a word a reader can check against the metric's prose.
///
/// **The on-disk words are `yields_null` and `fails`, and the first one is not cosmetic.** The
/// obvious spelling of the null case is `null`, and in YAML `null` is the null literal: a document
/// writing `zero_denominator: null` would hand the deserializer a unit value, and the author of the
/// most natural spelling in the vocabulary would get a type error about a line that looks right.
/// Naming the variants after what a zero denominator *does* means the field and its value read as
/// one sentence and neither of them can collide with a scalar YAML resolves itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ZeroDenominator {
    /// The measure is null for that row. The generator guards the denominator - a `NULLIF`, or the
    /// dialect's own safe-divide.
    #[serde(rename = "yields_null")]
    Null,
    /// The division is emitted unguarded, and the fault is raised where the value crosses back into
    /// the domain. A definition choosing this is saying an empty period is a fault and not a figure.
    ///
    /// **The word used to be a wish, and this paragraph is the correction.** "Unguarded" is not the
    /// same as "fails": both generators cast the numerator to a floating type first, because integer
    /// division truncates and `SUM(cents) / COUNT(*)` returning a whole number is wrong for every
    /// ratio anybody wants. So the division is IEEE float division, and IEEE float division by zero
    /// does not raise - it answers `inf`, or `NaN` when both halves are zero. A metric declaring that
    /// an empty period is a fault therefore answered the *string* `inf` under its own certified name,
    /// in both adapters, which is why the differential test agreed with itself and passed.
    ///
    /// What makes the word true is [`crate::warehouse::Real`]: a cell carries a checked finite `f64`,
    /// so an adapter handed a non-finite one has an error naming the column instead of a value. That
    /// is a stronger place for the check than a guard on the division would have been - it closes
    /// `-inf` and `NaN` too, and it holds for an adapter that renders no SQL at all.
    #[serde(rename = "fails")]
    Fail,
}

impl ZeroDenominator {
    /// The word a catalog writes, and the word a description reads back.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Null => "yields_null",
            Self::Fail => "fails",
        }
    }
}

/// What a metric measures.
///
/// Externally tagged, so a document names its shape: `simple:` or `ratio:`. That makes a measure's
/// shape a word an author writes rather than something inferred from which fields are present, and
/// it makes an unrecognised shape an error naming what it found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Measure {
    /// One term: `SUM(amount_cents)`, or a conditional count.
    Simple(Term),
    /// One term divided by another: an average revenue per user, a rate, a share.
    ///
    /// The two halves are separate terms over possibly different columns, which is what
    /// distinguishes this from `Simple(avg, column)`: `SUM(revenue) / COUNT(DISTINCT customer)` is
    /// not the mean of a column, and computing it as one is a different and wrong number.
    Ratio {
        numerator: Term,
        denominator: Term,
        zero_denominator: ZeroDenominator,
    },
}

impl Measure {
    /// The terms this measure is computed from, in the order a reader would say them.
    ///
    /// One place, so a shape added here is a shape everything that walks terms already handles.
    pub fn terms(&self) -> Vec<&Term> {
        match *self {
            Self::Simple(ref term) => vec![term],
            Self::Ratio {
                ref numerator,
                ref denominator,
                ..
            } => vec![numerator, denominator],
        }
    }

    /// Every column this measure reads.
    ///
    /// One place, so [`crate::catalog::Definitions`] can check them all against the model without
    /// knowing the shapes, and so a shape added here cannot be forgotten there.
    pub fn columns(&self) -> Vec<&ColumnName> {
        self.terms().into_iter().map(Term::column).collect()
    }

    /// The name of this shape, for a refusal or a description.
    #[inline]
    pub const fn shape(&self) -> &'static str {
        match *self {
            Self::Simple(_) => "simple",
            Self::Ratio { .. } => "ratio",
        }
    }
}

/// A predicate that is part of what a metric means.
///
/// **Definitional, not a question.** `mrr` means recurring revenue *from active subscriptions*, and
/// a statement that omits that predicate returns a different number under the same name - the exact
/// failure this repository exists to prevent, arrived at by omission rather than by tampering. So a
/// required filter is applied to every question about the metric, and a caller cannot see it, choose
/// it or turn it off.
///
/// Its values come from the catalog rather than from a caller, and they are still bound as
/// parameters rather than written into the statement. Not because the catalog is untrusted in the
/// way a caller is, but because a value that is sometimes inlined and sometimes bound is a generator
/// with two paths, and the inlining path is the one that would eventually be handed caller text.
///
/// Externally tagged for the same reason [`Measure`] is: the operator is a word, not an inference.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RequiredFilter {
    /// `column = value`.
    Equals { column: ColumnName, value: String },
    /// `column <> value`. Note what this does NOT match in SQL: a null column. `NotEquals` on a
    /// nullable column excludes null rows, and a definition that means "everything except x,
    /// including unknown" needs `IsNull` beside it - which does not exist yet, because nothing has
    /// needed it.
    NotEquals { column: ColumnName, value: String },
    /// `column IS TRUE`, for a boolean column.
    IsTrue { column: ColumnName },
    /// `column IS NOT NULL`.
    IsNotNull { column: ColumnName },
}

impl RequiredFilter {
    #[inline]
    pub const fn column(&self) -> &ColumnName {
        match *self {
            Self::Equals { ref column, .. }
            | Self::NotEquals { ref column, .. }
            | Self::IsTrue { ref column }
            | Self::IsNotNull { ref column } => column,
        }
    }

    /// The value this filter compares against, if it compares against one.
    ///
    /// `None` for the two that need no value, which is what tells the generator whether to emit a
    /// placeholder and the plan whether to bind a parameter.
    #[inline]
    pub const fn value(&self) -> Option<&String> {
        match *self {
            Self::Equals { ref value, .. } | Self::NotEquals { ref value, .. } => Some(value),
            Self::IsTrue { .. } | Self::IsNotNull { .. } => None,
        }
    }
}

/// How a measure reads to a person: `sum(amount_cents)`, `count_if(churned)`,
/// `count_if(churned) / count_distinct(subscription_key), zero denominator yields_null`.
///
/// **Catalog vocabulary, not SQL.** The aggregate names are the ones a document writes, which is
/// what `Aggregate::as_str` returns; the shapes and terms are spelled the way this module names
/// them. It resembles SQL because both describe the same arithmetic, and it must not be mistaken for
/// something to execute - a `count_if` renders as neither `COUNTIF` nor `SUM(CASE ..)` here, and a
/// ratio carries no null guard.
impl core::fmt::Display for Measure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Simple(ref term) => write!(f, "{term}"),
            Self::Ratio {
                ref numerator,
                ref denominator,
                zero_denominator,
            } => write!(
                f,
                "{numerator} / {denominator}, zero denominator {}",
                zero_denominator.as_str()
            ),
        }
    }
}

/// How one term reads to a person. Same caveat as [`Measure`]'s: vocabulary, not SQL.
impl core::fmt::Display for Term {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Aggregate(ref inner) => write!(f, "{}({})", inner.aggregate(), inner.column()),
            Self::CountIf { ref column } => write!(f, "count_if({column})"),
        }
    }
}

/// How a required filter reads to a person. Same caveat as [`Measure`]'s: vocabulary, not SQL.
impl core::fmt::Display for RequiredFilter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Equals { ref column, ref value } => write!(f, "{column} = {value:?}"),
            Self::NotEquals { ref column, ref value } => write!(f, "{column} <> {value:?}"),
            Self::IsTrue { ref column } => write!(f, "{column} is true"),
            Self::IsNotNull { ref column } => write!(f, "{column} is not null"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AggregatedColumn, InvalidTerm, Measure, RequiredFilter, Term, TermRepr, ZeroDenominator};
    use crate::model::{Aggregate, ColumnName};

    fn column(raw: &str) -> ColumnName {
        ColumnName::parse(raw).expect("a test column is a column")
    }

    fn aggregated(aggregate: Aggregate, raw: &str) -> Term {
        Term::Aggregate(AggregatedColumn::new(aggregate, column(raw)))
    }

    #[test]
    fn a_simple_measure_reads_one_column() {
        let measure = Measure::Simple(aggregated(Aggregate::Sum, "amount"));
        assert_eq!(measure.columns(), vec![&column("amount")]);
        assert_eq!(measure.shape(), "simple");
        assert_eq!(measure.terms().len(), 1);
    }

    #[test]
    fn a_ratio_reads_both_of_its_columns() {
        // The reason `columns()` exists: the consistency check walks it, so a shape whose second
        // column was not reported would let a metric name a column its model does not have.
        let measure = Measure::Ratio {
            numerator: aggregated(Aggregate::Sum, "mrr_eur"),
            denominator: aggregated(Aggregate::CountDistinct, "customer_key"),
            zero_denominator: ZeroDenominator::Null,
        };
        assert_eq!(measure.columns(), vec![&column("mrr_eur"), &column("customer_key")]);
        assert_eq!(measure.shape(), "ratio");
    }

    #[test]
    fn count_if_is_its_own_term_and_not_a_count() {
        // `COUNT(col)` counts non-null rows, so it counts the `false` ones too. Expressing "how
        // many rows are true" as a count of a boolean column is a wrong number that raises no
        // error, which is why this is a variant rather than a convention.
        let term = Term::CountIf {
            column: column("churned_in_month"),
        };
        assert_eq!(term.column(), &column("churned_in_month"));
        assert_eq!(term.kind(), "count_if");
        assert_eq!(aggregated(Aggregate::Count, "churned_in_month").kind(), "aggregate");

        let measure = Measure::Simple(term);
        assert_eq!(measure.columns(), vec![&column("churned_in_month")]);
        assert_eq!(measure.shape(), "simple");
    }

    #[test]
    fn a_conditional_count_can_be_half_of_a_ratio() {
        // The whole point of the two-level vocabulary, and the metric the three-shape one could not
        // say with every one of its ingredients already present: a churn rate is a conditional count
        // over a distinct count. When `count_if` was a sibling of `ratio` this was unrepresentable,
        // and the example catalog carried a paragraph explaining why instead of the metric.
        let measure = Measure::Ratio {
            numerator: Term::CountIf {
                column: column("churned_in_month"),
            },
            denominator: aggregated(Aggregate::CountDistinct, "subscription_key"),
            zero_denominator: ZeroDenominator::Null,
        };
        assert_eq!(
            measure.columns(),
            vec![&column("churned_in_month"), &column("subscription_key")]
        );
        assert_eq!(
            measure.to_string(),
            "count_if(churned_in_month) / count_distinct(subscription_key), zero denominator yields_null"
        );
    }

    #[test]
    fn a_zero_denominator_says_which_of_the_two_behaviours_it_means() {
        // What the boolean could not: `zero_safe: true` recorded that somebody had thought about it
        // and not what they concluded, so two catalogs could agree on the flag and disagree on
        // whether an empty period is a null or a fault.
        assert_eq!(ZeroDenominator::Null.as_str(), "yields_null");
        assert_eq!(ZeroDenominator::Fail.as_str(), "fails");
        let fails = Measure::Ratio {
            numerator: aggregated(Aggregate::Sum, "mrr_eur"),
            denominator: aggregated(Aggregate::CountDistinct, "customer_key"),
            zero_denominator: ZeroDenominator::Fail,
        };
        assert!(fails.to_string().ends_with("zero denominator fails"), "{fails}");
    }

    #[test]
    fn a_term_that_writes_half_of_itself_is_refused_by_what_is_missing() {
        // The cost of a flat on-disk term, paid here rather than by the author. Each of these is a
        // real mistake with its own variant, because "invalid term" would send whoever wrote the
        // line back to compare it against a grammar.
        let repr = |aggregate: Option<Aggregate>, col: Option<&str>, count_if: Option<&str>| TermRepr {
            aggregate,
            column: col.map(column),
            count_if: count_if.map(column),
        };
        assert_eq!(
            Term::try_from(repr(Some(Aggregate::Sum), None, None)),
            Err(InvalidTerm::NoColumn {
                aggregate: Aggregate::Sum
            })
        );
        assert_eq!(
            Term::try_from(repr(None, Some("amount"), None)),
            Err(InvalidTerm::NoAggregate {
                column: column("amount")
            })
        );
        assert_eq!(Term::try_from(repr(None, None, None)), Err(InvalidTerm::Empty));
        // Both terms at once is refused rather than resolved by precedence: the document means one
        // of them, and choosing would certify a number nobody asked for.
        assert_eq!(
            Term::try_from(repr(Some(Aggregate::Sum), Some("amount"), Some("churned"))),
            Err(InvalidTerm::TwoTerms)
        );
        assert_eq!(
            Term::try_from(repr(None, Some("amount"), Some("churned"))),
            Err(InvalidTerm::TwoTerms)
        );
        assert_eq!(
            Term::try_from(repr(Some(Aggregate::Sum), Some("amount"), None)),
            Ok(aggregated(Aggregate::Sum, "amount"))
        );
        assert_eq!(
            Term::try_from(repr(None, None, Some("churned"))),
            Ok(Term::CountIf {
                column: column("churned")
            })
        );
    }

    #[test]
    fn a_required_filter_reports_its_column_and_whether_it_binds_a_value() {
        // The plan reads both: the column to build the predicate, and the presence of a value to
        // decide whether a parameter is bound. A filter that reported neither would silently drop
        // out of the statement.
        let equals = RequiredFilter::Equals {
            column: column("status"),
            value: String::from("active"),
        };
        assert_eq!(equals.column(), &column("status"));
        assert_eq!(equals.value(), Some(&String::from("active")));

        let is_true = RequiredFilter::IsTrue {
            column: column("churned_in_month"),
        };
        assert_eq!(is_true.column(), &column("churned_in_month"));
        assert_eq!(is_true.value(), None);

        let not_null = RequiredFilter::IsNotNull {
            column: column("ended_at"),
        };
        assert_eq!(not_null.value(), None);

        let not_equals = RequiredFilter::NotEquals {
            column: column("status"),
            value: String::from("cancelled"),
        };
        assert_eq!(not_equals.value(), Some(&String::from("cancelled")));
    }
}

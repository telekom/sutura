//! What a metric measures, and the filters that are part of its definition rather than of a
//! question.
//!
//! **A closed vocabulary of shapes, not a closed set of aggregates, and the difference is the whole
//! design of this module.** The first version of this crate allowed exactly one aggregate over one
//! column, which was safe and could not express the metrics people actually certify: a revenue that
//! means "active subscriptions only", an average revenue per user that is one aggregate divided by
//! another, a churn rate that is a conditional count over a distinct count. Two of seven real
//! metrics fitted; five did not.
//!
//! The security property was never "one aggregate". It was **no free-text SQL**: every leaf is a
//! column the model declares, every operation is a variant the generator has an arm for, and there
//! is no string anywhere that reaches a statement unexamined. Three shapes and four predicates buy
//! back the expressiveness while keeping exactly that.
//!
//! What is still unrepresentable, deliberately: an expression over two columns
//! (`sum(price * quantity)`), a window function, a three-table join. Those need an expression
//! language, and an expression language on this path is the escape hatch
//! `docs/adr/0001-first-party-semantic-models.md` argues against. They belong to a definition
//! rendered upstream and taken as given.

use crate::model::{Aggregate, ColumnName};

/// One aggregate applied to one declared column.
///
/// The building block of every measure shape below. `Count` is the case where the column is not
/// read and still has to be named: a `COUNT(*)` over a joined result counts join products rather
/// than facts, so naming the column is what makes the generated count count the thing the model
/// says it counts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
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

/// What a metric measures.
///
/// Externally tagged, so a document names its shape: `simple:`, `count_if:` or `ratio:`. That makes
/// a measure's shape a word an author writes rather than something inferred from which fields are
/// present, and it makes an unrecognised shape an error naming what it found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Measure {
    /// One aggregate over one column: `SUM(amount_cents)`.
    Simple(AggregatedColumn),
    /// How many rows have this boolean column true.
    ///
    /// Its own shape rather than `Simple(count, column)`, because `COUNT(col)` counts non-null rows
    /// and would count the `false` ones too. It is spelled `COUNTIF` in one dialect and
    /// `SUM(CASE WHEN .. THEN 1 ELSE 0 END)` in another, which is the generator's problem and
    /// exactly the kind of thing that should not be in a catalog document.
    CountIf { column: ColumnName },
    /// One aggregate divided by another: an average revenue per user, a rate, a share.
    ///
    /// The two halves are separate aggregates over possibly different columns, which is what
    /// distinguishes this from `Simple(avg, column)`: `SUM(revenue) / COUNT(DISTINCT customer)` is
    /// not the mean of a column, and computing it as one is a different and wrong number.
    Ratio {
        numerator: AggregatedColumn,
        denominator: AggregatedColumn,
        /// Whether a zero denominator yields null instead of failing.
        ///
        /// Not a default, because the two behaviours are both defensible and the difference matters:
        /// a rate over an empty period is arguably null and arguably an error, and a definition
        /// should say which. The generator renders it per dialect - a `NULLIF` on the denominator,
        /// or the dialect's own safe-divide.
        zero_safe: bool,
    },
}

impl Measure {
    /// Every column this measure reads.
    ///
    /// One place, so [`crate::catalog::Definitions`] can check them all against the model without
    /// knowing the shapes, and so a shape added here cannot be forgotten there.
    pub fn columns(&self) -> Vec<&ColumnName> {
        match *self {
            Self::Simple(ref term) => vec![term.column()],
            Self::CountIf { ref column } => vec![column],
            Self::Ratio {
                ref numerator,
                ref denominator,
                ..
            } => vec![numerator.column(), denominator.column()],
        }
    }

    /// The name of this shape, for a refusal or a description.
    #[inline]
    pub const fn shape(&self) -> &'static str {
        match *self {
            Self::Simple(_) => "simple",
            Self::CountIf { .. } => "count_if",
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
/// `sum(mrr_eur) / count_distinct(customer_key)`.
///
/// **Catalog vocabulary, not SQL.** The aggregate names are the ones a document writes, which is
/// what `Aggregate::as_str` returns; the shapes are spelled the way this module names them. It
/// resembles SQL because both describe the same arithmetic, and it must not be mistaken for
/// something to execute - a `count_if` renders as neither `COUNTIF` nor `SUM(CASE ..)` here, and a
/// ratio carries no null guard.
impl core::fmt::Display for Measure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Simple(ref term) => write!(f, "{}({})", term.aggregate(), term.column()),
            Self::CountIf { ref column } => write!(f, "count_if({column})"),
            Self::Ratio {
                ref numerator,
                ref denominator,
                zero_safe,
            } => {
                write!(
                    f,
                    "{}({}) / {}({})",
                    numerator.aggregate(),
                    numerator.column(),
                    denominator.aggregate(),
                    denominator.column()
                )?;
                if zero_safe {
                    f.write_str(", zero-safe")?;
                }
                Ok(())
            }
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
    use super::{AggregatedColumn, Measure, RequiredFilter};
    use crate::model::{Aggregate, ColumnName};

    fn column(raw: &str) -> ColumnName {
        ColumnName::parse(raw).expect("a test column is a column")
    }

    #[test]
    fn a_simple_measure_reads_one_column() {
        let measure = Measure::Simple(AggregatedColumn::new(Aggregate::Sum, column("amount")));
        assert_eq!(measure.columns(), vec![&column("amount")]);
        assert_eq!(measure.shape(), "simple");
    }

    #[test]
    fn a_ratio_reads_both_of_its_columns() {
        // The reason `columns()` exists: the consistency check walks it, so a shape whose second
        // column was not reported would let a metric name a column its model does not have.
        let measure = Measure::Ratio {
            numerator: AggregatedColumn::new(Aggregate::Sum, column("mrr_eur")),
            denominator: AggregatedColumn::new(Aggregate::CountDistinct, column("customer_key")),
            zero_safe: true,
        };
        assert_eq!(measure.columns(), vec![&column("mrr_eur"), &column("customer_key")]);
        assert_eq!(measure.shape(), "ratio");
    }

    #[test]
    fn count_if_is_its_own_shape_and_not_a_count() {
        // `COUNT(col)` counts non-null rows, so it counts the `false` ones too. Expressing "how
        // many rows are true" as a count of a boolean column is a wrong number that raises no
        // error, which is why this is a variant rather than a convention.
        let measure = Measure::CountIf {
            column: column("churned_in_month"),
        };
        assert_eq!(measure.columns(), vec![&column("churned_in_month")]);
        assert_eq!(measure.shape(), "count_if");
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

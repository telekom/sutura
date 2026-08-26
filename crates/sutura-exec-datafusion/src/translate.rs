//! A plan becomes DataFusion expressions, and no SQL is produced anywhere in here.
//!
//! Split out of `lib.rs` when that file crossed the thousand-line gate, along one seam that was
//! already there: everything here turns a piece of a [`QueryPlan`] into an [`Expr`], and nothing
//! here reads a result. The other half of the adapter - the schema and array work that turns Arrow
//! back into domain rows - stayed behind, and the two halves share no state.
//!
//! What every function in this module has in common is the thing worth protecting: a plan arrives
//! as typed values and leaves as a logical expression tree. There is no point at which a value is
//! formatted into text, so the class of bug that dominates a SQL-rendering path - a literal pasted
//! into a statement, a placeholder in the wrong dialect's syntax, a date truncated to a timestamp -
//! is unreachable rather than tested for.

use datafusion::arrow::datatypes::DataType;
use datafusion::common::{Column, ScalarValue, TableReference};
use datafusion::functions::expr_fn::{date_trunc, nullif};
use datafusion::functions_aggregate::expr_fn::{avg, count, count_distinct, max, min, sum};
use datafusion::logical_expr::{Expr, cast, lit, when};
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::{Aggregate, Grain, TableName};
use sutura_domain::plan::{PlanColumn, PlanMeasure, PlanPredicate, PlanTerm, QueryPlan};
use sutura_domain::warehouse::ParamValue;

use crate::DataFusionError;

/// The one way a column is referenced in this adapter.
///
/// **`col("orders.order_date")` lowercases the identifier** - `Column::from_qualified_name`
/// normalises every part of a dotted name - and the domain deliberately preserves case, for the
/// reasons `parse_identifier` in `sutura_domain::model` gives. So a `col(..)` anywhere in this crate
/// is a bug, and this is the case-preserving form it is a bug instead of.
pub(crate) fn column(plan_column: &PlanColumn) -> Expr {
    Expr::Column(Column::new(
        Some(table_reference(plan_column.table())),
        plan_column.column().as_str(),
    ))
}

/// A table name, as the engine's reference type, without normalisation.
///
/// `TableReference::bare` and not `TableReference::from(&str)`: the `From` impl parses the string
/// and lowercases anything unquoted, which is the same case-folding trap as `col`. Registration and
/// reference both go through here, so the two cannot disagree about the name of a table.
pub(crate) fn table_reference(table: &TableName) -> TableReference {
    TableReference::bare(table.as_str())
}

/// A parameter, as a typed literal.
///
/// **This is not inlining in the injection sense, and it is worth being precise about why.** The SQL
/// path binds a placeholder because there a value would otherwise become text inside a statement,
/// where a quote is syntax. Here there is no parser and no statement: a `ScalarValue::Utf8` is a
/// value in a plan node, and the only thing the engine can do with it is compare it. It cannot
/// become syntax, because there is no syntax to become. That makes this the strongest form of
/// parameterisation available rather than a weakening of one - the engine is never handed a string
/// it has to interpret.
pub(crate) fn literal(param: &ParamValue) -> Expr {
    match *param {
        ParamValue::Text(ref v) => lit(v.as_str()),
        // Days since the epoch, which is what a `Date32` column actually holds, so the comparison is
        // exact rather than a cast of text the engine parsed.
        ParamValue::Date(d) => lit(ScalarValue::Date32(Some(d.days_since_epoch()))),
    }
}

/// The grain, as the argument the truncation function takes.
pub(crate) const fn unit(grain: Grain) -> &'static str {
    match grain {
        Grain::Day => "day",
        Grain::Week => "week",
        Grain::Month => "month",
        Grain::Quarter => "quarter",
        Grain::Year => "year",
    }
}

/// The truncated time column, cast back to a date.
///
/// **`date_trunc` over a `Date32` returns `Timestamp(Nanosecond, None)`, not a date.** Without the
/// cast the `period` column would come back as a timestamp here and as a date from the SQL path,
/// which renders differently and would make a differential comparison fail for a formatting reason.
/// `generate.rs` casts for the same reason, which is what keeps the two agreeing.
pub(crate) fn bucket_expression(grain: Grain, plan_column: &PlanColumn) -> Expr {
    cast(date_trunc(lit(unit(grain)), column(plan_column)), DataType::Date32)
}

/// One aggregate over one column.
pub(crate) fn aggregate_expr(kind: Aggregate, over: Expr) -> Expr {
    match kind {
        Aggregate::Sum => sum(over),
        Aggregate::Count => count(over),
        Aggregate::CountDistinct => count_distinct(over),
        Aggregate::Avg => avg(over),
        Aggregate::Min => min(over),
        Aggregate::Max => max(over),
    }
}

/// One term, as one expression, with the same arithmetic the SQL path emits.
///
/// A conditional count is a summed CASE rather than a filtered count, because that is what
/// `generate.rs` renders: it counts 0 rather than null for a false row, so a period with no matches
/// answers 0 instead of nothing. `is_true` is what makes a null row false here, the way falling
/// through to the ELSE branch does there.
pub(crate) fn term_expression(term: &PlanTerm) -> Result<Expr, DataFusionError> {
    match *term {
        PlanTerm::Aggregate {
            aggregate: kind,
            column: ref plan_column,
        } => Ok(aggregate_expr(kind, column(plan_column))),
        PlanTerm::CountIf { column: ref plan_column } => {
            let branch = when(column(plan_column).is_true(), lit(1_i64))
                .otherwise(lit(0_i64))
                .map_err(|cause| DataFusionError::Build { cause })?;
            Ok(sum(branch))
        }
    }
}

/// The measure, as one expression, with the same arithmetic the SQL path emits.
///
/// **Integer division truncates and errors on a zero denominator.** In this engine `lit(7i64) /
/// lit(2i64)` is `3`, and dividing by `lit(0i64)` is a hard "Divide by zero" rather than a null. So
/// a ratio casts its numerator to `Float64` first, which is what `generate.rs` emits as
/// `CAST(.. AS DOUBLE)`, and one whose zero denominator yields null wraps the denominator in a
/// null-if against `0.0`, which is the `NULLIF(.., 0)` the SQL path emits. Both halves are
/// load-bearing: without the cast a revenue-per-order ratio silently returns a whole number, and
/// without the null-if a period with no orders fails the whole query instead of answering null for
/// that one row.
pub(crate) fn measure_expression(measure: &PlanMeasure) -> Result<Expr, DataFusionError> {
    match *measure {
        PlanMeasure::Simple { ref term } => term_expression(term),
        PlanMeasure::Ratio {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => {
            let top = cast(term_expression(numerator)?, DataType::Float64);
            let bottom = term_expression(denominator)?;
            let bottom = match zero_denominator {
                ZeroDenominator::Null => nullif(cast(bottom, DataType::Float64), lit(0.0_f64)),
                ZeroDenominator::Fail => bottom,
            };
            Ok(top / bottom)
        }
    }
}

/// One predicate, with its value resolved by the index the plan recorded.
///
/// By index and not by position in the filter list, because that is what the plan records and what
/// the SQL path's placeholder positions are derived from. An adapter that walked the filters and
/// consumed parameters in order would agree with it right up until a filter stopped binding one.
pub(crate) fn predicate(plan: &QueryPlan, plan_predicate: &PlanPredicate) -> Result<Expr, DataFusionError> {
    let subject = column(plan_predicate.column());
    let bound = match plan_predicate.param() {
        None => None,
        Some(index) => Some(literal(plan.params().get(index).ok_or(DataFusionError::MissingParam {
            index,
            count: plan.params().len(),
        })?)),
    };
    match (plan_predicate, bound) {
        (&PlanPredicate::AtOrAfter { .. }, Some(value)) => Ok(subject.gt_eq(value)),
        (&PlanPredicate::Before { .. }, Some(value)) => Ok(subject.lt(value)),
        (&PlanPredicate::Equals { .. }, Some(value)) => Ok(subject.eq(value)),
        (&PlanPredicate::NotEquals { .. }, Some(value)) => Ok(subject.not_eq(value)),
        (&PlanPredicate::IsTrue { .. }, _) => Ok(subject.is_true()),
        (&PlanPredicate::IsNotNull { .. }, _) => Ok(subject.is_not_null()),
        // A comparing predicate whose parameter did not resolve. `param()` returns `Some` for
        // exactly the four arms above, so this is the shape a change to that method would produce,
        // and it is an error rather than a silently dropped comparison.
        (_, None) => Err(DataFusionError::MissingParam {
            index: usize::MAX,
            count: plan.params().len(),
        }),
    }
}

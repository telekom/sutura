//! A plan becomes DataFusion expressions, and no SQL is produced anywhere in here.
//!
//! Split out of `lib.rs` when that file crossed the thousand-line gate, along one seam that was
//! already there: everything here turns a PIECE of a plan into an [`Expr`], and nothing here reads
//! a result. The other half of the adapter - the schema and array work that turns Arrow back into
//! domain rows - stayed behind, and the two halves share no state.
//!
//! **A piece, deliberately, and that is what makes this module the shared half.** Which plan the
//! piece came out of is not a question anything here asks, so `crate::leg` and the whole-plan path
//! in `lib.rs` build their two different shapes out of these same functions - a column reference, a
//! grain truncation, a term's aggregate and a predicate's bound value cannot be one thing for a
//! whole answer and another for one source's share of one.
//!
//! What every function in this module has in common is the thing worth protecting: a plan arrives
//! as typed values and leaves as a logical expression tree. There is no point at which a value is
//! formatted into text, so the class of bug that dominates a SQL-rendering path - a literal pasted
//! into a statement, a placeholder in the wrong dialect's syntax, a date truncated to a timestamp -
//! is unreachable rather than tested for.

use datafusion::arrow::datatypes::DataType;
use datafusion::common::{Column, ScalarValue, TableReference};
use datafusion::functions::expr_fn::{date_trunc, named_struct, nullif};
use datafusion::functions_aggregate::expr_fn::{avg, count, count_distinct, max, min, sum};
use datafusion::logical_expr::{Expr, cast, lit, when};
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::{Aggregate, Grain, TableName};
use sutura_domain::plan::{PlanColumn, PlanMeasure, PlanPredicate, PlanTerm};
use sutura_domain::warehouse::ParamValue;
use sutura_domain::warehouse::cardinality::{DISTINCT_LABEL, DeclaredKey, ROWS_LABEL};

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

/// The two aggregates a declared key probe projects, under the domain's own labels.
///
/// **Aliased with `sutura-domain`'s constants rather than this crate's literals**, so the field name
/// the engine puts on the batch is the name
/// [`KeyUniqueness::read`](sutura_domain::warehouse::cardinality::KeyUniqueness::read) looks for -
/// the same one-definition argument the SQL renderer makes for the same pair.
///
/// **A row counts in EITHER aggregate only when every column of the whole key is non-null** - the
/// same rule `generate.rs`'s SQL probe carries, and for the same reason: a row null in any one
/// column of a compound key cannot match on either side of a join, so it is not a key row for
/// either count. A single column keeps `COUNT`'s own null exclusion (`over.len() == 1` below); a
/// compound key folds the per-column nullness checks into a `CASE` each aggregate counts the
/// non-null result of, which is what keeps this measurement of `rows` and `distinct` comparable -
/// `named_struct` alone is never itself null even when a field of it is, so counting a bare struct
/// counted a null-bearing row as one more distinct value than the SQL path's `COUNT(DISTINCT a, b)`
/// would, and `is_unique()` could hold over a real duplicate. Only the CASE-guarded form is used.
///
/// The distinct count runs over the WHOLE target key set, never one column alone: a compound key
/// determines a target row only as a whole, so the fan-out check counts distinct over every target
/// column at once - the same tuple the SQL path renders as `COUNT(DISTINCT a, b, …)`.
pub(crate) fn key_counts(key: &DeclaredKey<'_>) -> Result<Vec<Expr>, DataFusionError> {
    let over: Vec<Expr> = key
        .target_columns()
        .map(|column| Expr::Column(Column::new(Some(table_reference(key.table().name())), column.as_str())))
        .collect();
    let Some(first) = over.first().cloned() else {
        return Ok(Vec::new());
    };
    if over.len() == 1 {
        return Ok(vec![
            count(first.clone()).alias(ROWS_LABEL),
            count_distinct(first).alias(DISTINCT_LABEL),
        ]);
    }
    // `over.len() > 1` was just checked above, so this always has a first element; `unwrap_or_else`
    // rather than `expect` because `clippy::expect-used` is forbidden outside tests, and the
    // fallback is never reached rather than merely unlikely.
    let mut not_null_terms = over.iter().cloned().map(Expr::is_not_null);
    let first_not_null = not_null_terms.next().unwrap_or_else(|| lit(true));
    let all_not_null = not_null_terms.fold(first_not_null, Expr::and);
    let rows_marker = when(all_not_null.clone(), lit(1_i64))
        .end()
        .map_err(|cause| DataFusionError::Build { cause })?;
    // `COUNT(DISTINCT a, b, …)` is not a single-expr aggregate; DataFusion receives it as a struct
    // of the columns and de-duplicates on the whole tuple - the engine's shape of the same probe
    // the SQL path renders. Guarded by the same CASE `rows_marker` is, so a null-bearing row
    // contributes to neither count rather than to `distinct` alone.
    let fields = over
        .iter()
        .enumerate()
        .flat_map(|(i, expr)| vec![lit(format!("k{i}")), expr.clone()])
        .collect::<Vec<_>>();
    let distinct_marker = when(all_not_null, named_struct(fields))
        .end()
        .map_err(|cause| DataFusionError::Build { cause })?;
    Ok(vec![
        count(rows_marker).alias(ROWS_LABEL),
        count_distinct(distinct_marker).alias(DISTINCT_LABEL),
    ])
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
///
/// `AVG` receives `Float64`. Fixture integers are represented as `Decimal256(38, 0)` so their
/// `SUM` has room beyond the accepted 38-digit input, but `DataFusion`'s decimal average rounds to four places before a
/// result cast. Casting the input keeps the approximate mean every other adapter returns. The
/// Postgres renderer makes the same cast on its wire path.
pub(crate) fn aggregate_expr(kind: Aggregate, over: Expr) -> Expr {
    match kind {
        Aggregate::Sum => sum(over),
        Aggregate::Count => count(over),
        Aggregate::CountDistinct => count_distinct(over),
        Aggregate::Avg => avg(cast(over, DataType::Float64)),
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
///
/// **It takes the parameter LIST and not the plan**, so a whole plan and one leg of one resolve a
/// bound value through the same code. Both carry their parameters in placeholder order and neither
/// has anything else this needs; taking a `&QueryPlan` would have meant a second copy of the
/// index arithmetic for the leg path, which is the one place a comparing predicate could quietly
/// bind a different value on one path than on the other.
pub(crate) fn predicate(params: &[ParamValue], plan_predicate: &PlanPredicate) -> Result<Expr, DataFusionError> {
    let subject = column(plan_predicate.column());
    match *plan_predicate {
        PlanPredicate::AtOrAfter { param, .. } => Ok(subject.gt_eq(literal(resolve(params, param)?))),
        PlanPredicate::Before { param, .. } => Ok(subject.lt(literal(resolve(params, param)?))),
        PlanPredicate::Equals { param, .. } => Ok(subject.eq(literal(resolve(params, param)?))),
        PlanPredicate::NotEquals { param, .. } => Ok(subject.not_eq(literal(resolve(params, param)?))),
        PlanPredicate::In { params: ref indices, .. } => Ok(subject.in_list(values_of(params, indices)?, false)),
        PlanPredicate::NotIn { params: ref indices, .. } => Ok(subject.in_list(values_of(params, indices)?, true)),
        PlanPredicate::IsTrue { .. } => Ok(subject.is_true()),
        PlanPredicate::IsNotNull { .. } => Ok(subject.is_not_null()),
    }
}

/// The parameter at `index`, or the same error every arm above raises for an index the plan's own
/// parameter list does not hold - a defect in this workspace, since `PlanBindings::parse` is the
/// only way to build a plan and it refuses exactly this.
fn resolve(params: &[ParamValue], index: usize) -> Result<&ParamValue, DataFusionError> {
    params.get(index).ok_or(DataFusionError::MissingParam {
        index,
        count: params.len(),
    })
}

/// Every value an `In`/`NotIn` predicate's indices resolve to, as literals - `github.com/telekom/sutura#968`.
fn values_of(params: &[ParamValue], indices: &[usize]) -> Result<Vec<Expr>, DataFusionError> {
    indices.iter().map(|&index| resolve(params, index).map(literal)).collect()
}

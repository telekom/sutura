//! One measure, as one expression - split out of `generate.rs` for `cargo xtask max-lines`'s
//! per-file cap, not for thematic tidiness: this is the guard/aggregate rendering the multi-metric
//! shape added, and it is the one piece of that file large enough to earn its own seam.
//!
//! # Why a guard's parameters are tracked by emission, not by position
//!
//! A metric's guard is one `Expr` built once (see [`Guard`]) but it can be EMBEDDED into the
//! rendered text more than once: a ratio guards both its numerator and its denominator
//! (`measure_expression`'s `Ratio` arm calls `ratio_term_expression` twice, once per side), and
//! `ClickHouse`'s 64-bit-overflow `SUM` widening (`sum_for_clickhouse`) embeds the guarded column
//! three times, once per candidate slot (twice for a ratio's `sum_as_double_for_clickhouse`). For a
//! dialect that writes a bare `?` with no index (`PlaceholderStyle::Question`), a repeated embedding
//! is a repeated `?` in the final text - and `crate::generate::generate` has to hand the transport as
//! many bound values as there are `?`s, in the same order. So every place a guard is actually EMBEDDED calls [`Guard::record`], which
//! appends that guard's own parameter values (in the order its own placeholders render) to the
//! caller's `emitted` list - and `generate` uses that list, rather than `plan.params()` verbatim,
//! for any dialect that binds a plain statement by occurrence: `Question` (DuckDB, BigQuery,
//! ClickHouse) and, despite its NAMED placeholders, `Colon` too - the oracledb driver's
//! `add_bind` still pushes one `BindInfo` per occurrence for a plain (non-PL/SQL) statement and
//! binds a positional slice, so `:1` embedded twice still needs its value twice. `Numbered`
//! (Postgres's `$n`) is the one style that truly binds by index, so a repeated placeholder there
//! already resolves to the right value with no tracking, and `generate` keeps `plan.params()`
//! unchanged for it alone.

use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::Aggregate;
use sutura_domain::plan::{PlanMeasure, PlanPredicate, PlanTerm};
use sutura_domain::warehouse::ParamValue;

use super::{Dialect, Expr, aggregate, builder, column, predicate};

/// One metric's guard: the boolean expression it renders, and the parameter values a caller must
/// append to its own `emitted` list once per place this guard is actually embedded - see the module
/// doc for why position alone cannot say that for a `Question`-style dialect.
pub(super) struct Guard {
    expr: Expr,
    values: Vec<ParamValue>,
}

impl Guard {
    /// This metric's guard predicates, `AND`ed into one expression, with the parameter values
    /// each carries resolved from `params` in the same order - or `None` if it has none.
    pub(super) fn build(dialect: Dialect, predicates: &[PlanPredicate], params: &[ParamValue]) -> Option<Self> {
        let mut iter = predicates.iter();
        let first = iter.next()?;
        let mut expr = predicate(dialect, first);
        let mut values = predicate_values(first, params);
        for next in iter {
            expr = expr.and(predicate(dialect, next));
            values.extend(predicate_values(next, params));
        }
        Some(Self { expr, values })
    }

    const fn expr(&self) -> &Expr {
        &self.expr
    }

    /// Records one more embedding of this guard in the final text: appends this guard's own
    /// parameter values, in order, to `emitted`. Call exactly once per place this guard's
    /// [`Self::expr`] is cloned into the rendered tree.
    fn record(&self, emitted: &mut Vec<ParamValue>) {
        emitted.extend(self.values.iter().cloned());
    }
}

/// One predicate's own bound values, resolved from `params` in the order [`predicate`] places its
/// placeholders - `In`/`NotIn` bind one per listed value, the four comparing variants bind one, and
/// `IsTrue`/`IsNotNull` bind none.
pub(super) fn predicate_values(plan_predicate: &PlanPredicate, params: &[ParamValue]) -> Vec<ParamValue> {
    #[expect(
        clippy::indexing_slicing,
        reason = "every index here is bound by `PlanBindings::parse` before a predicate can carry it"
    )]
    let at = |index: usize| params[index].clone();
    match *plan_predicate {
        PlanPredicate::AtOrAfter { param, .. }
        | PlanPredicate::Before { param, .. }
        | PlanPredicate::Equals { param, .. }
        | PlanPredicate::NotEquals { param, .. } => vec![at(param)],
        PlanPredicate::In { params: ref indices, .. } | PlanPredicate::NotIn { params: ref indices, .. } => {
            indices.iter().map(|&index| at(index)).collect()
        }
        PlanPredicate::IsTrue { .. } | PlanPredicate::IsNotNull { .. } => Vec::new(),
    }
}

/// Cast a Postgres `AVG` to `DOUBLE`, where a mean must arrive as a float.
///
/// Postgres's `AVG` over an INTEGER column returns `NUMERIC`, where the engine (and `DuckDB`)
/// produce `DOUBLE` for the same query - and `NUMERIC` with a fraction is the case this repository
/// keeps exact as text ("so an exact total stays exact"), which would turn a mean into a `Text` leg
/// that disagrees with the float every other adapter reaches. Casting the Postgres `AVG` to
/// `DOUBLE` on the WIRE keeps a mean a float there. It is the same `DOUBLE` cast the ratio path
/// already makes (proven accepted by every target we render for), and it touches only the Postgres
/// dialect.
fn avg_for_postgres(over: Expr, kind: Aggregate, dialect: Dialect) -> Expr {
    if matches!(kind, Aggregate::Avg) && dialect == Dialect::Postgres {
        over.cast("DOUBLE")
    } else {
        over
    }
}

/// `ClickHouse`'s native `sum` wraps at 64 bits. Widen an integer sum; keep the original for Float and Decimal.
///
/// **The result is `tuple(type name, integer, decimal, float)`, never a `Dynamic`.** The sum's type
/// depends on the column's declared type, so one expression has to answer an integer, a decimal or a
/// float - and the server refuses to write a `Dynamic` into an Arrow stream (`Code: 48`, measured
/// through the vendored driver against the tier). The type name is a per-query constant and says which
/// of the three numeric slots holds the total; the other two are `NULL`. A slot is a typed number, not
/// text, because the same expression is re-rendered in `ORDER BY`, where a text total would sort
/// `"9"` after `"10"`: the tuple compares its constant name, then the numeric slot, and a `NULL`
/// total sorts last as a plain nullable column does. The integer slot is `Decimal(76, 0)` through
/// `toDecimal256` - the one route from an `Int128` to an exact decimal, since `CAST` refuses it - so
/// every `Int128` the widened sum can hold is exact. The decimal slot is `Decimal(76, 38)`; a decimal
/// column of a wider scale is refused by the adapter from the type name, not truncated silently.
///
/// **The type probe never needs the guard.** `toTypeName(sum(col))`'s result depends on `col`'s
/// declared column type, not on which rows are summed, so every type check reads `bare`. Only the
/// three candidate VALUES need the guard, and each embeds it exactly once - `Guard::record` is called
/// once per candidate, in the order they render, which is what makes the final `?` count match
/// `emitted`'s length for this dialect.
fn sum_for_clickhouse(bare: &Expr, guard: Option<&Guard>, emitted: &mut Vec<ParamValue>) -> Expr {
    let no_integer = || builder::null().cast("Nullable(Decimal(76, 0))");
    let no_decimal = || builder::null().cast("Nullable(Decimal(76, 38))");
    let no_float = || builder::null().cast("Nullable(Float64)");
    let widened = builder::func(
        "tuple",
        [
            builder::lit("Int128"),
            builder::func("toDecimal256", [widened_sum(bare, guard, emitted), builder::lit(0)]),
            no_decimal(),
            no_float(),
        ],
    );
    let decimal = builder::func(
        "toDecimal256",
        [builder::sum(guarded_value(bare, guard, emitted)), builder::lit(38)],
    );
    let float = builder::func("toFloat64", [builder::sum(guarded_value(bare, guard, emitted))]);
    let original = builder::func(
        "tuple",
        [
            probe_type(bare),
            no_integer(),
            builder::func(
                "if",
                [probe_type(bare).like(builder::lit("%Decimal(%")), decimal, builder::null()],
            ),
            builder::func("if", [probe_type(bare).like(builder::lit("%Float%")), float, builder::null()]),
        ],
    );
    builder::func("if", [is_integer_sum(bare), widened, original])
}

/// The same sum as one `Float64`, for a ratio: a ratio divides in floating point, so it needs the
/// magnitude and none of the exact digits the tuple carries.
fn sum_as_double_for_clickhouse(bare: &Expr, guard: Option<&Guard>, emitted: &mut Vec<ParamValue>) -> Expr {
    let widened = builder::func("toFloat64", [widened_sum(bare, guard, emitted)]);
    let original = builder::func("toFloat64", [builder::sum(guarded_value(bare, guard, emitted))]);
    builder::func("if", [is_integer_sum(bare), widened, original])
}

/// `toTypeName(sum(col))`, a constant per column type.
fn probe_type(bare: &Expr) -> Expr {
    builder::func("toTypeName", [builder::sum(bare.clone())])
}

/// Whether the column's sum is one of the integer types `ClickHouse` wraps (or, for `Int128`, would not
/// widen further), so the widened `Int128` sum is the one to read.
fn is_integer_sum(bare: &Expr) -> Expr {
    probe_type(bare).in_list(
        ["Int64", "UInt64", "Int128"]
            .into_iter()
            .flat_map(|name| [name.to_owned(), format!("Nullable({name})")])
            .map(|name| builder::lit(name.as_str())),
    )
}

/// The column's `Int128` sum: `accurateCastOrNull` gives `NULL`, not a wrapped number, for a value that
/// does not fit - and for a float row, which is why the type check comes first.
fn widened_sum(bare: &Expr, guard: Option<&Guard>, emitted: &mut Vec<ParamValue>) -> Expr {
    builder::sum(builder::func(
        "accurateCastOrNull",
        [guarded_value(bare, guard, emitted), builder::lit("Int128")],
    ))
}

/// The column, wrapped in the metric's guard where there is one - one embedding, recorded once.
fn guarded_value(bare: &Expr, guard: Option<&Guard>, emitted: &mut Vec<ParamValue>) -> Expr {
    guard.map_or_else(
        || bare.clone(),
        |g| {
            g.record(emitted);
            builder::case().when(g.expr().clone(), bare.clone()).build()
        },
    )
}

/// A term's column, wrapped in `guard` when one is present - one embedding, recorded once.
///
/// A metric guard is that metric's own REQUIRED filters, folded into a conditional aggregate
/// (`CASE WHEN <guard> THEN col END`) rather than the shared `WHERE`, which would constrain every
/// metric's column at once. `None` (a single-metric plan) returns the column bare, byte-identical
/// to before the guard existed.
fn guarded_column(col: Expr, guard: Option<&Guard>, emitted: &mut Vec<ParamValue>) -> Expr {
    match guard {
        Some(guard) => {
            guard.record(emitted);
            builder::case().when(guard.expr().clone(), col).build()
        }
        None => col,
    }
}

/// One term, as one expression.
///
/// A conditional count is `SUM(CASE WHEN col THEN 1 ELSE 0 END)` rather than the dialect layer's own
/// `CountIf` node. That node does lower correctly for all three of our targets, unlike `SafeDivide`
/// below, so this is the weaker of the two decisions - but it keeps every term rendered by one
/// mechanism we can read, and it counts 0 rather than null for a false row, so a period with no
/// matches answers 0 instead of nothing.
pub(super) fn term_expression(term: &PlanTerm, guard: Option<&Guard>, dialect: Dialect, emitted: &mut Vec<ParamValue>) -> Expr {
    match *term {
        PlanTerm::Aggregate {
            aggregate: kind,
            column: ref col,
        } if matches!(kind, Aggregate::Sum) && dialect == Dialect::ClickHouse => sum_for_clickhouse(&column(col), guard, emitted),
        PlanTerm::Aggregate {
            aggregate: kind,
            column: ref col,
        } => avg_for_postgres(aggregate(kind, guarded_column(column(col), guard, emitted)), kind, dialect),
        PlanTerm::CountIf { column: ref col } => {
            // A `CountIf` already guards its own (boolean) column; a metric guard is ANDed into
            // the same WHEN branch rather than wrapping the column again - one embedding.
            let when = guard.map_or_else(
                || column(col),
                |g| {
                    g.record(emitted);
                    column(col).and(g.expr().clone())
                },
            );
            builder::sum(builder::case().when(when, builder::lit(1)).else_(builder::lit(0)).build())
        }
    }
}

fn ratio_term_expression(term: &PlanTerm, guard: Option<&Guard>, dialect: Dialect, emitted: &mut Vec<ParamValue>) -> Expr {
    match *term {
        PlanTerm::Aggregate {
            aggregate: Aggregate::Sum,
            column: ref col,
        } if dialect == Dialect::ClickHouse => sum_as_double_for_clickhouse(&column(col), guard, emitted),
        _ => term_expression(term, guard, dialect, emitted),
    }
}

/// The measure, as one expression.
///
/// A ratio is rendered as a division with a `NULLIF` on the denominator, rather than through the
/// dialect layer's own `SafeDivide` node - and that is a measured decision rather than ignorance of
/// the node.
///
/// The dialect layer does carry typed `SafeDivide` and `CountIf` nodes, and they lower correctly for
/// some targets: `SAFE_DIVIDE` for one, a `CASE` for another, `COUNTIF` and `countIf` for two more.
/// But `SafeDivide` has **no Postgres lowering** - the generator falls through to writing the literal
/// text `SAFE_DIVIDE(x, y)`, which is not a function Postgres has - and Postgres is a target we
/// render for. Using the node would produce a statement that is valid in most of the dialects we
/// render for and a call to a non-existent function in Postgres.
///
/// `NULLIF` and `/` exist in every dialect we render for, and a division by null is null in each, so
/// this form is identical in behaviour and portable by construction. Revisit it if the node gains that lowering;
/// until then the golden that parses every statement in its target dialect is what would catch the
/// regression - with the limit `crate::dialect::DateTruncShape` records, which is that such a golden
/// sees syntax and not a function's argument contract.
///
/// The numerator is cast to a floating type first. Integer division truncates in Postgres and in
/// `DuckDB` - `SUM(cents) / COUNT(*)` would silently return a whole number - which is the wrong answer
/// for every ratio anybody actually wants. **A ratio guards both terms independently** - each of
/// [`ratio_term_expression`]'s two calls embeds and records the guard on its own side, so a guarded
/// ratio's parameter count in the final text is exactly twice the guard's own, which `emitted`
/// reflects because each embedding records itself.
pub(super) fn measure_expression(
    measure: &PlanMeasure,
    guard: Option<&Guard>,
    dialect: Dialect,
    emitted: &mut Vec<ParamValue>,
) -> Expr {
    match *measure {
        PlanMeasure::Simple { ref term } => term_expression(term, guard, dialect, emitted),
        PlanMeasure::Ratio {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => {
            let top = ratio_term_expression(numerator, guard, dialect, emitted).cast("DOUBLE");
            let bottom = ratio_term_expression(denominator, guard, dialect, emitted);
            let bottom = match zero_denominator {
                ZeroDenominator::Null => builder::null_if(bottom, builder::lit(0)),
                ZeroDenominator::Fail => bottom,
            };
            top.div(bottom)
        }
    }
}

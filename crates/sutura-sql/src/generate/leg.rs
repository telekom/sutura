//! One leg as one statement: the body [`super::generate_leg`] and each restriction of it share, so
//! a restricted leg projects, groups and binds exactly as the whole one does.

use polyglot_sql::builder::{self, Expr};
use polyglot_sql::expressions::Expression;
use sutura_domain::plan::{LegPlan, PlanJoin};

use super::{
    GenerateError, aliased, assert_placeholder_count, bucket_expression, joined, key_expr, ordered_nulls_last, predicate, render,
    table_path, term_expression,
};
use crate::GeneratedQuery;
use crate::dialect::Dialect;

/// [`super::generate_leg`], with `also` joined by `AND` after the leg's own filters, so their parameter order holds.
pub(super) fn leg_within(leg: &LegPlan, dialect: Dialect, also: Option<Expr>) -> Result<GeneratedQuery, GenerateError> {
    let ast = leg_statement(leg, dialect, also)?;
    finish(leg, dialect, &ast)
}

/// [`leg_within`]'s statement, before it is rendered.
pub(super) fn leg_statement(leg: &LegPlan, dialect: Dialect, also: Option<Expr>) -> Result<Expression, GenerateError> {
    // Keys first, in leg order, and both grouped by and projected. `LegPlan::result_labels` states
    // the same order for whatever reads the rows back.
    let mut projection = Vec::with_capacity(leg.keys().len().saturating_add(1));
    let mut grouping = Vec::with_capacity(leg.keys().len().saturating_add(1));
    for key in leg.keys() {
        projection.push(aliased(key_expr(key, dialect), key.label())?);
        grouping.push(key_expr(key, dialect));
    }

    // THE rendering arm. A third leg shape does not compile until it says what it projects.
    let joins: &[PlanJoin] = match *leg {
        LegPlan::Fact {
            ref bucket,
            ref terms,
            ref tables,
            ..
        } => {
            let bucket_expr = bucket_expression(bucket, dialect);
            projection.push(aliased(bucket_expr.clone(), bucket.label())?);
            grouping.push(bucket_expr);
            // Zero to four of them. Empty is the distinct-key leg, and it is not a special case
            // here: the projection is then the key list and the bucket, grouped by itself.
            for term in terms {
                // A leg projects bare decomposed terms, never a guarded measure - guards belong to
                // the whole-answer `generate` path, so `None` keeps this byte-identical.
                // A leg projects bare decomposed terms, never a guarded measure, so `emitted` is
                // never read - `generate_leg` keeps `leg.params()` unchanged below.
                projection.push(aliased(
                    term_expression(term.term(), None, dialect, &mut Vec::new()),
                    term.label(),
                )?);
            }
            tables.joins()
        }
        // No bucket, no terms. It projects its keys and groups by them, which is the distinct set
        // of dimension rows surviving its own filters, and joins whatever the chain carried on to
        // after it crossed onto this system.
        LegPlan::Lookup { ref table, .. } => table.joins(),
    };

    let mut statement = joined(
        builder::select(projection).from(&table_path(leg.table(), dialect)?),
        joins,
        dialect,
    )?;

    // Folded in leg order, which is parameter order, exactly as `generate` folds a plan's.
    let mut clauses = leg.filters().iter().map(|f| predicate(dialect, f.predicate())).chain(also);
    if let Some(first) = clauses.next() {
        statement = statement.where_(clauses.fold(first, Expr::and));
    }

    let tiebreak: Vec<Expr> = grouping.iter().cloned().map(ordered_nulls_last).collect();
    // Ordered by what it groups by, for `generate`'s reason: without it a leg's row order is
    // unspecified and a golden over it flaps. Each ascending, nulls last, like `generate`.
    Ok(statement.group_by(grouping).order_by(tiebreak).build())
}

/// `ast`, a statement of `leg`, rendered with the leg's parameters and checked against them.
pub(super) fn finish(leg: &LegPlan, dialect: Dialect, ast: &Expression) -> Result<GeneratedQuery, GenerateError> {
    let params = leg.params().to_vec();
    let sql = render(ast, dialect)?;
    assert_placeholder_count(ast, dialect, &params)?;
    Ok(GeneratedQuery::new(leg.source().clone(), sql, params))
}

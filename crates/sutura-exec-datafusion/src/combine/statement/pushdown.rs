//! The conjuncts of a filter over a Cube table that Cube may answer before the local text policy.
//!
//! Cube cannot apply a text policy, so the engine does, over the raw rows Cube answers. A conjunct
//! sent to Cube only narrows those rows when it reads no policy column and Cube compares as the
//! engine does. Every conjunct stays in the local filter as well, so a Cube that answers more rows
//! than the conjunct selects is corrected; one that answers fewer is not, which is why only a
//! comparison that cannot be stricter at Cube is listed.

use std::sync::Arc;

use datafusion::arrow::datatypes::DataType;
use datafusion::common::{Column, DFSchema, ExprSchema as _, ScalarValue};
use datafusion::logical_expr::expr::InList;
use datafusion::logical_expr::utils::{conjunction, split_conjunction};
use datafusion::logical_expr::{BinaryExpr, Expr, Filter, LogicalPlan, Operator, Projection};

use super::text::KEY;

/// The conjunction of the terms of `predicate` Cube answers exactly, or `None` when no term is.
///
/// Listed: a text column equal to a text literal or in a list of them, a null test of any column, and
/// an `AND` or `OR` of those, which is how the engine spells a short list. Everything else stays local. A text order or inequality is not listed, because Cube compares text
/// by its warehouse's collation and could answer fewer rows than the engine's comparison keeps. A
/// number or a time is not, because Cube holds it at its own precision. A boolean is not, because
/// the engine rewrites `flag = true` to a bare column, whose reading as a condition Cube is not
/// known to share. A function, cast, arithmetic or a subquery is not, because it could raise at Cube
/// on a row the local plan never reaches.
pub(super) fn is_filtered_scan(node: &LogicalPlan) -> bool {
    matches!(node, LogicalPlan::Filter(filter) if matches!(filter.input.as_ref(), LogicalPlan::TableScan(_)))
}

pub(super) fn cube_scan(filter: &Filter) -> datafusion::error::Result<Option<LogicalPlan>> {
    let Some(pushed) = answered_by_cube(&filter.predicate, filter.input.schema()) else {
        return Ok(None);
    };
    let scan = LogicalPlan::Filter(Filter::try_new(pushed, Arc::clone(&filter.input))?);
    let columns = scan.schema().columns().into_iter().map(Expr::Column).collect();
    Ok(Some(LogicalPlan::Projection(Projection::try_new(columns, Arc::new(scan))?)))
}

pub(super) fn answered_by_cube(predicate: &Expr, schema: &DFSchema) -> Option<Expr> {
    conjunction(
        split_conjunction(predicate)
            .into_iter()
            .filter(|term| exact(term, schema))
            .cloned(),
    )
}

fn exact(term: &Expr, schema: &DFSchema) -> bool {
    match term {
        Expr::BinaryExpr(BinaryExpr { left, op, right }) => match (op, left.as_ref(), right.as_ref()) {
            (Operator::Eq, Expr::Column(column), Expr::Literal(ScalarValue::Utf8(Some(_)), _))
            | (Operator::Eq, Expr::Literal(ScalarValue::Utf8(Some(_)), _), Expr::Column(column)) => text(column, schema),
            (Operator::And | Operator::Or, left, right) => exact(left, schema) && exact(right, schema),
            _ => false,
        },
        Expr::InList(InList {
            expr,
            list,
            negated: false,
        }) => {
            matches!(expr.as_ref(), Expr::Column(column) if text(column, schema))
                && list
                    .iter()
                    .all(|item| matches!(item, Expr::Literal(ScalarValue::Utf8(Some(_)), _)))
        }
        Expr::IsNull(expression) | Expr::IsNotNull(expression) => {
            matches!(expression.as_ref(), Expr::Column(column) if free(column, schema))
        }
        _ => false,
    }
}

/// A column that carries no text policy.
fn free(column: &Column, schema: &DFSchema) -> bool {
    schema
        .field_from_column(column)
        .is_ok_and(|field| !field.metadata().contains_key(KEY))
}

/// A text column that carries no text policy.
fn text(column: &Column, schema: &DFSchema) -> bool {
    free(column, schema)
        && schema
            .field_from_column(column)
            .is_ok_and(|field| *field.data_type() == DataType::Utf8)
}

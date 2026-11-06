//! A granted lookup leg read once (`github.com/telekom/sutura#1771`): its ambiguous-link check is
//! part of the plan that joins it, so the scan the join reads is the scan the check counts.
//!
//! The leg is grouped by its link columns, none of them null, each other column carried as its
//! group's one value, and a filter over each group's row count fails the plan under the code
//! [`CombineError::AmbiguousLink`](crate::combine::CombineError::AmbiguousLink) is read back from
//! when a count passes one. That relation is the build side of a RIGHT join onto the relation every
//! earlier leg built. A hash join reads its build side whole before its probe side, and a RIGHT
//! one reads its probe side even when its build side is empty or holds no joinable row, so the
//! join does not skip a count: neither this leg's when the relation has no row to match, nor an
//! earlier granted leg's, inside the relation, when this leg has none. An INNER leg drops the
//! relation's unmatched rows above the join, under a function the engine cannot read as rejecting
//! nulls, so the outer-join elimination does not turn the join back into the INNER one that would
//! skip its probe side. Both hold only while the join stays in the plan: see the limit on a leg nothing above reads.
//!
//! # Limits
//!
//! - The build side is the join order this plan builds, kept only because the session's
//!   statistics-driven join reordering is turned off for the whole combine that reads a granted leg.
//! - No filter the plan builds reaches the count from outside it: the engine pushes none into the
//!   build side of a RIGHT join, and a RIGHT join pushes no dynamic filter into its probe side. So
//!   an INNER leg's unmatched relation rows are not pruned at their scan either, and flow through
//!   the join to the filter that drops them. Not measured.
//! - The whole leg is held, as the count's group state and then as the join's build side, on a
//!   worker whose pool spills nowhere: a large leg can be refused for its working set. Not measured.
//! - Each other column is carried as `max` of its group, which is that group's one value in every
//!   group that is not refused. `max` takes every column type when the plan is built, so a column
//!   of a type it cannot compare is refused only when a worker updates its accumulator, after the
//!   partitions are read, as `CombineError::Execute` and not `CombineError::Build`; no cell carries
//!   such a column.
//! - A leg nothing above reads: a LEFT granted leg none of whose columns is read above its join is
//!   removed from the plan, count included, by `datafusion-optimizer` 55.1.0's default
//!   `EliminateJoin` rule. It replaces a RIGHT join with its probe side when no build-side column is
//!   read above the join and the build side is unique on the join keys, which the group by the link
//!   columns makes it. An ambiguous link on that leg would then be answered, not refused, and
//!   volatility does not stop the rule, which does not read it. No planned question has that shape
//!   today, since the planner gives every LEFT leg a dimension key or a later leg's link
//!   (`sutura-semantic`'s `lookup_legs`), and no type or gate holds that. A null-rejecting filter on
//!   a leg's column would make the RIGHT join INNER, which skips its probe side, and a filter on the
//!   relation's join key would be inferred onto the link and pushed into the count: the plan builds
//!   neither today, and nothing holds that either.
//! - The count is a stage of the question, so the refusal is an engine failure of that stage, read
//!   back by its code: a peer can raise it for a question it fails, never answer one with it.
//! - Every node must register both filters' functions by name: a node built without them cannot
//!   decode the stage, and the question fails as
//!   [`CombineError::Execute`](crate::combine::CombineError::Execute).

use std::sync::Arc;

use datafusion::arrow::array::AsArray as _;
use datafusion::arrow::compute::is_not_null;
use datafusion::arrow::datatypes::{DataType, Int64Type};
use datafusion::common::{Column, JoinType, ScalarValue};
use datafusion::error::DataFusionError;
use datafusion::execution::SessionStateBuilder;
use datafusion::functions_aggregate::expr_fn::{count, max};
use datafusion::logical_expr::{
    ColumnarValue, Expr, LogicalPlan, LogicalPlanBuilder, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature, Volatility,
    lit,
};

use super::relay;
use crate::combine::arm::qualified;
use crate::combine::probe_label;

/// The name the count's filter function is registered and sent under.
const NAME: &str = "sutura_one_row_per_link";

/// The name the INNER leg's filter function is registered and sent under.
const MATCHED: &str = "sutura_matched";

/// True for every group whose row count is at most one, and the plan's failure otherwise.
///
/// Volatile, as a guard: no optimizer rule may fold or move the filter it is. No cell tells that
/// from `Immutable`, which a filter over an aggregate's own output is not moved under either.
/// It does not keep the count in the plan: see the module's limit on a leg nothing above reads.
#[derive(Debug, PartialEq, Eq, Hash)]
struct OneRowPerLink {
    signature: Signature,
}

impl OneRowPerLink {
    fn new() -> Self {
        Self {
            signature: Signature::exact(vec![DataType::Int64], Volatility::Volatile),
        }
    }
}

impl ScalarUDFImpl for OneRowPerLink {
    fn name(&self) -> &str {
        NAME
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _: &[DataType]) -> Result<DataType, DataFusionError> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue, DataFusionError> {
        let [counted] = args.args.as_slice() else {
            return Err(DataFusionError::Internal(format!("`{NAME}` reads one row count")));
        };
        let counted = counted.to_array(args.number_rows)?;
        let repeated = counted
            .as_primitive_opt::<Int64Type>()
            .ok_or_else(|| DataFusionError::Internal(format!("`{NAME}` reads a 64-bit row count")))?
            .iter()
            .any(|rows| rows.is_some_and(|rows| rows > 1));
        if repeated {
            return Err(relay::ambiguous(DataFusionError::Execution(String::from(
                "a link value maps to more than one row of a granted leg",
            ))));
        }
        Ok(ColumnarValue::Scalar(ScalarValue::Boolean(Some(true))))
    }
}

/// True where its argument is not null, which for a link column of the build side is a row the
/// RIGHT join matched.
///
/// Not strict, the engine's default, and truly so: a null answers false. A function declared strict
/// over the build side's columns would let the engine's outer-join elimination make the join INNER.
/// Volatile as a guard, as the count's filter is; no cell tells that from `Immutable`. It keeps its
/// link column read above the join, so the join is not removed as unread: that holds for an INNER
/// leg only.
#[derive(Debug, PartialEq, Eq, Hash)]
struct Matched {
    signature: Signature,
}

impl Matched {
    fn new() -> Self {
        Self {
            signature: Signature::any(1, Volatility::Volatile),
        }
    }
}

impl ScalarUDFImpl for Matched {
    fn name(&self) -> &str {
        MATCHED
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _: &[DataType]) -> Result<DataType, DataFusionError> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue, DataFusionError> {
        let [value] = args.args.as_slice() else {
            return Err(DataFusionError::Internal(format!("`{MATCHED}` reads one column")));
        };
        let values = value.to_array(args.number_rows)?;
        Ok(ColumnarValue::Array(Arc::new(is_not_null(&values)?)))
    }
}

/// `builder`, its sessions able to decode the stages [`joined`] plans.
pub(super) fn knowing(mut builder: SessionStateBuilder) -> SessionStateBuilder {
    let functions = builder.scalar_functions().get_or_insert_default();
    functions.push(Arc::new(ScalarUDF::new_from_impl(OneRowPerLink::new())));
    functions.push(Arc::new(ScalarUDF::new_from_impl(Matched::new())));
    builder
}

/// `scanned`, the granted leg registered as `table`, [`checked`] on its link columns `links` and
/// joined onto `relation` on `on` as the build side of a RIGHT join, which keeps the relation's rows
/// the leg has none for when `include_unmatched` and drops them otherwise.
///
/// # Errors
///
/// The engine's failure to build the plan, and [`DataFusionError::Internal`] for a leg with no
/// link column, which nothing would drop an unmatched row on.
pub(crate) fn joined(
    relation: LogicalPlan,
    scanned: LogicalPlan,
    table: &str,
    links: &[String],
    on: Vec<Expr>,
    include_unmatched: bool,
) -> Result<LogicalPlan, DataFusionError> {
    let joined = LogicalPlanBuilder::from(checked(scanned, table, links)?).join_on(relation, JoinType::Right, on)?;
    if include_unmatched {
        return joined.build();
    }
    let matched = links
        .iter()
        .map(|link| ScalarUDF::new_from_impl(Matched::new()).call(vec![qualified(table, link)]))
        .reduce(Expr::and)
        .ok_or_else(|| DataFusionError::Internal(format!("the granted leg `{table}` joins on no link column")))?;
    joined.filter(matched)?.build()
}

/// `scanned`, the granted leg registered as `table`, as the relation its join reads: one row per
/// value of its link columns `on`, none of them null, and the plan failing as an ambiguous link
/// where a value has more than one.
fn checked(scanned: LogicalPlan, table: &str, on: &[String]) -> Result<LogicalPlan, DataFusionError> {
    let probe = probe_label();
    let links: Vec<Expr> = on.iter().map(|label| qualified(table, label)).collect();
    let joinable = links.iter().fold(lit(true), |all, link| all.and(link.clone().is_not_null()));
    let names: Vec<String> = scanned.schema().fields().iter().map(|field| field.name().clone()).collect();
    let carried = names
        .iter()
        .filter(|name| !on.contains(name))
        .map(|name| max(qualified(table, name)).alias(name.as_str()))
        .chain([count(lit(1_i64)).alias(probe.as_str())]);
    let kept = names.iter().map(|name| {
        if on.contains(name) {
            qualified(table, name)
        } else {
            Expr::Column(Column::new_unqualified(name.as_str()))
        }
    });
    let counted = Expr::Column(Column::new_unqualified(probe.as_str()));
    LogicalPlanBuilder::from(scanned)
        .filter(joinable)?
        .aggregate(links, carried)?
        .filter(ScalarUDF::new_from_impl(OneRowPerLink::new()).call(vec![counted]))?
        .project(kept)?
        .alias(table)?
        .build()
}

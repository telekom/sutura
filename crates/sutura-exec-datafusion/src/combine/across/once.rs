//! A granted lookup leg read once (`github.com/telekom/sutura#1771`): its ambiguous-link check is
//! part of the plan that joins it, so the scan the join reads is the scan the check counts.
//!
//! The leg is grouped by its link columns, none of them null, each other column carried as its
//! group's one value, and a filter over each group's row count fails the plan under the code
//! [`CombineError::AmbiguousLink`](crate::combine::CombineError::AmbiguousLink) is read back from
//! when a count passes one. The join reads the grouped relation as its build side, which a hash
//! join reads whole before the other side, so the check runs even when the side it joins onto has
//! no row to match. Only an arm's one granted leg is checked here
//! (`served::counted_in_plan` says why).
//!
//! # Limits
//!
//! - The build side is the join order this plan builds, kept only because the session's
//!   statistics-driven join reordering is turned off for the whole combine that reads a granted leg.
//! - The whole leg is held, as the count's group state and then as the join's build side, on a
//!   worker whose pool spills nowhere: a large leg can be refused for its working set. Not measured.
//! - Each other column is carried as `max` of its group, which is that group's one value in every
//!   group that is not refused. A column of a type `max` does not take fails the plan when it is
//!   built, as `CombineError::Build`; no cell carries such a column.
//! - The count is a stage of the question, so the refusal is an engine failure of that stage, read
//!   back by its code: a peer can raise it for a question it fails, never answer one with it.
//! - Every node must register the filter's function by name: a node built without it cannot decode
//!   the stage, and the question fails as [`CombineError::Execute`](crate::combine::CombineError::Execute).

use std::sync::Arc;

use datafusion::arrow::array::AsArray as _;
use datafusion::arrow::datatypes::{DataType, Int64Type};
use datafusion::common::{Column, ScalarValue};
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

/// The name the filter's function is registered and sent under.
const NAME: &str = "sutura_one_row_per_link";

/// True for every group whose row count is at most one, and the plan's failure otherwise.
///
/// Volatile, as a guard: no optimizer rule may fold or move the filter it is. No cell tells that
/// from `Immutable`, which a filter over an aggregate's own output is not moved under either.
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

/// `builder`, its sessions able to decode the stage [`checked`] plans.
pub(super) fn knowing(mut builder: SessionStateBuilder) -> SessionStateBuilder {
    builder
        .scalar_functions()
        .get_or_insert_default()
        .push(Arc::new(ScalarUDF::new_from_impl(OneRowPerLink::new())));
    builder
}

/// `scanned`, the granted leg registered as `table`, as the relation its join reads: one row per
/// value of its link columns `on`, none of them null, and the plan failing as an ambiguous link
/// where a value has more than one.
///
/// # Errors
///
/// The engine's failure to build the plan.
pub(crate) fn checked(scanned: LogicalPlan, table: &str, on: &[String]) -> Result<LogicalPlan, DataFusionError> {
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

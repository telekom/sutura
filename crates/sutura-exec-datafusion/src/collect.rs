//! The result side of this adapter: what the engine hands back, and what a domain row is.
//!
//! # The seam
//!
//! [`crate::translate`] is the half that never reads a result - a plan becomes expressions there,
//! and nothing in it has seen a row. This is the mirror half: an Arrow schema is checked against the
//! plan's labels. Between the two, `lib.rs` keeps what neither of them is about - the session, the
//! runtime, attaching a file, and executing.
//!
//! **The value mapping used to be here and is not any more.** `docs/adr/0039` moved it to
//! `sutura_domain::warehouse::arrow`, because an Arrow array becoming a `Value` was implemented
//! three times - here, in the `DuckDB` adapter, and in `BigQuery`'s via a cast to text - and the
//! agreement between those copies is a correctness property rather than tidiness. What is left here
//! is the half that reads a PLAN's labels, which is this adapter's and no other's.
//!
//! **The other half of the result side is the READ itself** - one frame streamed into the interior's
//! accumulation guard, under a byte budget - and it lives here for the same reason: `lib.rs` is at
//! the 1000-line gate and the gate's answer is to split the file rather than to shorten the change.
//! The seam is the one `lib.rs` already named in prose before there was a file on either side of it,
//! and it is the same seam either way: nothing above this module reads a value out of a batch.
//!

use std::sync::Arc;

use datafusion::common::{Column, DFSchema};
use datafusion::logical_expr::Expr;
use datafusion::prelude::DataFrame;
// Only for `StreamExt::next`, which is what lets `collected` charge a batch before asking for the
// next one. Already in this crate's graph through `datafusion` itself, so this is an edge and not a
// new package.
use futures_util::StreamExt as _;
use sutura_domain::warehouse::{Accumulating, ResultBatches};

use crate::DataFusionError;
use crate::pool::WorkingSet;

/// The aliased projection, and the expressions to order the result by.
///
/// Named rather than written out, because the two lists are produced together and consumed one line
/// apart: separating them into two passes over the same schema is how they would come to disagree
/// about which position is a grouped one.
pub(crate) type Projected = (Vec<Expr>, Vec<Expr>);

/// The final projection, and the expressions to order by.
///
/// The projection references the aggregate's own output fields rather than re-stating the grouped
/// expressions, because after aggregating there is no `orders.order_date` left to truncate - the
/// truncated value *is* a field. Each is aliased so the result labels are exactly
/// `QueryPlan::result_labels` in order: the keys, then the time bucket, then the measure.
///
/// Ordering is by the projected label columns, unaliased, for the grouped positions only. That is
/// the SQL path's "order by what it grouped by", and it is what makes two runs of one question
/// return rows in one order - which a differential test over row order depends on.
pub(crate) fn outputs(schema: &DFSchema, labels: &[String], group_count: usize) -> Result<Projected, DataFusionError> {
    if schema.fields().len() != labels.len() {
        return Err(DataFusionError::SchemaMismatch {
            expected: labels.to_vec(),
            actual: schema.fields().iter().map(|f| String::from(f.name().as_str())).collect(),
        });
    }
    let mut projection = Vec::with_capacity(labels.len());
    let mut ordering = Vec::with_capacity(group_count);
    for (index, ((qualifier, field), label)) in schema.iter().zip(labels.iter()).enumerate() {
        let reference = Expr::Column(Column::new(qualifier.cloned(), field.name().as_str()));
        projection.push(reference.alias(label.as_str()));
        if index < group_count {
            // Unqualified: an aliased projection field carries no qualifier, so this is the name the
            // sort resolves against.
            ordering.push(Expr::Column(Column::new_unqualified(label.as_str())));
        }
    }
    Ok((projection, ordering))
}

/// The field names a frame's own schema carries, which is what a result set is labelled by.
///
/// Read off the frame rather than off whatever asked for it, so a projection that came back a
/// different shape cannot be relabelled into the shape the caller wanted.
pub(crate) fn labels_of(frame: &DataFrame) -> Vec<String> {
    frame
        .schema()
        .fields()
        .iter()
        .map(|f| String::from(f.name().as_str()))
        .collect()
}

/// A frame's batches, checked against the frame's own announced schema.
///
/// **One collector for the answer path, the boot probe and the combine**, so the schema check
/// cannot be one thing for a question and another for a check. It no longer decodes: since
/// `docs/adr/0039` step 2 the port's currency IS [`ResultBatches`], so the batches leave this crate
/// as batches and `ResultBatches::to_rows` runs once, above the port - or, for the boot probe and
/// the key-uniqueness probe, at the one call that needs rows and says so.
///
/// **The row ceiling is `usize::MAX` here, and the byte budget beside it is why that is a decision
/// rather than an omission.** `docs/adr/0009` retired the per-leg ROW cap and said why: a row count
/// has no relationship to the memory a result costs, because a caller controls row width. So the
/// quantity this function bounds is bytes, and a second row-count ceiling on the engine's own output
/// would refuse legitimate narrow answers at a threshold unrelated to the resource being protected.
/// A FOREIGN driver still gets a row ceiling as well - `MOST_RESULT_ROWS` in the `BigQuery` adapter
/// is that caller - so `UnannouncedBatch::OverBound` is unreachable through this function and
/// `UnannouncedBatch::OverBudget` is not.
///
/// **Streamed rather than collected, which is the half that makes the budget a bound at all.** This
/// used to call `DataFrame::collect`, which retains every batch before anything can look at the
/// total: a budget checked after that returns arrives once the process is already dead. Round 7 of
/// `telekom/sutura#929`'s review measured exactly that. `execute_stream` produces one batch at a
/// time - coalesced across partitions by the engine, so the order is `collect`'s order - and each is
/// charged and refused before the next is asked for.
///
/// **Takes the `WorkingSet` and derives the budget here rather than taking a budget**, so no call
/// site has a number of its own to pass: `WorkingSet::result_budget` is the one conversion, and the
/// only value of that type in any caller's scope is the ceiling the adapter was constructed with.
pub(crate) async fn collected(frame: DataFrame, working_set: WorkingSet) -> Result<ResultBatches, DataFusionError> {
    let budget = working_set.result_budget();
    let announced = Arc::clone(frame.schema().inner());
    let mut stream = frame
        .execute_stream()
        .await
        .map_err(|cause| DataFusionError::Execute { cause })?;
    let mut accumulating = Accumulating::announcing(announced, usize::MAX, budget);
    while let Some(batch) = stream.next().await {
        accumulating
            .push(batch.map_err(|cause| DataFusionError::Execute { cause })?)
            .map_err(|cause| DataFusionError::Unannounced { cause })?;
    }
    Ok(accumulating.finish())
}

/// The byte budget, biting on the engine's own collection.
#[cfg(test)]
mod budget_tests;

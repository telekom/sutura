//! Materialized combine results under their existing Arrow budget.

use super::CombineError;
#[cfg(feature = "distributed")]
use super::across;
use crate::pool::{self, WorkingSet};
use futures_util::StreamExt as _;
use std::sync::Arc;
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::arrow::{Accumulating, UnannouncedBatch};

/// A frame's batches, streamed into the guard, checked against the frame's own announced schema and
/// charged against the byte budget as they arrive.
///
/// The row ceiling is `usize::MAX` for `crate::collected`'s own reason - `docs/adr/0009` retired the
/// row cap because a row count is not a memory bound - and the byte budget is what replaced it.
/// Nothing calls `DataFrame::collect` here any more: a combine that retained every batch before
/// charging anything would refuse after the memory was spent.
///
/// **An over-budget combined answer leaves as [`CombineError::Exhausted`] and not as
/// [`CombineError::Unannounced`]**, which is a classification decision rather than a convenience.
/// The budget is the working-set ceiling (`WorkingSet::result_budget`), so what the caller needs to
/// be told is that this question wanted more memory than the configured number - the same thing a
/// refused operator reservation means, from the same number, and `working_set_exhausted` is the one
/// predicate the combiner port has to say it with. Routing it through `Unannounced` instead would
/// reach the caller as a data-system failure inviting a retry.
pub(super) async fn collected(
    frame: datafusion::prelude::DataFrame,
    ceiling_bytes: u64,
    working_set: WorkingSet,
) -> Result<ResultBatches, CombineError> {
    let budget = working_set.result_budget();
    let announced = Arc::clone(frame.schema().inner());
    // `pool::exhausted` uses `find_root`, because a reservation is refused deep inside an operator
    // and the engine wraps the error on the way out. Matching the outermost variant would report
    // almost every real exhaustion as a transport failure, which is the direction that tells a
    // caller to retry a bound that fires again in the same place.
    let exhaustion = |cause: datafusion::error::DataFusionError| {
        if pool::exhausted(&cause) {
            CombineError::Exhausted { ceiling_bytes }
        } else {
            CombineError::Execute { cause }
        }
    };
    #[cfg(feature = "distributed")]
    let (mut stream, capture) = across::executing(frame, working_set).await.map_err(exhaustion)?;
    #[cfg(not(feature = "distributed"))]
    let mut stream = frame.execute_stream().await.map_err(exhaustion)?;
    let mut accumulating = Accumulating::announcing(announced, usize::MAX, budget);
    while let Some(batch) = stream.next().await {
        accumulating.push(batch.map_err(exhaustion)?).map_err(|cause| match cause {
            UnannouncedBatch::OverBudget { .. } => CombineError::Exhausted { ceiling_bytes },
            UnannouncedBatch::OverBound { .. } | UnannouncedBatch::Width { .. } | UnannouncedBatch::Mislabelled { .. } => {
                CombineError::Unannounced { cause }
            }
        })?;
    }
    drop(stream);
    let answered = accumulating.finish();
    #[cfg(feature = "distributed")]
    let answered = answered.with_executions(across::completed(capture, working_set)?);
    Ok(answered)
}

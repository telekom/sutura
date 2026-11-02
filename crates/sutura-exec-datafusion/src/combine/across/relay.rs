//! A refusal a node raised in a stage, read back off the failure its link relays to the node that
//! planned the question.
//!
//! `datafusion-distributed` sends a stage's failure across a link as the engine error's protobuf in
//! the call's status, and a `Context` error keeps its text and the error under it there. A node that
//! refuses a stage fails it under a `Context` whose text is a code; the node that planned the
//! question reads that code back and answers [`CombineError::RowsOverCeiling`], a variant with no
//! fields, so what the caller is told - the ceiling among it - is this node's own configuration.
//!
//! **Limits, next to the claim.**
//!
//! - The code comes from a peer, which can claim a refusal it did not make. Only the exact code is
//!   read, and it is read only off a failure, so the peer chooses at most which of this node's own
//!   refusals a failed question becomes: never an answer, and never a number or a text a caller sees.
//! - A code a node wraps in an Arrow error or an external one crosses its next link as text, which
//!   is not read, and stays the engine's failure. So does a code in a collection of errors past its
//!   first, and a code carried as an error's message rather than its context.
//! - No stage raises a code yet: the worker's row ceiling is the first that will.

use std::sync::Arc;

use datafusion::error::DataFusionError;

use super::chain;
use crate::combine::CombineError;

/// The code a stage refused for rows past the federated row ceiling crosses a link under.
const ROWS_OVER_CEILING: &str = "sutura.refusal.rows-over-ceiling";

/// `cause` as a stage's refusal for rows past the federated row ceiling, as the node that planned the
/// question reads it back.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no stage raises a relayed refusal yet; the worker's row ceiling will")
)]
pub(crate) fn over_ceiling(cause: DataFusionError) -> DataFusionError {
    DataFusionError::Context(String::from(ROWS_OVER_CEILING), Box::new(cause))
}

/// `error` as [`CombineError::RowsOverCeiling`] when a node relayed that refusal in it.
pub(crate) fn relayed(error: CombineError) -> CombineError {
    match error {
        CombineError::Execute { cause } if carries(&cause, ROWS_OVER_CEILING) => CombineError::RowsOverCeiling,
        other => other,
    }
}

/// Whether a `Context` error in `error`'s chain has exactly `code` for its text.
fn carries(error: &DataFusionError, code: &str) -> bool {
    chain(error)
        .filter_map(|link| {
            link.downcast_ref::<DataFusionError>()
                .or_else(|| link.downcast_ref::<Arc<DataFusionError>>().map(Arc::as_ref))
        })
        .any(|link| matches!(link, DataFusionError::Context(context, _) if context == code))
}

/// The code read back off a relayed failure, and only the exact code.
#[cfg(test)]
mod tests;

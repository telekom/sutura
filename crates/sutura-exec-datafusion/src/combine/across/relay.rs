//! A refusal a node raised in a stage, read back off the failure its link relays to the node that
//! planned the question.
//!
//! `datafusion-distributed` sends a stage's failure across a link as the engine error's protobuf in
//! the call's status, and a `Context` error keeps its text and the error under it there. A node that
//! refuses a stage fails it under a `Context` whose text is a code; the node that planned the
//! question reads that code back and answers [`CombineError::RowsOverCeiling`],
//! [`CombineError::LegNotCertified`], [`CombineError::PartitionDidNotFit`] or
//! [`CombineError::AmbiguousLink`], variants with no fields, so
//! what the caller is told - the ceiling, the source - is this node's own configuration.
//!
//! **Limits, next to the claim.**
//!
//! - The code comes from a peer, which can claim a refusal it did not make. Only the exact code is
//!   read, and it is read only off a failure, so the peer chooses at most which of this node's own
//!   refusals a failed question becomes: never an answer, and never a number or a text a caller sees.
//! - A code a node wraps in an Arrow error or an external one crosses its next link as text, which
//!   is not read, and stays the engine's failure. So does a code in a collection of errors past its
//!   first, and a code carried as an error's message rather than its context.
//! - Four stages raise a code: a task of a question's final stage past the worker's row ceiling, a
//!   reading worker's partition scan whose reader did not certify its leg or whose partition did not
//!   fit the reader's per-stream bound, and the count of a granted leg's link values that finds one
//!   held twice. Any other failure of a reader crosses as text and stays [`CombineError::Execute`]. The row count is per task, so tasks that each stay
//!   under the ceiling can sum past it, which only the planning node's own exact count refuses.
//! - [`CombineError::NoReadingNode`] is no peer's code: it is read off this node's own routing of a
//!   split leg to no node, as the typed error itself, which no link carries. It names the first
//!   source of a stage no resolved node reads at all; a stage whose sources each have a reader but
//!   none in common stays [`CombineError::Execute`].

use std::sync::Arc;

use datafusion::error::DataFusionError;

use super::chain;
use crate::combine::CombineError;

/// The code a stage refused for rows past the federated row ceiling crosses a link under.
const ROWS_OVER_CEILING: &str = "sutura.refusal.rows-over-ceiling";

/// `cause` as a stage's refusal for rows past the federated row ceiling, as the node that planned the
/// question reads it back.
pub(crate) fn over_ceiling(cause: DataFusionError) -> DataFusionError {
    DataFusionError::Context(String::from(ROWS_OVER_CEILING), Box::new(cause))
}

/// The code a reading worker's refused re-certification of a leg crosses a link under.
const LEG_NOT_CERTIFIED: &str = "sutura.refusal.leg-not-certified";

/// `cause` as a reading worker's refusal to certify the leg it was asked to read.
pub(crate) fn not_certified(cause: DataFusionError) -> DataFusionError {
    DataFusionError::Context(String::from(LEG_NOT_CERTIFIED), Box::new(cause))
}

/// The code a reading worker's granted partition past its per-stream bound crosses a link under.
const PARTITION_DID_NOT_FIT: &str = "sutura.refusal.partition-did-not-fit";

/// `cause` as a reading worker's partition that did not fit the bound it reads one stream under.
pub(crate) fn did_not_fit(cause: DataFusionError) -> DataFusionError {
    DataFusionError::Context(String::from(PARTITION_DID_NOT_FIT), Box::new(cause))
}

/// The code a granted leg's link value held by more than one of its rows crosses a link under.
const AMBIGUOUS_LINK: &str = "sutura.refusal.ambiguous-link";

/// `cause` as the count of a granted leg's link values finding one held more than once.
pub(crate) fn ambiguous(cause: DataFusionError) -> DataFusionError {
    DataFusionError::Context(String::from(AMBIGUOUS_LINK), Box::new(cause))
}

/// `error` as [`CombineError::RowsOverCeiling`], [`CombineError::LegNotCertified`],
/// [`CombineError::PartitionDidNotFit`] or [`CombineError::AmbiguousLink`] when a node relayed that
/// refusal in it, and as
/// [`CombineError::NoReadingNode`] when this node routed a split leg of a source no resolved node
/// reads.
pub(crate) fn relayed(error: CombineError) -> CombineError {
    match error {
        CombineError::Execute { cause } if carries(&cause, ROWS_OVER_CEILING) => CombineError::RowsOverCeiling,
        CombineError::Execute { cause } if carries(&cause, LEG_NOT_CERTIFIED) => CombineError::LegNotCertified,
        CombineError::Execute { cause } if carries(&cause, PARTITION_DID_NOT_FIT) => CombineError::PartitionDidNotFit,
        CombineError::Execute { cause } if carries(&cause, AMBIGUOUS_LINK) => CombineError::AmbiguousLink,
        CombineError::Execute { cause } => {
            let unread = chain(&cause)
                .find_map(|link| link.downcast_ref::<super::NoReadingNode>())
                .and_then(|refusal| refusal.unread.clone());
            unread.map_or(CombineError::Execute { cause }, |source_name| CombineError::NoReadingNode {
                source_name,
            })
        }
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

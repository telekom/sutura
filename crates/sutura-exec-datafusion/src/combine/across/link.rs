//! A split lookup leg's ambiguous-link check (`docs/adr/20261102082308-workers-read-sources.md`):
//! the plan `refuse_ambiguous_link` builds over a whole leg, run over a [`SplitScan`] through the
//! distributed session. Its leaf stage is the split stage, so each partition is read only by a task
//! on a node reading the leg's source, and that stage counts each link value only partially. The
//! engine shuffles the partial counts on a hash repartition by the link columns, and the stage above
//! that shuffle finishes the count and keeps the values past one, so a value held once by each of two
//! partitions meets in one task and is refused.
//!
//! # Limits
//!
//! - No served question calls this function: a combine checks a granted leg's link values in its own
//!   session, inside the plan that joins it (`served::joined`), and a served split leg that is not
//!   granted is read on the planning node.
//! - The repartition is the engine's, added because a split leaf claims no partitioning. A leaf
//!   claiming hash partitioning by the link columns would be counted per partition, wrongly.
//! - The session runs over the other nodes resolved when it is built, and never waits for one.
//! - The stage above the shuffle is routed by the engine to any node, which then holds the link
//!   values and their counts.

use std::sync::Arc;

use datafusion::common::TableReference;

use super::{Nodes, SplitScan, lost, relayed};
use crate::combine::{CombineError, refuse_ambiguous_link, working_set_of};

/// The name a split leg is registered under for its check, and the side word a failure to register it
/// carries.
const LOOKUP: &str = "lookup";

/// Refuses a split lookup `leg` holding two rows that share one value of every link column `on`, none
/// of them null, as [`CombineError::AmbiguousLink`], counted across `nodes` under a working set of
/// `ceiling_bytes`.
///
/// # Errors
///
/// [`CombineError::AmbiguousLink`] for such a pair, or why the check could not run.
pub async fn refuse_ambiguous_split(nodes: &Nodes, leg: SplitScan, on: &[&str], ceiling_bytes: u64) -> Result<(), CombineError> {
    let working_set = working_set_of(ceiling_bytes);
    let context = nodes.session(working_set)?;
    context
        .register_table(TableReference::bare(LOOKUP), Arc::new(leg))
        .map(|_replaced| ())
        .map_err(|cause| CombineError::Register { side: LOOKUP, cause })?;
    refuse_ambiguous_link(&context, LOOKUP, on, ceiling_bytes, working_set)
        .await
        .map_err(|error| relayed(lost(error)))
}

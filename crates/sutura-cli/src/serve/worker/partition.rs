//! A reading worker's [`PartitionReader`]: the leg a task carries is re-certified against this node's
//! own pinned catalog through [`sutura_app::read_recertified`], and only then is the granted
//! partition read, for a kind `split` dispatches - `PostgreSQL` alone.
//!
//! **Limits.** Only a `PostgreSQL` source is read. Any other kind whose leg this node certifies is
//! refused as a failed read - `BigQuery` among them, whose read needs the caller's forwarded source
//! token; a leg it does not certify is refused as uncertified first, except in a deployment that
//! holds no kind that splits, which names the source without certifying anything. A task
//! carrying a caller's token is refused (`split::worker_read_forwarded`). Each read is bounded by this node's own `server.request_timeout`, opened when the read
//! starts: the planning node's deadline does not reach the worker, the grant's expiry is checked
//! once, at admission, and dropping the stream does not stop a read already running - the deadline
//! does. The read runs as this node's own declared service user for the source,
//! `AdbcPostgres::read_partition`'s own limit; nothing checks it against the planning node's.

use std::sync::Arc;
use std::time::Instant;

use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_exec_datafusion::{Forwarded, PartitionReader, SchemaRef, SendableRecordBatchStream, Unread, read_later};

use super::super::OpenedSources;
use super::super::kind::split;

/// What a reading worker pinned and opened at boot, shared with every read it moves off the runtime.
pub(super) struct Reader(Arc<Opened>);

struct Opened {
    pinned: PinnedDefinitions,
    sources: OpenedSources,
    budget: Budget,
}

impl Reader {
    /// Reads from `sources` what `pinned` certifies, each read bounded by `budget`.
    pub(super) fn new(pinned: PinnedDefinitions, sources: OpenedSources, budget: Budget) -> Self {
        Self(Arc::new(Opened { pinned, sources, budget }))
    }
}

impl PartitionReader for Reader {
    fn read(
        &self,
        wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
    ) -> Result<SendableRecordBatchStream, Unread> {
        let opened = Arc::clone(&self.0);
        let (wire, source, partition) = (wire.to_vec(), source.clone(), partition.to_vec());
        Ok(read_later(Arc::clone(schema), async move {
            sutura_runtime::spawn_carrying_span(move || opened.read(&wire, &source, &partition))
                .await
                .map_err(Unread::failed)?
        }))
    }

    fn read_forwarded(
        &self,
        wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
        forwarded: Forwarded,
    ) -> Result<SendableRecordBatchStream, Unread> {
        let opened = Arc::clone(&self.0);
        let (wire, source, partition) = (wire.to_vec(), source.clone(), partition.to_vec());
        Ok(read_later(Arc::clone(schema), async move {
            sutura_runtime::spawn_carrying_span(move || opened.read_forwarded(&wire, &source, &partition, forwarded))
                .await
                .map_err(Unread::failed)?
        }))
    }
}

impl Opened {
    /// `source`'s `partition` of the leg `wire` holds, read only once this node certified the leg.
    fn read(&self, wire: &[u8], source: &SourceName, partition: &[u8]) -> Result<ResultBatches, Unread> {
        let deadline = Deadline::opened_at(Instant::now(), self.budget);
        split::worker_read(&self.pinned, &self.sources, source, wire, partition, deadline)
    }

    /// `source`'s `partition` of the leg `wire` holds, read with `forwarded` as
    /// [`split::worker_read_forwarded`] decides.
    fn read_forwarded(
        &self,
        wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        forwarded: Forwarded,
    ) -> Result<ResultBatches, Unread> {
        split::worker_read_forwarded(&self.pinned, &self.sources, source, wire, partition, forwarded, self.budget)
    }
}

#[cfg(test)]
mod tests;

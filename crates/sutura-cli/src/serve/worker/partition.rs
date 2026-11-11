//! A reading worker's [`PartitionReader`]: the leg a task carries is re-certified against this node's
//! own pinned catalog through [`sutura_app::read_recertified`], and only then is the granted
//! partition read, for a kind `split` dispatches - `PostgreSQL` as this node's service user, and
//! `BigQuery` with the caller's token the task's grant named.
//!
//! **Limits.** Any other kind whose leg this node certifies is refused as a failed read, and a leg
//! it does not certify is refused as uncertified first, except in a deployment that holds no kind
//! that splits, which names the source without certifying anything. A task carrying a token
//! is read only at a source of its name this node federates at the token's pool, else refused as
//! uncertified, and only while its grant lasts. Each read is bounded by this node's own
//! `server.request_timeout`, opened when the read starts, and a token's read by its grant's end
//! too: the planning node's deadline does not reach the worker, and dropping the stream does not
//! stop a read already running - the deadline does. A `PostgreSQL` read runs as this node's own
//! declared service user for the source, `AdbcPostgres::read_partition`'s own limit; nothing
//! checks it against the planning node's.

use std::sync::Arc;
use std::time::Instant;

use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_exec_datafusion::{
    Forwarded, PartitionReader, SchemaRef, SendableRecordBatchStream, SubmissionCapture, Unread, read_later,
};

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
        capture: Option<SubmissionCapture>,
    ) -> Result<SendableRecordBatchStream, Unread> {
        let opened = Arc::clone(&self.0);
        let (wire, source, partition, granted) = (wire.to_vec(), source.clone(), partition.to_vec(), Arc::clone(schema));
        Ok(read_later(Arc::clone(schema), async move {
            sutura_runtime::spawn_carrying_span(move || {
                let recording = capture.as_ref().map(SubmissionCapture::begin).transpose()?;
                let mut answered = opened.read(&wire, &source, &partition, &granted)?;
                if let Some(recording) = recording {
                    recording.complete(&mut answered)?;
                }
                Ok(answered)
            })
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
        capture: Option<SubmissionCapture>,
    ) -> Result<SendableRecordBatchStream, Unread> {
        let opened = Arc::clone(&self.0);
        let (wire, source, partition) = (wire.to_vec(), source.clone(), partition.to_vec());
        Ok(read_later(Arc::clone(schema), async move {
            sutura_runtime::spawn_carrying_span(move || {
                let recording = capture.as_ref().map(SubmissionCapture::begin).transpose()?;
                let mut answered = opened.read_forwarded(&wire, &source, &partition, forwarded)?;
                if let Some(recording) = recording {
                    recording.complete(&mut answered)?;
                }
                Ok(answered)
            })
            .await
            .map_err(Unread::failed)?
        }))
    }
}

impl Opened {
    /// `source`'s `partition` of the leg `wire` holds, into `granted`, read only once this node
    /// certified the leg.
    fn read(&self, wire: &[u8], source: &SourceName, partition: &[u8], granted: &SchemaRef) -> Result<ResultBatches, Unread> {
        let deadline = Deadline::opened_at(Instant::now(), self.budget);
        split::worker_read(&self.pinned, &self.sources, source, wire, partition, granted, deadline)
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

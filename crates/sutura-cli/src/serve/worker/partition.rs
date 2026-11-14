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
//! checks it against the planning node's. The concrete reader checks its own source's verifier
//! membership before decoding or dispatching, using only the granting node's authenticated id.
//! It does not reverify a human token or constrain other `PartitionReader` implementations.

use std::sync::Arc;
use std::time::Instant;

use sutura_config::credentials::SourceAdmissions;
use sutura_domain::identity::VerifierId;
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
    admissions: SourceAdmissions,
}

impl Reader {
    /// Reads from `sources` what `pinned` certifies, each read bounded by `budget`.
    pub(super) fn new(pinned: PinnedDefinitions, sources: OpenedSources, budget: Budget, admissions: SourceAdmissions) -> Self {
        Self(Arc::new(Opened {
            pinned,
            sources,
            budget,
            admissions,
        }))
    }
}

impl PartitionReader for Reader {
    fn read(
        &self,
        verifier: &VerifierId,
        wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
        capture: Option<SubmissionCapture>,
    ) -> Result<SendableRecordBatchStream, Unread> {
        // The granting node authenticated this id; this worker checks its own source declaration.
        if !self.0.admissions.admits_verifier(source, verifier) {
            return Err(Unread::LegNotCertified);
        }
        let opened = Arc::clone(&self.0);
        let (wire, source, partition, granted) = (wire.to_vec(), source.clone(), partition.to_vec(), Arc::clone(schema));
        Ok(read_later(Arc::clone(schema), async move {
            sutura_runtime::spawn_carrying_span(move || {
                tracing::debug!(target: "sutura_diagnostic", phase = "capture_begin", "native read phase");
                let recording = capture.as_ref().map(SubmissionCapture::begin).transpose()?;
                tracing::debug!(target: "sutura_diagnostic", phase = "native_begin", "native read phase");
                let answer = opened.read(&wire, &source, &partition, &granted);
                tracing::debug!(target: "sutura_diagnostic", phase = "native_end", succeeded = answer.is_ok(), "native read phase");
                let mut answered = answer?;
                if let Some(recording) = recording {
                    tracing::debug!(target: "sutura_diagnostic", phase = "capture_complete_start", "native read phase");
                    let result = recording.complete(&mut answered);
                    tracing::debug!(target: "sutura_diagnostic", phase = "capture_complete_end", succeeded = result.is_ok(), "native read phase");
                    result?;
                }
                Ok(answered)
            })
            .await
            .map_err(Unread::failed)?
        }))
    }

    fn read_forwarded(
        &self,
        verifier: &VerifierId,
        wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
        forwarded: Forwarded,
        capture: Option<SubmissionCapture>,
    ) -> Result<SendableRecordBatchStream, Unread> {
        // The granting node authenticated this id; this worker checks its own source declaration.
        if !self.0.admissions.admits_verifier(source, verifier) {
            return Err(Unread::LegNotCertified);
        }
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

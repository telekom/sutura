//! A reading worker's [`PartitionReader`]: the leg a task carries is re-certified against this node's
//! own pinned catalog through [`sutura_app::read_recertified`], and only then is the granted
//! partition of a `PostgreSQL` source read.
//!
//! **Limits.** Only a `PostgreSQL` source is read. Any other kind whose leg this node certifies is
//! refused as a failed read - `BigQuery` among them, whose read needs the caller's forwarded source
//! token a task does not carry; a leg it does not certify is refused as uncertified first, whatever
//! the kind. Each read is bounded by this node's own `server.request_timeout`, opened when the read
//! starts: the planning node's deadline does not reach the worker, the grant's expiry is checked
//! once, at admission, and dropping the stream does not stop a read already running - the deadline
//! does. The read runs as this node's own declared service user for the source,
//! `AdbcPostgres::read_partition`'s own limit; nothing checks it against the planning node's.

use std::sync::Arc;
use std::time::Instant;

use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::LegPlan;
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_exec_datafusion::{PartitionReader, SchemaRef, SendableRecordBatchStream, Unread, read_later};

use super::super::kind::AnyWarehouse;
use super::super::{OpenedSources, PostgresSource};

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
}

impl Opened {
    /// `source`'s `partition` of the leg `wire` holds, read only once this node certified the leg.
    fn read(&self, wire: &[u8], source: &SourceName, partition: &[u8]) -> Result<ResultBatches, Unread> {
        let deadline = Deadline::opened_at(Instant::now(), self.budget);
        let not_postgres = || Unread::source_not_read(source);
        let read = match &self.sources {
            OpenedSources::Postgres(sources) => {
                sutura_app::read_recertified(&self.pinned, sources, source, wire, |postgres, leg| {
                    read_partition(postgres, &leg, partition, deadline)
                })
            }
            OpenedSources::Mixed(mixed) => {
                sutura_app::read_recertified(&self.pinned, &mixed.engines, source, wire, |engine, leg| match engine {
                    AnyWarehouse::Postgres(postgres) => read_partition(postgres, &leg, partition, deadline),
                    _ => Err(not_postgres()),
                })
            }
            _ => Ok(Err(not_postgres())),
        };
        read.unwrap_or_else(|_refused| Err(Unread::LegNotCertified))
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the one production caller: the admit rule verified the task's reading grant, and the leg was re-certified against this node's catalog first"
)]
fn read_partition(
    postgres: &PostgresSource,
    leg: &LegPlan,
    partition: &[u8],
    deadline: Deadline,
) -> Result<ResultBatches, Unread> {
    postgres.read_partition(leg, partition, deadline).map_err(Unread::failed)
}

#[cfg(test)]
mod tests;

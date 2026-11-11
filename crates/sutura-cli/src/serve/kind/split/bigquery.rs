//! The `BigQuery` arm of [`SplitKind`]: a lookup leg at a source that federates at a pool is run
//! once as the asking subject and its streams granted to the nodes that read the source, each read
//! there with the caller's own forwarded token (`docs/adr/20261102082308-workers-read-sources.md`,
//! W2 and its amendments).
//!
//! **Limits.** A leg at a shared, Google-login or delegating source, and one offered to no
//! grantee, is read whole, as before. `above_bytes` is not read: the adapter answers no dry run,
//! so the split is decided by the job's own stream count, which the driver aims at half the
//! per-stream bound each - a leg of one stream is read here, with the forwarded token and no later
//! than it lasts, never run twice. The grant carries no estimate of its own, and holds nothing open:
//! the job's result lives at `BigQuery`.
//!
//! No offline cell reaches the driver, and no venue cell reads a split yet, so which principal
//! `BigQuery` lets read a stream is not measured; only a venue run that reads one under two
//! principals can measure it. [`Grantee::grant`] does not refuse a grant carrying no token, so only
//! the call order here - [`Grantee::forwarding`] before it - forwards the token, and the reading
//! worker's refusal of a task with none is the backstop.

use core::iter;
use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use sutura_domain::identity::{Presented, Secret, SourceToken, WorkloadPool};
use sutura_domain::plan::{Grantee, LegPlan, LookupSplit, Partitioned};
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_exec_bigquery::BigQueryError;
use sutura_exec_bigquery::adbc::{AdbcError, MOST_RESULT_ROWS};
use sutura_exec_datafusion::{SchemaRef, TokenUnread, Unread};

use super::super::super::BigQuerySource;
use super::super::AnyWarehouseError;
use super::{PartitionReads, SplitKind};

impl SplitKind for BigQuerySource {
    /// Through `split_leg`, only at a pool source and only for a leg offered to a `grantee`; then
    /// two or more streams are granted to the nodes that read the source, read with the forwarded
    /// token [`Grantee::forwarding`] ties to the credential the request presented for the leg's
    /// source, and fewer are read here with it. [`Grantee::grant`]'s refusal is the answer.
    ///
    /// **Limit:** `grant` itself takes the descriptors with or without a token, so the order of the
    /// two calls below is what forwards it; the worker's refusal of a tokenless task backs it.
    fn split(
        &self,
        leg: &LegPlan,
        presented: &Presented,
        deadline: Deadline,
        _above_bytes: NonZeroU64,
        readers: NonZeroUsize,
        grantee: Option<Grantee>,
        labels: &JobLabels,
    ) -> LookupSplit<'_, AnyWarehouseError> {
        let Some(grantee) = grantee.filter(|_| self.federates_at().is_some()) else {
            return Ok(Partitioned::Whole);
        };
        let mut split = self
            .split_leg(leg, presented, deadline, readers, grantee.until(), labels)
            .map_err(AnyWarehouseError::BigQuery)?;
        let (schema, until) = (Arc::clone(split.schema()), split.until());
        let executions = split.take_executions();
        let (descriptors, token) = split.into_parts();
        if let [] | [_] = descriptors.as_slice() {
            let read = descriptors.first().map_or_else(
                || Ok(ResultBatches::none_under(schema)),
                |descriptor| read_here(self, (leg, descriptor), token, until, deadline),
            );
            let read = read.map(|rows| rows.with_prior_executions(executions));
            return Ok(Partitioned::Split(Box::new(iter::once(read))));
        }
        let rows = NonZeroUsize::new(MOST_RESULT_ROWS)
            .and_then(|rows| NonZeroU32::try_from(rows).ok())
            .unwrap_or(NonZeroU32::MAX);
        Ok(grantee
            .forwarding(token)
            .map_err(AnyWarehouseError::NotTheCallersToken)?
            .grant(
                schema,
                descriptors.into_iter().map(Some),
                rows,
                EstimatedBytes::parse(0),
                (),
                SystemTime::now(),
            )
            .map_or_else(Partitioned::NotGranted, |grant| Partitioned::Granted(grant.with_executions(executions))))
    }

    /// Refused: a `BigQuery` partition is read only with its caller's token.
    fn read_granted(
        &self,
        leg: &LegPlan,
        _partition: &[u8],
        _granted: &SchemaRef,
        _reads: &PartitionReads,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Unread> {
        Err(Unread::failed(TokenUnread::NoToken {
            at: leg.source().clone(),
        }))
    }

    fn forwarded_at(&self) -> Option<&WorkloadPool> {
        self.federates_at()
    }

    /// With `token` as the read's only credential, which `split::worker_read_forwarded` handed
    /// over only for this source's own pool and only while its grant lasts.
    #[expect(
        clippy::disallowed_methods,
        reason = "the reading worker's call: the admit rule verified the task's grant naming this token, the token was forwarded for this source's own pool, and the leg was re-certified against this node's catalog first"
    )]
    fn read_forwarded(
        &self,
        leg: &LegPlan,
        partition: &[u8],
        token: &Secret,
        deadline: Deadline,
    ) -> Result<ResultBatches, Unread> {
        self.read_partition(leg, partition, token, deadline).map_err(Unread::failed)
    }
}

/// The one stream `descriptor` names of the split `bigquery` just ran for `leg`, read here with
/// that split's own forwarded `token`, ending no later than `until` - when the token stops being
/// one - or the question's `deadline`.
///
/// # Errors
///
/// [`AdbcError::DeadlineElapsed`] when either has already passed, before the driver loads; then
/// the read's own failure.
#[expect(
    clippy::disallowed_methods,
    reason = "the planning node reads the one stream of the split it just ran, with the token exchanged for that split from the request's own credential"
)]
fn read_here(
    bigquery: &BigQuerySource,
    (leg, descriptor): (&LegPlan, &[u8]),
    token: SourceToken,
    until: SystemTime,
    deadline: Deadline,
) -> Result<ResultBatches, AnyWarehouseError> {
    let now = Instant::now();
    let budget = until
        .duration_since(SystemTime::now())
        .ok()
        .zip(deadline.remaining_at(now))
        .and_then(|(usable, left)| Budget::parse(usable.min(left)).ok())
        .ok_or(AnyWarehouseError::BigQuery(BigQueryError::Endpoint {
            cause: AdbcError::DeadlineElapsed,
        }))?;
    bigquery
        .read_partition(leg, descriptor, &token.into_token(), Deadline::opened_at(now, budget))
        .map_err(AnyWarehouseError::BigQuery)
}

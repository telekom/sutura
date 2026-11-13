//! A fresh bounded receipt barrier per physical execution; no caller deadline reaches this seam.

use std::sync::Arc;
use std::time::Duration;

use datafusion::common::DataFusionError;
use datafusion::common::tree_node::{TreeNode as _, TreeNodeRecursion};
use datafusion::execution::SessionStateBuilder;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::DataFrame;
use datafusion_distributed::{CompletionRequest, QueryCompletion, TaskCompletionProducer};
use sutura_domain::warehouse::execution::{Execution, wire};

use super::partition::PartitionScanExec;
use crate::combine::CombineError;
use crate::pool::WorkingSet;

mod capture;
pub(super) use capture::TaskReceiptBudget;
pub use capture::{RecordingSubmission, SubmissionCapture};

/// Installed only on sessions that plan distributed executions.
pub(super) struct CaptureDistributed;

// The combiner port has no caller Deadline. This caps this operation, not the served request.
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);

pub(in crate::combine) async fn executing(
    frame: DataFrame,
    working_set: WorkingSet,
) -> Result<(datafusion::execution::SendableRecordBatchStream, Option<Arc<QueryCompletion>>), DataFusionError> {
    let (state, logical) = frame.into_parts();
    if state.config().get_extension::<CaptureDistributed>().is_none() {
        return Ok((DataFrame::new(state, logical).execute_stream().await?, None));
    }
    let limit = wire::byte_limit(working_set.bytes());
    let count = limit.checked_div(64).unwrap_or_default().max(1);
    let deadline = tokio::time::Instant::now() + COMPLETION_TIMEOUT;
    let capture = Arc::new(QueryCompletion::new(count, count, limit, deadline)?);
    let mut builder = SessionStateBuilder::new_from_existing(state);
    let config = builder
        .config()
        .take()
        .unwrap_or_default()
        .with_extension(Arc::clone(&capture));
    let state = builder.with_config(config).build();
    let frame = DataFrame::new(state, logical);
    let context = Arc::new(frame.task_ctx());
    let plan = tokio::time::timeout_at(deadline, frame.create_physical_plan())
        .await
        .map_err(|cause| DataFusionError::External(Box::new(cause)))??;
    let mut distributed = 0usize;
    let mut source_leaves = false;
    plan.apply(|node| {
        if node.is::<datafusion_distributed::DistributedExec>() {
            distributed = distributed.saturating_add(1);
        }
        source_leaves |= node.is::<PartitionScanExec>();
        Ok(TreeNodeRecursion::Continue)
    })?;
    if distributed > 1 || (distributed == 0 && source_leaves) {
        return Err(DataFusionError::Execution(String::from(
            "physical plan has no single worker completion scope",
        )));
    }
    // Execute exactly the inspected Arc. Local plans without source leaves submit no worker tasks.
    let stream = datafusion::physical_plan::execute_stream(plan, context)?;
    Ok((stream, (distributed == 1).then_some(capture)))
}

pub(in crate::combine) fn completed(
    capture: Option<Arc<QueryCompletion>>,
    working_set: WorkingSet,
) -> Result<Vec<Execution>, CombineError> {
    let Some(capture) = capture else {
        return Ok(Vec::new());
    };
    let limit = wire::byte_limit(working_set.bytes());
    let mut records = Vec::new();
    for task in capture.take().map_err(|cause| CombineError::Execute { cause })? {
        let [envelope] = task.records.as_slice() else {
            return Err(incomplete("one execution envelope is required per task"));
        };
        let captured = wire::decode(envelope, limit).map_err(|cause| CombineError::Execute {
            cause: DataFusionError::External(Box::new(cause)),
        })?;
        if records.len().saturating_add(captured.len()) > (limit >> 6) {
            return Err(incomplete("aggregate execution count exceeded"));
        }
        records.extend(captured);
    }
    Ok(records)
}

pub(super) fn install(mut builder: SessionStateBuilder) -> SessionStateBuilder {
    let config = builder.config().take().unwrap_or_default();
    let config = if let Some(request) = config.get_extension::<CompletionRequest>() {
        config.with_extension(Arc::new(TaskReceiptBudget::new(request.max_records(), request.max_bytes())))
    } else {
        config
    };
    let producer = TaskCompletionProducer::new(|_, plan, request| Box::pin(produce(plan, request)));
    *builder.config() = Some(config.with_extension(Arc::new(producer)));
    builder
}

async fn produce(plan: Arc<dyn ExecutionPlan>, request: CompletionRequest) -> Result<Vec<Vec<u8>>, DataFusionError> {
    let mut leaves: Vec<Arc<dyn ExecutionPlan>> = Vec::new();
    plan.apply(|node| {
        if node.is::<PartitionScanExec>() && !leaves.iter().any(|known| Arc::ptr_eq(known, node)) {
            if leaves.len() >= request.max_records() {
                return Err(DataFusionError::Execution(String::from("source leaf count exceeded")));
            }
            leaves.push(Arc::clone(node));
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    let mut captures = Vec::with_capacity(leaves.len());
    for leaf in &leaves {
        let scan = leaf
            .downcast_ref::<PartitionScanExec>()
            .ok_or_else(|| DataFusionError::Execution(String::from("source leaf changed during capture")))?;
        let capture = scan
            .submission_capture()
            .ok_or_else(|| DataFusionError::Execution(String::from("source submission capture is missing")))?;
        capture.seal();
        captures.push(capture);
    }
    let mut submissions = Vec::new();
    for capture in captures {
        submissions.extend(capture.finish().await?);
    }
    submissions.sort_unstable_by_key(|(sequence, _)| *sequence);
    let records: Vec<_> = submissions.into_iter().flat_map(|(_, records)| records).collect();
    let envelope = wire::encode(&records, request.max_bytes()).map_err(|cause| DataFusionError::External(Box::new(cause)))?;
    Ok(vec![envelope])
}

fn incomplete(message: &'static str) -> CombineError {
    CombineError::Execute {
        cause: DataFusionError::Execution(String::from(message)),
    }
}

#[cfg(test)]
mod tests;

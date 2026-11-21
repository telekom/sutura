use crate::ExecuteTaskRequest;
use crate::worker::worker_service::Worker;
use datafusion::common::exec_datafusion_err;
use datafusion::common::{Result, exec_err};
use datafusion::error::DataFusionError;
use datafusion::execution::{SendableRecordBatchStream, TaskContext};
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use std::sync::Arc;
use std::time::Duration;

const WAIT_PLAN_TIMEOUT_SECS: u64 = 10;

/// Builds several per-partition streams by retrieving the appropriate entry from [TaskDataEntries]
/// based on the task key extracted from [ExecuteTaskRequest].
///
/// This method is async mainly for the key retrieval operation from [TaskDataEntries], but it does
/// not start polling any stream, it just instantiates them.
impl Worker {
    pub async fn execute_task(
        &self,
        request: ExecuteTaskRequest,
    ) -> Result<(Vec<SendableRecordBatchStream>, Arc<TaskContext>)> {
        let key = request.task_key;
        let diagnostic_started = std::time::Instant::now();
        eprintln!("plan-handoff key={key:?} phase=execute outcome=start elapsed_us={}", diagnostic_started.elapsed().as_micros());
        let entry = self
            .task_data_entries
            .get_with(request.task_key, async { Default::default() })
            .await;
        eprintln!("plan-handoff key={key:?} phase=execute_entry outcome=obtained elapsed_us={}", diagnostic_started.elapsed().as_micros());


        // Other request is responsible for writing the plan that belongs to this TaskKey, so
        // we'll resolve immediately if it was already there, or wait until it's ready.
        eprintln!("plan-handoff key={key:?} phase=wait outcome=start elapsed_us={}", diagnostic_started.elapsed().as_micros());
        let task_data = entry
            .read(Duration::from_secs(WAIT_PLAN_TIMEOUT_SECS))
            .await
            .inspect(|value| { eprintln!("plan-handoff key={key:?} phase=wait outcome={} elapsed_us={}", if value.is_ok() { "ready" } else { "preparation_error" }, diagnostic_started.elapsed().as_micros()); })
            .inspect_err(|cause| { eprintln!("plan-handoff key={key:?} phase=wait outcome={} elapsed_us={}", match cause {
                super::single_write_multi_read::SingleWriteMultiReadError::AlreadyWritten => "already_written",
                super::single_write_multi_read::SingleWriteMultiReadError::Timeout => "timeout",
                super::single_write_multi_read::SingleWriteMultiReadError::NoValue => "missing_value",
            }, diagnostic_started.elapsed().as_micros()); })
            .map_err(|e| exec_datafusion_err!("Worker::execute_task timed-out while waiting for the plan to be set by the coordinator. ({e})"))?
            .map_err(DataFusionError::Shared)?;
        task_data.task_data_metrics.mark_execution_started_once();

        let plan = task_data.plan(request.producer_head)?;
        let task_ctx = task_data.task_ctx;
        let partition_count = plan.properties().partitioning.partition_count();
        let plan_name = plan.name();

        // Execute all the requested partitions at once, and collect all the streams so that they
        // can be merged into a single one at the end of this function.
        let n_streams = request.target_partition_end - request.target_partition_start;
        let mut streams = Vec::with_capacity(n_streams);
        for partition in request.target_partition_start..request.target_partition_end {
            if partition >= partition_count {
                return exec_err!(
                    "partition {partition} not available. The head plan {plan_name} of the stage just has {partition_count} partitions"
                );
            }

            let stream = plan.execute(partition, Arc::clone(&task_ctx))?;
            let stream_schema = plan.schema();

            streams.push(Box::pin(RecordBatchStreamAdapter::new(stream_schema, stream)) as _);
        }
        Ok((streams, task_ctx))
    }
}

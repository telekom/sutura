use crate::TaskKey;
use datafusion::common::{Result, exec_datafusion_err, exec_err};
use datafusion::physical_plan::ExecutionPlan;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

/// Per-query extension; create a fresh instance for each execution.
pub struct QueryCompletion {
    max_tasks: usize,
    max_records: usize,
    max_bytes: usize,
    pub(crate) deadline: Instant,
    pub(crate) ended: tokio_util::sync::CancellationToken,
    state: Mutex<CompletionState>,
}

#[derive(Default)]
struct CompletionState {
    tasks: HashMap<TaskKey, Option<Vec<Vec<u8>>>>,
    records: usize,
    bytes: usize,
    ready: bool,
}

impl QueryCompletion {
    pub fn new(max_tasks: usize, max_records: usize, max_bytes: usize, deadline: Instant) -> Result<Self> {
        if max_tasks == 0 || max_records == 0 || max_bytes == 0 || deadline <= Instant::now() {
            return exec_err!("invalid query completion budget");
        }
        Ok(Self { max_tasks, max_records, max_bytes, deadline, ended: tokio_util::sync::CancellationToken::new(), state: Mutex::default() })
    }

    pub(crate) fn register(&self, key: TaskKey) -> Result<CompletionRequest> {
        let timeout = self.deadline.checked_duration_since(Instant::now())
            .ok_or_else(|| exec_datafusion_err!("query completion deadline elapsed"))?;
        let mut state = self.state.lock().unwrap();
        if state.ready || state.tasks.len() >= self.max_tasks || state.tasks.contains_key(&key)
            || state.tasks.keys().any(|other| other.query_id != key.query_id)
        {
            return exec_err!("duplicate, foreign or excessive completion task");
        }
        state.tasks.insert(key, None);
        Ok(CompletionRequest { max_records: self.max_records, max_bytes: self.max_bytes, timeout })
    }

    pub(crate) fn accept(&self, expected: TaskKey, completion: TaskCompletion) -> Result<()> {
        if completion.task_key != expected {
            return exec_err!("task completion identity mismatch");
        }
        let bytes = completion.records.iter().try_fold(0usize, |total, record| total.checked_add(record.len()))
            .ok_or_else(|| exec_datafusion_err!("task completion size overflow"))?;
        let mut state = self.state.lock().unwrap();
        if state.ready || !matches!(state.tasks.get(&expected), Some(None)) {
            return exec_err!("duplicate or unexpected task completion");
        }
        let records = state.records.checked_add(completion.records.len())
            .filter(|count| *count <= self.max_records)
            .ok_or_else(|| exec_datafusion_err!("query completion record budget exceeded"))?;
        let bytes = state.bytes.checked_add(bytes).filter(|count| *count <= self.max_bytes)
            .ok_or_else(|| exec_datafusion_err!("query completion byte budget exceeded"))?;
        state.tasks.insert(expected, Some(completion.records));
        state.records = records;
        state.bytes = bytes;
        Ok(())
    }

    pub(crate) fn finish(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.ready || Instant::now() >= self.deadline || state.tasks.values().any(Option::is_none) {
            return exec_err!("missing task completion");
        }
        state.ready = true;
        Ok(())
    }

    /// Available only after the output stream's successful EOF, including its completion barrier.
    /// Task order is stable; records retain the producer's local submission order.
    pub fn take(&self) -> Result<Vec<TaskCompletion>> {
        let mut state = self.state.lock().unwrap();
        if !state.ready {
            return exec_err!("query completion barrier has not succeeded");
        }
        let mut completions = state.tasks.drain().map(|(task_key, records)| {
            records.map(|records| TaskCompletion { task_key, records })
                .ok_or_else(|| exec_datafusion_err!("missing task completion"))
        }).collect::<Result<Vec<_>>>()?;
        completions.sort_unstable_by_key(|v| (v.task_key.stage_id, v.task_key.task_number));
        Ok(completions)
    }
}

/// Also installed in the worker's session, alongside TaskKey, before its session builder runs.
#[derive(Clone, Copy)]
pub struct CompletionRequest {
    pub(crate) max_records: usize,
    pub(crate) max_bytes: usize,
    pub(crate) timeout: Duration,
}

impl CompletionRequest {
    pub fn max_records(&self) -> usize { self.max_records }
    pub fn max_bytes(&self) -> usize { self.max_bytes }

    pub(crate) fn parse(max_records: u64, max_bytes: u64, timeout_ms: u64) -> Result<Self> {
        if max_records == 0 || max_bytes == 0 || timeout_ms == 0 {
            return exec_err!("invalid task completion budget");
        }
        let max_records = usize::try_from(max_records)
            .map_err(|cause| exec_datafusion_err!("invalid record budget: {cause}"))?;
        let max_bytes = usize::try_from(max_bytes)
            .map_err(|cause| exec_datafusion_err!("invalid byte budget: {cause}"))?;
        let timeout = Duration::from_millis(timeout_ms);
        if Instant::now().checked_add(timeout).is_none() {
            return exec_err!("invalid task completion timeout");
        }
        Ok(Self { max_records, max_bytes, timeout })
    }

    pub(crate) fn check(&self, records: &[Vec<u8>]) -> Result<()> {
        let bytes = records.iter().try_fold(0usize, |total, record| total.checked_add(record.len()));
        if records.len() > self.max_records || bytes.is_none_or(|bytes| bytes > self.max_bytes) {
            return exec_err!("task completion budget exceeded");
        }
        Ok(())
    }
}

/// One atomic terminal message, even when no rows or no submissions were produced.
/// Opaque records must not contain credentials, endpoints or serialized physical plans.
pub struct TaskCompletion {
    pub task_key: TaskKey,
    pub records: Vec<Vec<u8>>,
}

type ProduceCompletion = dyn Fn(TaskKey, Arc<dyn ExecutionPlan>, CompletionRequest)
    -> BoxFuture<'static, Result<Vec<Vec<u8>>>> + Send + Sync;

/// Worker-session hook. It receives the actual task plan after execution/drop.
/// Before returning Ok, seal new submissions and await every started native operation.
/// Incomplete capture, cancellation without finalized evidence, or overflow must return Err.
/// The vendor checks returned bounds; the producer must bound allocation while capturing.
pub struct TaskCompletionProducer(Arc<ProduceCompletion>);

impl TaskCompletionProducer {
    pub fn new(produce: impl Fn(TaskKey, Arc<dyn ExecutionPlan>, CompletionRequest)
        -> BoxFuture<'static, Result<Vec<Vec<u8>>>> + Send + Sync + 'static) -> Self {
        Self(Arc::new(produce))
    }

    pub(crate) async fn produce(&self, key: TaskKey, plan: Arc<dyn ExecutionPlan>, request: CompletionRequest) -> Result<Vec<Vec<u8>>> {
        (self.0)(key, plan, request).await
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn completion_requires_registered_identity_full_receipts_and_shared_budget() {
        use super::*;
        let key = TaskKey { query_id: uuid::Uuid::from_u128(1), stage_id: 0, task_number: 0 };
        let other = TaskKey { task_number: 1, ..key };
        let query = || QueryCompletion::new(2, 1, 1, Instant::now() + Duration::from_secs(10)).unwrap();
        let payload = |key, records| TaskCompletion { task_key: key, records };
        let q = query();
        q.register(key).unwrap();
        q.register(other).unwrap();
        assert!(q.take().is_err());
        assert!(q.accept(key, payload(other, vec![vec![1]])).is_err());
        q.accept(key, payload(key, vec![vec![1]])).unwrap();
        assert!(q.accept(key, payload(key, vec![])).is_err());
        assert!(q.finish().is_err());
        assert!(q.accept(other, payload(other, vec![vec![2]])).is_err());
        q.accept(other, payload(other, vec![])).unwrap();
        q.finish().unwrap();
        let got = q.take().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].records, vec![vec![1]]);
        assert!(got[1].records.is_empty());
        assert!(q.register(key).is_err());
        let q = query();
        q.register(key).unwrap();
        assert!(q.accept(key, payload(key, vec![vec![1, 2]])).is_err());
        assert!(q.take().is_err());
    }
}

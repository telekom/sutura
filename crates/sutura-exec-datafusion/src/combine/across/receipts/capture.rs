//! Native submissions owned by one decoded source leaf, including reads surviving stream drop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use datafusion::common::DataFusionError;
use parking_lot::Mutex;
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::{ResultBatches, execution::Execution};
use tokio::sync::watch;

use super::super::partition::Unread;

/// One source leaf's bounded capture. It never crosses the plan codec.
#[derive(Clone)]
pub struct SubmissionCapture(Arc<Capture>, Arc<AtomicBool>);

struct Capture {
    source: SourceName,
    state: Mutex<State>,
    changed: watch::Sender<usize>,
    budget: Arc<TaskReceiptBudget>,
}

#[derive(Default)]
struct State {
    sealed: bool,
    active: usize,
    failed: bool,
    records: Vec<(usize, Vec<Execution>)>,
}

impl SubmissionCapture {
    pub(in crate::combine::across) fn new(source: SourceName, budget: Arc<TaskReceiptBudget>) -> Self {
        let (changed, _) = watch::channel(0);
        Self(Arc::new(Capture { source, state: Mutex::default(), changed, budget }), Arc::new(AtomicBool::new(false)))
    }

    pub(in crate::combine::across) fn belongs_to(&self, budget: &Arc<TaskReceiptBudget>) -> bool {
        Arc::ptr_eq(&self.0.budget, budget)
    }

    /// Begin inside the native read's blocking closure, before any source submission.
    pub fn begin(&self) -> Result<RecordingSubmission, Unread> {
        let mut state = self.0.state.lock();
        if state.sealed || state.failed {
            return Err(failed("source capture is sealed or failed"));
        }
        let Some(active) = state.active.checked_add(1) else {
            state.failed = true;
            return Err(failed("source capture count overflow"));
        };
        let sequence = self.0.budget.reserve()?;
        state.active = active;
        Ok(RecordingSubmission { capture: self.clone(), sequence, completed: false })
    }

    pub(in crate::combine::across) fn for_read(&self) -> Self {
        Self(Arc::clone(&self.0), Arc::new(AtomicBool::new(false)))
    }

    pub(in crate::combine::across) fn require_recorded(&self) -> Result<(), Unread> {
        if !self.1.load(Ordering::Acquire) {
            return Err(failed("successful source read supplied no submission facts"));
        }
        Ok(())
    }

    pub(super) fn seal(&self) {
        self.0.state.lock().sealed = true;
    }

    pub(super) async fn finish(&self) -> Result<Vec<(usize, Vec<Execution>)>, DataFusionError> {
        let mut changed = self.0.changed.subscribe();
        loop {
            {
                let mut state = self.0.state.lock();
                if !state.sealed || state.failed || self.0.budget.state.lock().failed {
                    return Err(DataFusionError::Execution(String::from("incomplete source capture")));
                }
                if state.active == 0 {
                    return Ok(core::mem::take(&mut state.records));
                }
            }
            changed.changed().await.map_err(|cause| DataFusionError::External(Box::new(cause)))?;
        }
    }
}

/// Owned by the running native closure. Dropping its async receiver does not finalize it.
pub struct RecordingSubmission {
    capture: SubmissionCapture,
    sequence: usize,
    completed: bool,
}

impl RecordingSubmission {
    /// Move the adapter's actual facts before read_later discards everything except rows.
    pub fn complete(mut self, batches: &mut ResultBatches) -> Result<(), Unread> {
        let records = batches.take_executions();
        let mut state = self.capture.0.state.lock();
        if records.iter().any(|record| {
            let source = match record {
                Execution::Sql { source, .. } | Execution::Cube { source, .. }
                | Execution::Node { source, .. } | Execution::BigQueryRead { source, .. } => Some(source),
                Execution::DataFusion { source, .. } => source.as_ref(),
            };
            source != Some(&self.capture.0.source)
        }) {
            return Err(failed("native facts name another source"));
        }
        self.capture.0.budget.commit(&records)?;
        state.records.push((self.sequence, records));
        state.active = state.active.saturating_sub(1);
        self.completed = true;
        self.capture.1.store(true, Ordering::Release);
        self.capture.0.changed.send_replace(state.active);
        Ok(())
    }
}

impl Drop for RecordingSubmission {
    fn drop(&mut self) {
        if !self.completed {
            let mut state = self.capture.0.state.lock();
            state.failed = true;
            self.capture.0.budget.state.lock().failed = true;
            state.active = state.active.saturating_sub(1);
            self.capture.0.changed.send_replace(state.active);
        }
    }
}

fn failed(message: &'static str) -> Unread {
    Unread::Failed(DataFusionError::Execution(String::from(message)))
}

/// One byte/count budget shared by every leaf of this task.
pub(in crate::combine::across) struct TaskReceiptBudget {
    max_records: usize,
    max_bytes: u64,
    state: Mutex<BudgetState>,
}

#[derive(Default)]
struct BudgetState { records: usize, bytes: u64, failed: bool }

impl TaskReceiptBudget {
    pub(in crate::combine::across) fn new(max_records: usize, max_bytes: usize) -> Self {
        Self { max_records, max_bytes: u64::try_from(max_bytes).unwrap_or(u64::MAX), state: Mutex::default() }
    }

    fn reserve(&self) -> Result<usize, Unread> {
        let mut state = self.state.lock();
        if state.failed || state.records >= self.max_records {
            state.failed = true;
            return Err(failed("task capture count exceeded"));
        }
        let sequence = state.records;
        state.records = state.records.saturating_add(1);
        Ok(sequence)
    }

    fn commit(&self, records: &[Execution]) -> Result<(), Unread> {
        let mut state = self.state.lock();
        let count = state.records.checked_add(records.len().saturating_sub(1));
        let bytes = records.iter().fold(state.bytes, |total, record| total.saturating_add(record.rendered_byte_len()));
        if records.is_empty() || state.failed || count.is_none_or(|count| count > self.max_records) || bytes > self.max_bytes {
            state.failed = true;
            return Err(failed("task submission facts missing or over budget"));
        }
        state.records = count.ok_or_else(|| failed("task capture count overflow"))?;
        state.bytes = bytes;
        Ok(())
    }
}

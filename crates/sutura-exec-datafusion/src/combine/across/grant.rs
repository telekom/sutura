//! The task grant a node asks of every call to its worker, keyed per question (Decision 5).
//!
//! The node that plans a question draws a key for it and binds the question to that key before any
//! stage leaves the node. Each call that installs a stage on another node carries the key and an
//! HMAC grant under it; each call that executes a task carries the grant alone, which a node that
//! was installed with a stage forwards. A worker keeps the first key a question is installed under
//! and admits a later call for that question only with a grant that key verifies, so a node of the
//! authority that was sent no stage of a question reads none of its output.
//!
//! The engine hands the grant each call's headers and decoded task key before the call reaches the
//! worker's tasks, through an authorization hook the engine copy vendored in this repository
//! carries until a release of the engine does (`VENDOR.md`).
//!
//! # Limits
//!
//! - Who may install a question is the link's mutual TLS alone: the installing node supplies the
//!   key. The first key a question is installed under stays, so another node of the authority that
//!   learns a question's id cannot re-key it, but by installing it first on a worker it can have the
//!   question refused there - a refusal, not a read.
//! - A call to execute a task waits up to ten seconds, the engine's own wait for a stage, for its
//!   question to be installed, and is refused after it.
//! - A worker keeps at most 65536 questions, each until ten minutes pass with no call for it, and
//!   refuses to install one more.
//! - A call the planning node makes to its own worker in process crosses no gRPC and is not asked
//!   for a grant; the question is that node's own.
//! - The grant is a bearer token for its question while the question is kept, held off the wire by
//!   the link's TLS.

use core::time::Duration;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::time::Instant;

use datafusion::catalog::Session;
use datafusion::common::tree_node::{TreeNode as _, TreeNodeRecursion};
use datafusion::error::DataFusionError;
use datafusion::execution::SessionStateBuilder;
use datafusion::execution::context::QueryPlanner;
use datafusion::logical_expr::LogicalPlan;
use datafusion::physical_plan::ExecutionPlan;
use datafusion_distributed::grpc::{WorkerCall, WorkerCallAuthorizer};
use datafusion_distributed::{DistributedExt as _, NetworkBoundaryExt as _};
use ring::hmac;
use tonic::Status;
use tonic::body::Body;
use tonic::codegen::http::{self, HeaderMap, HeaderValue};

const KEY: &str = "x-sutura-task-key";
const GRANT: &str = "x-sutura-task-grant";
const SIGNED: &[u8] = b"sutura task grant";
const INSTALLS: &str = "/worker.WorkerService/CoordinatorChannel";
const AWAITED: Duration = Duration::from_secs(10);
const POLLED: Duration = Duration::from_millis(20);
const IDLE: Duration = Duration::from_secs(600);
const KEPT: usize = 1 << 16;

/// The key one question's grants are signed under. No `Debug`, so it is never printed.
#[derive(Clone)]
struct Key([u8; 32]);

impl Key {
    fn drawn() -> Result<Self, DataFusionError> {
        let mut key = [0; 32];
        #[expect(
            clippy::disallowed_methods,
            reason = "the key one question's task grants are signed under; random so no node can mint a grant \
                      for a question it was sent no stage of"
        )]
        let drawn = getrandom::fill(&mut key);
        drawn.map_err(|cause| DataFusionError::Execution(format!("no key could be drawn for the question: {cause}")))?;
        Ok(Self(key))
    }

    fn signing(&self) -> hmac::Key {
        hmac::Key::new(hmac::HMAC_SHA256, &self.0)
    }

    fn grant(&self) -> Vec<u8> {
        hmac::sign(&self.signing(), SIGNED).as_ref().to_vec()
    }

    fn verifies(&self, grant: &[u8]) -> bool {
        hmac::verify(&self.signing(), SIGNED, grant).is_ok()
    }
}

/// The questions a worker was installed with, each under the first key it was installed under.
#[derive(Clone, Default)]
pub(super) struct Keys(Arc<tokio::sync::Mutex<Questions>>);

type Questions = HashMap<[u8; 16], Kept>;

struct Kept {
    key: Key,
    used: Instant,
}

impl Kept {
    /// Whether this key verifies `grant`, which keeps the question until [`IDLE`] after `now`.
    fn verified(&mut self, grant: &[u8], now: Instant) -> bool {
        let verified = self.key.verifies(grant);
        if verified {
            self.used = now;
        }
        verified
    }
}

impl Keys {
    /// Whether `question` is kept under a key that verifies `grant`, keeping it under `key` first if
    /// it is not kept yet and fewer than [`KEPT`] questions are.
    async fn installed(&self, question: [u8; 16], key: Key, grant: &[u8]) -> bool {
        let now = Instant::now();
        let mut kept = self.0.lock().await;
        kept.retain(|_, held| now.duration_since(held.used) < IDLE);
        let full = kept.len() >= KEPT;
        let verified = match kept.entry(question) {
            Entry::Occupied(held) => held.into_mut().verified(grant, now),
            Entry::Vacant(_) if full => false,
            Entry::Vacant(unseen) => unseen.insert(Kept { key, used: now }).verified(grant, now),
        };
        drop(kept);
        verified
    }

    /// Whether `question`'s key verifies `grant`, or `None` while it is not kept.
    async fn used(&self, question: [u8; 16], grant: &[u8]) -> Option<bool> {
        self.0
            .lock()
            .await
            .get_mut(&question)
            .map(|held| held.verified(grant, Instant::now()))
    }

    /// [`Self::used`], waiting up to [`AWAITED`] for `question` to be installed.
    async fn verified(&self, question: [u8; 16], grant: &[u8]) -> bool {
        let deadline = Instant::now().checked_add(AWAITED);
        loop {
            if let Some(verified) = self.used(question, grant).await {
                return verified;
            }
            if deadline.is_none_or(|deadline| Instant::now() >= deadline) {
                return false;
            }
            tokio::time::sleep(POLLED).await;
        }
    }
}

/// The task grant a node's worker asks of every call it is sent over gRPC.
///
/// A call to install a stage is admitted with a key and a grant under it, the first key a question is
/// installed under kept; a call to execute a task is admitted with a grant its question's kept key
/// verifies. Any other call is refused.
#[derive(Clone, Default)]
pub struct TaskGrant(Keys);

impl core::fmt::Debug for TaskGrant {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TaskGrant").finish_non_exhaustive()
    }
}

impl TaskGrant {
    /// The grant admitting the questions `keys` holds, which the node planning them binds before any
    /// of their stages leaves it.
    pub(super) const fn of(keys: Keys) -> Self {
        Self(keys)
    }
}

#[async_trait::async_trait]
impl WorkerCallAuthorizer for TaskGrant {
    async fn authorize(&self, call: WorkerCall<'_>) -> Result<(), Status> {
        let grant = header(call.headers(), GRANT).ok_or_else(refused)?;
        let asked = *call.task_key().query_id.as_bytes();
        let admitted = match call {
            WorkerCall::SetPlan { headers, .. } => {
                let key = header(headers, KEY)
                    .and_then(|key| <[u8; 32]>::try_from(key).ok())
                    .map(Key)
                    .ok_or_else(refused)?;
                key.verifies(&grant) && self.0.installed(asked, key, &grant).await
            }
            WorkerCall::ExecuteTask { .. } => self.0.verified(asked, &grant).await,
            _ => false,
        };
        admitted.then_some(()).ok_or_else(refused)
    }
}

fn refused() -> Status {
    Status::permission_denied("this node was granted no such task")
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0xf])
        .filter_map(|digit| char::from_digit(u32::from(digit), 16))
        .collect()
}

fn unhex(text: &[u8]) -> Option<Vec<u8>> {
    let (pairs, []) = text.as_chunks::<2>() else {
        return None;
    };
    let digit = |byte: &u8| u8::try_from(char::from(*byte).to_digit(16)?).ok();
    pairs
        .iter()
        .map(|[high, low]| Some((digit(high)? << 4) | digit(low)?))
        .collect()
}

fn header(headers: &HeaderMap, name: &str) -> Option<Vec<u8>> {
    unhex(headers.get(name)?.as_bytes())
}

fn valued(bytes: &[u8]) -> Result<HeaderValue, DataFusionError> {
    let mut value = HeaderValue::try_from(hex(bytes)).map_err(|cause| DataFusionError::External(Box::new(cause)))?;
    value.set_sensitive(true);
    Ok(value)
}

/// `builder`, whose distributed planner is set, planning each question under a key of its own: the
/// key and its grant ride on every call the session makes to another node, and each question it
/// plans is bound to the key in `keys` before any of its stages is sent.
pub(super) fn keyed(mut builder: SessionStateBuilder, keys: Keys) -> Result<SessionStateBuilder, DataFusionError> {
    let key = Key::drawn()?;
    let grant = key.grant();
    let mut sent = HeaderMap::new();
    sent.insert(KEY, valued(&key.0)?);
    sent.insert(GRANT, valued(&grant)?);
    let slot = builder.query_planner();
    let planner = slot.take().ok_or_else(|| {
        DataFusionError::Configuration(String::from("a question is keyed only once its distributed planner is set"))
    })?;
    *slot = Some(Arc::new(Keyed {
        planner,
        keys,
        key,
        grant,
    }));
    builder.with_distributed_passthrough_headers(sent)
}

/// A planner that binds each question it plans to its session's key.
struct Keyed {
    planner: Arc<dyn QueryPlanner + Send + Sync>,
    keys: Keys,
    key: Key,
    grant: Vec<u8>,
}

impl core::fmt::Debug for Keyed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Keyed")
            .field("planner", &self.planner)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl QueryPlanner for Keyed {
    async fn create_physical_plan(
        &self,
        logical_plan: &LogicalPlan,
        session: &dyn Session,
    ) -> Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        let plan = self.planner.create_physical_plan(logical_plan, session).await?;
        let mut asked = None;
        plan.apply(|node| {
            asked = node
                .as_network_boundary()
                .map(|boundary| *boundary.input_stage().query_id().as_bytes());
            Ok(if asked.is_some() {
                TreeNodeRecursion::Stop
            } else {
                TreeNodeRecursion::Continue
            })
        })?;
        if let Some(asked) = asked
            && !self.keys.installed(asked, self.key.clone(), &self.grant).await
        {
            return Err(DataFusionError::Execution(String::from(
                "the question is already installed on this node under another key",
            )));
        }
        Ok(plan)
    }
}

/// `request` with no key unless it installs a stage: a call to execute a task carries the grant alone.
pub(super) fn keyless(mut request: http::Request<Body>) -> http::Request<Body> {
    if request.uri().path() != INSTALLS {
        request.headers_mut().remove(KEY);
    }
    request
}

/// `builder`, forwarding the grant a stage was installed with on each call its tasks make to another
/// node, and not the key.
pub(super) fn forwarded(builder: SessionStateBuilder, headers: &HeaderMap) -> Result<SessionStateBuilder, DataFusionError> {
    let mut sent = HeaderMap::new();
    if let Some(grant) = headers.get(GRANT) {
        let mut grant = grant.clone();
        grant.set_sensitive(true);
        sent.insert(GRANT, grant);
    }
    builder.with_distributed_passthrough_headers(sent)
}

#[cfg(test)]
mod tests;

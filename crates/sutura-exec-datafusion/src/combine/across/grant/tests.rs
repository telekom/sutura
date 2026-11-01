use core::time::Duration;
use std::sync::Arc;
use std::time::Instant;

use datafusion::arrow::datatypes::Schema;
use datafusion::execution::TaskContext;
use datafusion::physical_plan::empty::EmptyExec;
use datafusion::physical_plan::metrics::ExecutionPlanMetricsSet;
use datafusion::prelude::{SessionConfig, SessionContext};
use datafusion_distributed::grpc::{BoxCloneSyncChannel, WorkerCall, WorkerCallAuthorizer as _, create_worker_client};
use datafusion_distributed::{
    DistributedConfig, ExecuteTaskRequest, MaybeEncoded, ProducerHead, SetPlanRequest, TaskKey, Worker,
};
use futures_util::{TryStreamExt as _, future, stream};
use tonic::Code;
use tonic::body::Body;
use tonic::codegen::http::{self, HeaderMap};
use url::Url;

use super::{AWAITED, GRANT, KEY, Key, TaskGrant, valued};

const ASKED: &str = "07070707-0707-0707-0707-070707070707";

/// Stage 1's task 0 of the question `asked`.
fn task(asked: &str) -> TaskKey {
    TaskKey {
        query_id: asked.parse().expect("a question id"),
        stage_id: 1,
        task_number: 0,
    }
}

fn headers(key: Option<&Key>, grant: Option<&[u8]>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(key) = key {
        headers.insert(KEY, valued(&key.0).expect("a key is a header value"));
    }
    if let Some(grant) = grant {
        headers.insert(GRANT, valued(grant).expect("a grant is a header value"));
    }
    headers
}

/// What `grant` answers a call that installs a stage of `asked` with `sent` headers: the refusal's
/// code, or `None` when it is admitted.
async fn installed(grant: &TaskGrant, asked: &str, sent: &HeaderMap) -> Option<Code> {
    let task_key = task(asked);
    let call = WorkerCall::SetPlan {
        headers: sent,
        task_key: &task_key,
    };
    grant.authorize(call).await.err().map(|refused| refused.code())
}

/// [`installed`], for a call that executes a task of `asked`.
async fn executed(grant: &TaskGrant, asked: &str, sent: &HeaderMap) -> Option<Code> {
    let task_key = task(asked);
    let call = WorkerCall::ExecuteTask {
        headers: sent,
        task_key: &task_key,
    };
    grant.authorize(call).await.err().map(|refused| refused.code())
}

fn run<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a runtime builds")
        .block_on(future)
}

#[test]
fn a_call_is_admitted_only_with_a_grant_the_key_its_question_was_first_installed_under_verifies() {
    let (key, other) = (Key::drawn().expect("a key draws"), Key::drawn().expect("a key draws"));
    let (grant, forged) = (key.grant(), other.grant());
    let answers = run(async {
        let granted = TaskGrant::default();
        [
            installed(&granted, ASKED, &headers(Some(&key), None)).await,
            installed(&granted, ASKED, &headers(None, Some(&grant))).await,
            installed(&granted, ASKED, &headers(Some(&key), Some(&forged))).await,
            executed(&granted, ASKED, &headers(None, Some(&grant))).await,
            installed(&granted, ASKED, &headers(Some(&key), Some(&grant))).await,
            installed(&granted, ASKED, &headers(Some(&other), Some(&forged))).await,
            executed(&granted, ASKED, &headers(None, Some(&forged))).await,
            executed(&granted, ASKED, &headers(None, Some(&grant))).await,
        ]
    });
    let refused = Some(Code::PermissionDenied);
    assert_eq!(answers, [refused, refused, refused, refused, None, refused, refused, None]);
}

#[test]
fn a_task_waits_for_its_question_to_be_installed_and_no_longer_than_the_engine_waits() {
    let key = Key::drawn().expect("a key draws");
    let grant = key.grant();
    let started = Instant::now();
    let answers = run(async {
        let granted = TaskGrant::default();
        let (waited, installs) = future::join(executed(&granted, ASKED, &headers(None, Some(&grant))), async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            installed(&granted, ASKED, &headers(Some(&key), Some(&grant))).await
        })
        .await;
        let never = executed(&granted, "09090909-0909-0909-0909-090909090909", &headers(None, Some(&grant))).await;
        [waited, installs, never]
    });
    assert_eq!(answers, [None, None, Some(Code::PermissionDenied)]);
    assert!(
        started.elapsed() >= AWAITED,
        "a task was refused before the engine stops waiting for its stage"
    );
}

#[test]
fn an_engine_encoded_stage_and_its_task_pass_with_their_grant_and_are_refused_without_it() {
    let (key, other) = (Key::drawn().expect("a key draws"), Key::drawn().expect("a key draws"));
    let grant = key.grant();
    let stage = || SetPlanRequest {
        task_key: task(ASKED),
        task_count: 1,
        plan: MaybeEncoded::Decoded(Arc::new(EmptyExec::new(Arc::new(Schema::empty())))),
        work_unit_feed_declarations: vec![],
        target_worker_url: Url::parse("https://127.0.0.1:1").expect("an address is a URL"),
        query_start_time_ns: 0,
    };
    let executes = || ExecuteTaskRequest {
        task_key: task(ASKED),
        target_partition_start: 0,
        target_partition_end: 1,
        producer_head: ProducerHead::None,
    };
    let refused = run(async {
        let context: Arc<TaskContext> =
            SessionContext::new_with_config(SessionConfig::new().with_option_extension(DistributedConfig::default())).task_ctx();
        let mut link = create_worker_client(BoxCloneSyncChannel::new(tower::ServiceExt::<http::Request<Body>>::map_err(
            Worker::default()
                .with_call_authorizer(TaskGrant::default())
                .into_worker_server(),
            |never| match never {},
        )));
        let mut install = async |sent: HeaderMap| {
            link.coordinator_channel(
                sent,
                stage(),
                Box::pin(stream::pending()),
                ExecutionPlanMetricsSet::new(),
                &context,
            )
            .await
        };
        let unsigned = install(headers(None, None)).await.err();
        let installed = install(headers(Some(&key), Some(&grant)))
            .await
            .expect("a stage with its key's grant is installed");
        let mut execute = async |sent: HeaderMap| {
            let mut answered = link
                .execute_task(sent, executes(), ExecutionPlanMetricsSet::new(), &context)
                .await?;
            match answered.first_mut() {
                Some(rows) => rows.try_next().await.map(drop),
                None => Ok(()),
            }
        };
        let forged = execute(headers(None, Some(&other.grant()))).await.err();
        execute(headers(None, Some(&grant)))
            .await
            .expect("a task with its question's grant runs");
        drop(installed);
        [unsigned, forged]
    });
    for refused in refused {
        assert!(
            refused
                .as_ref()
                .is_some_and(|refused| refused.to_string().contains("granted no such task")),
            "{refused:?}"
        );
    }
}

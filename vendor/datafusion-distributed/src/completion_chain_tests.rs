//! The real coordinator and worker, with loss injected only on the existing channel port.
use crate::{
    ChannelResolver, CoordinatorToWorkerMsg, DistributedExec, DistributedExt, ExecuteTaskRequest, GetWorkerInfoRequest,
    GetWorkerInfoResponse, NetworkCoalesceExec, QueryCompletion, SessionStateBuilderExt, SetPlanRequest, TaskCompletionProducer,
    Worker, WorkerChannel, WorkerQueryContext, WorkerToCoordinatorMsg,
};
use async_trait::async_trait;
use datafusion::arrow::array::{ArrayRef, Int32Array};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::common::tree_node::TreeNodeRecursion;
use datafusion::common::{DataFusionError, Result};
use datafusion::datasource::memory::MemorySourceConfig;
use datafusion::execution::{SessionStateBuilder, TaskContext};
use datafusion::physical_expr::PhysicalExpr;
use datafusion::physical_plan::metrics::ExecutionPlanMetricsSet;
use datafusion::physical_plan::{DisplayAs, DisplayFormatType, ExecutionPlan, PlanProperties, SendableRecordBatchStream};
use datafusion::prelude::{SessionConfig, SessionContext};
use futures::stream::BoxStream;
use futures::{Stream, StreamExt, TryStreamExt};
use http::HeaderMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::Instant;
use url::Url;

struct Channel {
    inner: Box<dyn WorkerChannel>,
    cut_after_row: Option<Arc<Notify>>,
    worker_timeout: Option<Duration>,
    cuts: Arc<AtomicUsize>,
    preparations: Arc<AtomicUsize>,
    preparation_gate: Option<Arc<PreparationGate>>,
}

#[derive(Debug, Default)]
struct PreparationGate {
    entered: Notify,
    release: Notify,
    released: AtomicBool,
    executions: AtomicUsize,
    head_executions: AtomicUsize,
}

#[derive(Debug)]
struct ObservedHead {
    inner: Arc<dyn ExecutionPlan>,
    gate: Arc<PreparationGate>,
}

impl DisplayAs for ObservedHead {
    fn fmt_as(&self, format: DisplayFormatType, output: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.inner.fmt_as(format, output)
    }
}

impl ExecutionPlan for ObservedHead {
    fn name(&self) -> &str { "ObservedHead" }
    fn properties(&self) -> &Arc<PlanProperties> { self.inner.properties() }
    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> { vec![&self.inner] }
    fn apply_expressions(
        &self,
        _f: &mut dyn FnMut(&Arc<dyn PhysicalExpr>) -> Result<TreeNodeRecursion>,
    ) -> Result<TreeNodeRecursion> {
        Ok(TreeNodeRecursion::Continue)
    }
    fn with_new_children(self: Arc<Self>, children: Vec<Arc<dyn ExecutionPlan>>) -> Result<Arc<dyn ExecutionPlan>> {
        assert_eq!(children.len(), 1);
        Ok(Arc::new(Self { inner: Arc::clone(&children[0]), gate: Arc::clone(&self.gate) }))
    }
    fn execute(&self, partition: usize, context: Arc<TaskContext>) -> Result<SendableRecordBatchStream> {
        self.gate.head_executions.fetch_add(1, Ordering::SeqCst);
        self.inner.execute(partition, context)
    }
}

#[async_trait]
impl WorkerChannel for Channel {
    async fn coordinator_channel(
        &mut self,
        headers: HeaderMap,
        mut request: SetPlanRequest,
        input: BoxStream<'static, CoordinatorToWorkerMsg>,
        metrics: ExecutionPlanMetricsSet,
        context: &Arc<TaskContext>,
    ) -> Result<BoxStream<'static, Result<WorkerToCoordinatorMsg>>> {
        if let Some(gate) = &self.preparation_gate {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
        if let (Some(request), Some(timeout)) = (&mut request.completion, self.worker_timeout) {
            request.timeout = timeout;
        }
        let output = self
            .inner
            .coordinator_channel(headers, request, input, metrics, context)
            .await?;
        self.preparations.fetch_add(1, Ordering::SeqCst);
        let Some(row_observed) = self.cut_after_row.as_ref().map(Arc::clone) else {
            return Ok(output);
        };
        let cuts = Arc::clone(&self.cuts);
        // Cut only after the outer query has observed a real row.
        Ok(output
            .then(move |message| {
                let row_observed = Arc::clone(&row_observed);
                async move {
                    if matches!(message, Ok(WorkerToCoordinatorMsg::TaskCompletion(_))) {
                        row_observed.notified().await;
                    }
                    message
                }
            })
            .take_while(move |message| {
                let cut = matches!(message, Ok(WorkerToCoordinatorMsg::TaskCompletion(_)));
                if cut {
                    cuts.fetch_add(1, Ordering::SeqCst);
                }
                futures::future::ready(!cut)
            })
            .boxed())
    }

    async fn execute_task(
        &mut self,
        headers: HeaderMap,
        request: ExecuteTaskRequest,
        metrics: ExecutionPlanMetricsSet,
        context: &Arc<TaskContext>,
    ) -> Result<Vec<BoxStream<'static, Result<RecordBatch>>>> {
        if let Some(gate) = &self.preparation_gate {
            gate.executions.fetch_add(1, Ordering::SeqCst);
            if !gate.released.load(Ordering::SeqCst) || self.preparations.load(Ordering::SeqCst) == 0 {
                return Err(datafusion::common::exec_datafusion_err!("ExecuteTask entered before plan readiness"));
            }
        }
        self.inner.execute_task(headers, request, metrics, context).await
    }

    async fn get_worker_info(&mut self, request: GetWorkerInfoRequest) -> Result<GetWorkerInfoResponse> {
        self.inner.get_worker_info(request).await
    }
}

struct Resolver {
    worker: Worker,
    cut_after_row: Option<Arc<Notify>>,
    worker_timeout: Option<Duration>,
    cuts: Arc<AtomicUsize>,
    preparations: Arc<AtomicUsize>,
    preparation_gate: Option<Arc<PreparationGate>>,
}

#[async_trait]
impl ChannelResolver for Resolver {
    async fn get_worker_client_for_url(&self, _: &Url) -> Result<Box<dyn WorkerChannel>> {
        Ok(Box::new(Channel {
            inner: crate::LocalWorkerContext {
                local_worker: self.worker.clone(),
                self_url: Url::parse("http://worker").unwrap(),
            }
            .to_worker_channel(),
            cut_after_row: self.cut_after_row.as_ref().map(Arc::clone),
            worker_timeout: self.worker_timeout,
            cuts: Arc::clone(&self.cuts),
            preparations: Arc::clone(&self.preparations),
            preparation_gate: self.preparation_gate.as_ref().map(Arc::clone),
        }))
    }
}

fn worker(producer: Option<Arc<TaskCompletionProducer>>) -> Worker {
    Worker::from_session_builder(move |mut context: WorkerQueryContext| {
        let producer = producer.clone();
        async move {
            if let Some(producer) = producer {
                let config = context.builder.config().take().unwrap_or_default();
                *context.builder.config() = Some(config.with_extension(producer));
            }
            Ok(context.builder.build())
        }
    })
}

fn execution(
    producer: Option<Arc<TaskCompletionProducer>>,
    cut_after_row: Option<Arc<Notify>>,
    worker_timeout: Option<Duration>,
) -> (
    SendableRecordBatchStream,
    Arc<QueryCompletion>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    execution_with_preparation(producer, cut_after_row, worker_timeout, None, false)
}

fn execution_with_preparation(
    producer: Option<Arc<TaskCompletionProducer>>,
    cut_after_row: Option<Arc<Notify>>,
    worker_timeout: Option<Duration>,
    preparation_gate: Option<Arc<PreparationGate>>,
    dynamic: bool,
) -> (
    SendableRecordBatchStream,
    Arc<QueryCompletion>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let worker = worker(producer);
    let tasks = if dynamic { 2 } else { 1 };
    let completion = Arc::new(QueryCompletion::new(tasks, tasks, 8 * tasks, Instant::now() + Duration::from_secs(2)).unwrap());
    let cuts = Arc::new(AtomicUsize::new(0));
    let preparations = Arc::new(AtomicUsize::new(0));
    let state = SessionStateBuilder::new()
        .with_default_features()
        .with_config(SessionConfig::new().with_extension(Arc::clone(&completion)))
        .with_distributed_planner()
        .with_distributed_dynamic_task_count(dynamic)
        .unwrap()
        .with_distributed_max_tasks_per_stage(1)
        .unwrap()
        .with_distributed_route_tasks_handler(|_: crate::RouteTasksEvent<'_>| {
            Some(Ok(crate::RouteTasksEventResponse::new(vec![
                Url::parse("http://worker").unwrap(),
            ])))
        })
        .with_distributed_channel_resolver(Resolver {
            worker,
            cut_after_row,
            worker_timeout,
            cuts: Arc::clone(&cuts),
            preparations: Arc::clone(&preparations),
            preparation_gate: preparation_gate.as_ref().map(Arc::clone),
        })
        .build();
    let values: ArrayRef = Arc::new(Int32Array::from(vec![7]));
    let batch = RecordBatch::try_from_iter([("value", values)]).unwrap();
    let leaf = MemorySourceConfig::try_new_exec(&[vec![batch.clone()]], batch.schema(), None).unwrap();
    let boundary: Arc<dyn ExecutionPlan> = if dynamic {
        let column: Arc<dyn datafusion::physical_expr::PhysicalExpr> =
            Arc::new(datafusion::physical_expr::expressions::Column::new("value", 0));
        let repartition = Arc::new(datafusion::physical_plan::repartition::RepartitionExec::try_new(
            leaf, datafusion::physical_expr::Partitioning::Hash(vec![Arc::clone(&column)], 1),
        ).unwrap());
        let projected = Arc::new(datafusion::physical_plan::projection::ProjectionExec::try_new(
            vec![(column, "value".to_owned())], repartition,
        ).unwrap());
        Arc::new(datafusion::physical_plan::coalesce_partitions::CoalescePartitionsExec::new(projected))
    } else {
        Arc::new(NetworkCoalesceExec::try_new(leaf, 1, 1).unwrap())
    };
    let boundary = match preparation_gate {
        Some(gate) => Arc::new(ObservedHead { inner: boundary, gate }) as Arc<dyn ExecutionPlan>,
        None => boundary,
    };
    let plan = DistributedExec::new(boundary);
    let stream = plan.execute(0, SessionContext::from(state).task_ctx()).unwrap();
    (stream, completion, cuts, preparations)
}

async fn drain(
    mut stream: impl Stream<Item = Result<RecordBatch>> + Unpin,
) -> Result<(usize, Option<DataFusionError>), tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(10), async move {
        let mut rows = 0;
        while let Some(batch) = stream.next().await {
            match batch {
                Ok(batch) => {
                    let values = batch.column(0).as_any().downcast_ref::<Int32Array>().unwrap();
                    for value in values.iter() {
                        assert_eq!(value, Some(7));
                    }
                    rows += batch.num_rows();
                }
                Err(cause) => return (rows, Some(cause)),
            }
        }
        (rows, None)
    })
    .await
}

fn ready(calls: Arc<AtomicUsize>) -> Arc<TaskCompletionProducer> {
    Arc::new(TaskCompletionProducer::new(move |_, _, _| {
        let calls = Arc::clone(&calls);
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![vec![42]])
        })
    }))
}

#[tokio::test]
async fn completion_chain_waits_for_real_worker_plan_readiness_before_execute() {
    let gate = Arc::new(PreparationGate::default());
    let (stream, completion, _, preparations) = execution_with_preparation(
        Some(ready(Arc::new(AtomicUsize::new(0)))), None, None, Some(Arc::clone(&gate)), false,
    );
    tokio::time::timeout(Duration::from_secs(1), gate.entered.notified()).await.unwrap();
    // On this current-thread runtime, the old synchronous preparation executes the head
    // before its spawned SetPlan task can signal `entered`. This assertion needs no sleep.
    assert_eq!(gate.head_executions.load(Ordering::SeqCst), 0, "head executed before gated SetPlan readiness");
    assert_eq!(preparations.load(Ordering::SeqCst), 0);
    assert_eq!(gate.executions.load(Ordering::SeqCst), 0, "ExecuteTask entered while SetPlan was gated");
    gate.released.store(true, Ordering::SeqCst);
    gate.release.notify_one();
    let (rows, failure) = drain(stream).await.unwrap();
    assert_eq!(rows, 1);
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(preparations.load(Ordering::SeqCst), 1);
    assert_eq!(gate.executions.load(Ordering::SeqCst), 1);
    assert_eq!(gate.head_executions.load(Ordering::SeqCst), 1);
    assert_eq!(completion.take().unwrap()[0].records, vec![vec![42]]);

    let (stream, completion, _, preparations) = execution_with_preparation(
        Some(ready(Arc::new(AtomicUsize::new(0)))), None, None, None, true,
    );
    let (rows, failure) = drain(stream).await.unwrap();
    assert_eq!(rows, 1);
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(preparations.load(Ordering::SeqCst), 2);
    let receipts = completion.take().unwrap();
    assert_eq!(receipts.len(), 2);
    assert!(receipts.iter().all(|receipt| receipt.records == vec![vec![42]]));
}

#[tokio::test]
async fn completion_chain_publishes_receipts_only_after_real_worker_rows() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (stream, completion, cuts, _) = execution(Some(ready(Arc::clone(&calls))), None, None);
    assert!(completion.take().is_err());
    let (rows, failure) = drain(stream)
        .await
        .expect("the production deadline must terminate the query before the harness");
    assert_eq!(rows, 1);
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(cuts.load(Ordering::SeqCst), 0);
    let receipts = completion.take().unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].records, vec![vec![42]]);
}

#[tokio::test]
async fn completion_chain_refuses_worker_channel_eof_after_rows_without_a_receipt() {
    let calls = Arc::new(AtomicUsize::new(0));
    let row_observed = Arc::new(Notify::new());
    let (stream, completion, cuts, _) = execution(Some(ready(Arc::clone(&calls))), Some(Arc::clone(&row_observed)), None);
    let stream = stream.inspect_ok(move |batch| {
        if batch.num_rows() > 0 {
            row_observed.notify_one();
        }
    });
    let (rows, failure) = drain(stream)
        .await
        .expect("the production deadline must terminate the query before the harness");
    assert_eq!(rows, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(cuts.load(Ordering::SeqCst), 1);
    let failure = failure.expect("row EOF must include the worker receipt barrier");
    assert!(
        format!("{failure}").contains("worker channel closed without task completion"),
        "{failure}"
    );
    assert!(completion.take().is_err());
}

#[tokio::test]
async fn completion_chain_refuses_a_missing_producer_before_worker_rows() {
    let (stream, completion, _, preparations) = execution(None, None, None);
    let (rows, failure) = drain(stream)
        .await
        .expect("the production deadline must terminate the query before the harness");
    assert_eq!(preparations.load(Ordering::SeqCst), 0);
    assert_eq!(rows, 0);
    let failure = failure.expect("required producer is absent");
    assert!(
        format!("{failure}").contains("required task completion producer is absent"),
        "{failure}"
    );
    assert!(completion.take().is_err());
}

#[tokio::test]
async fn completion_chain_bounds_a_pending_producer_after_worker_rows() {
    let calls = Arc::new(AtomicUsize::new(0));
    let entered = Arc::clone(&calls);
    let producer = Arc::new(TaskCompletionProducer::new(move |_, _, _| {
        let entered = Arc::clone(&entered);
        Box::pin(async move {
            entered.fetch_add(1, Ordering::SeqCst);
            futures::future::pending().await
        })
    }));
    // Keep the worker timer beyond the harness so only the query timer can satisfy this cell.
    let (stream, completion, _, _) = execution(Some(producer), None, Some(Duration::from_secs(20)));
    let (rows, failure) = drain(stream)
        .await
        .expect("the production deadline must terminate the query before the harness");
    assert_eq!(rows, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let failure = failure.expect("a pending producer must not turn row EOF into success");
    assert!(format!("{failure}").contains("deadline"), "{failure}");
    assert!(completion.take().is_err());
}

#[tokio::test]
async fn completion_worker_bounds_its_pending_producer_without_a_query_timer() {
    let calls = Arc::new(AtomicUsize::new(0));
    let entered = Arc::clone(&calls);
    let producer = Arc::new(TaskCompletionProducer::new(move |_, _, _| {
        let entered = Arc::clone(&entered);
        Box::pin(async move {
            entered.fetch_add(1, Ordering::SeqCst);
            futures::future::pending().await
        })
    }));
    let plan = Arc::new(datafusion::physical_plan::empty::EmptyExec::new(Arc::new(
        datafusion::arrow::datatypes::Schema::empty(),
    )));
    let request = SetPlanRequest {
        completion: Some(crate::CompletionRequest::parse(1, 8, 2000).unwrap()),
        task_key: crate::TaskKey {
            query_id: uuid::Uuid::from_u128(1),
            stage_id: 0,
            task_number: 0,
        },
        task_count: 1,
        plan: crate::MaybeEncoded::Decoded(plan),
        work_unit_feed_declarations: vec![],
        target_worker_url: Url::parse("http://worker").unwrap(),
        query_start_time_ns: 0,
    };
    let mut output = worker(Some(producer))
        .coordinator_channel(HeaderMap::new(), request, futures::stream::empty().boxed())
        .await
        .unwrap();
    let failure = tokio::time::timeout(Duration::from_secs(10), async move {
        while let Some(message) = output.next().await {
            match message {
                Err(cause) => return cause,
                Ok(WorkerToCoordinatorMsg::TaskCompletion(_)) => panic!("pending producer completed"),
                Ok(_) => (),
            }
        }
        panic!("worker omitted its deadline error")
    })
    .await
    .expect("the worker timer must fire before the harness");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(format!("{failure}").contains("task completion deadline"), "{failure}");
}

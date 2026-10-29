use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Poll;
use std::time::{Duration, Instant};

use arrow_array::{RecordBatch, StringArray};
use arrow_flight::error::FlightError;
use arrow_flight::flight_service_server::FlightServiceServer;
use arrow_flight::{FlightClient, FlightData, FlightDescriptor};
use arrow_schema::{DataType, Field, Schema};
use futures_util::TryStreamExt as _;
use parking_lot::Mutex;
use sutura_app::surface::{ErasedCause, SurfaceFailure};
use sutura_app::{Asked, Capability, Permitted};
use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};
use sutura_domain::model::{MetricName, SourceName};
use sutura_domain::plan::remote::{LegWire, RemoteLegOutcome};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::arrow::{Accumulating, ResultBudget};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Server;
use tonic::{Code, Status};

use super::{DescriptorLimit, FrameBudget, RemoteLegService, Surface, call_budget, status};

const DESCRIPTOR_LIMIT: usize = 1 << 16;
const FRAME_BUDGET: usize = 1 << 20;

fn budget(duration: Duration) -> Budget {
    Budget::parse(duration).expect("a nonzero budget")
}

/// A leg whose content a fake [`Surface`] never reads - only that framing accepted it.
///
/// Built by deserializing, the same path a real peer's bytes take: a metric digest has no public
/// Rust constructor on purpose, and a crafted one that satisfies the wire shape is exactly what
/// this crate certifies nothing about before handing it to [`Surface::run_remote_leg`].
fn a_lookup_leg() -> LegWire {
    let metric = MetricName::parse("revenue").expect("a metric name");
    let digest = DefinitionDigest::parse("a".repeat(64)).expect("a digest-shaped string");
    let payload = serde_json::json!({
        "leg": "lookup",
        "source": "warehouse",
        "metric": {"metric": metric, "digest": digest},
        "table": "orders",
        "joins": [],
        "keys": [],
        "filters": [],
    });
    serde_json::from_value(payload).expect("a well-formed lookup leg")
}

fn one_row_batch() -> RecordBatch {
    let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Utf8, false)]));
    RecordBatch::try_new(schema, vec![Arc::new(StringArray::from(vec!["berlin"]))]).expect("one row")
}

/// The leg and one parameter batch, encoded the way a caller sends them.
fn a_request() -> crate::RequestStream {
    crate::request_stream(&a_lookup_leg(), futures_util::stream::iter(vec![Ok(one_row_batch())])).expect("an encodable leg")
}

fn an_answer() -> RemoteLegOutcome {
    let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Utf8, false)]));
    let mut accumulating = Accumulating::announcing(
        Arc::clone(&schema),
        1,
        ResultBudget::of_bytes(std::num::NonZeroUsize::new(1 << 20).expect("nonzero")),
    );
    accumulating.push(one_row_batch()).expect("the schema matches");
    RemoteLegOutcome::Answer {
        batches: accumulating.finish(),
    }
}

fn a_refusal() -> RemoteLegOutcome {
    RemoteLegOutcome::Refusal {
        reason: RefusalReason::RemoteLegNotCertified {
            source: SourceName::parse("warehouse").expect("a source name"),
        },
    }
}

fn a_warehouse_outage() -> Result<RemoteLegOutcome, SurfaceFailure> {
    Err(SurfaceFailure::Warehouse {
        cause: ErasedCause::from("the fake data system names a table"),
    })
}

/// The driving-port fake, mirroring `sutura-mcp`'s `testing.rs` fakes: `do_exchange` never
/// reaches [`Surface::definitions`], `adopt`, `answer`, `run_sql`, or either spend accessor, so
/// none is given a real body.
type FakeOutcome = fn() -> Result<RemoteLegOutcome, SurfaceFailure>;

struct FakeSurface {
    calls: AtomicUsize,
    last_deadline: Mutex<Option<Deadline>>,
    outcome: FakeOutcome,
}

impl FakeSurface {
    fn answering(outcome: FakeOutcome) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            last_deadline: Mutex::new(None),
            outcome,
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn last_deadline(&self) -> Option<Deadline> {
        *self.last_deadline.lock()
    }
}

impl Surface for FakeSurface {
    fn definitions(&self) -> Arc<sutura_domain::pinned::PinnedDefinitions> {
        panic!("do_exchange never reads the pinned bundle")
    }

    #[expect(clippy::panic_in_result_fn, reason = "never called: do_exchange never adopts a bundle")]
    fn adopt(
        &self,
        _next: sutura_domain::pinned::PinnedDefinitions,
    ) -> Result<sutura_app::surface::Adopted, sutura_app::surface::NotAdopted> {
        panic!("do_exchange never adopts a bundle")
    }

    #[expect(clippy::panic_in_result_fn, reason = "never called: do_exchange never calls answer")]
    fn answer(
        &self,
        _context: &RequestContext,
        _query: &sutura_domain::query::Query,
        _deadline: Deadline,
    ) -> Result<sutura_domain::query::ToolOutcome, SurfaceFailure> {
        panic!("do_exchange never calls answer")
    }

    #[expect(clippy::panic_in_result_fn, reason = "never called: do_exchange never calls run_sql")]
    fn run_sql(
        &self,
        _context: &RequestContext,
        _statement: &sutura_domain::raw::RawStatement,
        _deadline: Deadline,
    ) -> Result<sutura_domain::raw::RawOutcome, SurfaceFailure> {
        panic!("do_exchange never calls run_sql")
    }

    fn run_remote_leg(
        &self,
        _context: &RequestContext,
        _leg: LegWire,
        _params: Vec<sutura_domain::warehouse::ParamValue>,
        deadline: Deadline,
    ) -> Result<RemoteLegOutcome, SurfaceFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_deadline.lock() = Some(deadline);
        (self.outcome)()
    }

    fn spend_headroom_bytes(&self) -> Option<u64> {
        None
    }

    fn spent_bytes_total(&self) -> Option<u64> {
        None
    }
}

/// The service over `surface`, with a thirty-second budget and generous framing limits.
fn service_over(surface: &Arc<FakeSurface>) -> RemoteLegService<FakeSurface> {
    limited(surface, budget(Duration::from_secs(30)), DESCRIPTOR_LIMIT, FRAME_BUDGET)
}

fn limited(
    surface: &Arc<FakeSurface>,
    own: Budget,
    descriptor_limit: usize,
    frame_budget: usize,
) -> RemoteLegService<FakeSurface> {
    RemoteLegService::new(
        Arc::clone(surface),
        own,
        DescriptorLimit::parse(descriptor_limit).expect("nonzero"),
        FrameBudget::parse(frame_budget).expect("nonzero"),
    )
}

/// A test-only interceptor standing in for whatever later establishes an [`Asked`] - out of
/// this crate's scope (module docs, point 1). `None` leaves a request with no established
/// caller, exercising the `Unauthenticated` arm.
#[derive(Clone)]
struct Admit(Option<Asked>);

impl Interceptor for Admit {
    fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, Status> {
        if let Some(ref asked) = self.0 {
            drop(request.extensions_mut().insert(asked.clone()));
        }
        Ok(request)
    }
}

fn asked(permitted: Permitted) -> Asked {
    Asked::established(
        RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself)),
        permitted,
    )
}

fn admitted() -> Admit {
    Admit(Some(asked(Permitted::every_capability())))
}

/// Serves `service` behind `admit` on a loopback port and returns a connected [`FlightClient`].
async fn serve(service: RemoteLegService<FakeSurface>, admit: Admit) -> FlightClient {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
    let addr = listener.local_addr().expect("a bound address");
    let incoming = futures_util::stream::unfold(listener, |listener| async move {
        listener
            .accept()
            .await
            .ok()
            .map(|(stream, _peer)| (Ok::<_, std::io::Error>(stream), listener))
    });
    let flight = InterceptedService::new(FlightServiceServer::new(service), admit);
    tokio::spawn(Server::builder().add_service(flight).serve_with_incoming(incoming));
    let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
        .expect("a valid loopback endpoint")
        .connect()
        .await
        .expect("the loopback server is listening");
    FlightClient::new(channel)
}

/// Sends [`a_request`] and returns the status it was refused with.
async fn refused(client: &mut FlightClient) -> Status {
    let outcome = client.do_exchange(a_request()).await;
    let Err(error) = outcome else {
        panic!("the call was expected to be refused");
    };
    let FlightError::Tonic(status) = error else {
        panic!("expected a tonic status, got {error:?}");
    };
    *status
}

#[tokio::test]
async fn a_certified_leg_returns_its_batches() {
    let surface = FakeSurface::answering(|| Ok(an_answer()));
    let mut client = serve(service_over(&surface), admitted()).await;
    let batches: Vec<RecordBatch> = client
        .do_exchange(a_request())
        .await
        .expect("a certified leg answers")
        .try_collect()
        .await
        .expect("a well-formed reply");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), 1);
}

#[tokio::test]
async fn a_refusal_crosses_as_the_closed_code_only() {
    let surface = FakeSurface::answering(|| Ok(a_refusal()));
    let mut client = serve(service_over(&surface), admitted()).await;
    let status = refused(&mut client).await;
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert_eq!(status.message(), "remote_leg_not_certified");
}

#[tokio::test]
async fn no_asked_is_unauthenticated_and_never_runs_the_leg() {
    let surface = FakeSurface::answering(|| Ok(an_answer()));
    let mut client = serve(service_over(&surface), Admit(None)).await;
    assert_eq!(refused(&mut client).await.code(), Code::Unauthenticated);
    assert_eq!(surface.calls(), 0);
}

#[tokio::test]
async fn no_capability_is_permission_denied_and_never_runs_the_leg() {
    let surface = FakeSurface::answering(|| Ok(an_answer()));
    let without = Permitted::every_capability().without(Capability::RunRemoteLeg);
    let mut client = serve(service_over(&surface), Admit(Some(asked(without)))).await;
    assert_eq!(refused(&mut client).await.code(), Code::PermissionDenied);
    assert_eq!(surface.calls(), 0);
}

/// A refused caller's request is not read at all: neither its `grpc-timeout` header nor its stream.
/// A malformed header would be `InvalidArgument` if it were parsed first, and the stream records
/// whether anything polled it.
#[tokio::test]
async fn a_refused_caller_never_has_its_request_read() {
    let surface = FakeSurface::answering(|| Ok(an_answer()));
    let service = service_over(&surface);
    let malformed = MetadataValue::from_static("not-a-timeout");
    let without = asked(Permitted::every_capability().without(Capability::RunRemoteLeg));
    for (caller, code) in [(None, Code::Unauthenticated), (Some(without), Code::PermissionDenied)] {
        let polled = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&polled);
        let peer = futures_util::stream::poll_fn(move |_context| {
            seen.store(true, Ordering::SeqCst);
            Poll::Ready(None::<Result<FlightData, FlightError>>)
        });
        let Err(status) = service.exchange(caller, Some(&malformed), peer).await else {
            panic!("a refused caller was answered");
        };
        assert_eq!(status.code(), code);
        assert!(
            !polled.load(Ordering::SeqCst),
            "the request stream of a {code:?} caller was polled"
        );
    }
    assert_eq!(surface.calls(), 0);
}

/// A parameter batch with zero rows: `params::decode`'s own `RowCount` refusal, reached only once
/// framing itself succeeded.
#[tokio::test]
async fn a_parameter_decode_refusal_is_invalid_argument() {
    let surface = FakeSurface::answering(|| Ok(an_answer()));
    let mut client = serve(service_over(&surface), admitted()).await;
    let zero_rows = RecordBatch::try_new_with_options(
        Arc::new(Schema::empty()),
        Vec::new(),
        &arrow_array::RecordBatchOptions::new().with_row_count(Some(0)),
    )
    .expect("zero columns, zero rows is a valid batch shape");
    let request =
        crate::request_stream(&a_lookup_leg(), futures_util::stream::iter(vec![Ok(zero_rows)])).expect("an encodable leg");
    let Err(FlightError::Tonic(status)) = client.do_exchange(request).await else {
        panic!("a malformed parameter batch was not refused with a status");
    };
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(status.message(), "the remote leg parameter batch must carry exactly one row");
    assert_eq!(surface.calls(), 0);
}

/// A descriptor over the descriptor limit, under a frame budget it fits: framing's own refusal,
/// and the one that tells the two limits apart - with them swapped this is the frame budget's.
#[tokio::test]
async fn a_descriptor_over_its_limit_is_invalid_argument() {
    let surface = FakeSurface::answering(|| Ok(an_answer()));
    let service = limited(&surface, budget(Duration::from_secs(30)), 16, FRAME_BUDGET);
    let mut client = serve(service, admitted()).await;
    let status = refused(&mut client).await;
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(status.message(), "the remote leg descriptor exceeds its byte limit");
    assert_eq!(surface.calls(), 0);
}

#[tokio::test]
async fn a_header_timeout_shorter_than_the_budget_is_the_deadline() {
    let surface = FakeSurface::answering(|| Ok(an_answer()));
    let mut client = serve(service_over(&surface), admitted()).await;
    client.add_header("grpc-timeout", "1S").expect("a well-formed header");
    let _reply: Vec<RecordBatch> = client
        .do_exchange(a_request())
        .await
        .expect("a certified leg still answers")
        .try_collect()
        .await
        .expect("a well-formed reply");
    let deadline = surface.last_deadline().expect("run_remote_leg was called");
    assert_eq!(deadline.budget(), budget(Duration::from_secs(1)));
    let remaining = deadline.remaining_at(Instant::now()).expect("not yet past");
    assert!(remaining <= Duration::from_secs(1));
}

#[test]
fn a_header_timeout_longer_than_the_budget_leaves_the_budget() {
    let own = budget(Duration::from_secs(30));
    assert_eq!(call_budget(own, Some(Duration::from_secs(99_999_999))), own);
}

#[test]
fn no_header_timeout_leaves_the_budget() {
    let own = budget(Duration::from_secs(30));
    assert_eq!(call_budget(own, None), own);
}

#[test]
fn a_zero_header_timeout_is_one_nanosecond() {
    let own = budget(Duration::from_secs(30));
    assert_eq!(call_budget(own, Some(Duration::ZERO)), budget(Duration::from_nanos(1)));
}

#[tokio::test]
async fn a_malformed_header_timeout_is_invalid_argument() {
    let surface = FakeSurface::answering(|| Ok(an_answer()));
    let mut client = serve(service_over(&surface), admitted()).await;
    client
        .add_header("grpc-timeout", "not-a-timeout")
        .expect("tonic accepts any ASCII header value");
    let status = refused(&mut client).await;
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(status.message(), "the grpc-timeout header is malformed");
}

/// Each variant's code and fixed message. The cause is never in the message.
#[test]
fn every_surface_failure_crosses_as_its_own_status() {
    let cause = || ErasedCause::from("a cause that names a table");
    let cases = [
        (
            SurfaceFailure::Compile { cause: cause() },
            Code::Internal,
            "this node could not run the remote leg",
        ),
        (
            SurfaceFailure::Warehouse { cause: cause() },
            Code::Unavailable,
            "the data system did not answer",
        ),
        (
            SurfaceFailure::Broker { cause: cause() },
            Code::Unavailable,
            "the credential broker did not answer",
        ),
        (
            SurfaceFailure::NotLinked { cause: cause() },
            Code::FailedPrecondition,
            "identity_not_linked",
        ),
        (
            SurfaceFailure::Miswired { cause: cause() },
            Code::Internal,
            "this node could not run the remote leg",
        ),
    ];
    for (failure, code, message) in cases {
        let status = status::failed(&failure);
        assert_eq!((status.code(), status.message()), (code, message), "{failure:?}");
    }
}

#[tokio::test]
async fn a_surface_failure_crosses_as_its_variants_status() {
    let surface = FakeSurface::answering(a_warehouse_outage);
    let mut client = serve(service_over(&surface), admitted()).await;
    let status = refused(&mut client).await;
    assert_eq!(status.code(), Code::Unavailable);
    assert_eq!(status.message(), "the data system did not answer");
}

#[tokio::test]
async fn a_leg_that_did_not_finish_is_internal() {
    let surface = FakeSurface::answering(|| panic!("the fake leg does not finish"));
    let mut client = serve(service_over(&surface), admitted()).await;
    let status = refused(&mut client).await;
    assert_eq!(status.code(), Code::Internal);
    assert_eq!(status.message(), "the remote leg did not finish");
}

/// Every method but `do_exchange` is refused as `Unimplemented`, never as a failure.
#[tokio::test]
async fn every_other_flight_method_is_unimplemented() {
    let surface = FakeSurface::answering(|| Ok(an_answer()));
    let client = serve(service_over(&surface), admitted()).await;
    let mut inner = client.into_inner();
    let descriptor = || FlightDescriptor::new_cmd("unused");
    let codes = [
        inner
            .handshake(futures_util::stream::empty())
            .await
            .err()
            .map(|status| status.code()),
        inner
            .list_flights(arrow_flight::Criteria::default())
            .await
            .err()
            .map(|status| status.code()),
        inner.get_flight_info(descriptor()).await.err().map(|status| status.code()),
        inner.poll_flight_info(descriptor()).await.err().map(|status| status.code()),
        inner.get_schema(descriptor()).await.err().map(|status| status.code()),
        inner
            .do_get(arrow_flight::Ticket::default())
            .await
            .err()
            .map(|status| status.code()),
        inner
            .do_put(futures_util::stream::empty())
            .await
            .err()
            .map(|status| status.code()),
        inner
            .do_action(arrow_flight::Action::default())
            .await
            .err()
            .map(|status| status.code()),
        inner
            .list_actions(arrow_flight::Empty::default())
            .await
            .err()
            .map(|status| status.code()),
    ];
    assert_eq!(codes, [Some(Code::Unimplemented); 9]);
    assert_eq!(surface.calls(), 0);
}

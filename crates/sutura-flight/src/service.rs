//! Node B's `DoExchange` server: the ADR's one-call-per-leg, received and executed.
//!
//! [`RemoteLegService`] is the only `FlightService` method this crate implements with real work -
//! every other RPC this trait defines returns [`Status::unimplemented`], because this crate's whole
//! reason to exist is the one typed call the remote-leg ADR chooses, not a general Flight server.
//!
//! **Per-call order**, and why it is this order:
//!
//! 1. An [`Asked`] must already be in the request's extensions, and its context must carry the
//!    caller's verified assertion, or the call is [`Status::unauthenticated`] - never a fallback to
//!    this process's own identity. Whatever mounts this service establishes that value; this
//!    service refuses outright rather than guess, and a remote leg runs only for a verified caller.
//! 2. The established caller must hold [`Capability::RunRemoteLeg`], or the call is
//!    [`Status::permission_denied`]. Both checks run before the `grpc-timeout` header is parsed or
//!    the request stream is polled, so an unauthorized caller's bytes are never framed, decoded or
//!    executed.
//! 3. The deadline opens at the EARLIER of this service's own [`Budget`] and the caller's
//!    `grpc-timeout` header - never later than either, so a caller cannot extend this node's own
//!    bound by asking for more.
//! 4. The request is received inside what that deadline has left: a stream that has not ended when
//!    it runs out is [`Status::deadline_exceeded`]. Framing ([`crate::receive_request`]) and
//!    parameter decode ([`crate::params::decode`]) each close over their own error: the `Display`
//!    of a [`crate::ReceiveError`] or [`crate::ParamsError`] names no peer-supplied value, so
//!    surfacing it verbatim in [`Status::invalid_argument`] discloses nothing a closed refusal
//!    would not.
//! 5. The leg takes a slot from the deployment's [`Admission`] - the same bound both other
//!    transports take - or is shed as [`Status::resource_exhausted`]. The slot moves into the
//!    blocking task, so it is held until the leg returns, not until the handler does.
//! 6. [`Surface::run_remote_leg`] runs on the blocking pool via
//!    `sutura_runtime::spawn_carrying_span`, which is what keeps the request's tracing span
//!    attached to a synchronous call this async handler must not block on directly.
//! 7. An [`RemoteLegOutcome::Answer`] is encoded as the reply stream. A
//!    [`RemoteLegOutcome::Refusal`] crosses as ONE closed [`Status::failed_precondition`] whose
//!    message is exactly `reason.code()` -
//!    [`RefusalReason::code`](sutura_domain::query::RefusalReason::code) is already the exhaustive,
//!    no-wildcard, no-field-interpolated rendering this boundary needs, so this service adds no
//!    second one. A [`SurfaceFailure`](sutura_app::surface::SurfaceFailure) - this node's own
//!    defect or outage, never the caller's fault - crosses as the status its variant chooses, with a
//!    fixed message and never the error's own `Display` or source chain (`status::failed`).
//!
//! **Why `FailedPrecondition` for a refusal.** gRPC's own status-code guidance reserves
//! `InvalidArgument` for a malformed REQUEST - true of a framing or parameter-decode failure, which
//! this service already uses it for - and reserves `FailedPrecondition` for a well-formed request
//! the system's current STATE refuses, which is exactly what `RefusalReason::RemoteLegNotCertified`
//! is: the leg parsed, but this node's own pinned definitions do not certify it (wrong posture, no
//! `executes_legs`, or a digest mismatch). `PermissionDenied` is reserved above for the capability
//! gate and would conflate two different refusals under one code if reused here.
//!
//! **The caller's token never appears** in any descriptor, result, log line or error on this path -
//! this module reads [`Asked::context`] only to check that it carries an assertion and to hand it
//! unmodified to [`Surface::run_remote_leg`], and emits no log line naming it.

mod limits;
mod status;

use std::sync::Arc;
use std::time::{Duration, Instant};

use arrow_flight::encode::FlightDataEncoderBuilder;
use arrow_flight::error::FlightError;
use arrow_flight::flight_service_server::FlightService;
use arrow_flight::{
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor, FlightInfo, HandshakeRequest, HandshakeResponse, PollInfo,
    PutResult, SchemaResult, Ticket,
};
use futures_util::stream::BoxStream;
use futures_util::{Stream, StreamExt as _};
use sutura_app::surface::Surface;
use sutura_app::{Asked, Capability};
use sutura_domain::plan::remote::RemoteLegOutcome;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_runtime::Admission;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::{Request, Response, Status, Streaming};

pub use limits::{DescriptorLimit, FrameBudget, NoLimit};

use crate::{grpc_timeout, params, receive_request};

/// The metadata key the gRPC-over-HTTP2 spec defines for a per-call timeout.
const GRPC_TIMEOUT_HEADER: &str = "grpc-timeout";

/// What one `DoExchange` call streams back.
type Reply = BoxStream<'static, Result<FlightData, Status>>;

/// Node B's `DoExchange` server over one [`Surface`]. See the module docs for the per-call order.
pub struct RemoteLegService<S: ?Sized> {
    surface: Arc<S>,
    admission: Admission,
    budget: Budget,
    descriptor_limit: DescriptorLimit,
    frame_budget: FrameBudget,
}

impl<S: ?Sized> RemoteLegService<S> {
    /// Build the service over `surface`, bound by the deployment's `admission`, by `budget` and by
    /// the two framing limits [`crate::receive_request`] takes.
    ///
    /// `admission` is the composition root's one bound, handed down: an [`Admission`] has no
    /// constructor but `Admission::from_settings`, so a service cannot be built without one.
    pub const fn new(
        surface: Arc<S>,
        admission: Admission,
        budget: Budget,
        descriptor_limit: DescriptorLimit,
        frame_budget: FrameBudget,
    ) -> Self {
        Self {
            surface,
            admission,
            budget,
            descriptor_limit,
            frame_budget,
        }
    }
}

/// The budget for one call: the earlier of this service's own budget and `header`, when a caller
/// sent a `grpc-timeout`. A caller can only ever shorten this node's own bound, never extend it - a
/// header longer than `own` leaves `own` governing.
///
/// A header that parses to zero is clamped to one nanosecond rather than left at zero:
/// [`Budget::parse`] refuses zero (it would read as "no timeout" to a client underneath), and a
/// caller asking for essentially no time at all should see a deadline that expires on its next
/// check, not a framing refusal for a header that parsed just fine.
fn call_budget(own: Budget, header: Option<Duration>) -> Budget {
    header.map_or(own, |header| {
        let shorter = own.duration().min(header.max(Duration::from_nanos(1)));
        Budget::parse(shorter).unwrap_or(own)
    })
}

/// The request did not arrive inside the call's deadline.
fn past_deadline() -> Status {
    Status::deadline_exceeded("the remote leg request did not arrive inside its deadline")
}

/// Parse the `grpc-timeout` header, if the caller sent one.
///
/// A header present but not valid ASCII, or not a valid gRPC timeout value, is
/// [`Status::invalid_argument`] - the behavioural difference from tonic's own internal parse stated
/// in `crate::grpc_timeout`'s module docs: a malformed header here is a hard refusal, never silently
/// treated as absent.
fn header_timeout(value: Option<&MetadataValue<Ascii>>) -> Result<Option<Duration>, Status> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value
        .to_str()
        .map_err(|_cause| Status::invalid_argument("the grpc-timeout header is not valid ASCII"))?;
    grpc_timeout::parse(text)
        .map(Some)
        .map_err(|_cause| Status::invalid_argument("the grpc-timeout header is malformed"))
}

impl<S: Surface + ?Sized> RemoteLegService<S> {
    /// One `DoExchange` call, in the module docs' order, over the parts `do_exchange` takes out of
    /// the request.
    async fn exchange<R>(&self, asked: Option<Asked>, timeout: Option<&MetadataValue<Ascii>>, peer: R) -> Result<Reply, Status>
    where
        R: Stream<Item = Result<FlightData, FlightError>> + Send + 'static,
    {
        let asked = asked.ok_or_else(|| Status::unauthenticated("no established caller for this remote leg"))?;
        if asked.context().assertion().is_none() {
            return Err(Status::unauthenticated("this remote leg needs a verified caller"));
        }
        if !asked.permitted().includes(Capability::RunRemoteLeg) {
            return Err(Status::permission_denied("this caller may not run a remote leg"));
        }
        let deadline = Deadline::opened_at(Instant::now(), call_budget(self.budget, header_timeout(timeout)?));

        let left = deadline.remaining_at(Instant::now()).ok_or_else(past_deadline)?;
        let received = receive_request(peer, self.descriptor_limit.bytes(), self.frame_budget.bytes());
        let (leg, batch) = tokio::time::timeout(left, received)
            .await
            .map_err(|_elapsed| past_deadline())?
            .map_err(|cause| Status::invalid_argument(cause.to_string()))?;
        let params = params::decode(&batch).map_err(|cause| Status::invalid_argument(cause.to_string()))?;

        let slot = self.admission.admit().await.map_err(|shed| {
            tracing::warn!(
                max_concurrent_queries = shed.bound(),
                admission_timeout_seconds = shed.waited().as_secs(),
                "a remote leg was shed: every execution slot was taken"
            );
            Status::resource_exhausted("every execution slot is taken; retry")
        })?;
        let surface = Arc::clone(&self.surface);
        let outcome = match sutura_runtime::spawn_carrying_span(move || {
            let _slot = slot;
            surface.run_remote_leg(asked.context(), leg, params, deadline)
        })
        .await
        {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(failure)) => return Err(status::failed(&failure)),
            Err(error) => {
                tracing::error!(error = %error, "the blocking task running a remote leg did not finish");
                return Err(Status::internal("the remote leg did not finish"));
            }
        };

        match outcome {
            RemoteLegOutcome::Answer { batches } => {
                let schema = Arc::clone(batches.schema());
                let records = batches.into_batches();
                let encoded = FlightDataEncoderBuilder::new()
                    .with_schema(schema)
                    .build(futures_util::stream::iter(records.into_iter().map(Ok::<_, FlightError>)))
                    .map(|item| item.map_err(Status::from));
                Ok(Box::pin(encoded))
            }
            RemoteLegOutcome::Refusal { reason } => Err(Status::failed_precondition(reason.code())),
        }
    }
}

#[tonic::async_trait]
impl<S: Surface + ?Sized> FlightService for RemoteLegService<S> {
    type HandshakeStream = BoxStream<'static, Result<HandshakeResponse, Status>>;
    type ListFlightsStream = BoxStream<'static, Result<FlightInfo, Status>>;
    type DoGetStream = BoxStream<'static, Result<FlightData, Status>>;
    type DoPutStream = BoxStream<'static, Result<PutResult, Status>>;
    type DoActionStream = BoxStream<'static, Result<arrow_flight::Result, Status>>;
    type ListActionsStream = BoxStream<'static, Result<ActionType, Status>>;
    type DoExchangeStream = Reply;

    async fn handshake(&self, _request: Request<Streaming<HandshakeRequest>>) -> Result<Response<Self::HandshakeStream>, Status> {
        Err(Status::unimplemented("this service serves only do_exchange"))
    }

    async fn list_flights(&self, _request: Request<Criteria>) -> Result<Response<Self::ListFlightsStream>, Status> {
        Err(Status::unimplemented("this service serves only do_exchange"))
    }

    async fn get_flight_info(&self, _request: Request<FlightDescriptor>) -> Result<Response<FlightInfo>, Status> {
        Err(Status::unimplemented("this service serves only do_exchange"))
    }

    async fn poll_flight_info(&self, _request: Request<FlightDescriptor>) -> Result<Response<PollInfo>, Status> {
        Err(Status::unimplemented("this service serves only do_exchange"))
    }

    async fn get_schema(&self, _request: Request<FlightDescriptor>) -> Result<Response<SchemaResult>, Status> {
        Err(Status::unimplemented("this service serves only do_exchange"))
    }

    async fn do_get(&self, _request: Request<Ticket>) -> Result<Response<Self::DoGetStream>, Status> {
        Err(Status::unimplemented("this service serves only do_exchange"))
    }

    async fn do_put(&self, _request: Request<Streaming<FlightData>>) -> Result<Response<Self::DoPutStream>, Status> {
        Err(Status::unimplemented("this service serves only do_exchange"))
    }

    async fn do_action(&self, _request: Request<Action>) -> Result<Response<Self::DoActionStream>, Status> {
        Err(Status::unimplemented("this service serves only do_exchange"))
    }

    async fn list_actions(&self, _request: Request<Empty>) -> Result<Response<Self::ListActionsStream>, Status> {
        Err(Status::unimplemented("this service serves only do_exchange"))
    }

    async fn do_exchange(&self, mut request: Request<Streaming<FlightData>>) -> Result<Response<Self::DoExchangeStream>, Status> {
        let asked = request.extensions_mut().remove::<Asked>();
        let timeout = request.metadata().get(GRPC_TIMEOUT_HEADER).cloned();
        let peer = request
            .into_inner()
            .map(|item| item.map_err(|status| FlightError::Tonic(Box::new(status))));
        self.exchange(asked, timeout.as_ref(), peer).await.map(Response::new)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::task::Poll;
    use std::time::{Duration, Instant};

    use arrow_array::{RecordBatch, StringArray};
    use arrow_flight::error::FlightError;
    use arrow_flight::flight_service_server::FlightServiceServer;
    use arrow_flight::{FlightClient, FlightData, FlightDescriptor};
    use arrow_schema::{DataType, Field, Schema};
    use futures_util::{StreamExt as _, TryStreamExt as _};
    use parking_lot::Mutex;
    use sutura_app::surface::{ErasedCause, SurfaceFailure};
    use sutura_app::{Asked, Capability, Permitted};
    use sutura_domain::definitions::DefinitionDigest;
    use sutura_domain::identity::{PrincipalChain, RequestContext, Secret, Subject};
    use sutura_domain::model::{MetricName, SourceName};
    use sutura_domain::plan::remote::{LegWire, RemoteLegOutcome};
    use sutura_domain::query::RefusalReason;
    use sutura_domain::warehouse::arrow::{Accumulating, ResultBudget};
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_runtime::Admission;
    use tonic::metadata::MetadataValue;
    use tonic::service::Interceptor;
    use tonic::service::interceptor::InterceptedService;
    use tonic::transport::Server;
    use tonic::{Code, Status};

    use super::{DescriptorLimit, FrameBudget, RemoteLegService, Surface, call_budget, header_timeout, status};

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

    /// The admission bound a settings document describes, read the way a deployment writes it.
    fn admission(overlay: &str) -> Admission {
        let sources = sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay);
        Admission::from_settings(
            sutura_config::Settings::load(&sources)
                .expect("the test settings load")
                .runtime(),
        )
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
            admission(""),
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

    /// A caller whose assertion the mounting transport verified.
    fn asked(permitted: Permitted) -> Asked {
        let subject = Subject::verified("someone@example.com").expect("a test subject is a subject");
        let context = RequestContext::with_assertion(
            PrincipalChain::of(subject),
            Secret::new("a-verified-assertion"),
            4_102_444_800,
        );
        Asked::established(context, permitted)
    }

    /// The deployment's own identity: established, but with no caller assertion behind it.
    fn the_deployment(permitted: Permitted) -> Asked {
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
        let unverified = the_deployment(Permitted::every_capability());
        for (caller, code) in [
            (None, Code::Unauthenticated),
            (Some(unverified), Code::Unauthenticated),
            (Some(without), Code::PermissionDenied),
        ] {
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

    #[tokio::test]
    async fn a_caller_with_no_verified_assertion_is_unauthenticated_and_never_runs_the_leg() {
        let surface = FakeSurface::answering(|| Ok(an_answer()));
        let mut client = serve(
            service_over(&surface),
            Admit(Some(the_deployment(Permitted::every_capability()))),
        )
        .await;
        let status = refused(&mut client).await;
        assert_eq!(status.code(), Code::Unauthenticated);
        assert_eq!(status.message(), "this remote leg needs a verified caller");
        assert_eq!(surface.calls(), 0);
    }

    /// With every slot taken the leg is shed before it runs; once the slot comes back the next one
    /// runs, and hands its own slot back when it returns.
    #[tokio::test]
    async fn a_leg_with_no_free_slot_is_shed_and_never_runs() {
        let surface = FakeSurface::answering(|| Ok(an_answer()));
        let bound = admission("runtime:\n  max_concurrent_queries: 1\n  admission_timeout_seconds: 1\n");
        let service = RemoteLegService::new(
            Arc::clone(&surface),
            bound.clone(),
            budget(Duration::from_secs(30)),
            DescriptorLimit::parse(DESCRIPTOR_LIMIT).expect("nonzero"),
            FrameBudget::parse(FRAME_BUDGET).expect("nonzero"),
        );
        let mut client = serve(service, admitted()).await;
        let held = bound.admit().await.expect("the only slot is free");
        let status = refused(&mut client).await;
        assert_eq!(status.code(), Code::ResourceExhausted);
        assert_eq!(status.message(), "every execution slot is taken; retry");
        assert_eq!(surface.calls(), 0);

        drop(held);
        let _reply: Vec<RecordBatch> = client
            .do_exchange(a_request())
            .await
            .expect("a free slot admits the leg")
            .try_collect()
            .await
            .expect("a well-formed reply");
        assert_eq!(surface.calls(), 1);
        assert_eq!(bound.free(), 1);
    }

    /// A request stream that sends its leg and parameters and then never ends is cut off when the
    /// call's deadline runs out, and the leg never runs.
    #[tokio::test]
    async fn a_request_stream_that_never_ends_is_deadline_exceeded() {
        let surface = FakeSurface::answering(|| Ok(an_answer()));
        let service = limited(&surface, budget(Duration::from_millis(200)), DESCRIPTOR_LIMIT, FRAME_BUDGET);
        let mut client = serve(service, admitted()).await;
        let parameters = futures_util::stream::iter(vec![Ok(one_row_batch())]).chain(futures_util::stream::pending());
        let request = crate::request_stream(&a_lookup_leg(), parameters).expect("an encodable leg");
        let Err(FlightError::Tonic(status)) = client.do_exchange(request).await else {
            panic!("a request that never ends was not refused with a status");
        };
        assert_eq!(status.code(), Code::DeadlineExceeded);
        assert_eq!(status.message(), "the remote leg request did not arrive inside its deadline");
        assert_eq!(surface.calls(), 0);
    }

    /// A parameter batch with two rows: `params::decode`'s own `RowCount` refusal, reached only once
    /// framing itself succeeded. A zero-row batch would not do: the encoder sends no batch for it, so
    /// framing refuses first.
    #[tokio::test]
    async fn a_parameter_decode_refusal_is_invalid_argument() {
        let surface = FakeSurface::answering(|| Ok(an_answer()));
        let mut client = serve(service_over(&surface), admitted()).await;
        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Utf8, false)]));
        let two_rows = RecordBatch::try_new(schema, vec![Arc::new(StringArray::from(vec!["berlin", "bonn"]))]).expect("two rows");
        let request =
            crate::request_stream(&a_lookup_leg(), futures_util::stream::iter(vec![Ok(two_rows)])).expect("an encodable leg");
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

    /// A header that is not visible ASCII is refused, never read as absent.
    #[test]
    fn a_non_ascii_header_timeout_is_invalid_argument() {
        let value = MetadataValue::try_from(&[b'5', 0x80, b'S'][..]).expect("an obs-text header value");
        let Err(status) = header_timeout(Some(&value)) else {
            panic!("a non-ASCII grpc-timeout was accepted");
        };
        assert_eq!(status.code(), Code::InvalidArgument);
        assert_eq!(status.message(), "the grpc-timeout header is not valid ASCII");
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
}

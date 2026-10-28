//! Node B's `DoExchange` server: the ADR's one-call-per-leg, received and executed.
//!
//! [`RemoteLegService`] is the only `FlightService` method this crate implements with real work -
//! every other RPC this trait defines returns [`Status::unimplemented`], because this crate's whole
//! reason to exist is the one typed call the remote-leg ADR chooses, not a general Flight server.
//!
//! **Per-call order**, and why it is this order:
//!
//! 1. An [`Asked`] must already be in the request's extensions, or the call is
//!    [`Status::unauthenticated`] - never a fallback to this process's own identity. Whatever
//!    mounts this service establishes that value; this service refuses outright rather than guess.
//! 2. The established caller must hold [`Capability::RunRemoteLeg`], or the call is
//!    [`Status::permission_denied`] - checked before anything peer-supplied is read, so an
//!    unauthorized caller's bytes are never framed, decoded or executed.
//! 3. Framing ([`crate::receive_request`]) and parameter decode ([`crate::params::decode`]) each
//!    close over their own error: the `Display` of a [`crate::ReceiveError`] or
//!    [`crate::ParamsError`] names no peer-supplied value, so surfacing it verbatim in
//!    [`Status::invalid_argument`] discloses nothing a closed refusal would not.
//! 4. The deadline opens at the EARLIER of this service's own [`Budget`] and the caller's
//!    `grpc-timeout` header - never later than either, so a caller cannot extend this node's own
//!    bound by asking for more.
//! 5. [`Surface::run_remote_leg`] runs on the blocking pool via
//!    `sutura_runtime::spawn_carrying_span`, which is what keeps the request's tracing span
//!    attached to a synchronous call this async handler must not block on directly.
//! 6. An [`RemoteLegOutcome::Answer`] is encoded as the reply stream. A
//!    [`RemoteLegOutcome::Refusal`] crosses as ONE closed [`Status::failed_precondition`] whose
//!    message is exactly `reason.code()` -
//!    [`RefusalReason::code`](sutura_domain::query::RefusalReason::code) is already the exhaustive,
//!    no-wildcard, no-field-interpolated rendering this boundary needs, so this service adds no
//!    second one. A [`SurfaceFailure`](sutura_app::surface::SurfaceFailure) - this node's own
//!    compile, warehouse, broker or wiring defect, never the caller's fault - crosses as
//!    [`Status::internal`] with a fixed sentence, never this error's own `Display` or source chain.
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
//! this module reads [`Asked::context`] only to hand it unmodified to [`Surface::run_remote_leg`],
//! and emits no log line naming it.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arrow_flight::encode::FlightDataEncoderBuilder;
use arrow_flight::error::FlightError;
use arrow_flight::flight_service_server::FlightService;
use arrow_flight::{
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor, FlightInfo, HandshakeRequest, HandshakeResponse, PollInfo,
    PutResult, SchemaResult, Ticket,
};
use futures_util::StreamExt as _;
use futures_util::stream::BoxStream;
use sutura_app::surface::{Surface, cause_chain};
use sutura_app::{Asked, Capability};
use sutura_domain::plan::remote::RemoteLegOutcome;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use tonic::{Request, Response, Status, Streaming};

use crate::{grpc_timeout, params, receive_request};

/// The metadata key the gRPC-over-HTTP2 spec defines for a per-call timeout.
const GRPC_TIMEOUT_HEADER: &str = "grpc-timeout";

/// Node B's `DoExchange` server over one [`Surface`]. See the module docs for the per-call order.
pub struct RemoteLegService<S: ?Sized> {
    surface: Arc<S>,
    budget: Budget,
    descriptor_limit: NonZeroUsize,
    frame_budget: NonZeroUsize,
}

impl<S: ?Sized> RemoteLegService<S> {
    /// Build the service over `surface`, bound by `budget` and the two framing limits
    /// [`crate::receive_request`] takes.
    pub const fn new(surface: Arc<S>, budget: Budget, descriptor_limit: NonZeroUsize, frame_budget: NonZeroUsize) -> Self {
        Self {
            surface,
            budget,
            descriptor_limit,
            frame_budget,
        }
    }

    /// The deadline for one call: the earlier of this service's own budget and `header`, when a
    /// caller sent a `grpc-timeout`. A caller can only ever shorten this node's own bound, never
    /// extend it - a header longer than `self.budget` leaves `self.budget` governing.
    ///
    /// A header that parses to zero is clamped to one nanosecond rather than left at zero:
    /// [`Budget::parse`] refuses zero (it would read as "no timeout" to a client underneath), and a
    /// caller asking for essentially no time at all should see a deadline that expires on its next
    /// check, not a framing refusal for a header that parsed just fine.
    fn deadline_for(&self, header: Option<Duration>) -> Deadline {
        let budget = header.map_or(self.budget, |header| {
            let shorter = self.budget.duration().min(header.max(Duration::from_nanos(1)));
            Budget::parse(shorter).unwrap_or(self.budget)
        });
        Deadline::opened_at(Instant::now(), budget)
    }
}

/// Read and parse the `grpc-timeout` header, if the caller sent one.
///
/// A header present but not valid ASCII, or not a valid gRPC timeout value, is
/// [`Status::invalid_argument`] - the behavioural difference from tonic's own internal parse stated
/// in `crate::grpc_timeout`'s module docs: a malformed header here is a hard refusal, never silently
/// treated as absent.
fn header_timeout<T>(request: &Request<T>) -> Result<Option<Duration>, Status> {
    let Some(value) = request.metadata().get(GRPC_TIMEOUT_HEADER) else {
        return Ok(None);
    };
    let text = value
        .to_str()
        .map_err(|_cause| Status::invalid_argument("the grpc-timeout header is not valid ASCII"))?;
    grpc_timeout::parse(text)
        .map(Some)
        .map_err(|_cause| Status::invalid_argument("the grpc-timeout header is malformed"))
}

#[tonic::async_trait]
impl<S: Surface + ?Sized> FlightService for RemoteLegService<S> {
    type HandshakeStream = BoxStream<'static, Result<HandshakeResponse, Status>>;
    type ListFlightsStream = BoxStream<'static, Result<FlightInfo, Status>>;
    type DoGetStream = BoxStream<'static, Result<FlightData, Status>>;
    type DoPutStream = BoxStream<'static, Result<PutResult, Status>>;
    type DoActionStream = BoxStream<'static, Result<arrow_flight::Result, Status>>;
    type ListActionsStream = BoxStream<'static, Result<ActionType, Status>>;
    type DoExchangeStream = BoxStream<'static, Result<FlightData, Status>>;

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
        let asked = request
            .extensions_mut()
            .remove::<Asked>()
            .ok_or_else(|| Status::unauthenticated("no established caller for this remote leg"))?;
        if !asked.permitted().includes(Capability::RunRemoteLeg) {
            return Err(Status::permission_denied("this caller may not run a remote leg"));
        }
        let header = header_timeout(&request)?;
        let deadline = self.deadline_for(header);

        let stream = request
            .into_inner()
            .map(|item| item.map_err(|status| FlightError::Tonic(Box::new(status))));
        let (leg, batch) = receive_request(stream, self.descriptor_limit, self.frame_budget)
            .await
            .map_err(|cause| Status::invalid_argument(cause.to_string()))?;
        let params = params::decode(&batch).map_err(|cause| Status::invalid_argument(cause.to_string()))?;

        let surface = Arc::clone(&self.surface);
        let outcome =
            match sutura_runtime::spawn_carrying_span(move || surface.run_remote_leg(asked.context(), leg, params, deadline))
                .await
            {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(failure)) => {
                    tracing::error!(error = %failure, causes = ?cause_chain(&failure), "this node could not run a remote leg");
                    return Err(Status::internal("this node could not run the remote leg"));
                }
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
                Ok(Response::new(Box::pin(encoded)))
            }
            RemoteLegOutcome::Refusal { reason } => Err(Status::failed_precondition(reason.code())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use arrow_array::{RecordBatch, StringArray};
    use arrow_flight::FlightClient;
    use arrow_flight::error::FlightError;
    use arrow_flight::flight_service_server::FlightServiceServer;
    use arrow_schema::{DataType, Field, Schema};
    use futures_util::TryStreamExt as _;
    use parking_lot::Mutex;
    use sutura_app::surface::SurfaceFailure;
    use sutura_domain::definitions::DefinitionDigest;
    use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};
    use sutura_domain::model::{MetricName, SourceName};
    use sutura_domain::plan::remote::LegWire;
    use sutura_domain::query::RefusalReason;
    use sutura_domain::warehouse::arrow::{Accumulating, ResultBudget};
    use tonic::service::Interceptor;
    use tonic::service::interceptor::InterceptedService;
    use tonic::transport::Server;

    use super::{Asked, Budget, Capability, Deadline, RemoteLegOutcome, RemoteLegService, Surface};

    const DESCRIPTOR_LIMIT: usize = 1 << 16;
    const FRAME_BUDGET: usize = 1 << 20;

    /// A leg whose content a fake [`Surface`] never reads - only that framing accepted it.
    ///
    /// Built by deserializing, the same path a real peer's bytes take: [`MetricDigest`] has no
    /// public Rust constructor on purpose (see its own docs), and a crafted one that satisfies the
    /// wire shape is exactly what this crate certifies nothing about before handing it to
    /// [`Surface::run_remote_leg`].
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
        let schema = std::sync::Arc::new(Schema::new(vec![Field::new("x", DataType::Utf8, false)]));
        RecordBatch::try_new(schema, vec![std::sync::Arc::new(StringArray::from(vec!["berlin"]))]).expect("one row")
    }

    fn an_answer() -> RemoteLegOutcome {
        let schema = std::sync::Arc::new(Schema::new(vec![Field::new("x", DataType::Utf8, false)]));
        let mut accumulating = Accumulating::announcing(
            std::sync::Arc::clone(&schema),
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

    fn context() -> RequestContext {
        RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))
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
        fn answering(outcome: FakeOutcome) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                last_deadline: Mutex::new(None),
                outcome,
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_deadline(&self) -> Option<Deadline> {
            *self.last_deadline.lock()
        }
    }

    impl Surface for FakeSurface {
        fn definitions(&self) -> std::sync::Arc<sutura_domain::pinned::PinnedDefinitions> {
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

    /// A test-only interceptor standing in for whatever later establishes an [`Asked`] - out of
    /// this crate's scope (module docs, point 1). `None` leaves a request with no established
    /// caller, exercising the `Unauthenticated` arm.
    #[derive(Clone)]
    struct Admit(Option<Asked>);

    impl Interceptor for Admit {
        fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
            if let Some(ref asked) = self.0 {
                drop(request.extensions_mut().insert(asked.clone()));
            }
            Ok(request)
        }
    }

    fn asked(permitted: sutura_app::Permitted) -> Asked {
        Asked::established(context(), permitted)
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

    #[tokio::test]
    async fn a_certified_leg_returns_its_batches() {
        let service = RemoteLegService::new(
            std::sync::Arc::new(FakeSurface::answering(|| Ok(an_answer()))),
            Budget::parse(Duration::from_secs(30)).expect("a nonzero budget"),
            std::num::NonZeroUsize::new(DESCRIPTOR_LIMIT).expect("nonzero"),
            std::num::NonZeroUsize::new(FRAME_BUDGET).expect("nonzero"),
        );
        let mut client = serve(service, Admit(Some(asked(sutura_app::Permitted::every_capability())))).await;
        let leg = a_lookup_leg();
        let request =
            crate::request_stream(&leg, futures_util::stream::iter(vec![Ok(one_row_batch())])).expect("an encodable leg");
        let batches: Vec<RecordBatch> = client
            .do_exchange(request)
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
        let service = RemoteLegService::new(
            std::sync::Arc::new(FakeSurface::answering(|| Ok(a_refusal()))),
            Budget::parse(Duration::from_secs(30)).expect("a nonzero budget"),
            std::num::NonZeroUsize::new(DESCRIPTOR_LIMIT).expect("nonzero"),
            std::num::NonZeroUsize::new(FRAME_BUDGET).expect("nonzero"),
        );
        let mut client = serve(service, Admit(Some(asked(sutura_app::Permitted::every_capability())))).await;
        let leg = a_lookup_leg();
        let request =
            crate::request_stream(&leg, futures_util::stream::iter(vec![Ok(one_row_batch())])).expect("an encodable leg");
        let outcome = client.do_exchange(request).await;
        let error = outcome.expect_err("a refusal does not answer");
        let status = match error {
            FlightError::Tonic(status) => *status,
            other => panic!("expected a tonic status, got {other:?}"),
        };
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(status.message(), "remote_leg_not_certified");
    }

    #[tokio::test]
    async fn no_asked_is_unauthenticated_and_never_runs_the_leg() {
        let surface = std::sync::Arc::new(FakeSurface::answering(|| Ok(an_answer())));
        let service = RemoteLegService::new(
            std::sync::Arc::clone(&surface),
            Budget::parse(Duration::from_secs(30)).expect("a nonzero budget"),
            std::num::NonZeroUsize::new(DESCRIPTOR_LIMIT).expect("nonzero"),
            std::num::NonZeroUsize::new(FRAME_BUDGET).expect("nonzero"),
        );
        let mut client = serve(service, Admit(None)).await;
        let leg = a_lookup_leg();
        let request =
            crate::request_stream(&leg, futures_util::stream::iter(vec![Ok(one_row_batch())])).expect("an encodable leg");
        let outcome = client.do_exchange(request).await;
        let error = outcome.expect_err("no established caller is refused");
        let FlightError::Tonic(status) = error else {
            panic!("expected a tonic status, got {error:?}");
        };
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
        assert_eq!(surface.calls(), 0);
    }

    #[tokio::test]
    async fn no_capability_is_permission_denied_and_never_runs_the_leg() {
        let surface = std::sync::Arc::new(FakeSurface::answering(|| Ok(an_answer())));
        let service = RemoteLegService::new(
            std::sync::Arc::clone(&surface),
            Budget::parse(Duration::from_secs(30)).expect("a nonzero budget"),
            std::num::NonZeroUsize::new(DESCRIPTOR_LIMIT).expect("nonzero"),
            std::num::NonZeroUsize::new(FRAME_BUDGET).expect("nonzero"),
        );
        let without = sutura_app::Permitted::every_capability().without(Capability::RunRemoteLeg);
        let mut client = serve(service, Admit(Some(asked(without)))).await;
        let leg = a_lookup_leg();
        let request =
            crate::request_stream(&leg, futures_util::stream::iter(vec![Ok(one_row_batch())])).expect("an encodable leg");
        let outcome = client.do_exchange(request).await;
        let error = outcome.expect_err("a caller without the capability is refused");
        let FlightError::Tonic(status) = error else {
            panic!("expected a tonic status, got {error:?}");
        };
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
        assert_eq!(surface.calls(), 0);
    }

    #[tokio::test]
    async fn a_framing_refusal_is_invalid_argument() {
        let service = RemoteLegService::new(
            std::sync::Arc::new(FakeSurface::answering(|| Ok(an_answer()))),
            Budget::parse(Duration::from_secs(30)).expect("a nonzero budget"),
            std::num::NonZeroUsize::new(DESCRIPTOR_LIMIT).expect("nonzero"),
            std::num::NonZeroUsize::new(FRAME_BUDGET).expect("nonzero"),
        );
        let mut client = serve(service, Admit(Some(asked(sutura_app::Permitted::every_capability())))).await;
        // Zero rows: `params::decode`'s own `RowCount` refusal, reached only once framing itself
        // succeeds - the cheapest way to drive a closed `InvalidArgument` without a malformed
        // descriptor.
        let zero_rows = RecordBatch::try_new_with_options(
            std::sync::Arc::new(Schema::empty()),
            Vec::new(),
            &arrow_array::RecordBatchOptions::new().with_row_count(Some(0)),
        )
        .expect("zero columns, zero rows is a valid batch shape");
        let leg = a_lookup_leg();
        let request = crate::request_stream(&leg, futures_util::stream::iter(vec![Ok(zero_rows)])).expect("an encodable leg");
        let outcome = client.do_exchange(request).await;
        let error = outcome.expect_err("a malformed parameter batch is refused");
        let FlightError::Tonic(status) = error else {
            panic!("expected a tonic status, got {error:?}");
        };
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn a_spent_header_timeout_shorter_than_the_budget_is_the_deadline() {
        let surface = std::sync::Arc::new(FakeSurface::answering(|| Ok(an_answer())));
        let service = RemoteLegService::new(
            std::sync::Arc::clone(&surface),
            Budget::parse(Duration::from_secs(30)).expect("a nonzero budget"),
            std::num::NonZeroUsize::new(DESCRIPTOR_LIMIT).expect("nonzero"),
            std::num::NonZeroUsize::new(FRAME_BUDGET).expect("nonzero"),
        );
        let mut client = serve(service, Admit(Some(asked(sutura_app::Permitted::every_capability())))).await;
        client.add_header("grpc-timeout", "1S").expect("a well-formed header");
        let leg = a_lookup_leg();
        let request =
            crate::request_stream(&leg, futures_util::stream::iter(vec![Ok(one_row_batch())])).expect("an encodable leg");
        let _reply: Vec<RecordBatch> = client
            .do_exchange(request)
            .await
            .expect("a certified leg still answers")
            .try_collect()
            .await
            .expect("a well-formed reply");
        let deadline = surface.last_deadline().expect("run_remote_leg was called");
        let remaining = deadline.remaining_at(std::time::Instant::now()).expect("not yet past");
        // The 1-second header beats the 30-second service budget, so what the fake captured must
        // reflect the SHORTER of the two, not the service's own budget.
        assert!(remaining <= Duration::from_secs(1));
    }

    #[tokio::test]
    async fn a_malformed_header_timeout_is_invalid_argument() {
        let service = RemoteLegService::new(
            std::sync::Arc::new(FakeSurface::answering(|| Ok(an_answer()))),
            Budget::parse(Duration::from_secs(30)).expect("a nonzero budget"),
            std::num::NonZeroUsize::new(DESCRIPTOR_LIMIT).expect("nonzero"),
            std::num::NonZeroUsize::new(FRAME_BUDGET).expect("nonzero"),
        );
        let mut client = serve(service, Admit(Some(asked(sutura_app::Permitted::every_capability())))).await;
        client
            .add_header("grpc-timeout", "not-a-timeout")
            .expect("tonic accepts any ASCII header value");
        let leg = a_lookup_leg();
        let request =
            crate::request_stream(&leg, futures_util::stream::iter(vec![Ok(one_row_batch())])).expect("an encodable leg");
        let outcome = client.do_exchange(request).await;
        let error = outcome.expect_err("a malformed grpc-timeout is refused");
        let FlightError::Tonic(status) = error else {
            panic!("expected a tonic status, got {error:?}");
        };
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }
}

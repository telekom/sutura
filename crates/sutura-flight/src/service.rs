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
//!    [`Status::permission_denied`]. Both checks run before the `grpc-timeout` header is parsed or
//!    the request stream is polled, so an unauthorized caller's bytes are never framed, decoded or
//!    executed.
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
//! this module reads [`Asked::context`] only to hand it unmodified to [`Surface::run_remote_leg`],
//! and emits no log line naming it.

mod limits;
mod status;
#[cfg(test)]
mod tests;

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
    budget: Budget,
    descriptor_limit: DescriptorLimit,
    frame_budget: FrameBudget,
}

impl<S: ?Sized> RemoteLegService<S> {
    /// Build the service over `surface`, bound by `budget` and the two framing limits
    /// [`crate::receive_request`] takes.
    pub const fn new(surface: Arc<S>, budget: Budget, descriptor_limit: DescriptorLimit, frame_budget: FrameBudget) -> Self {
        Self {
            surface,
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
        if !asked.permitted().includes(Capability::RunRemoteLeg) {
            return Err(Status::permission_denied("this caller may not run a remote leg"));
        }
        let deadline = Deadline::opened_at(Instant::now(), call_budget(self.budget, header_timeout(timeout)?));

        let (leg, batch) = receive_request(peer, self.descriptor_limit.bytes(), self.frame_budget.bytes())
            .await
            .map_err(|cause| Status::invalid_argument(cause.to_string()))?;
        let params = params::decode(&batch).map_err(|cause| Status::invalid_argument(cause.to_string()))?;

        let surface = Arc::clone(&self.surface);
        let outcome =
            match sutura_runtime::spawn_carrying_span(move || surface.run_remote_leg(asked.context(), leg, params, deadline))
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

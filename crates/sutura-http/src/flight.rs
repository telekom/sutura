//! The adapter that lets a tonic-generated Flight server run under `axum::Router::nest_service`.
//!
//! `nest_service` strips the matched prefix from the request's `Uri` before calling the nested
//! service, but tonic's generated `FlightServiceServer::call` dispatches by matching the FULL
//! literal service path (`crate::constants::FLIGHT_MOUNT_PATH` plus the method name) against
//! `req.uri().path()` - a stripped request matches none of tonic's routes and every call falls
//! through to tonic's own `UNIMPLEMENTED` response, never reaching [`sutura_flight::RemoteLegService`].
//! Axum inserts the pre-strip URI into the request's extensions as [`OriginalUri`] before
//! `nest_service`'s own prefix-stripping runs, so [`RestoreOriginalUri`] only has to put it back.

use std::task::{Context, Poll};

use arrow_flight::flight_service_server::FlightServiceServer;
use axum::body::Body;
use axum::extract::OriginalUri;
use axum::http::Request;
use sutura_flight::{DescriptorLimit, FrameBudget, RemoteLegService};

use crate::ServiceState;
use crate::state::FlightMount;

/// Restores the pre-strip request [`axum::http::Uri`] before delegating to the wrapped service.
///
/// Wraps a tonic-generated server such as `arrow_flight::flight_service_server::FlightServiceServer`
/// so it can be handed to [`crate::router::Ungoverned::mount`] the same way every other transport
/// is. Nothing else about the request changes - this adapter only undoes what `nest_service` did to
/// the path tonic itself needs to see.
#[derive(Clone)]
pub struct RestoreOriginalUri<S> {
    inner: S,
}

impl<S> RestoreOriginalUri<S> {
    /// Wraps `inner`.
    pub const fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S> tower::Service<Request<Body>> for RestoreOriginalUri<S>
where
    S: tower::Service<Request<Body>, Error = std::convert::Infallible>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        if let Some(OriginalUri(original)) = req.extensions().get::<OriginalUri>().cloned() {
            *req.uri_mut() = original;
        }
        self.inner.call(req)
    }
}

/// Builds the mounted Flight transport: [`RemoteLegService`] over `state`'s own surface, behind
/// tonic's generated server and [`RestoreOriginalUri`].
///
/// A leg shares everything a `POST /v1/query` has: the admission permits, the request budget, and
/// `server.max_body_bytes` as both the descriptor limit and the per-frame budget.
#[must_use]
pub fn mount(state: &ServiceState) -> FlightMount {
    let server = state.settings().server();
    let limit = server.max_body().limit();
    let service = RemoteLegService::new(
        state.surface(),
        state.admission().clone(),
        server.request_timeout().budget(),
        DescriptorLimit::new(limit),
        FrameBudget::new(limit),
    );
    FlightMount::new(RestoreOriginalUri::new(FlightServiceServer::new(service)))
}

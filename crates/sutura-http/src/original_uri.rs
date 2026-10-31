//! The adapter that lets a tonic-generated server run under `axum::Router::nest_service`.
//!
//! `nest_service` strips the matched prefix from the request's `Uri` before calling the nested
//! service, but a tonic-generated server dispatches by matching the FULL literal method path, its
//! service name included, against `req.uri().path()` - a stripped request matches none of its routes
//! and every call falls through to tonic's own `UNIMPLEMENTED` response. Axum inserts the pre-strip
//! URI into the request's extensions as [`OriginalUri`] before `nest_service`'s own prefix-stripping
//! runs, so [`RestoreOriginalUri`] only has to put it back. `FlightMount::new` and `WorkerMount::new`
//! wrap every service in it.

use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::OriginalUri;
use axum::http::Request;

/// Restores the pre-strip request [`axum::http::Uri`] before delegating to the wrapped service.
///
/// Nothing else about the request changes - this adapter only undoes what `nest_service` did to the
/// path tonic itself needs to see.
#[derive(Clone)]
pub(crate) struct RestoreOriginalUri<S> {
    inner: S,
}

impl<S> RestoreOriginalUri<S> {
    /// Wraps `inner`.
    pub(crate) const fn new(inner: S) -> Self {
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

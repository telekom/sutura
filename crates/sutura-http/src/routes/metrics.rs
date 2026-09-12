//! The authenticated metrics endpoint.
//!
//! # Its own credential, and a state that cannot answer a question
//!
//! `docs/adr/0015` Decision 1: `/metrics` is gated by `security.metrics_token`, never the
//! deployment's API token, so a scrape cannot interrogate the business. The endpoint lives on the
//! one listener (Decision 2), outside the version prefix so a scrape config survives a version
//! bump, and is rendered from a registry the composition root built.
//!
//! # A scrape must not make the service work
//!
//! **The state type deliberately does not contain the [`crate::surface::Surface`].** The handler
//! cannot answer a question because it does not have the means, and a future change that wanted to
//! would have to change the type. It holds only the already-retained [`Registry`], whose `render`
//! reads atomics and fixed strings - nothing touches the catalog, the plan path, the engine or a
//! data source, and nothing acquires a lock the request path holds. A scrape is O(series) and
//! constant-time per series.
//!
//! # Not behind the admission bound
//!
//! The endpoint answers while every execution slot is full - an operator watching the drain needs
//! it to keep answering. It is behind its own limiter tier, outside the documentation and versioned
//! gates, for the same reason the API limiter sits outside the token gate: a wrong-token scrape
//! attempt costs a cell.
use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse as _, Response};
use sutura_runtime::metrics::Registry;

/// The state a scrape is rendered from: the registry, and nothing that could execute.
///
/// **No `Surface` here is the whole of `docs/adr/0015` Decision 1's "a scrape must not make the
/// service work".** A handler built over this type cannot reach a data system because it holds no
/// means to.
#[derive(Debug, Clone)]
pub(super) struct MetricsState {
    registry: Arc<Registry>,
}

impl MetricsState {
    /// The registry a scrape renders.
    #[must_use]
    pub(super) const fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }
}

/// The tag this route is grouped under in the generated document. Metrics is deliberately not in
/// the versioned document - it is a process surface, not an API operation.
const TAG: &str = "metrics";

/// The authenticated metrics endpoint.
///
/// Renders the whole registry in Prometheus Text Exposition format. Behind the metrics token gate
/// (a sibling of the versioned API's token gate applied to this path alone), and deliberately not
/// behind the admission bound.
#[utoipa::path(
    get,
    path = "/metrics",
    tag = TAG,
    responses(
        (status = 200, description = "The deployment's counters, as Prometheus Text Exposition.", body = String),
        (status = 401, description = "No bearer token, or a token that is not the configured metrics token."),
    )
)]
pub(super) async fn scrape(State(state): State<MetricsState>) -> Response {
    let body = state.registry.render();
    // A tuple response rather than `Response::builder().expect(..)`: everything here is fixed
    // (the status, the content type, a body that cannot fail to construct), so there is nothing
    // fallible to unwrap and no possible builder error to ignore. `IntoResponse` for this tuple
    // is total.
    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response()
}

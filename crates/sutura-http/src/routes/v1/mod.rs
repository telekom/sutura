//! Version 1 of the API.
//!
//! # How a `v2` arrives
//!
//! As a sibling of this module, not as a rewrite of it. The shape below is the whole of the
//! contract: one function returning an [`OpenApiRouter`] whose handler paths are *relative*, and a
//! prefix applied once by the caller. So `v2` is a second module with a second
//! [`openapi_router`](self::openapi_router), a second constant beside
//! [`API_V1_PREFIX`](crate::constants::API_V1_PREFIX), and one more `nest` in
//! [`crate::router`]. Nothing here moves, and nothing here is called from there.
//!
//! **A `v2` does not delegate to a `v1`.** If a shape changes, `v2` gets its own handler and its own
//! wire type and `v1` is frozen as it stands until it is deleted whole. Delegating between versions
//! is how a change to `v2` silently changes `v1`, which is the one thing a version number is
//! supposed to prevent.
//!
//! # Why the paths are relative
//!
//! Because the prefix is applied to the router and to the generated document by the same call.
//! `utoipa_axum` derives the axum route and the documented path from the same `#[utoipa::path]`
//! attribute, so a handler that spelled its own full path could be mounted somewhere else and the
//! document would still claim the old one.

use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::state::ServiceState;

mod catalog;
mod query;

/// Every route in this version, with paths relative to the version prefix.
///
/// One builder, consumed twice by the caller: once with the state, to become an `axum::Router`, and
/// once without, to become the fragment of the generated document. That is what makes it impossible
/// for the two to describe different sets of routes.
pub(crate) fn openapi_router() -> OpenApiRouter<ServiceState> {
    OpenApiRouter::new()
        // `routes!` is what makes a handler with no `#[utoipa::path]` a compile error rather than a
        // gap in the document.
        .routes(routes!(catalog::catalog))
        .routes(routes!(query::ask))
}

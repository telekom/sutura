//! What this catalog defines.
//!
//! Descriptive content only, and that is a governance boundary rather than a scoping decision.
//! `SemanticCatalog::load` takes no request context and cannot be given one, so nothing a caller
//! sends can select, widen or parameterize what this returns - it is the *pinned* bundle, the same
//! one every answer is computed from, rendered for a reader.
//!
//! It is also the surface a caller needs in order to ask a valid question at all: which metrics
//! exist, at which grains, with which dimensions, and which values a filter may use. Without it
//! every question is a guess that comes back as a refusal.
//!
//! **It is behind the access token when one is configured.** A catalog listing is a description of
//! what this deployment measures, which is business information even when no row of data is in it.

use axum::Json;
use axum::extract::State;

use crate::state::ServiceState;
use crate::wire::CatalogBody;

/// The tag this route is grouped under in the generated document.
const TAG: &str = "catalog";

/// The metrics this catalog defines, with the version and digest that identify the snapshot.
#[utoipa::path(
    get,
    path = "/catalog",
    // The capability this operation IS, so the generated document names the same thing the agent
    // surface names its tool. It is `sutura_app::Capability::DescribeCatalog::id()`'s literal, written
    // out because a `#[utoipa::path]` attribute takes a literal - and
    // `crate::openapi::tests::both_transports_describe_the_same_tools` is what asserts the two agree.
    operation_id = "describe_catalog",
    tag = TAG,
    responses(
        (status = 200, description = "The pinned bundle, as a reader's view.", body = CatalogBody),
        (status = 401, description = "No valid bearer token was presented.", body = crate::problem::ProblemBody),
        (
            status = 403,
            description = "`code: insufficient_scope`. Your credential is valid and does not carry \
                           the scope this operation requires; the detail names it. Not a statement \
                           about the catalog - nothing in it is hidden from a caller who may read it \
                           at all.",
            body = crate::problem::ProblemBody
        ),
        (status = 429, description = "Too many requests from this address.", body = crate::problem::ProblemBody),
    )
)]
pub(crate) async fn catalog(State(state): State<ServiceState>) -> Json<CatalogBody> {
    // No blocking pool and no data system: this reads a bundle that was pinned at startup and has
    // not changed since. A catalog edit cannot reach this - it would be a different process.
    //
    // The prose setting is read here, from the settings the state already holds, and `crate::state`
    // is right that a value a handler can read is a value a handler can branch on - so the reason
    // this one is read per request is worth stating rather than leaving to the reader. It is fixed
    // at startup and the branch is on a constant; what makes reading it better than a constructor
    // argument on `ServiceState` is the direction the two fail in. A `catalog_prose` argument that a
    // composition root forgot would take `CatalogProse::default()`, which is `quoted`, and ship the
    // prose of a deployment that asked for none. The settings are the one place that cannot be out
    // of date about what the operator wrote.
    Json(CatalogBody::of(
        state.definitions(),
        state.settings().prompt().catalog_prose(),
    ))
}

//! The generated interface description.
//!
//! **Generated from the handlers, never written by hand.** `utoipa_axum::routes!` registers a
//! handler in the router and in the document from the same `#[utoipa::path]` attribute, so a
//! handler with no attribute is a compile error and a handler mounted somewhere else moves in both
//! places at once. A hand-written document is a second source of truth that goes stale on the first
//! change nobody remembered to mirror.
//!
//! The derive below carries only the components and the tags. Every *path* comes from the router
//! fragments, merged here.
//!
//! # Determinism, and the hazard to know about
//!
//! An interface description is something a client generator runs over and a reviewer diffs, so
//! identical inputs have to produce identical bytes. Two things about `utoipa` bear on that:
//!
//! * The `preserve_order` feature is on. Without it the component and path maps are unordered, and
//!   the document's key order changes between builds.
//! * `utoipa::openapi::extensions::Extensions` - the `x-*` keys - is backed by a `HashMap`, whose
//!   iteration order is randomised **per map instance**, not just per process. So a document
//!   carrying any extension serializes differently on every call to `to_json`, and no feature flag
//!   fixes it. The repair is to reparse the output into an order-preserving tree, sort only the
//!   extension keys and re-emit.
//!
//! **This service sets no extension anywhere, so the repair is not built.** That is a statement
//! about today rather than a claim about the format, and it is enforced rather than asserted:
//! `tests::the_document_is_byte_stable_across_independent_builds` builds the document twice, from
//! scratch, and compares the bytes. Because the randomisation is per instance, that test fails the
//! moment an `x-*` extension is added - and the failure is the signal to build the ordered-tree
//! repair, which needs one more dependency and belongs in this file.
//!
//! # No security scheme is declared, and that is honest
//!
//! `utoipa` can describe a bearer scheme, and describing one here would put an `Authorize` button
//! in the browser UI. It is deliberately absent: a scheme in the document reads as an
//! authentication model, and this service has none - the token authenticates the *deployment*, not
//! the caller. The `401` on each operation says what actually happens, and
//! [`DESCRIPTION`] says what it means.

use utoipa::OpenApi;

use crate::constants::API_V1_PREFIX;
use crate::routes;

/// The document's own description, and the one place the identity gap is stated to a reader of the
/// API rather than to an operator reading a log.
///
/// It is long on purpose. Somebody integrating against this surface is exactly the person who will
/// otherwise assume that a `401` implies a per-caller identity behind it.
const DESCRIPTION: &str = "\
Answers questions about data using metric definitions somebody certified.

A question names a metric, a grain, a bounded period and up to four dimensions. There is no field \
for SQL, a table, a filter expression or a list of row ids, so an uncertified question is \
unrepresentable rather than refused.

A refusal is a RESULT, not an error - and it comes back with an explicit status rather than a 200. \
`POST /v1/query` answers 403, 404, 409, 413, 422 or 503 with `outcome: refusal` when a question is \
one the caller may not have. Every one of those carries a stable `code` and a sentence saying what \
to change. Repeating an unchanged question will not change the answer; the one refusal worth \
retrying is 503 `source_unavailable`. 200 means the question was ANSWERED and nothing else does.

WHO IS ASKING, and what it does not buy. A deployment may be configured to verify your own token - \
signature, issuer, expiry, and an audience matching its own resource identifier - in which case a \
401 carries a WWW-Authenticate challenge naming the realm, and your subject is what each call is \
recorded against. A deployment configured without it authenticates the DEPLOYMENT instead, with a \
shared access token, and records every call against the deployment itself. Which of the two this \
one is, is a deployment decision and this document does not say.\n\
\n\
NEITHER IS PER-CALLER ACCESS. A credential IS minted per question, for the data system the question \
reads - no question executes without one - and on this build it is the identity the service process \
holds for that source rather than yours, because no adapter here can carry a per-subject one. So \
there is no row-level scoping either way: every question is answered with whatever access the \
service process already had, whoever asked it. One 403 IS about a credential and it is not the one \
you presented here: credential_unavailable means you have no access at that data system, and this \
deployment will not read it as itself instead. Every other 403 is the catalog's answer about a \
metric. And no request field carries an identity: a body naming a subject is a 400 that says so.";

/// The document, before the route fragments are merged into it.
///
/// Components and tags only. The schemas listed here are the ones referenced from a response body
/// or a request body; anything reachable from one of them is pulled in by the derive.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "sutura",
        description = DESCRIPTION,
        license(name = "Apache-2.0", url = "https://www.apache.org/licenses/LICENSE-2.0"),
    ),
    components(schemas(
        crate::problem::ProblemBody,
        crate::wire::CatalogBody,
        crate::wire::DimensionBody,
        crate::wire::FilterBody,
        crate::wire::MetricBody,
        crate::wire::OutcomeBody,
        crate::wire::ProvenanceBody,
        crate::wire::LegBody,
        crate::wire::QuestionBody,
        crate::wire::RangeBody,
        crate::wire::RefusalBody,
    )),
    tags(
        (name = "catalog", description = "What this catalog defines: metrics, grains, dimensions and the values a filter may use."),
        (name = "query", description = "Asking one certified question."),
    ),
)]
pub struct ApiDoc;

/// The whole document: the derived shell plus one fragment per version.
///
/// A `v2` adds one line here and nothing else, which is what makes versioning additive: the
/// fragment carries its own absolute paths because the prefix is applied by the `nest` below.
///
/// **It describes the GOVERNED operations and nothing else.** Liveness is mounted on its own
/// router (`crate::router`) and deliberately left out: it is a property of the process rather than
/// an operation of the API (`crate::constants::HEALTH_PATH` says so), and every operation this
/// document lists becomes a tool a client such as a chat interface may offer its model. A probe in
/// that list is a tool nothing should call. The set is exhaustive in both directions - a route the
/// document describes that `crate::capability::governed` does not name, and a governed route it
/// omits, are each a failure (`tests/operations.rs`).
#[must_use]
pub fn document() -> utoipa::openapi::OpenApi {
    let mut document = ApiDoc::openapi();
    document.merge(
        utoipa_axum::router::OpenApiRouter::new()
            .nest(API_V1_PREFIX, routes::v1::openapi_router())
            .into_openapi(),
    );
    document
}

/// The document as JSON.
///
/// One function, used by the route that serves it and by any tooling that dumps it, so the two
/// cannot emit different bytes. See the determinism note in this module's documentation for what
/// that currently rests on.
pub fn document_json() -> Result<String, serde_json::Error> {
    document().to_json()
}

#[cfg(test)]
mod tests;

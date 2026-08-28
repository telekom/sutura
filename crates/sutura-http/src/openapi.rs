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
//! [`tests::the_document_is_byte_stable_across_independent_builds`] builds the document twice, from
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

NO PER-CALLER IDENTITY. Where an access token is configured, presenting it proves the caller holds \
a secret an operator configured - it authenticates the deployment, not the caller. There is no \
per-request credential and no row-level scoping: every question is answered with whatever access \
the service process already had, whoever asked it. Each call IS recorded against a principal chain \
before its outcome is returned, and on this deployment that chain names the deployment itself - it \
is this paragraph as a value, not an identity for you to set. No request field carries one: a body \
naming a subject is a 400 that says so.";

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
        crate::routes::health::HealthBody,
        crate::wire::CatalogBody,
        crate::wire::DimensionBody,
        crate::wire::FilterBody,
        crate::wire::MetricBody,
        crate::wire::OutcomeBody,
        crate::wire::ProvenanceBody,
        crate::wire::QuestionBody,
        crate::wire::RangeBody,
        crate::wire::RefusalBody,
    )),
    tags(
        (name = "health", description = "Liveness. Unversioned, unauthenticated, and carries no information about the deployment."),
        (name = "catalog", description = "What this catalog defines: metrics, grains, dimensions and the values a filter may use."),
        (name = "query", description = "Asking one certified question."),
    ),
)]
pub struct ApiDoc;

/// The whole document: the derived shell plus one fragment per version.
///
/// A `v2` adds one line here and nothing else, which is what makes versioning additive: the
/// fragment carries its own absolute paths because the prefix is applied by the `nest` below.
#[must_use]
pub fn document() -> utoipa::openapi::OpenApi {
    let mut document = ApiDoc::openapi();
    document.merge(
        utoipa_axum::router::OpenApiRouter::new()
            .routes(utoipa_axum::routes!(routes::health::liveness))
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
mod tests {
    use crate::constants::{API_V1_PREFIX, HEALTH_PATH, base_paths};

    use super::{document, document_json};

    #[test]
    fn the_document_is_byte_stable_across_independent_builds() {
        // THE determinism check, and the one that matters. Each call builds a fresh document, so
        // each call builds fresh maps - which is what makes this sensitive to a `HashMap`-backed
        // extension rather than only to a process-wide seed. If it ever fails, the module
        // documentation says what to build.
        let first = document_json().expect("the document serializes");
        let second = document_json().expect("the document serializes");
        assert_eq!(first, second);
    }

    #[test]
    fn every_route_the_router_mounts_is_in_the_document() {
        // The two are generated from one attribute per handler, so this asserts the wiring rather
        // than the generator: a handler registered on the router and not merged into the document
        // is the mistake it catches.
        let paths = document().paths;
        for expected in [
            HEALTH_PATH,
            &format!("{API_V1_PREFIX}{}", base_paths::CATALOG),
            &format!("{API_V1_PREFIX}{}", base_paths::QUERY),
        ] {
            assert!(
                paths.paths.contains_key(expected),
                "{expected} is missing from the document: {:?}",
                paths.paths.keys().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn liveness_is_documented_outside_the_version_prefix() {
        // Deliberate: an orchestrator's probe must not need reconfiguring for a version bump.
        let paths = document().paths;
        assert!(paths.paths.keys().any(|path| path == HEALTH_PATH));
        assert!(!HEALTH_PATH.starts_with(API_V1_PREFIX));
    }

    #[test]
    fn the_document_tells_a_reader_that_there_is_no_per_caller_identity() {
        // The one thing somebody integrating against this surface will otherwise assume. It is in
        // the document rather than only in an operator's log, because they are different readers.
        let rendered = document_json().expect("the document serializes");
        assert!(rendered.contains("NO PER-CALLER IDENTITY"), "the description lost the notice");
        assert!(
            rendered.contains("refusal is a RESULT"),
            "the description lost the refusal notice"
        );
    }

    #[test]
    fn every_refusal_status_is_declared_on_the_query_operation() {
        // The document is what somebody integrating reads, and a status nothing declares is a status
        // they will meet in production instead. Asserted through the merged document rather than by
        // reading the attribute, because the attribute is only half the wiring.
        // Read out of the SERIALIZED document rather than off the typed tree: the JSON is what a
        // client generator consumes, and it is also the thing that does not move under a `utoipa`
        // release that renames a type in its path model.
        let rendered = document_json().expect("the document serializes");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("the document is JSON");
        let responses = &parsed["paths"][format!("{API_V1_PREFIX}{}", base_paths::QUERY)]["post"]["responses"];
        for status in ["200", "403", "404", "409", "413", "422", "503"] {
            let declared = &responses[status];
            assert!(!declared.is_null(), "{status} is not declared on POST /v1/query: {responses}");
            let description = declared["description"].as_str().unwrap_or_default();
            assert!(!description.is_empty(), "{status} is declared with no meaning given");
        }
        // And the refusal statuses say so, so a reader can tell them from the failures that share a
        // number with them.
        for status in ["403", "404", "409", "413", "422"] {
            assert!(
                responses[status]["description"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("outcome: refusal"),
                "{status} does not tell a reader it carries a refusal"
            );
        }
    }

    #[test]
    fn no_security_scheme_is_declared() {
        // Absent on purpose: a declared scheme reads as an authentication model, and a shared
        // deployment secret is not one. If this ever becomes false it should be because a credential
        // broker exists, and this test is where that decision surfaces.
        assert!(
            document()
                .components
                .and_then(|components| { (!components.security_schemes.is_empty()).then_some(components.security_schemes.len()) })
                .is_none(),
            "a security scheme was declared without a per-caller identity behind it"
        );
    }
}

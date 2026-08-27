//! Liveness, and nothing else.
//!
//! # What it does not say
//!
//! No version, no build identifier, no dependency list, no configuration, no catalog. This is the
//! one route an unauthenticated caller can always reach - an orchestrator's probe has no credential
//! to present - so every field it might carry is a field handed to anybody who can route a packet.
//! A version string is a lookup into a list of known vulnerabilities; a dependency list is that
//! list, pre-assembled.
//!
//! The body is therefore a single fixed word, and a test asserts it byte for byte. That test is
//! there to fail when somebody adds "just the version".
//!
//! # Why there is no readiness route
//!
//! Because there is nothing for it to report that is not already true. This service does not start
//! unless the pinned bundle validated - `Validated<PinnedDefinitions>` has no other constructor,
//! and every anchor is re-executed against the data system to get one - so a process that is
//! listening is a process whose definitions reproduced the numbers their author certified. A
//! readiness route today would answer `ready` unconditionally, which is worse than not having one:
//! it would look like a dependency check.
//!
//! It arrives when something can become unready *after* startup - a connection pool, a token that
//! expires, a catalog that is reloaded - and not before.

use axum::Json;

/// The liveness body.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub(crate) struct HealthBody {
    /// Always `ok`. The status code is the signal; this is here so the body is valid JSON rather
    /// than empty.
    #[schema(example = "ok")]
    status: &'static str,
}

/// The tag this route is grouped under in the generated document.
const TAG: &str = "health";

/// Is the process up?
///
/// Answers `200` whenever it can answer at all, which is the whole of what liveness means: a
/// process that cannot serve this route cannot serve anything, and one that can may still be
/// refusing every question for a governance reason. Those are different questions and this is the
/// cheap one.
#[utoipa::path(
    get,
    path = "/health",
    tag = TAG,
    responses(
        (status = 200, description = "The process is up. The body carries no version, no configuration and no catalog content.", body = HealthBody),
    )
)]
pub(crate) async fn liveness() -> Json<HealthBody> {
    Json(HealthBody { status: "ok" })
}

#[cfg(test)]
mod tests {
    use super::{HealthBody, liveness};

    #[tokio::test]
    async fn the_body_is_exactly_one_field_and_that_field_is_ok() {
        // Byte for byte, on purpose. This is the test that fails when somebody adds the version -
        // which is the change that turns a liveness probe into a disclosure.
        let body = liveness().await;
        let rendered = serde_json::to_string(&body.0).expect("the body serializes");
        assert_eq!(rendered, r#"{"status":"ok"}"#);
    }

    #[test]
    fn the_body_has_no_field_that_could_describe_the_deployment() {
        // A second angle on the same rule, expressed over the serialized keys rather than the whole
        // string, so it keeps working if a future field is legitimately added and this list is the
        // thing that has to be argued with.
        let rendered = serde_json::to_string(&HealthBody { status: "ok" }).expect("the body serializes");
        for forbidden in ["version", "commit", "build", "catalog", "digest", "config", "environment"] {
            assert!(!rendered.contains(forbidden), "{forbidden} must not appear: {rendered}");
        }
    }
}

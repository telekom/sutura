//! Every path this service answers on, in one place.
//!
//! The version prefix is a constant rather than a literal at each mount point, and that is the
//! whole of the versioning story: a `v2` is a second module that mounts under a second prefix
//! beside this one, sharing the state, the layers and the generated document. Nothing existing has
//! to move, which is what "additive" means.
//!
//! The generated `OpenAPI` document is built from the same constants, so a path in the document and
//! a path in the router cannot disagree about where a handler lives.

/// The versioned API prefix.
pub const API_V1_PREFIX: &str = "/v1";

/// Liveness. Deliberately *outside* the version prefix.
///
/// A liveness probe is not part of the API contract - it is a property of the process, and it must
/// keep working across a version bump without an orchestrator being reconfigured. It is also the
/// one path that is not behind the access token, because a probe has no credential to present.
pub const HEALTH_PATH: &str = "/health";

/// Where the generated interface description is served, when it is served at all.
pub const OPENAPI_JSON_PATH: &str = "/openapi.json";

/// Where the browser interface over that description is served.
pub const SWAGGER_UI_PATH: &str = "/docs";

/// The mount point of each group inside a version.
pub mod base_paths {
    /// What this catalog defines.
    pub const CATALOG: &str = "/catalog";
    /// Asking one certified question.
    pub const QUERY: &str = "/query";
}

#[cfg(test)]
mod tests {
    use super::{API_V1_PREFIX, HEALTH_PATH, OPENAPI_JSON_PATH, SWAGGER_UI_PATH, base_paths};

    #[test]
    fn every_path_starts_with_a_slash_and_does_not_end_with_one() {
        // An axum route with a trailing slash matches a different request than one without, and a
        // segment missing its leading slash silently concatenates with what it is nested under.
        // Both produce a route nobody can reach and no error anywhere.
        for path in [
            API_V1_PREFIX,
            HEALTH_PATH,
            OPENAPI_JSON_PATH,
            SWAGGER_UI_PATH,
            base_paths::CATALOG,
            base_paths::QUERY,
        ] {
            assert!(path.starts_with('/'), "{path}");
            assert!(!path.ends_with('/'), "{path}");
        }
    }

    #[test]
    fn liveness_is_not_inside_the_version_prefix() {
        // Deliberate, and worth pinning: an orchestrator's probe must survive a version bump
        // without being reconfigured.
        assert!(!HEALTH_PATH.starts_with(API_V1_PREFIX));
    }
}

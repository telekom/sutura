//! RFC 9728 metadata for a deployment that directly validates its callers' access tokens.
//!
//! The document and route are built from one [`ProtectedResource`] at startup. The challenge points
//! at them only when an origin-form target and raw `Host` reproduce that same resource, as RFC 9728
//! section 3.3 requires.

use axum::Router;
use axum::http::{HeaderMap, Uri};
use axum::routing::get;
use serde::Serialize;
use sutura_config::{TokenLocation, TokenRequirement};

/// The well-known prefix from RFC 9728 section 3.1.
const WELL_KNOWN_PATH: &str = "/.well-known/oauth-protected-resource";

/// The two fields this deployment publishes.
#[derive(Serialize)]
struct MetadataDocument {
    resource: String,
    authorization_servers: [String; 1],
}

/// One direct inbound declaration rendered as a route, a document and its absolute URL.
pub(crate) struct ProtectedResource {
    authority: String,
    // Serialized once, at startup, and served from a clone - the same choice `openapi::document_json`
    // makes, and for the same reason: this route is unauthenticated, so serializing per request would
    // put allocation and JSON encoding behind a path anyone can poll for free.
    body: String,
    path: String,
    resource_path: Option<String>,
    url: String,
}

impl ProtectedResource {
    /// Builds metadata only for direct inbound identity.
    #[expect(
        clippy::expect_used,
        reason = "the document is two plain `String` fields - no float, no non-UTF-8 byte, no map \
                  whose key order would matter - so serialization cannot fail; keep in hand so a \
                  malformed serializer would be a visible panic rather than a silently absent route"
    )]
    pub(crate) fn for_requirement(requirement: TokenRequirement<'_>) -> Option<Self> {
        if requirement.location() != TokenLocation::AuthorizationBearer {
            return None;
        }
        let resource = requirement.audience().as_str();
        let after_scheme = resource.strip_prefix("https://").unwrap_or(resource);
        let (authority, resource_path) = after_scheme
            .split_once('/')
            .map_or((after_scheme, None), |(authority, path)| (authority, Some(path)));

        let mut path = String::from(WELL_KNOWN_PATH);
        if let Some(resource_path) = resource_path.filter(|path| !path.is_empty()) {
            path.push('/');
            path.push_str(resource_path);
        }
        let mut url = String::from("https://");
        url.push_str(authority);
        url.push_str(&path);
        let resource_path = resource_path.map(|path| format!("/{path}"));

        let document = MetadataDocument {
            resource: String::from(resource),
            authorization_servers: [String::from(requirement.issuer().as_str())],
        };
        let body = serde_json::to_string(&document).expect("a two-string struct always serializes");

        Some(Self {
            authority: String::from(authority),
            body,
            path,
            resource_path,
            url,
        })
    }

    /// The exact absolute URL placed in the Bearer challenge.
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    /// Whether the request can use this document under RFC 9728 section 3.3.
    ///
    /// Only an origin-form request is matched byte for byte: its authority remains in the raw
    /// `Host` value. Absolute-form targets are not matched: `http::Uri` canonicalises standard schemes
    /// in absolute form, so a match there would compare against a normalised spelling rather than the
    /// configured identifier's exact bytes. The configured identifier supplies the absent `https`
    /// scheme. An identifier with a path describes only that exact path. A pathless identifier is
    /// still served for direct discovery, but cannot describe a request whose URL carries a path.
    pub(crate) fn describes(&self, uri: &Uri, headers: &HeaderMap) -> bool {
        if uri.scheme().is_some() || uri.authority().is_some() {
            return false;
        }
        headers
            .get(axum::http::header::HOST)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|authority| authority == self.authority)
            && self
                .resource_path
                .as_deref()
                .is_some_and(|resource_path| uri.path_and_query().is_some_and(|path| path.as_str() == resource_path))
    }

    /// The public route, outside both the versioned and capability-gated subtrees.
    pub(crate) fn router(&self) -> Router {
        let body = self.body.clone();
        Router::new()
            // A configured resource path may legally contain a segment beginning `:` or `*`. Axum
            // treats both as literals but refuses their legacy `:param`/`*wild` spellings unless
            // this check is disabled. `{capture}` cannot arrive: the parsed resource alphabet
            // contains no braces.
            .without_v07_checks()
            .route(
                &self.path,
                get(move || {
                    let body = body.clone();
                    async move { ([(axum::http::header::CONTENT_TYPE, "application/json")], body) }
                }),
            )
    }
}

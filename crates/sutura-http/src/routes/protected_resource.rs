//! RFC 9728 metadata for a deployment that directly validates its callers' access tokens.
//!
//! The document and route are built from one [`ProtectedResource`] at startup. The challenge points
//! at them only when an origin-form target and raw `Host` reproduce that same resource, as RFC 9728
//! section 3.3 requires.

use axum::http::{HeaderMap, Uri};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sutura_config::{TokenLocation, TokenRequirement};

/// The well-known prefix from RFC 9728 section 3.1.
const WELL_KNOWN_PATH: &str = "/.well-known/oauth-protected-resource";

/// The two fields this deployment publishes.
#[derive(Clone, Serialize)]
struct MetadataDocument {
    resource: String,
    authorization_servers: [String; 1],
}

/// One direct inbound declaration rendered as a route, a document and its absolute URL.
pub(crate) struct ProtectedResource {
    authority: String,
    document: MetadataDocument,
    path: String,
    resource_path: Option<String>,
    url: String,
}

impl ProtectedResource {
    /// Builds metadata only for direct inbound identity.
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

        Some(Self {
            authority: String::from(authority),
            document: MetadataDocument {
                resource: String::from(resource),
                authorization_servers: [String::from(requirement.issuer().as_str())],
            },
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
    /// Only an origin-form request can be matched byte for byte: its authority remains in the raw
    /// `Host` value, while `http::Uri` canonicalises standard schemes in absolute form and loses their
    /// original spelling. The configured identifier supplies the absent `https` scheme. An identifier
    /// with a path describes only that exact path. A pathless identifier is still served for direct
    /// discovery, but cannot describe a request whose URL carries a path.
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
        let document = self.document.clone();
        Router::new()
            // A configured resource path may legally contain a segment beginning `:` or `*`. Axum
            // 0.8 treats both as literals but refuses their old 0.7 spellings unless this check is
            // disabled. `{capture}` cannot arrive: the parsed resource alphabet contains no braces.
            .without_v07_checks()
            .route(
                &self.path,
                get(move || {
                    let document = document.clone();
                    async move { Json(document) }
                }),
            )
    }
}

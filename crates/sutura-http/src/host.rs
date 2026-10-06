//! Which `Host` this deployment answers.
//!
//! One list, derived from the deployment, is checked on every route that answers a caller: `/v1/*`,
//! the documentation, and `/mcp`. `/health` and `/metrics` are outside it because a probe and a scrape
//! arrive with the pod's own address as their `Host`.
//!
//! **Matched like the agent transport matches**: the `Host` header, else the request's own
//! authority; the port ignored; the name compared ASCII case-insensitively with an IPv6 literal's
//! brackets removed. A `Host` that is not an authority is refused.
//!
//! **Two limits, stated where they bind.** A bind off the loopback that declares no
//! `server.allowed_hosts` answers every `Host` ([`HostAllowlist::of`] is `None`, and the router says
//! so at startup): such a bind already needs a credential, so the `Host` is not what guards it. And a
//! request with no `Host` and no authority is answered, because an HTTP/1.1 client always names one -
//! the check is for a client that names a host, not for an HTTP/1.0 one.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::HOST;
use axum::http::uri::Authority;
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use sutura_config::{AllowedHost, Settings};

use crate::problem::Failure;

/// The names a loopback bind is reached by.
const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// The hosts one deployment answers, in the form [`AllowedHost`] stores them.
#[derive(Debug, Clone)]
pub struct HostAllowlist(Arc<[String]>);

impl HostAllowlist {
    /// The list this deployment enforces, or `None` where it enforces none.
    ///
    /// Enforced when the bind is loopback, or when `server.allowed_hosts` names a host. The list is the
    /// loopback names, those declared hosts, and the host of the deployment's own resource identifier
    /// (`security.inbound`), which is the name its callers reach it by.
    #[must_use]
    pub fn of(settings: &Settings) -> Option<Self> {
        let declared = settings.server().allowed_hosts();
        if !settings.server().bind().is_loopback() && declared.is_empty() {
            return None;
        }
        let resource = settings
            .security()
            .inbound()
            .and_then(|inbound| inbound.requirement().audience().host());
        let hosts = LOOPBACK
            .iter()
            .copied()
            .chain(declared.iter().chain(resource.as_ref()).map(AllowedHost::as_str))
            .map(String::from)
            .collect();
        Some(Self(hosts))
    }

    /// Whether this request's `Host` is on the list.
    fn permits(&self, request: &Request) -> bool {
        let authority = match request.headers().get(HOST) {
            Some(written) => match written.to_str().ok().and_then(|text| Authority::try_from(text).ok()) {
                Some(authority) => authority,
                None => return false,
            },
            None => match request.uri().authority() {
                Some(authority) => authority.clone(),
                None => return true,
            },
        };
        let host = authority.host();
        let host = host.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')).unwrap_or(host);
        self.0.iter().any(|allowed| allowed.eq_ignore_ascii_case(host))
    }
}

/// Says what the check does, because the off-host case answers everything and must not look like
/// it is guarded.
pub fn announce(allowlist: Option<&HostAllowlist>) {
    if let Some(hosts) = allowlist {
        tracing::info!(
            hosts = hosts.0.len(),
            "the Host header is checked against this deployment's own hosts"
        );
    } else {
        tracing::warn!(
            key = AllowedHost::KEY,
            "this deployment binds off the loopback and declares no hosts, so it answers every Host; \
             list the names it is reached by in `server.allowed_hosts` to check them"
        );
    }
}

/// Refuses a request whose `Host` is not on the list.
///
/// A `from_fn_with_state` middleware for the reason [`crate::middleware::require_token`] is one: it
/// has to run for a whole subtree. The value refused is not logged - it is the caller's own text.
pub async fn require_host(State(allowed): State<HostAllowlist>, request: Request, next: Next) -> Response {
    if allowed.permits(&request) {
        return next.run(request).await;
    }
    tracing::warn!("refused a request whose Host this deployment does not answer");
    Failure::HostNotAllowed.into_response()
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;

    use super::HostAllowlist;

    fn allowing(hosts: &[&str]) -> HostAllowlist {
        HostAllowlist(hosts.iter().map(|host| String::from(*host)).collect())
    }

    fn with_host(host: Option<&str>, uri: &str) -> axum::extract::Request {
        let mut request = Request::builder().uri(uri);
        if let Some(host) = host {
            request = request.header("host", host);
        }
        request.body(Body::empty()).expect("a test request builds")
    }

    #[test]
    fn the_name_is_compared_without_its_port_case_or_brackets() {
        let allowed = allowing(&["localhost", "::1", "sutura.example.com"]);
        for host in ["localhost", "LocalHost:8080", "[::1]:8080", "[::1]", "Sutura.Example.COM:443"] {
            assert!(allowed.permits(&with_host(Some(host), "/")), "{host}");
        }
    }

    #[test]
    fn a_name_that_is_not_on_the_list_or_not_a_host_is_refused() {
        let allowed = allowing(&["localhost", "sutura.example.com"]);
        for host in [
            "other.example.com",
            "localhost.example.com",
            "sutura.example.com.evil.test",
            "a b",
            "",
            "@",
        ] {
            assert!(!allowed.permits(&with_host(Some(host), "/")), "{host:?}");
        }
    }

    #[test]
    fn the_request_authority_stands_in_for_an_absent_host_header_and_no_host_at_all_is_answered() {
        let allowed = allowing(&["localhost"]);
        assert!(allowed.permits(&with_host(None, "http://localhost:8080/v1/catalog")));
        assert!(!allowed.permits(&with_host(None, "http://other.example.com/v1/catalog")));
        assert!(allowed.permits(&with_host(None, "/v1/catalog")));
    }
}

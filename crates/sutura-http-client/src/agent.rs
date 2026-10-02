//! [`agent`], the one constructor of an outbound `ureq::Agent` in this workspace, and the two ways a
//! reader gets one: fixed over an already-loaded bundle, or rebuilt on every
//! `sutura_tls::POLL_INTERVAL` poll from a `security.outbound` declaration.

use std::time::Duration;

use crate::bounds::ReadBounds;
use crate::budget::Budget;
use crate::tls;

/// A cap on response HEADERS, read before any body - a foreign endpoint's header block is
/// untrusted input like anything else read off the wire, and has to be bounded before this reads
/// any of it.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// Builds a fresh `ureq::Agent` with a reader's pins over the given TLS configuration - the one
/// place the pins are written, so the fixed and rotating constructors use the same client. There is
/// no `https_only(true)` here: a caller pursues a validated `Endpoint` (which already refuses a
/// non-loopback plaintext host) rather than a compile-time `https://` constant, so the scheme pin
/// lives in the parse, not in the agent.
fn agent_from_tls(socket: Duration, tls: ureq::tls::TlsConfig) -> ureq::Agent {
    agent(|config| {
        config
            .http_status_as_error(false)
            .timeout_global(Some(socket))
            .max_response_header_size(MAX_HEADER_BYTES)
            .tls_config(tls)
    })
}

/// `ureq`'s own agent builder, as [`agent`]'s `configure` receives it.
pub type AgentConfig = ureq::config::ConfigBuilder<ureq::typestate::AgentScope>;

/// **The one way this workspace builds a `ureq::Agent`** - `clippy.toml` bans every other
/// constructor, so an agent without the two pins below does not pass `just lint`.
///
/// `configure` starts from `ureq`'s defaults, which take a proxy from `ALL_PROXY`, `HTTPS_PROXY` or
/// `HTTP_PROXY` (either case) unless `NO_PROXY` names the host. The pins are applied after it, so
/// no caller can undo them:
///
/// - **Only `https://` to a host that is not an IP loopback literal may use a proxy.** Every other
///   request is dialled directly, whatever proxy the agent or the request itself carries.
/// - **No redirects.** `ureq` follows one inside the request, after that decision was made for the
///   original target.
///
/// The decision is a `ureq` middleware, read from the AGENT's configuration on every request, so a
/// request-level configuration cannot remove it.
#[must_use]
#[expect(
    clippy::disallowed_methods,
    reason = "the one sanctioned agent constructor: every other site is banned so that each agent carries the direct-dial middleware"
)]
pub fn agent(configure: impl FnOnce(AgentConfig) -> AgentConfig) -> ureq::Agent {
    ureq::Agent::new_with_config(
        configure(ureq::Agent::config_builder())
            .max_redirects(0)
            .middleware(dial_directly_unless_tls_to_a_remote_host)
            .build(),
    )
}

/// How a request may leave this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// `https://` to a host that is not an IP loopback literal: the agent's proxy, if any.
    MayProxy,
    /// Anything else: straight to the target.
    Direct,
}

impl Route {
    fn of(uri: &ureq::http::Uri) -> Self {
        let host = uri.host().unwrap_or_default().trim_start_matches('[').trim_end_matches(']');
        if uri.scheme() == Some(&ureq::http::uri::Scheme::HTTPS) && !sutura_domain::source::host_is_loopback(host) {
            Self::MayProxy
        } else {
            Self::Direct
        }
    }
}

/// A [`Route::Direct`] request whose proxy could not be cleared, so it was not sent.
#[derive(Debug, thiserror::Error)]
#[error("a plaintext or loopback request could not be taken off the proxy, so it was not sent")]
struct NotTakenOffTheProxy;

fn dial_directly_unless_tls_to_a_remote_host(
    request: ureq::http::Request<ureq::SendBody>,
    next: ureq::middleware::MiddlewareNext,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    use ureq::RequestExt as _;
    if Route::of(request.uri()) == Route::MayProxy {
        return next.handle(request);
    }
    // `None` only outside a middleware; refused rather than sent through the proxy.
    let direct = request
        .middleware_config()
        .ok_or_else(|| ureq::Error::Other(Box::new(NotTakenOffTheProxy)))?;
    next.handle(direct.proxy(None).build())
}

/// A reader's rotating agent handle and (when a declaration exists) the poll handle that keeps it
/// current - named because the spelled-out pair is over this workspace's `type_complexity`
/// threshold.
pub type OutboundAgent = (sutura_tls::Rotating<ureq::Agent>, Option<sutura_tls::Rotator<ureq::Agent>>);

/// A fixed agent over an already-loaded anchor bundle, presenting no client identity.
///
/// `anchors` is `None` for `ureq`'s compiled-in roots - the constructor-time half of a reader's TLS
/// posture; [`rotating_agent`] is the one that also presents an identity, over a
/// `security.outbound` declaration a composition root polls.
#[must_use]
pub fn fixed(bounds: ReadBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> sutura_tls::Rotating<ureq::Agent> {
    sutura_tls::Rotating::fixed(agent_from_tls(Budget::socket(bounds.timeout()), tls::config(anchors, None)))
}

/// Builds a reader's rotating agent handle for a declared `security.outbound` set.
///
/// Also returns, when one is declared, the [`sutura_tls::Rotator`] the composition root drives on
/// [`sutura_tls::POLL_INTERVAL`]. `None` (no declaration) returns a fixed handle over `ureq`'s
/// compiled-in roots, presenting no identity, and no poll handle. Rebuilt over
/// `RootCerts::Specific` from each freshly loaded bundle and, when
/// [`sutura_tls::Declared::identity`] is declared, the freshly loaded identity too - never a union,
/// never a second external read.
///
/// # Errors
///
/// The declared bundle or client identity cannot be loaded at boot.
pub fn rotating_agent(
    bounds: ReadBounds,
    declared: Option<sutura_tls::Declared>,
) -> Result<OutboundAgent, sutura_tls::LoadError> {
    let Some(declared) = declared else {
        return Ok((fixed(bounds, None), None));
    };
    let (anchors, identity) = declared.into_parts();
    let socket = Budget::socket(bounds.timeout());
    let rebuild = move |loaded, identity: Option<sutura_tls::LoadedIdentity>| {
        Ok::<_, sutura_tls::LoadError>(agent_from_tls(socket, tls::config(Some(loaded), identity)))
    };
    let initial_anchors = sutura_tls::load_anchors(&anchors)?;
    let initial_identity = identity.as_ref().map(sutura_tls::load_identity).transpose()?;
    let initial = agent_from_tls(socket, tls::config(Some(initial_anchors), initial_identity));
    let rotator = sutura_tls::Rotator::new(anchors, identity, rebuild, initial);
    Ok((rotator.rotating(), Some(rotator)))
}

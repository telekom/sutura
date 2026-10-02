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
/// `HTTP_PROXY` (either case) unless `NO_PROXY` names the host. Both pins are set on every request,
/// after the agent's and the request's own configuration, so no caller can undo them:
///
/// - **Only `https://` to a host that is not loopback may use a proxy.** Every other request is
///   dialled directly, whatever proxy the agent or the request itself carries. Loopback is
///   `localhost` or a name under it, and any loopback or unspecified address once read the way the
///   resolver reads it (`127.1`, `0x7f000001`, `[::ffff:127.0.0.1]`).
/// - **No redirects.** `ureq` follows one inside the request, after that decision was made for the
///   original target.
///
/// The pins are a `ureq` middleware, read from the AGENT's configuration on every request, so a
/// request-level configuration cannot remove it.
#[must_use]
#[expect(
    clippy::disallowed_methods,
    reason = "the one sanctioned agent constructor: every other site is banned so that each agent carries the routing middleware"
)]
pub fn agent(configure: impl FnOnce(AgentConfig) -> AgentConfig) -> ureq::Agent {
    ureq::Agent::new_with_config(
        configure(ureq::Agent::config_builder())
            .middleware(pin_routing_and_redirects)
            .build(),
    )
}

/// How a request may leave this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// `https://` to a host that is not loopback: the agent's proxy, if any.
    MayProxy,
    /// Anything else: straight to the target.
    Direct,
}

impl Route {
    fn of(uri: &ureq::http::Uri) -> Self {
        if uri.scheme() == Some(&ureq::http::uri::Scheme::HTTPS) && !loopback(uri.host().unwrap_or_default()) {
            Self::MayProxy
        } else {
            Self::Direct
        }
    }
}

/// Whether `host` names this host's own loopback; the unspecified address dials it too. Wider than
/// `sutura_domain::source::host_is_loopback`, which decides what may be PLAINTEXT and so stays
/// narrow; here a wider answer only ever routes more requests directly.
fn loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let name = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
    let address = host
        .parse::<std::net::IpAddr>()
        .ok()
        .map(|address| address.to_canonical())
        .or_else(|| ipv4_numbers(&name).map(std::net::IpAddr::V4));
    name == "localhost"
        || name.ends_with(".localhost")
        || address.is_some_and(|address| address.is_loopback() || address.is_unspecified())
}

/// `host` read as the C resolver's `inet_aton` reads it: one to four dot-separated parts, each
/// decimal, `0x` hex or `0`-led octal, the last filling every byte the others left.
fn ipv4_numbers(host: &str) -> Option<std::net::Ipv4Addr> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut numbers = Vec::with_capacity(parts.len());
    for part in &parts {
        let (digits, radix) = match (part.strip_prefix("0x"), part.strip_prefix('0')) {
            (Some(hex), _) => (hex, 16),
            (None, Some(octal)) if !octal.is_empty() => (octal, 8),
            _ => (*part, 10),
        };
        numbers.push(u32::from_str_radix(digits, radix).ok()?);
    }
    let (last, leading) = numbers.split_last()?;
    let mut address: u32 = 0;
    for (index, &byte) in leading.iter().enumerate() {
        address |= u8::try_from(byte).ok().map(u32::from)? << (24 - 8 * index);
    }
    let room = 32 - 8 * u32::try_from(leading.len()).ok()?;
    if room < 32 && *last >> room != 0 {
        return None;
    }
    Some(std::net::Ipv4Addr::from(address | last))
}

/// A request whose routing could not be pinned, so it was not sent.
#[derive(Debug, thiserror::Error)]
#[error("a request's routing could not be pinned, so it was not sent")]
struct RoutingNotPinned;

fn pin_routing_and_redirects(
    request: ureq::http::Request<ureq::SendBody>,
    next: ureq::middleware::MiddlewareNext,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    use ureq::RequestExt as _;
    let route = Route::of(request.uri());
    // `None` only outside a middleware; refused rather than sent unpinned.
    let config = request
        .middleware_config()
        .ok_or_else(|| ureq::Error::Other(Box::new(RoutingNotPinned)))?
        .max_redirects(0);
    let config = match route {
        Route::MayProxy => config,
        Route::Direct => config.proxy(None),
    };
    next.handle(config.build())
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

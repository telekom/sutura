//! The two ways a reader gets an outbound `ureq::Agent`: fixed over an already-loaded bundle, or
//! rebuilt on every `sutura_tls::POLL_INTERVAL` poll from a `security.outbound` declaration.

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
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(socket))
            .max_response_header_size(MAX_HEADER_BYTES)
            .proxy(ureq::Proxy::try_from_env())
            .tls_config(tls)
            .build(),
    )
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

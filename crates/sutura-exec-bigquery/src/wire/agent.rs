//! [`WireAgent`] - the one `ureq::Agent` every request in this crate goes through.
//!
//! Its six pins are fixed by the type, and its `security.outbound` declaration (anchors, and
//! since `github.com/telekom/sutura#911` an optional client identity) is folded into it.
//!
//! Split out of `wire.rs` for the file-length gate the way `bounds.rs`/`credential.rs`/`document.rs`
//! already are: `wire.rs` sat at (and then over) `cargo xtask max-lines`'s 1000-line cap, and this
//! type - one struct, one impl block - is a separable concept rather than a slice of prose.

use crate::wire::JobBounds;
use crate::wire::tls;

/// The client every request in this crate goes through, with the four settings that matter PINNED BY
/// THE TYPE rather than by a call site.
///
/// **This newtype is the whole mechanism, and it exists because the previous shape was a convention.**
/// The settings below used to live in a free function returning a bare `ureq::Agent`, and both
/// [`super::BigQueryWire::new`] and `credential::ApplicationDefault::read` accepted any agent - so a
/// composition root writing `ureq::Agent::new_with_defaults()` got redirects on, plaintext allowed
/// and no timeout, while every test passed because the tests all called the right function. A private
/// field with one constructor is what *a newtype parses rather than validates* asks for: if an
/// instance of this exists, the pins hold.
///
/// It also carries the [`JobBounds`], so the deadline that shapes the socket timeout and the deadline
/// that goes into the request body are **the same value**. Two arguments could have disagreed.
#[derive(Debug, Clone)]
pub struct WireAgent {
    agent: sutura_tls::Rotating<ureq::Agent>,
    bounds: JobBounds,
}

impl WireAgent {
    /// The compiled-in-roots constructor: [`Self::secured`] with no declared anchors.
    ///
    /// This is every deployment's behaviour before `github.com/telekom/sutura#125` and stays the
    /// default for one with no `security.outbound.transport_anchors` block - see [`Self::secured`]
    /// for the one setting that differs when a deployment declares one.
    #[must_use]
    pub fn pinned(bounds: JobBounds) -> Self {
        Self::secured(bounds, None)
    }

    /// The one place every non-default setting is decided, and every one of them is a decision:
    ///
    /// - `http_status_as_error(false)`, because the client's default turns a `4xx` into an error and
    ///   discards the body - and the body is where the endpoint says *which* refusal this is. Status is
    ///   read explicitly instead, in `refusal`.
    /// - `https_only(true)`, so a bearer token cannot leave over plaintext even if a URL somewhere
    ///   loses its scheme. `HOST` is already `https`; this is the second lock.
    /// - `max_redirects(0)`, so the credential has no second host to reach. `ureq-proto` also strips
    ///   `authorization` on a redirect, which was verified rather than assumed - so this is belt and
    ///   braces, and the belt is ours.
    /// - `timeout_global`, at the job's deadline plus `CONNECT_MARGIN`, so the socket cannot outlive
    ///   the job it is waiting for by more than connection setup.
    /// - `max_response_header_size`, because headers are read before the body's own limit applies.
    /// - `proxy(Proxy::try_from_env())`, which is the client's own default WRITTEN OUT rather than
    ///   inherited. An egress proxy is a legitimate deployment shape and the tunnel is still TLS to
    ///   `HOST`, so what the environment chooses is the route and not the destination. The module
    ///   header states that distinction, because a previous version of it claimed the stronger thing.
    /// - `tls_config`, over [`crate::wire::tls::config`] - `RootCerts::WebPki` (`ureq`'s own default)
    ///   for `anchors: None`, which is every call [`Self::pinned`] makes and every deployment before
    ///   `#125`; `RootCerts::Specific` built from `anchors` for `Some`, which is what
    ///   `security.outbound.transport_anchors` resolves to. This constructor never presents a client
    ///   identity - [`Self::rotating_agent`] is the one that does, over the same declaration.
    ///
    /// The agent is wrapped in a never-rotating [`sutura_tls::Rotating`] - this constructor has no
    /// declaration to re-read. The rotation lane is [`Self::rotating`], fed by
    /// [`Self::rotating_agent`].
    #[must_use]
    pub fn secured(bounds: JobBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self {
        Self::rotating(
            bounds,
            sutura_tls::Rotating::fixed(tls::agent_from_tls(bounds.deadline().socket(), tls::config(anchors, None))),
        )
    }

    /// The rotation-lane constructor over a handle built by [`Self::rotating_agent`].
    #[must_use]
    pub const fn rotating(bounds: JobBounds, agent: sutura_tls::Rotating<ureq::Agent>) -> Self {
        Self { agent, bounds }
    }

    /// Builds the wire's rotating agent handle for a declared `security.outbound` set (and, when one
    /// is declared, the poll handle the composition root drives on [`sutura_tls::POLL_INTERVAL`]).
    /// `None` returns a fixed handle over `ureq`'s compiled-in roots and no presented identity (the
    /// pre-`#125` behaviour, nothing to re-read); `Some` rebuilds `RootCerts::Specific` from each
    /// freshly loaded bundle and, when [`sutura_tls::Declared::identity`] is declared, presents the
    /// freshly loaded [`sutura_tls::LoadedIdentity`] too - both adopted by the next request.
    ///
    /// # Errors
    ///
    /// The declared bundle or client identity cannot be loaded at boot.
    pub fn rotating_agent(
        bounds: JobBounds,
        declared: Option<sutura_tls::Declared>,
    ) -> Result<tls::OutboundAgent, sutura_tls::LoadError> {
        let Some(declared) = declared else {
            return Ok((
                sutura_tls::Rotating::fixed(tls::agent_from_tls(bounds.deadline().socket(), tls::config(None, None))),
                None,
            ));
        };
        let (anchors, identity) = declared.into_parts();
        let timeout = bounds.deadline().socket();
        let rebuild = move |loaded, identity: Option<sutura_tls::LoadedIdentity>| {
            Ok::<_, sutura_tls::LoadError>(tls::agent_from_tls(timeout, tls::config(Some(loaded), identity)))
        };
        let initial_anchors = sutura_tls::load_anchors(&anchors)?;
        let initial_identity = identity.as_ref().map(sutura_tls::load_identity).transpose()?;
        let initial = tls::agent_from_tls(timeout, tls::config(Some(initial_anchors), initial_identity));
        let rotator = sutura_tls::Rotator::new(anchors, identity, rebuild, initial);
        Ok((rotator.rotating(), Some(rotator)))
    }

    /// The client, for the two modules in this crate that send a request, as an `Arc` clone resolved
    /// from the rotating handle - the per-request read that adopts a rotation on the next request.
    ///
    /// `pub(crate)`, so nothing outside can take the agent out of its wrapper and reconfigure it.
    #[inline]
    pub(crate) fn agent(&self) -> std::sync::Arc<ureq::Agent> {
        self.agent.current()
    }

    /// What every job through this client is bounded by.
    #[inline]
    #[must_use]
    pub const fn bounds(&self) -> JobBounds {
        self.bounds
    }
}

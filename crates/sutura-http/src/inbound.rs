//! Leg 1 of the identity path: how a caller proves who it is, on this transport.
//!
//! `docs/adr/0014` is the record. It decides two inbound modes with no default, and it puts every
//! piece of this in the transport on purpose: *"All of it is transport: it parses a wire shape and
//! produces a domain value, and it decides nothing about what a question may ask."* That is what this
//! module is - a header becomes a [`VerifiedCaller`], which becomes a
//! `sutura_domain::identity::RequestContext` through `crate::principal::of_verified`, and nothing
//! here can widen, narrow or parameterize what executes.
//!
//! | File | What it owns |
//! | --- | --- |
//! | [`keys`] | the key set, its cache, the **rate-limited** refetch on an unknown key id, and the **age bound** that is what makes revocation bounded |
//! | [`token`] | algorithm pinning, the audience check, and claims into a principal chain |
//! | [`caller`] | [`VerifiedCaller`] and [`Scopes`] - the conclusion of a verification, as a type nothing can deserialize |
//! | [`gate`] | the layer, and the `401` with its challenge |
//!
//! `sutura_config::inbound` owns the *declaration* - which mode, which issuer, which audience, which
//! algorithms - and names no JWT library at all. The two vocabularies meet in exactly one function,
//! `token::map_algorithm`, which is an exhaustive match.
//!
//! # What this delivers, and what it must not be read as delivering
//!
//! **Delivered:** a caller's identity is established from a signed, audience-bound, unexpired token,
//! and `sutura_domain::identity::Subject::Verified` finally has a constructor with something real
//! behind it. Every audit record written for such a call names the person rather than the deployment.
//!
//! **Not delivered, and `docs/adr/0014` says so in the same words:** leg 1 proves who is asking. It
//! does *not* make a data source execute as that person - that is leg 2, and it needs a credential per
//! leg plus a source that declares it can impersonate. A deployment with leg 1 and no leg 2 knows who
//! is asking and still reads every row as one identity. The startup log prints that sentence on every
//! boot, out of `sutura_config::InboundIdentity::what_it_does_not_do`, rather than leaving a reader to
//! infer it.
//!
//! # The five things this does not build, and each is named rather than left to be discovered
//!
//! An overstated claim is itself the defect, so each of these is written down here rather than found:
//!
//! 1. **A JWKS endpoint.** Keys are read from a file. The cache, the unknown-key refetch and the rate
//!    limit on it are built and are what a URL source would need anyway - see [`keys`] for the whole
//!    argument and for the one property a file cannot have.
//! 2. **The two metadata documents.** A directly validating deployment is supposed to serve
//!    protected-resource metadata a client can read to learn which authorization server governs it.
//!    There is no such route. The `401` carries an RFC 6750 challenge naming the realm and no
//!    `resource_metadata` parameter, so a client is configured with its issuer out of band.
//! 3. **Anything about client registration or client authentication.** Those are decisions for the
//!    authorization server and for the client; this deployment is a resource server and validates what
//!    arrives.
//! 4. **A ceiling derived from a scope.** [`Scopes`] is now read by exactly one thing -
//!    `crate::capability`, which decides which of this surface's *operations* a caller may invoke and
//!    decides nothing about which rows an answer contains. A per-caller *budget* still has no port to
//!    live behind, and `docs/adr/0013`'s raw tool is not built. See [`caller`] for the limit stated
//!    beside the claim.
//! 5. **Binding a gateway assertion to a request.** Added by review: in the `behind-gateway` mode the
//!    replay *window* is bounded - an `iat` is required and `exp - iat` is capped by a value this
//!    deployment chose - and inside that window an intercepted assertion replays. There is no nonce
//!    store and nothing hashes a method, a path or a body into the assertion. That is why nothing here
//!    calls it a proof that *this request* transited anything, and why the hop between the component
//!    and this process is a trusted transport boundary rather than an incidental one.
//!
//! # Why this is not shared with the agent surface
//!
//! `sutura-mcp` has its own `principal` module and it still answers
//! `sutura_domain::identity::Subject::TheDeploymentItself`, honestly: it speaks over standard input and
//! output, where there is no header for a token to arrive in. **Nothing here is reachable from it** -
//! an adapter never calls another adapter, and that rule is what keeps this module from being the
//! shape a second transport has to bend around. Which crate this code moves to when that surface
//! acquires an inbound transport is an architecture decision, not a refactor.

pub mod caller;
pub mod gate;
pub mod keys;
pub mod token;

pub use crate::inbound::caller::{InvalidScope, Scopes, VerifiedCaller};
pub use crate::inbound::gate::{InboundGate, InboundNotUsable, require_verified_caller};
pub use crate::inbound::keys::{
    FileKeySet, InvalidKeySet, KeyId, KeySet, KeySetCache, KeySetSource, KeySetUnavailable, KeyUnavailable, MAX_KEY_SET_AGE,
    MIN_REFETCH_INTERVAL, NotAKeyId, Refreshed,
};
pub use crate::inbound::token::{MAX_TOKEN_BYTES, PresentedType, TokenRejected, TokenValidator};

#[cfg(test)]
mod tests;

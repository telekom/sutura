//! The layer that turns a presented token into a verified caller, or answers `401` with a challenge.
//!
//! # Where it sits, and why after the deployment token rather than before
//!
//! `crate::router` installs the layers so a request travels: limiter, then the deployment token gate,
//! then this. Two reasons, and neither is style:
//!
//! - **Cost.** The deployment token comparison is two hashes; this is a signature verification. Doing
//!   the expensive one first would let an unauthenticated caller spend this deployment's CPU.
//! - **The limiter stays outermost**, which `crate::router` explains at length: a wrong-credential
//!   attempt has to cost a rate-limit cell or it is an unlimited guessing loop. That argument applies
//!   to a forged signature exactly as it applies to a wrong shared secret.
//!
//! In the `direct` mode there is no deployment token to be after -
//! `sutura_config::NotFitToServe::DeploymentTokenSharesTheHeader` refuses that combination - so the
//! order matters only in the `behind-gateway` mode, where both are configured and each reads its own
//! header.
//!
//! # What a refused request is told, and what it is not
//!
//! A `401` with an RFC 6750 `WWW-Authenticate` challenge naming the realm, which for a directly
//! validating deployment is its own resource identifier. Its `resource_metadata` parameter is the
//! absolute URL of the public RFC 9728 document built from the same token requirement as the
//! validator. A gateway assertion arrives somewhere other than `Authorization: Bearer`, so that mode
//! offers no Bearer challenge.
//!
//! What the response does **not** say is which check failed. The log says - through the `#[source]`
//! chain on `TokenRejected` - and the caller does not, because "the signature verified and the
//! audience did not" tells somebody which half of a forgery to fix.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use sutura_config::{InboundIdentity, TokenLocation};

use sutura_runtime::Shutdown;

use crate::inbound::caller::VerifiedCaller;
use crate::inbound::keys::{FileKeySet, KeySetCache, KeySetUnavailable, MAX_KEY_SET_AGE};
use crate::inbound::token::{TokenRejected, TokenValidator};
use crate::problem::Failure;

/// The scheme an `Authorization` token carries, and the header it arrives in.
const AUTHORIZATION: &str = "authorization";
const BEARER: &str = "Bearer";

/// Everything one deployment needs to establish who a caller is, built once at startup.
///
/// **Built by the composition root and not by [`crate::ServiceState::new`]**, because building it
/// reads a file: a constructor that could not fail would have to either swallow an unreadable key set
/// or read it lazily on the first request, and both turn a refusal to start into a deployment that
/// authenticates nobody. `crate::router` refuses to assemble a router for a deployment whose settings
/// declare an inbound identity and whose state carries no gate, which is what makes forgetting to
/// attach one a startup failure rather than an open door.
pub struct InboundGate {
    validator: TokenValidator,
    /// Behind an `Arc` so [`crate::inbound::keys::KeySetCache::watch_until_shutdown`] can hold a
    /// `Weak` to it, and stop when this gate is dropped rather than keeping it alive.
    keys: Arc<KeySetCache>,
    /// Where the token arrives, resolved from the declaration at startup rather than per request.
    ///
    /// An owned `String` rather than the borrowed `TokenLocation`, so the gate does not hold a
    /// lifetime into the settings tree and can live in an `Arc` for the life of the process.
    header: String,
    /// Whether the value carries the `Bearer ` prefix. The `direct` mode does, per RFC 6750; a
    /// component setting its own header does not.
    ///
    /// It also decides whether a refused request gets a `WWW-Authenticate: Bearer` challenge at all -
    /// see [`InboundGate::challenge`].
    bearer_prefixed: bool,
    /// What the challenge names as its realm.
    realm: String,
    /// The public discovery document and the exact absolute URL the challenge points at.
    protected_resource: Option<crate::routes::protected_resource::ProtectedResource>,
}

/// The gate could not be built.
#[derive(Debug, thiserror::Error)]
#[error("the inbound identity declared by this deployment is not usable")]
pub struct InboundNotUsable {
    #[source]
    cause: KeySetUnavailable,
}

impl InboundGate {
    /// Builds the gate from a declaration `sutura-config` already accepted, reading the key set once.
    ///
    /// **Called before the listener opens.** An unreadable or unusable key set is an error here, so it
    /// is a process that does not start rather than one that answers `401` to everybody.
    pub fn from_declaration(inbound: &InboundIdentity) -> Result<Self, InboundNotUsable> {
        let requirement = inbound.requirement();
        let source = FileKeySet::at(requirement.key_set().path());
        // The pinned family travels into the cache, so **every** path that adopts a key set - this one
        // and every later refresh - refuses one that holds no key of it. A deployment whose key set and
        // whose `algorithms` disagree about the kind of key starts and answers `401` to everybody
        // otherwise, which is the shape review asked to have refused.
        let keys = KeySetCache::primed(
            Box::new(source),
            requirement.algorithms().family(),
            requirement.algorithms().to_string(),
            Instant::now(),
        )
        .map_err(|cause| InboundNotUsable { cause })?;
        Ok(Self::over(inbound, keys))
    }

    /// The same, over a key set that is already in hand.
    ///
    /// `pub(crate)` and the seam every test in this module uses: a fake
    /// [`crate::inbound::keys::KeySetSource`] is how the rate limit and the rotation are asserted
    /// without a filesystem, which is the same argument `AGENTS.md` makes for a port getting a fake.
    pub(crate) fn over(inbound: &InboundIdentity, keys: KeySetCache) -> Self {
        let requirement = inbound.requirement();
        let protected_resource = crate::routes::protected_resource::ProtectedResource::for_requirement(requirement);
        let (header, bearer_prefixed) = match requirement.location() {
            TokenLocation::AuthorizationBearer => (String::from(AUTHORIZATION), true),
            TokenLocation::Header { name } => (String::from(name.as_str()), false),
        };
        Self {
            realm: String::from(requirement.audience().as_str()),
            validator: TokenValidator::new(&requirement),
            keys: Arc::new(keys),
            header,
            bearer_prefixed,
            protected_resource,
        }
    }

    /// Starts the timer that bounds how long a revoked key keeps verifying.
    ///
    /// Called from the composition root, inside the runtime, for the reason
    /// `crate::tls::Renewal::watch_until_shutdown` is: the gate is built before a runtime exists, so it
    /// cannot spawn its own task at construction.
    ///
    /// **Forgetting it does not leave revocation unbounded**, and that is deliberate rather than
    /// forgiving: `crate::inbound::keys::KeySetCache::key_for` checks the age itself, so a deployment
    /// serving traffic re-reads within the same horizon. What the timer adds is the bound holding while
    /// nothing is being asked.
    pub fn watch_keys_until_shutdown(&self, shutdown: Shutdown) {
        KeySetCache::watch_until_shutdown(&self.keys, MAX_KEY_SET_AGE, shutdown);
    }

    /// The header this gate reads, for a startup log line and for a test.
    #[inline]
    #[must_use]
    pub fn header(&self) -> &str {
        &self.header
    }

    /// How many keys are cached, and their ids. For the startup log.
    pub async fn describe_keys(&self) -> (usize, Vec<String>) {
        self.keys.describe().await
    }

    /// The RFC 6750 challenge a refused request carries, where one is meaningful.
    ///
    /// **`None` in the `behind-gateway` mode, and that is a fix rather than an omission.** A `Bearer`
    /// challenge tells a client to present a bearer token to *this* resource; behind a component, the
    /// caller holds no token for us and the thing that was missing was a header the component sets.
    /// Sending the challenge anyway would send a well-formed instruction that cannot be followed, and a
    /// client that followed it would start putting credentials in a header this deployment refuses to
    /// read.
    ///
    /// **No `error_description`** in the direct case, and that is the same decision the response body
    /// makes: a description would have to say which check failed to be worth anything, and that is the
    /// one thing a caller must not learn.
    #[must_use]
    pub fn challenge(&self) -> Option<String> {
        if !self.bearer_prefixed {
            return None;
        }
        let protected_resource = self.protected_resource.as_ref()?;
        Some(format!(
            "Bearer realm=\"{}\", error=\"invalid_token\", resource_metadata=\"{}\"",
            self.realm,
            protected_resource.url()
        ))
    }

    /// The discovery document this gate built, only in the direct mode.
    pub(crate) const fn protected_resource(&self) -> Option<&crate::routes::protected_resource::ProtectedResource> {
        self.protected_resource.as_ref()
    }

    /// Establishes who is asking, or says why it could not.
    ///
    /// The whole request path of leg 1, in one function, so the order of the four steps is readable in
    /// one place: read the header, read the key id it names, find the key, verify.
    ///
    /// **Takes the headers rather than the request, and both reasons are worth keeping.** The narrow
    /// one is that it is the whole of what leg 1 may read: a gate that was handed a request could
    /// establish an identity from a path, a query parameter or a body, and the signature is what makes
    /// that unavailable rather than merely unwise. The mechanical one is that `axum::body::Body` is not
    /// `Sync`, so a future holding `&Request` across an await is not `Send` and cannot run as a layer
    /// at all - which is how the narrow reason got discovered.
    pub async fn establish(&self, headers: &HeaderMap, now: Instant) -> Result<VerifiedCaller, TokenRejected> {
        let presented = self.presented(headers)?;
        let id = TokenValidator::key_id(presented)?;
        let key = self
            .keys
            .key_for(&id, now)
            .await
            .map_err(|cause| TokenRejected::NoKey { cause })?;
        self.validator.verify(presented, &key)
    }

    /// The token as presented, without the scheme prefix where there is one.
    ///
    /// **The scheme is matched case-insensitively, which is a fix.** RFC 9110 section 11.1 makes an
    /// authentication scheme name case-insensitive, so `bearer abc` is a bearer token and a
    /// `strip_prefix("Bearer ")` refused it - as *absent*, which is the wrong diagnostic on top of the
    /// wrong outcome. A client that sends the lower-case spelling is not a client presenting nothing.
    ///
    /// The three failures are three variants rather than one, because they are three different things
    /// for whoever is debugging: no header at all is a client nobody configured; a value with another
    /// scheme is a client configured for a different service; and a header this HTTP implementation
    /// will not hand back as a string is a value with bytes no scheme could carry.
    fn presented<'request>(&self, headers: &'request HeaderMap) -> Result<&'request str, TokenRejected> {
        let absent = || TokenRejected::Absent {
            location: self.header.clone(),
        };
        let value = headers
            .get(self.header.as_str())
            .ok_or_else(absent)?
            .to_str()
            .map_err(|_not_a_string| absent())?;
        if !self.bearer_prefixed {
            return Ok(value);
        }
        let (scheme, credential) = value.split_once(' ').ok_or_else(|| TokenRejected::NotABearerToken {
            location: self.header.clone(),
        })?;
        if !scheme.eq_ignore_ascii_case(BEARER) {
            return Err(TokenRejected::NotABearerToken {
                location: self.header.clone(),
            });
        }
        Ok(credential)
    }
}

impl core::fmt::Debug for InboundGate {
    /// Hand-written, because the cache holds a trait object and because what a reader wants named is
    /// which header and which audience. No key material and no token, at any depth.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InboundGate")
            .field("header", &self.header)
            .field("realm", &self.realm)
            .finish_non_exhaustive()
    }
}

/// Requires a verified caller, and puts one in the request extensions.
///
/// A `from_fn_with_state` middleware over the gate rather than over
/// [`crate::ServiceState`](crate::state::ServiceState), so the state a handler is given has no way to
/// reach the validator: the only thing that crosses into the handler is the *result*, as a
/// [`VerifiedCaller`] extension that only this function inserts.
///
/// **The insertion overwrites**, which matters: `axum` extensions are a map, and a request arriving
/// with something already under that type - which nothing can construct, but the reasoning should not
/// rest on that alone - is replaced rather than joined.
pub async fn require_verified_caller(State(gate): State<Arc<InboundGate>>, mut request: Request, next: Next) -> Response {
    // One clock read per request, passed down, so the rate-limit window is decided once and the same
    // instant is what a retry inside it is measured against.
    let now = Instant::now();
    // Cloned rather than borrowed, so nothing holds a borrow of the request across the await: the
    // whole request is not `Sync` - see `InboundGate::establish` - and a `HeaderMap` clone is a handful
    // of small allocations against a signature verification.
    let headers = request.headers().clone();
    match gate.establish(&headers, now).await {
        Ok(caller) => {
            drop(request.extensions_mut().insert(caller));
            next.run(request).await
        }
        Err(rejected) => refused(&gate, &rejected),
    }
}

/// The `401`, its challenge, and the one place the reason is written down.
///
/// A function of its own so the middleware above stays a branch rather than a body: two log fields, a
/// header that may not build and a response to return was over the cognitive-complexity threshold in
/// `clippy.toml`, and the split puts the whole "what a refused caller is told" decision in one place.
fn refused(gate: &InboundGate, rejected: &TokenRejected) -> Response {
    // The cause chain, not the variant alone: which of signature, expiry, issuer and audience failed is
    // what an operator needs, and it goes here rather than to the caller.
    tracing::warn!(
        error = %rejected,
        causes = ?crate::surface::cause_chain(rejected),
        header = gate.header(),
        "no verified caller: the presented token did not establish one"
    );
    let mut response = Failure::Unauthorized.into_response();
    if let Some(challenge) = gate.challenge() {
        challenged(&mut response, &challenge);
    }
    response
}

/// Puts the challenge on a response, or says why it could not.
///
/// Its own function because the fallible-header branch put [`refused`] over the cognitive-complexity
/// threshold in `clippy.toml`, and because the branch deserves the sentence: the failure is
/// unreachable from a loaded configuration - the realm is a `sutura_config::ResourceIdentifier`, whose
/// parse accepts an ASCII subset a header value can always carry - and it is logged rather than
/// ignored. **The `401` goes out either way**: a missing challenge is a client somebody has to
/// configure by hand, and answering anything else would be a hole.
fn challenged(response: &mut Response, challenge: &str) {
    match axum::http::HeaderValue::from_str(challenge) {
        Ok(value) => {
            drop(response.headers_mut().insert(axum::http::header::WWW_AUTHENTICATE, value));
        }
        Err(cause) => tracing::error!(error = %cause, "the configured resource identifier is not a header value"),
    }
}

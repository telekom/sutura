//! The layer that turns a presented token into a verified caller, or answers `401` with a challenge.
//!
//! # Where it sits, and why after the deployment token rather than before
//!
//! `crate::router` installs the layers so a request travels: limiter, then the `Host` check, then the
//! deployment token gate, then this. Two reasons, and neither is style:
//!
//! - **Cost.** The deployment token comparison is two hashes; this is a signature verification. Doing
//!   the expensive one first would let an unauthenticated caller spend this deployment's CPU.
//! - **The limiter stays outermost**, which `crate::router` explains at length: a wrong-credential
//!   attempt has to cost a rate-limit cell or it is an unlimited guessing loop. That argument applies
//!   to a forged signature exactly as it applies to a wrong shared secret.
//!
//! Where the verified token arrives in `Authorization` - the `direct` mode, or a component that
//! replaced the caller's token there - there is no deployment token to be after:
//! `sutura_config::NotFitToServe::DeploymentTokenSharesTheHeader` refuses that combination. So the
//! order matters only in the `behind-gateway` mode with a header of the component's own, where both
//! are configured and each reads its own header.
//!
//! # What a refused request is told, and what it is not
//!
//! A `401` with an RFC 6750 `WWW-Authenticate` challenge naming the realm, which for a directly
//! validating deployment is its own resource identifier. When an origin-form target and raw `Host`
//! reproduce that exact resource, its `resource_metadata` parameter is the absolute URL of the public
//! RFC 9728 document built from the same token requirement as the validator. It is omitted for any
//! other request, because RFC 9728 section 3.3 requires a client to discard mismatched metadata. A
//! gateway assertion is minted by the component, not by an authorization server a client can use, so
//! that mode offers no Bearer challenge - wherever the assertion arrives.
//!
//! An unknown key id while the key source is down is `503 identity_unavailable` instead, with no
//! challenge: the token may be fine, and the outage is this deployment's. So is a new assertion while
//! the store of seen assertions is full, where single use is declared.
//!
//! What the response does **not** say is which check failed. The log says - through the `#[source]`
//! chain on `TokenRejected` - and the caller does not, because "the signature verified and the
//! audience did not" tells somebody which half of a forgery to fix.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::http::Uri;
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use sutura_config::{InboundIdentity, KeySetLocation, ResourceIdentifier, TokenLocation};

use sutura_runtime::Shutdown;

use crate::inbound::caller::VerifiedCaller;
use crate::inbound::keys::{
    FileKeySet, KeySetCache, KeySetClientUnusable, KeySetSource, KeySetUnavailable, KeyUnavailable, MAX_KEY_SET_AGE, UrlKeySet,
};
use crate::inbound::token::{TokenRejected, TokenValidator};
use crate::problem::Failure;

/// The scheme an `Authorization` token carries, and the header it arrives in.
const AUTHORIZATION: &str = "authorization";
const BEARER: &str = "Bearer";

/// Everything one deployment needs to establish who a caller is, built once at startup.
///
/// **Built by the composition root and not by [`crate::ServiceState::new`]**, because building it
/// reads the key set: a constructor that could not fail would have to either swallow an unreadable key set
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
    /// Whether the value carries the `Bearer ` prefix. Every token in `Authorization` does, per
    /// RFC 6750; a component setting its own header does not.
    bearer_prefixed: bool,
    /// Whether a refused request gets a `WWW-Authenticate: Bearer` challenge at all. Only in the
    /// `direct` mode - see [`InboundGate::challenge`].
    challenges: bool,
    /// What the challenge names as its realm.
    realm: String,
    /// The public discovery document and the exact absolute URL the challenge points at.
    protected_resource: Option<crate::routes::protected_resource::ProtectedResource>,
}

/// The gate could not be built.
#[derive(Debug, thiserror::Error)]
pub enum InboundNotUsable {
    #[error("the inbound identity declared by this deployment is not usable")]
    Unreadable(#[source] KeySetUnavailable),
    #[error("the client for `security.inbound.key_set_url` could not be built")]
    Client(#[source] KeySetClientUnusable),
}

impl InboundGate {
    /// Builds the gate from a declaration `sutura-config` already accepted, reading the key set once.
    ///
    /// **Called before the listener opens.** An unreadable or unusable key set is an error here, so it
    /// is a process that does not start rather than one that answers `401` to everybody.
    ///
    /// A `key_set_url` is fetched over the compiled-in web roots here, whatever `security.outbound`
    /// declares; [`Self::from_declaration_trusting`] is the call that honours a declared bundle.
    pub fn from_declaration(inbound: &InboundIdentity) -> Result<Self, InboundNotUsable> {
        Self::from_declaration_trusting(inbound, None)
    }

    /// The same, fetching a key set URL over `anchors` - the deployment's `security.outbound`
    /// trust - or, with none declared, the compiled-in web roots.
    pub fn from_declaration_trusting(
        inbound: &InboundIdentity,
        anchors: Option<&sutura_tls::Anchors>,
    ) -> Result<Self, InboundNotUsable> {
        let requirement = inbound.requirement();
        let source: Box<dyn KeySetSource> = match requirement.key_set() {
            KeySetLocation::File(file) => Box::new(FileKeySet::at(file.path())),
            KeySetLocation::Url(url) => Box::new(UrlKeySet::trusting(url, anchors).map_err(InboundNotUsable::Client)?),
        };
        // The pinned family travels into the cache, so **every** path that adopts a key set - this one
        // and every later refresh - refuses one that holds no key of it. A deployment whose key set and
        // whose `algorithms` disagree about the kind of key starts and answers `401` to everybody
        // otherwise, which is the shape review asked to have refused.
        let keys = KeySetCache::primed(
            source,
            requirement.algorithms().family(),
            requirement.algorithms().to_string(),
            Instant::now(),
        )
        .map_err(InboundNotUsable::Unreadable)?;
        if requirement.single_use() {
            tracing::info!(
                capacity = crate::inbound::replay::CAPACITY,
                max_jti_bytes = crate::inbound::replay::MAX_ASSERTION_ID_BYTES,
                "single use: each assertion is admitted once by its `jti`, per process - another replica or \
                 a restart admits it again inside its lifetime, and a full store refuses with 503"
            );
        }
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
        let (header, bearer_prefixed, challenges) = match requirement.location() {
            TokenLocation::AuthorizationBearer => (String::from(AUTHORIZATION), true, true),
            TokenLocation::GatewayBearer => (String::from(AUTHORIZATION), true, false),
            TokenLocation::Header { name } => (String::from(name.as_str()), false, false),
        };
        Self {
            realm: String::from(
                requirement
                    .resource()
                    .map_or_else(|| requirement.audience().as_str(), ResourceIdentifier::as_str),
            ),
            validator: TokenValidator::new(&requirement),
            // `docs/adr/20261015095237`: only a mode with a lifetime ceiling has a grace, and it is that
            // ceiling plus the leeway an old key's last assertion can still be accepted inside.
            keys: Arc::new(
                keys.with_grace(
                    requirement
                        .max_lifetime()
                        .map(|ceiling| Duration::from_secs(ceiling.seconds()).saturating_add(TokenValidator::leeway())),
                ),
            ),
            header,
            bearer_prefixed,
            challenges,
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
    /// caller holds no token for us and the thing that was missing was an assertion the component sets.
    /// Sending the challenge anyway would send a well-formed instruction that cannot be followed, and a
    /// client that followed it would present its own token, which is not an assertion this deployment
    /// accepts in any header.
    ///
    /// **No `error_description`** in the direct case, and that is the same decision the response body
    /// makes: a description would have to say which check failed to be worth anything, and that is the
    /// one thing a caller must not learn.
    ///
    /// Both request parts are required because an origin-form target carries its path in the URI and
    /// its authority in the raw `Host` value. Absolute-form targets are not matched: `http::Uri`
    /// canonicalises standard schemes, so a match there would compare against a normalised spelling
    /// rather than the configured identifier's exact bytes.
    #[must_use]
    pub fn challenge(&self, request_uri: &Uri, headers: &HeaderMap) -> Option<String> {
        if !self.challenges {
            return None;
        }
        let mut challenge = format!("Bearer realm=\"{}\", error=\"invalid_token\"", self.realm);
        if let Some(protected_resource) = self
            .protected_resource
            .as_ref()
            .filter(|protected_resource| protected_resource.describes(request_uri, headers))
        {
            challenge.push_str(", resource_metadata=\"");
            challenge.push_str(protected_resource.url());
            challenge.push('"');
        }
        Some(challenge)
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
    /// Four failures, three variants, because they are different things for whoever is debugging:
    /// no header at all is a client nobody configured (`Absent`); a header sent twice asks this gate
    /// to choose which caller to believe (`Repeated`); and a value with another scheme is a client
    /// configured for a different service (`NotABearerToken`). A header this HTTP implementation will
    /// not hand back as a string carries bytes no scheme could, and reads as `Absent`.
    fn presented<'request>(&self, headers: &'request HeaderMap) -> Result<&'request str, TokenRejected> {
        let absent = || TokenRejected::Absent {
            location: self.header.clone(),
        };
        let mut lines = headers.get_all(self.header.as_str()).iter();
        let value = lines.next().ok_or_else(absent)?;
        if lines.next().is_some() {
            return Err(TokenRejected::Repeated {
                location: self.header.clone(),
            });
        }
        let value = value.to_str().map_err(|_not_a_string| absent())?;
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
        Err(rejected) => refused(&gate, &rejected, request.uri(), &headers),
    }
}

/// The `401` and its challenge - or a `503` when the key source is down or the store of seen
/// assertions is full - and the one place the reason is written down.
///
/// A function of its own so the middleware above stays a branch rather than a body: two log fields, a
/// header that may not build and a response to return was over the cognitive-complexity threshold in
/// `clippy.toml`, and the split puts the whole "what a refused caller is told" decision in one place.
fn refused(gate: &InboundGate, rejected: &TokenRejected, request_uri: &Uri, headers: &HeaderMap) -> Response {
    if let Some(failure) = not_the_callers_fault(rejected) {
        return failure.into_response();
    }
    // The cause chain, not the variant alone: which of signature, expiry, issuer and audience failed is
    // what an operator needs, and it goes here rather than to the caller.
    tracing::warn!(
        error = %rejected,
        causes = ?crate::surface::cause_chain(rejected),
        header = gate.header(),
        "no verified caller: the presented token did not establish one"
    );
    if matches!(
        rejected,
        TokenRejected::NoKey {
            cause: KeyUnavailable::SourceUnavailable { .. }
        }
    ) {
        return Failure::IdentityUnavailable.into_response();
    }
    let mut response = Failure::Unauthorized.into_response();
    if let Some(challenge) = gate.challenge(request_uri, headers) {
        challenged(&mut response, &challenge);
    }
    response
}

/// A refusal that is this process's state rather than the caller's credential, and is not a `401`.
///
/// A full store of seen assertions is announced once per window where it is raised, so it gets no
/// line per request here: that would be a line per request of the flood that filled it.
const fn not_the_callers_fault(rejected: &TokenRejected) -> Option<Failure> {
    match *rejected {
        TokenRejected::SeenStoreFull => Some(Failure::IdentityUnavailable),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::{Failure, TokenRejected, not_the_callers_fault};

    #[test]
    fn a_full_seen_store_answers_identity_unavailable_and_a_replay_answers_401() {
        assert!(matches!(
            not_the_callers_fault(&TokenRejected::SeenStoreFull),
            Some(Failure::IdentityUnavailable)
        ));
        assert!(not_the_callers_fault(&TokenRejected::Replayed).is_none());
    }
}

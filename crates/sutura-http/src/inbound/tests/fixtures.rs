//! The keys, tokens, declarations and header maps every cell next door is driven with.
//!
//! **Its own module because `tests.rs` was eight lines under the thousand-line limit
//! `cargo xtask max-lines` enforces and cannot exempt**, and the cut is the banner that file already
//! carried: everything here was under `// ---- fixtures ----` and every assertion was below it. Not
//! one assertion moved - what moved is the apparatus, which is the half a reader of a failing test
//! wants in a second file rather than above the test.
//!
//! **Every key here is generated and every token is signed for the test that reads it**, and that is
//! the property this file exists to keep: a private key committed to a public repository is a
//! committed private key whatever the comment beside it says, and a hand-written token would be a
//! token this suite's own encoder produced. `rcgen` generates a P-256 key pair, `jsonwebtoken` signs
//! with it, and the public half goes into a JWK the way an issuer would publish it. The limit -
//! which algorithms are reachable end to end and which are covered by a match and a reviewer
//! instead - is stated in `tests.rs`'s own header, next to what those tests claim.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, HeaderValue};
use base64::Engine as _;
use jsonwebtoken::{EncodingKey, Header};
use sutura_config::{
    InboundIdentity, IssuerUrl, KeyFamily, KeySetFile, PinnedAlgorithms, ProofHeader, ProofLifetime, RequiredTokenType,
    ResourceIdentifier, SigningAlgorithm, TokenType, TransitProof,
};

use super::TRANSIT_HEADER;
use crate::inbound::gate::InboundGate;
use crate::inbound::keys::{KeySetCache, KeySetSource, KeySetUnavailable, MAX_KEY_SET_AGE};
use crate::testing::{ISSUER, KID, RESOURCE};

/// A generated P-256 key pair. One per test, so no test can depend on another's key.
pub(super) fn key_pair() -> rcgen::KeyPair {
    rcgen::KeyPair::generate().expect("a test key pair is generatable")
}

/// The public half of a key pair, as a JWK set an issuer would publish.
///
/// `public_key_raw` is the uncompressed point - `0x04 || X || Y` - so the two halves after the tag are
/// the `x` and `y` a JWK carries, base64url without padding. An Ed25519 key has no tag and no `y`,
/// which is why the shape is decided by the length rather than assumed.
pub(super) fn jwks(kid: &str, pair: &rcgen::KeyPair) -> String {
    format!("{{\"keys\":[{}]}}", jwk(kid, pair))
}

/// One JWK, so a test can build a set of two.
pub(super) fn jwk(kid: &str, pair: &rcgen::KeyPair) -> String {
    let point = pair.public_key_raw();
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    if point.len() == 32 {
        let x = engine.encode(point);
        return format!(r#"{{"kty":"OKP","crv":"Ed25519","use":"sig","alg":"EdDSA","kid":"{kid}","x":"{x}"}}"#);
    }
    // The two curves this suite generates, named rather than derived by arithmetic: a P-256 point is
    // 65 bytes and a P-384 one is 97, both `0x04` plus two equal halves.
    let (half, curve, algorithm) = match point.len() {
        97 => (48_usize, "P-384", "ES384"),
        _ => (32_usize, "P-256", "ES256"),
    };
    let x = engine.encode(&point[1..=half]);
    let y = engine.encode(&point[half + 1..]);
    format!(r#"{{"kty":"EC","crv":"{curve}","use":"sig","alg":"{algorithm}","kid":"{kid}","x":"{x}","y":"{y}"}}"#)
}

/// The two key set documents a deployment must refuse: one of the wrong FAMILY, and one holding a
/// shared secret.
///
/// `sutura_dev::issuer` writes both, and they are re-exported here rather than written twice. Neither
/// needs a real key: what is asserted with them is a refusal *at load*, before anything verifies a
/// signature.
pub(super) use sutura_dev::issuer::MockIssuer;

/// A claim set as JSON, so each test writes exactly the claims it is about.
pub(super) fn claims(subject: &str, audience: &str, issuer: &str, extra: &str) -> serde_json::Value {
    let expiry = expiry_in(3600);
    let mut object = serde_json::json!({
        "sub": subject,
        "aud": audience,
        "iss": issuer,
        "exp": expiry,
        "nbf": expiry - 7200,
    });
    if !extra.is_empty() {
        let more: serde_json::Value = serde_json::from_str(extra).expect("a test claim fragment is JSON");
        let object = object.as_object_mut().expect("a claim set is an object");
        for (key, value) in more.as_object().expect("a fragment is an object") {
            drop(object.insert(key.clone(), value.clone()));
        }
    }
    object
}

/// A unix timestamp `seconds` from now.
pub(super) fn expiry_in(seconds: i64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs();
    i64::try_from(now).expect("a unix timestamp fits in an i64") + seconds
}

/// The ordinary token: this subject, our audience, our issuer, unexpired.
pub(super) fn a_token(pair: &rcgen::KeyPair) -> String {
    signed(pair, KID, &claims("someone@example.com", RESOURCE, ISSUER, ""))
}

/// A gateway assertion that satisfies the lifetime ceiling: an `iat` now, an `exp` a minute out.
pub(super) fn short_lived_assertion(pair: &rcgen::KeyPair) -> String {
    assertion_living(pair, 0, 60)
}

/// An assertion issued `issued_ago` seconds back and expiring `lifetime` seconds after that.
///
/// Both offsets rather than one, because the two failures are different: an assertion whose LIFETIME
/// is too long, and one whose `iat` is in the future. A single "expires in" parameter could not
/// express either.
pub(super) fn assertion_living(pair: &rcgen::KeyPair, issued_ago: i64, lifetime: i64) -> String {
    let issued = expiry_in(-issued_ago);
    signed(
        pair,
        KID,
        &serde_json::json!({
            "sub": "someone@example.com",
            "aud": RESOURCE,
            "iss": ISSUER,
            "iat": issued,
            "exp": issued + lifetime,
        }),
    )
}

/// Signs a claim set with `ES256` and a header naming `kid`.
pub(super) fn signed(pair: &rcgen::KeyPair, kid: &str, claims: &serde_json::Value) -> String {
    signed_as(pair, kid, claims, Some(TokenType::ACCESS_TOKEN))
}

/// Signs a claim set with a `typ` of the caller's choosing, including none at all.
///
/// **The seam the cross-JWT substitution test needs**, and the reason it exists as a fixture rather
/// than inline: the interesting token is one that is correct in every respect except its class, so the
/// only thing a test should have to vary is the `typ`.
pub(super) fn signed_as(pair: &rcgen::KeyPair, kid: &str, claims: &serde_json::Value, typ: Option<&str>) -> String {
    let mut header = Header::new(algorithm_for(pair));
    header.kid = Some(String::from(kid));
    header.typ = typ.map(String::from);
    jsonwebtoken::encode(&header, claims, &encoding_key(pair)).expect("a test token signs")
}

/// The algorithm a generated key pair signs with, derived from the curve rather than assumed.
///
/// Three of the nine pinned algorithms are reachable end to end here - `ES256`, `ES384` and `EdDSA` -
/// because those are the three the linked crypto backend can GENERATE a key for. `RS*` and `PS*` are
/// covered by `map_algorithm`'s exhaustive match, by the family check in `keys.rs`, and by nothing
/// end to end: generating an RSA key needs a dependency this workspace does not have, and a committed
/// private key in a public repository is a committed private key whatever the comment beside it says.
fn algorithm_for(pair: &rcgen::KeyPair) -> jsonwebtoken::Algorithm {
    match pair.public_key_raw().len() {
        // A P-384 uncompressed point: 0x04 plus two 48-byte halves.
        97 => jsonwebtoken::Algorithm::ES384,
        // An Ed25519 public key is the bare 32-byte point, with no 0x04 tag.
        32 => jsonwebtoken::Algorithm::EdDSA,
        _ => jsonwebtoken::Algorithm::ES256,
    }
}

/// The signing key, in the shape the library's DER constructors expect.
fn encoding_key(pair: &rcgen::KeyPair) -> EncodingKey {
    let der = pair.serialize_der();
    if algorithm_for(pair) == jsonwebtoken::Algorithm::EdDSA {
        return EncodingKey::from_ed_der(&der);
    }
    EncodingKey::from_ec_der(&der)
}

/// A `direct` declaration over a key set file that may or may not exist.
pub(super) fn direct_over(path: &str) -> InboundIdentity {
    direct_pinning(
        path,
        PinnedAlgorithms::of(SigningAlgorithm::Es256),
        RequiredTokenType::access_token(),
    )
}

/// A `direct` declaration pinning a named algorithm and a named class.
pub(super) fn direct_pinning(path: &str, algorithms: PinnedAlgorithms, token_type: RequiredTokenType) -> InboundIdentity {
    InboundIdentity::Direct {
        resource: ResourceIdentifier::parse(RESOURCE).expect("a test resource is a resource"),
        authorization_server: IssuerUrl::parse(ISSUER).expect("a test issuer is an issuer"),
        key_set: KeySetFile::parse(path).expect("a test path is a path"),
        algorithms,
        token_type,
    }
}

/// The declaration every gate test uses. The key set path is never read: `InboundGate::over` takes the
/// cache directly, which is the seam that keeps these tests off a filesystem.
pub(super) fn direct() -> InboundIdentity {
    direct_over("/unread/in/these/tests.json")
}

/// A `behind-gateway` declaration, with the class and the lifetime ceiling a test wants.
pub(super) fn gateway_declaring(token_type: RequiredTokenType, lifetime: ProofLifetime) -> InboundIdentity {
    InboundIdentity::BehindGateway {
        transit: TransitProof::new(
            ProofHeader::parse(TRANSIT_HEADER).expect("a test header is a header"),
            IssuerUrl::parse_transit_issuer(ISSUER).expect("a test issuer is an issuer"),
            ResourceIdentifier::parse_transit_audience(RESOURCE).expect("a test audience is an audience"),
            KeySetFile::parse("/unread.json").expect("a test path is a path"),
            PinnedAlgorithms::of(SigningAlgorithm::Es256),
            token_type,
            lifetime,
        ),
    }
}

/// A key set source a cell scripts look by look, counts, and may hold one look inside.
///
/// **The instrument the rate limit is measured with.** `read` is what must not happen twice inside
/// the window, so the count is the assertion - `AtomicUsize` rather than a lock, because
/// `clippy.toml` bans `std::sync::Mutex` and there is nothing here to mutate but a counter. The
/// same fake is the instrument for a look that is slow or that fails, which is why an entry can be
/// `None` and one look can wait at [`Self::gate`] - `super::refresh` is where both are used.
pub(super) struct StubSource {
    /// One entry per look, in order; the last one repeats. `None` is a look that fails, so an empty
    /// script or a trailing `None` is a source that answers nothing from there on.
    script: Vec<Option<String>>,
    /// Which look waits at [`Self::gate`], if any. Look zero is the priming read, so a cell that
    /// wants the first *refresh* held names look one.
    hold_at: Option<usize>,
    /// Which look panics instead of returning, if any. Same indexing as [`Self::hold_at`]. Exists to
    /// provoke `LookFailed::DidNotFinish` - a refusal variant no other cell reaches - and to prove
    /// `InFlight` is released across an unwinding blocking closure.
    panic_at: Option<usize>,
    /// Two waiters: the held look, and whoever releases it - which must be a thread that is not the
    /// runtime's, or a build whose read runs on the executor deadlocks instead of failing.
    pub(super) gate: Arc<std::sync::Barrier>,
    pub(super) calls: AtomicUsize,
}

impl StubSource {
    pub(super) fn serving(documents: &[String]) -> Self {
        Self::scripted(documents.iter().cloned().map(Some).collect(), None)
    }

    pub(super) fn failing() -> Self {
        Self::scripted(Vec::new(), None)
    }

    pub(super) fn scripted(script: Vec<Option<String>>, hold_at: Option<usize>) -> Self {
        Self {
            script,
            hold_at,
            panic_at: None,
            gate: Arc::new(std::sync::Barrier::new(2)),
            calls: AtomicUsize::new(0),
        }
    }

    /// Makes the given look (by index, same scheme as [`Self::hold_at`]) panic instead of returning.
    #[must_use]
    pub(super) fn panicking_at(mut self, call: usize) -> Self {
        self.panic_at = Some(call);
        self
    }

    /// Panics if `call` is the scripted one - split out of [`Self::read`] so `clippy` does not read a
    /// deliberate panic as one escaping a `Result`-returning function by accident.
    fn panic_if_scripted(&self, call: usize) {
        assert!(self.panic_at != Some(call), "this look was scripted to panic");
    }
}

impl KeySetSource for StubSource {
    fn read(&self) -> Result<String, KeySetUnavailable> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.panic_if_scripted(call);
        if self.hold_at == Some(call) {
            // Blocking, on whichever thread this is. That is the whole instrument: a cell asserts
            // what the rest of the process can still do while this line has not returned.
            let _released = self.gate.wait();
        }
        self.script
            .get(call)
            .or_else(|| self.script.last())
            .cloned()
            .flatten()
            .ok_or_else(|| KeySetUnavailable::Unreadable {
                path: PathBuf::from("stub"),
                cause: std::io::Error::other("this look was scripted to fail"),
            })
    }
}

/// A cache over prepared documents, and the counter to read afterwards.
///
/// The family is the elliptic-curve one because every generated key here is, and it is passed rather
/// than inferred: it is the value `KeySet::holds` is checked against on every adoption, so a test that
/// wants the mismatch refused has to be able to name a different one.
pub(super) fn cache_over(documents: &[String], window: Duration, now: Instant) -> (KeySetCache, Arc<StubSource>) {
    cache_of_family(documents, KeyFamily::EllipticCurve, window, MAX_KEY_SET_AGE, now)
}

/// The same, with the pinned family and the age bound a test wants.
pub(super) fn cache_of_family(
    documents: &[String],
    family: KeyFamily,
    window: Duration,
    max_age: Duration,
    now: Instant,
) -> (KeySetCache, Arc<StubSource>) {
    cache_around(StubSource::serving(documents), family, window, max_age, now)
}

/// A primed cache over the given source, and the source to read afterwards.
pub(super) fn cache_around(
    source: StubSource,
    family: KeyFamily,
    window: Duration,
    max_age: Duration,
    now: Instant,
) -> (KeySetCache, Arc<StubSource>) {
    let source = Arc::new(source);
    let cache = KeySetCache::primed_with_window(
        Box::new(Arc::clone(&source)),
        family,
        String::from("ES256"),
        window,
        max_age,
        now,
    )
    .expect("the first document is a key set");
    (cache, source)
}

/// So a test can keep the counter while the cache owns the source.
impl KeySetSource for Arc<StubSource> {
    fn read(&self) -> Result<String, KeySetUnavailable> {
        self.as_ref().read()
    }
}

/// The headers a request would arrive with, holding a bearer token.
pub(super) fn bearer(token: &str) -> HeaderMap {
    scheme_and_token("Bearer", token)
}

/// The same, with the scheme spelled however a test wants it.
pub(super) fn scheme_and_token(scheme: &str, token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    drop(headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("{scheme} {token}")).expect("a test token is a header value"),
    ));
    headers
}

/// The headers a gateway deployment reads, holding an assertion.
pub(super) fn transit(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    drop(headers.insert(
        TRANSIT_HEADER,
        HeaderValue::from_str(token).expect("a test token is a header value"),
    ));
    headers
}

/// A gate over a declaration and a key set built from one document.
pub(super) fn gate_over(declaration: &InboundIdentity, document: &str) -> InboundGate {
    let (cache, _) = cache_over(&[String::from(document)], Duration::from_secs(30), Instant::now());
    InboundGate::over(declaration, cache)
}

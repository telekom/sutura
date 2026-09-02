//! What leg 1 accepts, what it refuses, and what a refusal costs.
//!
//! **Every key here is generated and every token is signed for the test that reads it.** Not a
//! committed fixture, and not for tidiness: a private key committed to a public repository is a
//! committed private key whatever the comment beside it says, and a hand-written token would be a
//! token this suite's own encoder produced - which is the shape `AGENTS.md` calls a test asserting on
//! source text. `rcgen` generates a P-256 key pair, `jsonwebtoken` signs with it, and the public half
//! goes into a JWK the way an issuer would publish it.
//!
//! **Which algorithms are reachable end to end, and which are not.** `ES256`, `ES384` and `EdDSA` are
//! signed and verified here for real, because those are the three the linked crypto backend can
//! GENERATE a key for. `RS*` and `PS*` are not: an RSA key needs a dependency nothing else in this
//! workspace wants, and a committed private key in a public repository is a committed private key
//! whatever the comment beside it says. What covers them instead is `token::map_algorithm`'s exhaustive
//! match, the family check in `keys.rs` - which an RSA key set exercises here - and a reviewer. Saying
//! that plainly is the point; a list of nine algorithms with three tested would read as coverage.

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
use sutura_domain::identity::Attribution;

use crate::inbound::caller::Scopes;
use crate::inbound::gate::InboundGate;
use crate::inbound::keys::{
    FileKeySet, InvalidKeySet, KeyId, KeySet, KeySetCache, KeySetSource, KeySetUnavailable, KeyUnavailable, MAX_KEY_SET_AGE,
    NotAKeyId,
};
use crate::inbound::token::{MAX_TOKEN_BYTES, TokenRejected, TokenValidator};

/// The deployment's own names, and the key id the tokens below name.
///
/// **Imported rather than declared**, because `crate::testing` is where the mock issuer builds its
/// own from - so a fixture here and a fixture there cannot disagree about which audience this
/// deployment answers to. Private, and reachable from the child modules through `super::`.
use crate::testing::{ISSUER, KID, RESOURCE};

/// The header a gateway deployment reads its assertion out of.
const TRANSIT_HEADER: &str = "x-transit-proof";

// ------------------------------------------------------------------ fixtures ----

/// A generated P-256 key pair. One per test, so no test can depend on another's key.
fn key_pair() -> rcgen::KeyPair {
    rcgen::KeyPair::generate().expect("a test key pair is generatable")
}

/// The public half of a key pair, as a JWK set an issuer would publish.
///
/// `public_key_raw` is the uncompressed point - `0x04 || X || Y` - so the two halves after the tag are
/// the `x` and `y` a JWK carries, base64url without padding. An Ed25519 key has no tag and no `y`,
/// which is why the shape is decided by the length rather than assumed.
fn jwks(kid: &str, pair: &rcgen::KeyPair) -> String {
    format!("{{\"keys\":[{}]}}", jwk(kid, pair))
}

/// One JWK, so a test can build a set of two.
fn jwk(kid: &str, pair: &rcgen::KeyPair) -> String {
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
use sutura_dev::issuer::MockIssuer;

/// A claim set as JSON, so each test writes exactly the claims it is about.
fn claims(subject: &str, audience: &str, issuer: &str, extra: &str) -> serde_json::Value {
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
fn expiry_in(seconds: i64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs();
    i64::try_from(now).expect("a unix timestamp fits in an i64") + seconds
}

/// The ordinary token: this subject, our audience, our issuer, unexpired.
fn a_token(pair: &rcgen::KeyPair) -> String {
    signed(pair, KID, &claims("someone@example.com", RESOURCE, ISSUER, ""))
}

/// A gateway assertion that satisfies the lifetime ceiling: an `iat` now, an `exp` a minute out.
fn short_lived_assertion(pair: &rcgen::KeyPair) -> String {
    assertion_living(pair, 0, 60)
}

/// An assertion issued `issued_ago` seconds back and expiring `lifetime` seconds after that.
///
/// Both offsets rather than one, because the two failures are different: an assertion whose LIFETIME
/// is too long, and one whose `iat` is in the future. A single "expires in" parameter could not
/// express either.
fn assertion_living(pair: &rcgen::KeyPair, issued_ago: i64, lifetime: i64) -> String {
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
fn signed(pair: &rcgen::KeyPair, kid: &str, claims: &serde_json::Value) -> String {
    signed_as(pair, kid, claims, Some(TokenType::ACCESS_TOKEN))
}

/// Signs a claim set with a `typ` of the caller's choosing, including none at all.
///
/// **The seam the cross-JWT substitution test needs**, and the reason it exists as a fixture rather
/// than inline: the interesting token is one that is correct in every respect except its class, so the
/// only thing a test should have to vary is the `typ`.
fn signed_as(pair: &rcgen::KeyPair, kid: &str, claims: &serde_json::Value, typ: Option<&str>) -> String {
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
fn direct_over(path: &str) -> InboundIdentity {
    direct_pinning(
        path,
        PinnedAlgorithms::of(SigningAlgorithm::Es256),
        RequiredTokenType::access_token(),
    )
}

/// A `direct` declaration pinning a named algorithm and a named class.
fn direct_pinning(path: &str, algorithms: PinnedAlgorithms, token_type: RequiredTokenType) -> InboundIdentity {
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
fn direct() -> InboundIdentity {
    direct_over("/unread/in/these/tests.json")
}

/// A `behind-gateway` declaration, with the class and the lifetime ceiling a test wants.
fn gateway_declaring(token_type: RequiredTokenType, lifetime: ProofLifetime) -> InboundIdentity {
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

/// A key set source that hands out a prepared document per call and counts how often it was asked.
///
/// **The instrument the rate limit is measured with.** `fetch` is what must not happen twice inside the
/// window, so the count is the assertion - `AtomicUsize` rather than a lock, because
/// `clippy.toml` bans `std::sync::Mutex` and there is nothing here to mutate but a counter.
struct StubSource {
    /// One document per fetch; the last one repeats. Empty means every fetch fails.
    documents: Vec<String>,
    calls: AtomicUsize,
}

impl StubSource {
    fn serving(documents: &[String]) -> Self {
        Self {
            documents: documents.to_vec(),
            calls: AtomicUsize::new(0),
        }
    }

    fn failing() -> Self {
        Self {
            documents: Vec::new(),
            calls: AtomicUsize::new(0),
        }
    }
}

impl KeySetSource for StubSource {
    fn read(&self) -> Result<String, KeySetUnavailable> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.documents
            .get(call)
            .or_else(|| self.documents.last())
            .cloned()
            .ok_or_else(|| KeySetUnavailable::Unreadable {
                path: PathBuf::from("stub"),
                cause: std::io::Error::other("this source answers nothing"),
            })
    }
}

/// A cache over prepared documents, and the counter to read afterwards.
///
/// The family is the elliptic-curve one because every generated key here is, and it is passed rather
/// than inferred: it is the value `KeySet::holds` is checked against on every adoption, so a test that
/// wants the mismatch refused has to be able to name a different one.
fn cache_over(documents: &[String], window: Duration, now: Instant) -> (KeySetCache, Arc<StubSource>) {
    cache_of_family(documents, KeyFamily::EllipticCurve, window, MAX_KEY_SET_AGE, now)
}

/// The same, with the pinned family and the age bound a test wants.
fn cache_of_family(
    documents: &[String],
    family: KeyFamily,
    window: Duration,
    max_age: Duration,
    now: Instant,
) -> (KeySetCache, Arc<StubSource>) {
    let source = Arc::new(StubSource::serving(documents));
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
fn bearer(token: &str) -> HeaderMap {
    scheme_and_token("Bearer", token)
}

/// The same, with the scheme spelled however a test wants it.
fn scheme_and_token(scheme: &str, token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    drop(headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("{scheme} {token}")).expect("a test token is a header value"),
    ));
    headers
}

/// The headers a gateway deployment reads, holding an assertion.
fn transit(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    drop(headers.insert(
        TRANSIT_HEADER,
        HeaderValue::from_str(token).expect("a test token is a header value"),
    ));
    headers
}

/// A gate over a declaration and a key set built from one document.
fn gate_over(declaration: &InboundIdentity, document: &str) -> InboundGate {
    let (cache, _) = cache_over(&[String::from(document)], Duration::from_secs(30), Instant::now());
    InboundGate::over(declaration, cache)
}

// ------------------------------------------------------- what a token establishes ----

#[tokio::test]
async fn a_token_the_issuer_signed_establishes_the_person_who_asked() {
    // The thing that could not exist before leg 1: `Subject::Verified` with something real behind it.
    // Driven through the gate rather than through the validator alone, so the header reading, the key
    // lookup and the verification are all in the path.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let caller = gate
        .establish(&bearer(&a_token(&pair)), Instant::now())
        .await
        .expect("a token this issuer signed for this audience establishes a caller");
    let chain = caller.chain();
    assert_eq!(chain.subject().established(), "verified");
    assert_eq!(
        chain.subject().id().map(sutura_domain::identity::SubjectId::as_str),
        Some("someone@example.com")
    );
    // No `act` claim, so no actor - the case a reader has to name rather than infer.
    assert!(matches!(chain.attribution(), Attribution::BareSubject { .. }));
    assert!(chain.task().is_none(), "no claim names a task");
    assert_eq!(caller.scopes().count(), 0, "this token carried no scope claim");
}

#[tokio::test]
async fn a_nested_actor_claim_becomes_a_chain_in_the_order_the_domain_reads() {
    // RFC 8693 nests `act` BACKWARDS in time - the top level acted most recently - and
    // `ActorChain` iterates nearest-the-subject first. Three links, because two cannot tell "kept the
    // order" from "reversed it", which is the same reason the domain's own test uses three.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let token = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            RESOURCE,
            ISSUER,
            r#"{"act":{"sub":"query_agent","act":{"sub":"planner","act":{"sub":"orchestrator"}}}}"#,
        ),
    );
    let caller = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect("a delegated token establishes a caller");
    let Attribution::ActingFor { subject, actors } = caller.chain().attribution() else {
        panic!("an agent acting for a subject is not a bare subject");
    };
    assert_eq!(subject.established(), "verified");
    let names: Vec<&str> = actors.iter().map(sutura_domain::identity::Actor::as_str).collect();
    assert_eq!(
        names,
        vec!["orchestrator", "planner", "query_agent"],
        "the deepest nesting acted first and the top level called this deployment"
    );
    assert_eq!(actors.immediate().as_str(), "query_agent");
    assert_eq!(actors.outermost().as_str(), "orchestrator");
}

#[tokio::test]
async fn the_scopes_a_token_carried_are_parsed_and_read_by_nothing_on_the_request_path() {
    // The claim shape three blocked consumers need. What this test asserts is that it arrives; what it
    // deliberately does NOT assert is that anything acts on it, because nothing does - see
    // `crate::inbound::caller`.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let token = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            RESOURCE,
            ISSUER,
            r#"{"scope":"metrics:read  metrics:read raw:sql"}"#,
        ),
    );
    let caller = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect("a scoped token establishes a caller");
    let scopes = caller.scopes();
    // Two, not three: a repeated scope says the same thing twice, and a doubled separator is an
    // issuer's formatting rather than a caller's error.
    assert_eq!(scopes.count(), 2);
    assert!(scopes.grants("metrics:read"));
    assert!(scopes.grants("raw:sql"));
    assert!(!scopes.grants("metrics:write"));
}

#[test]
fn a_scope_that_could_forge_a_log_line_or_flood_one_does_not_parse() {
    // A scope reaches a count on a log line today and a filter tomorrow, so it is bounded and parsed
    // now rather than when something reads it.
    //
    // A newline is not refused and does not need to be: RFC 6749 delimits scopes by whitespace, so the
    // split removes it and neither resulting scope can carry it. Asserted rather than assumed, because
    // "a newline cannot reach a scope value" is the property and the refusal is not the mechanism.
    let split = Scopes::parse("metrics:read\nsubject=admin").expect("whitespace delimits rather than refuses");
    assert_eq!(split.count(), 2);
    assert!(split.grants("metrics:read") && split.grants("subject=admin"));
    // What IS refused is a character inside one scope: a control character the split does not treat as
    // whitespace, and an invisible or direction-changing code point.
    assert!(matches!(
        Scopes::parse("metrics:\u{7}read"),
        Err(crate::inbound::InvalidScope::NotAScopeToken { position: 8 })
    ));
    assert!(matches!(
        Scopes::parse("metrics:\u{202E}read"),
        Err(crate::inbound::InvalidScope::NotAScopeToken { .. })
    ));
    // And the two RFC 6749 excludes from an otherwise printable set.
    assert!(matches!(
        Scopes::parse("metrics:\"read"),
        Err(crate::inbound::InvalidScope::NotAScopeToken { .. })
    ));
    let long = "a".repeat(129);
    assert!(matches!(
        Scopes::parse(&long),
        Err(crate::inbound::InvalidScope::TooLong { found: 129, limit: 128 })
    ));
    let many: Vec<String> = (0..70).map(|n| format!("scope{n}")).collect();
    assert!(matches!(
        Scopes::parse(&many.join(" ")),
        Err(crate::inbound::InvalidScope::TooMany { limit: 64 })
    ));
    // And the empty claim is a real answer rather than a refusal: an issuer may grant no scopes.
    assert_eq!(Scopes::parse("").expect("no scopes is a scope set").count(), 0);
    assert_eq!(Scopes::none().count(), 0);
}

// --------------------------------------------------- what a token does not establish ----

#[tokio::test]
async fn a_token_minted_for_somebody_elses_resource_is_not_accepted_here() {
    // THE security decision in `docs/adr/0014`. The audience check is ours and unconditional: this
    // token is correctly signed by the right issuer for the right subject, and its audience is
    // somebody else's resource.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let token = signed(
        &pair,
        KID,
        &claims("someone@example.com", "https://elsewhere.example.com", ISSUER, ""),
    );
    let rejected = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("a token for another resource establishes nobody");
    assert!(matches!(rejected, TokenRejected::NotVerified { .. }), "{rejected:?}");
}

#[tokio::test]
async fn a_token_with_no_audience_at_all_is_refused_rather_than_passed_for_want_of_a_claim() {
    // The trap `required_spec_claims` closes, and the reason it is set explicitly: the library
    // validates an audience only when there IS one, so with the default claim set a token carrying no
    // `aud` would satisfy the audience check by having nothing to check.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let token = signed(
        &pair,
        KID,
        &serde_json::json!({ "sub": "someone@example.com", "iss": ISSUER, "exp": expiry_in(3600) }),
    );
    let rejected = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("a token with no audience establishes nobody");
    assert!(matches!(rejected, TokenRejected::NotVerified { .. }), "{rejected:?}");
    // And the same for a token with no subject: a verified caller with no identity is not a caller.
    let token = signed(
        &pair,
        KID,
        &serde_json::json!({ "aud": RESOURCE, "iss": ISSUER, "exp": expiry_in(3600) }),
    );
    assert!(
        gate.establish(&bearer(&token), Instant::now()).await.is_err(),
        "a token with no subject establishes nobody"
    );
}

#[tokio::test]
async fn a_token_from_another_issuer_or_past_its_expiry_establishes_nobody() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let elsewhere = signed(
        &pair,
        KID,
        &claims("someone@example.com", RESOURCE, "https://other-issuer.example.com", ""),
    );
    assert!(
        gate.establish(&bearer(&elsewhere), Instant::now()).await.is_err(),
        "another issuer's token establishes nobody"
    );
    // Expired by more than the leeway, so the assertion is about the expiry rather than about the
    // window. The leeway is a stated value rather than the library's default - see `TokenValidator`.
    assert_eq!(TokenValidator::leeway(), Duration::from_secs(30));
    let expired = signed(
        &pair,
        KID,
        &serde_json::json!({
            "sub": "someone@example.com",
            "aud": RESOURCE,
            "iss": ISSUER,
            "exp": expiry_in(-120),
        }),
    );
    assert!(
        gate.establish(&bearer(&expired), Instant::now()).await.is_err(),
        "an expired token establishes nobody"
    );
}

#[tokio::test]
async fn a_token_signed_by_a_key_the_issuer_never_published_establishes_nobody() {
    // The signature check itself, and the shape that would slip past a validator that trusted the
    // `kid`: the header names the issuer's real key id and the signature is from a key nobody
    // published.
    let published = key_pair();
    let attacker = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &published));
    let forged = signed(&attacker, KID, &claims("admin@example.com", RESOURCE, ISSUER, ""));
    let rejected = gate
        .establish(&bearer(&forged), Instant::now())
        .await
        .expect_err("a signature from an unpublished key establishes nobody");
    assert!(matches!(rejected, TokenRejected::NotVerified { .. }), "{rejected:?}");
}

#[tokio::test]
async fn an_algorithm_the_deployment_did_not_pin_is_refused_even_with_a_key_that_could_verify_it() {
    // Algorithm pinning, at the level a test can reach: the key is the right key, the audience and the
    // issuer are right, and the token says `ES256` where this deployment pinned `ES384`. Nothing reads
    // the algorithm out of the token in order to choose one.
    let pair = key_pair();
    let pinned = direct_pinning(
        "/unread.json",
        PinnedAlgorithms::of(SigningAlgorithm::Es384),
        RequiredTokenType::access_token(),
    );
    let gate = gate_over(&pinned, &jwks(KID, &pair));
    let token = a_token(&pair);
    let rejected = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("an unpinned algorithm establishes nobody");
    assert!(matches!(rejected, TokenRejected::NotVerified { .. }), "{rejected:?}");
    // The same token against the same key set, with `ES256` pinned, does establish one - so the
    // refusal above is about the pinning and not about the fixture.
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    drop(
        gate.establish(&bearer(&token), Instant::now())
            .await
            .expect("the same token with ES256 pinned establishes a caller"),
    );
}

#[tokio::test]
async fn a_token_over_the_size_bound_is_refused_before_anything_reads_it() {
    // Bounded at the edge, because everything before the signature check is work done on behalf of an
    // unauthenticated caller. Asserted by the variant rather than by a timing, so what is pinned is
    // that the check happens first.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let oversized = "a".repeat(MAX_TOKEN_BYTES + 1);
    let rejected = gate
        .establish(&bearer(&oversized), Instant::now())
        .await
        .expect_err("an oversized token establishes nobody");
    assert!(
        matches!(rejected, TokenRejected::TooLong { limit } if limit == MAX_TOKEN_BYTES),
        "{rejected:?}"
    );
    // And an absent header is its own variant, because "no client configured" and "wrong token" are
    // different things to whoever is debugging.
    let rejected = gate
        .establish(&HeaderMap::new(), Instant::now())
        .await
        .expect_err("no header establishes nobody");
    assert!(matches!(rejected, TokenRejected::Absent { .. }), "{rejected:?}");
    // A value with another scheme is a DIFFERENT refusal from an absent header, which it used to
    // share - see `the_bearer_scheme_is_matched_case_insensitively_and_another_scheme_says_so`.
    let mut headers = HeaderMap::new();
    drop(headers.insert("authorization", HeaderValue::from_static("Basic abc")));
    assert!(matches!(
        gate.establish(&headers, Instant::now())
            .await
            .expect_err("another scheme establishes nobody"),
        TokenRejected::NotABearerToken { .. }
    ));
}

#[tokio::test]
async fn a_token_that_names_no_key_is_refused_rather_than_tried_against_every_key() {
    // A decision rather than a limitation: verifying a token with no `kid` means trying every key,
    // which makes an unknown key indistinguishable from a bad signature and makes a rotation invisible.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let header = Header::new(jsonwebtoken::Algorithm::ES256);
    let key = EncodingKey::from_ec_der(&pair.serialize_der());
    let token =
        jsonwebtoken::encode(&header, &claims("someone@example.com", RESOURCE, ISSUER, ""), &key).expect("a test token signs");
    let rejected = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("a token naming no key establishes nobody");
    assert!(matches!(rejected, TokenRejected::NoKeyId), "{rejected:?}");
}

// ------------------------------------------------------- the key set and the refetch ----

#[tokio::test]
async fn a_forged_key_id_costs_one_read_of_the_key_set_and_then_none() {
    // **THE rate limit, SEQUENTIALLY.** `docs/adr/0014`: without it, a forged key id turns every
    // request into an outbound call to the authorization server - a denial-of-service primitive
    // pointed at our own dependency. The fetch count is the assertion, because the count is the
    // primitive.
    //
    // **The name is still true and it is not the whole bound.** This test walks the requests one at a
    // time, so it cannot see whether two callers that arrive together cost one read or two - and
    // review found they cost two. `super::review`'s
    // `two_concurrent_callers_past_the_same_pre_state_perform_exactly_one_read` is that case, and this
    // one is kept rather than replaced because a sequential caller running down the window is what a
    // real forged-id loop looks like.
    let pair = key_pair();
    let document = jwks(KID, &pair);
    let now = Instant::now();
    let (cache, source) = cache_over(&[document], Duration::from_secs(30), now);
    assert_eq!(source.calls.load(Ordering::SeqCst), 1, "priming reads it once");

    // Ten forged ids arriving inside the window, starting immediately. **None of them reaches the
    // source, the first one included**, and that is the window being measured from the last ATTEMPT
    // rather than from the last miss: re-reading a key set that was read a moment ago cannot turn an
    // unknown id into a known one, so the read would be pure cost.
    let forged = KeyId::parse("not-a-key-anybody-published").expect("a test key id is a key id");
    for _ in 0..10_u8 {
        let refused = cache
            .key_for(&forged, now)
            .await
            .map(drop)
            .expect_err("an unknown key is unavailable");
        assert!(matches!(refused, KeyUnavailable::RefetchRateLimited { .. }), "{refused:?}");
    }
    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        1,
        "ten forged requests must not be ten reads of the key set"
    );

    // Past the window, ONE read - and then the window closes again, so the next ten are free too. That
    // is the bound the record asks for: a forged key id costs at most one read per window, whatever
    // the request rate.
    let later = now + Duration::from_secs(31);
    let looked = cache
        .key_for(&forged, later)
        .await
        .map(drop)
        .expect_err("still unknown after a read");
    assert!(matches!(looked, KeyUnavailable::UnknownKeyId), "{looked:?}");
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    for _ in 0..10_u8 {
        drop(cache.key_for(&forged, later).await.map(drop).expect_err("still unknown"));
    }
    assert_eq!(source.calls.load(Ordering::SeqCst), 2, "the window closed behind the read");
    // And a key the set does hold is still served while the window is closed, so the limit costs
    // nothing to a legitimate caller.
    let known = KeyId::parse(KID).expect("a test key id is a key id");
    drop(
        cache
            .key_for(&known, now)
            .await
            .expect("a key the set holds is served while the window is closed"),
    );
}

#[tokio::test]
async fn a_rotated_key_is_picked_up_by_the_refetch_and_the_window_reopens() {
    // The other half of the same mechanism: the refetch has to actually work, or the rate limit is
    // just a deployment that stops authenticating after a rotation.
    let old = key_pair();
    let new = key_pair();
    let now = Instant::now();
    let (cache, source) = cache_over(&[jwks("old-key", &old), jwks("new-key", &new)], Duration::from_millis(1), now);
    let rotated = KeyId::parse("new-key").expect("a test key id is a key id");
    // Inside the window, the new id is refused without a read.
    assert!(matches!(
        cache.key_for(&rotated, now).await.map(drop).expect_err("too soon"),
        KeyUnavailable::RefetchRateLimited { .. }
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    // Past it, the refetch happens and finds the rotated key. `now` is a parameter rather than a clock
    // read, which is why this needs no sleep.
    let later = now + Duration::from_secs(1);
    assert!(
        cache.key_for(&rotated, later).await.is_ok(),
        "the refetch did not pick up the rotated key"
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    let (count, ids) = cache.describe().await;
    assert_eq!(count, 1);
    assert_eq!(ids, vec![String::from("new-key")]);
}

#[tokio::test]
async fn a_key_set_that_cannot_be_read_still_consumes_the_window() {
    // The rate limit stamped on ATTEMPT and not on success, which is the same primitive arriving from
    // the other direction: an issuer that is down would otherwise mean one outbound call per request.
    let pair = key_pair();
    let now = Instant::now();
    let source = Arc::new(StubSource::failing());
    // Priming fails, which is what makes an unreadable key set a refusal to start rather than a
    // deployment that answers 401 to everybody.
    let refused = KeySetCache::primed_with_window(
        Box::new(Arc::clone(&source)),
        KeyFamily::EllipticCurve,
        String::from("ES256"),
        Duration::from_secs(30),
        MAX_KEY_SET_AGE,
        now,
    )
    .expect_err("a source that answers nothing primes no cache");
    assert!(matches!(refused, KeySetUnavailable::Unreadable { .. }), "{refused:?}");

    // Now a cache that primed and whose source then starts failing, which is the deployment case: the
    // stub serves its one document, and every later fetch of an unknown id fails.
    let (cache, source) = cache_over(&[jwks(KID, &pair)], Duration::from_millis(1), now);
    let missing = KeyId::parse("rotated-to-this").expect("a test key id is a key id");
    let later = now + Duration::from_secs(1);
    // The document repeats, so the refetch succeeds and the id is still not in it.
    assert!(matches!(
        cache.key_for(&missing, later).await.map(drop).expect_err("still unknown"),
        KeyUnavailable::UnknownKeyId
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    // And immediately afterwards the window is closed again against the same instant.
    assert!(matches!(
        cache.key_for(&missing, later).await.map(drop).expect_err("too soon"),
        KeyUnavailable::RefetchRateLimited { .. }
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn a_symmetric_key_in_a_key_set_is_refused_at_load() {
    // The second place algorithm confusion dies. A pinned list cannot NAME `HS256` - the config enum
    // has no such variant - and a key set holding an `oct` key would still hand the library a key of
    // the HMAC family whose secret the issuer published. Both halves have to be closed.
    let refused =
        KeySet::parse(&MockIssuer::key_set_of_symmetric_keys()).expect_err("a published shared secret is not a verifying key");
    assert!(matches!(refused, InvalidKeySet::SymmetricKey), "{refused:?}");
    let rendered = refused.to_string();
    assert!(rendered.contains("mint a token"), "{rendered}");
}

#[test]
fn a_key_set_this_deployment_could_not_look_up_is_refused_rather_than_partly_read() {
    // A key with no `kid` cannot be selected by the id a token names, so a deployment reading one would
    // have to fall back to trying every key. Refused, not skipped: a key set that silently lost half
    // its entries would authenticate an arbitrary subset of callers, which reads as an outage.
    let pair = key_pair();
    // The same document `jwks` builds, with the `kid` left out - so what differs from the accepted
    // fixture is exactly the one key this test is about.
    let anonymous = jwks(KID, &pair).replace(&format!(r#""kid":"{KID}","#), "");
    assert!(!anonymous.contains("kid"), "the fixture still names a key: {anonymous}");
    assert!(matches!(
        KeySet::parse(&anonymous).expect_err("a key nothing can name is not usable"),
        InvalidKeySet::KeyWithoutAnId
    ));
    assert!(matches!(
        KeySet::parse("{\"keys\":[]}").expect_err("an empty set verifies nothing"),
        InvalidKeySet::NoUsableKey
    ));
    assert!(matches!(
        KeySet::parse("not json").expect_err("prose is not a key set"),
        InvalidKeySet::NotAJwkSet { .. }
    ));
}

#[test]
fn a_key_id_that_could_forge_a_record_line_does_not_parse() {
    // A `kid` is caller-supplied: it decides whether an outbound read happens and it reaches a log
    // field. So it is bounded and parsed before either.
    assert!(matches!(
        KeyId::parse("kid\nsubject=admin").expect_err("a newline is a second log line"),
        NotAKeyId::NotPrintable { position: 3 }
    ));
    assert!(matches!(
        KeyId::parse("kid\u{202E}reversed").expect_err("a direction override is not printable ASCII"),
        NotAKeyId::NotPrintable { .. }
    ));
    assert_eq!(KeyId::parse("").expect_err("empty is not a key id"), NotAKeyId::Empty);
    let long = "k".repeat(129);
    assert!(matches!(
        KeyId::parse(&long).expect_err("a paragraph is not a key id"),
        NotAKeyId::TooLong { found: 129, limit: 128 }
    ));
    // **Refused rather than trimmed**, which is the same choice `AccessToken` makes about a pasted
    // credential: a `kid` is compared byte for byte against what an issuer published, so trimming here
    // would make a token whose id has a stray space match a key whose id does not - and the two are
    // different strings to whoever wrote them.
    assert!(matches!(
        KeyId::parse(" kid").expect_err("a space is not part of a key id"),
        NotAKeyId::NotPrintable { position: 0 }
    ));
    assert_eq!(KeyId::parse("kid").expect("a plain id is an id").as_str(), "kid");
}

// ----------------------------------------------------------- the file source, once ----

#[tokio::test]
async fn a_key_set_read_off_disk_verifies_a_token_and_a_missing_file_starts_no_gate() {
    // The one test that touches a filesystem, because `FileKeySet` is the only source that ships and a
    // fake of it would be asserting on the fake. The scratch file is named after the process and
    // removed again.
    let pair = key_pair();
    let path = std::env::temp_dir().join(format!("sutura-jwks-{}.json", std::process::id()));
    std::fs::write(&path, jwks(KID, &pair)).expect("the scratch key set is writable");
    let declaration = direct_over(&path.to_string_lossy());
    let gate = InboundGate::from_declaration(&declaration).expect("a readable key set builds a gate");
    let (count, ids) = gate.describe_keys().await;
    assert_eq!(count, 1);
    assert_eq!(ids, vec![String::from(KID)]);
    assert!(
        gate.establish(&bearer(&a_token(&pair)), Instant::now()).await.is_ok(),
        "a key set read off disk verifies a token signed by its key"
    );
    std::fs::remove_file(&path).expect("the scratch key set is removable");

    // And the refusal that keeps a deployment from starting with nothing to verify against.
    let missing = direct_over("/no/such/key/set.json");
    let refused = InboundGate::from_declaration(&missing).expect_err("a missing key set starts no gate");
    // The path is in the cause chain, which is where an operator looks, and the message says what is
    // wrong without it.
    assert!(
        crate::surface::cause_chain(&refused)
            .iter()
            .any(|cause| cause.contains("could not be read")),
        "{:?}",
        crate::surface::cause_chain(&refused)
    );
    // The file source names its path for a startup line, and that is all it exposes.
    assert_eq!(
        FileKeySet::at("/some/where.json").path(),
        std::path::Path::new("/some/where.json")
    );
}

#[tokio::test]
async fn a_gateway_assertion_arrives_in_its_own_header_and_leaves_the_deployment_token_alone() {
    // The `behind-gateway` mode, and the property that makes `docs/adr/0014`'s "both survive" true: the
    // assertion is a SIGNED TOKEN in a header of the component's own, so `Authorization` is still the
    // deployment token's - and it goes through the same signature, issuer, audience and algorithm
    // checks as the direct mode, in the same validator.
    let gateway = key_pair();
    let declaration = gateway_declaring(RequiredTokenType::access_token(), ProofLifetime::default_lifetime());
    let gate = gate_over(&declaration, &jwks(KID, &gateway));
    assert_eq!(gate.header(), TRANSIT_HEADER);

    let headers = transit(&short_lived_assertion(&gateway));
    let caller = gate
        .establish(&headers, Instant::now())
        .await
        .expect("a signed transit proof establishes the caller the component authenticated");
    assert_eq!(caller.chain().subject().established(), "verified");

    // A HEADER SAYING WHO, with no signature, establishes nobody. This is the failure the mode exists
    // to prevent: anything that can reach the port can write a header.
    assert!(
        gate.establish(&transit("admin@example.com"), Instant::now()).await.is_err(),
        "a header holding a name is not an assertion"
    );

    // And the same token in `Authorization` establishes nobody here, because that is not where this
    // mode reads: the deployment bearer token owns it.
    assert!(
        matches!(
            gate.establish(&bearer(&short_lived_assertion(&gateway)), Instant::now())
                .await
                .expect_err("the wrong header establishes nobody"),
            TokenRejected::Absent { .. }
        ),
        "a gateway deployment must not read Authorization"
    );

    // No `Bearer` challenge in this mode, which is a fix: the caller holds no bearer token for this
    // resource, and a challenge telling it to present one is a well-formed instruction it cannot
    // follow - and following it would put a credential in a header this deployment refuses to read.
    assert_eq!(gate.challenge(), None, "a gateway deployment issues no Bearer challenge");
}

#[test]
fn the_subject_a_verified_token_names_is_still_parsed_by_the_domain() {
    // A `sub` claim is signed, which bounds who wrote it and not what they wrote. It is written into an
    // audit record that is one line per call, so the domain's own parse is what refuses a newline -
    // and this asserts that the path goes through it rather than around it.
    let pair = key_pair();
    let document = jwks(KID, &pair);
    let (cache, _) = cache_over(&[document], Duration::from_secs(30), Instant::now());
    let gate = InboundGate::over(&direct(), cache);
    let forged = signed(
        &pair,
        KID,
        &claims("someone@example.com\nsubject=admin", RESOURCE, ISSUER, ""),
    );
    let rejected = block_on(gate.establish(&bearer(&forged), Instant::now()));
    assert!(matches!(rejected, Err(TokenRejected::UnusableSubject { .. })), "{rejected:?}");
}

/// A one-line block-on for the two synchronous tests above, so they do not each need a runtime
/// attribute for one await.
fn block_on<F>(future: F) -> F::Output
where
    F: core::future::Future,
{
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a test runtime builds")
        .block_on(future)
}

#[test]
fn nothing_in_this_module_is_sendable_by_accident() {
    // The bound `axum` needs and reports badly when it is missing: a layer's state must be `Send +
    // Sync`, and the error for a state that is not is an unresolved trait bound on a tuple type. This
    // is the assertion that names it instead.
    fn assert_send_sync<T>()
    where
        T: Send + Sync + 'static,
    {
    }
    assert_send_sync::<InboundGate>();
    assert_send_sync::<KeySetCache>();
    assert_send_sync::<TokenValidator>();
    assert_send_sync::<KeySet>();
    assert_send_sync::<crate::inbound::VerifiedCaller>();
}

/// What review found, as regressions, sharing every fixture above.
///
/// A file of its own for a mechanical reason and not a conceptual one: this one reached the
/// thousand-line limit `cargo xtask max-lines` enforces. The split is where it is because those tests
/// are the ones a reviewer should be able to find, run and delete-if-wrong as a group.
mod review;

/// Leg 1 against an issuer that PUBLISHES its key set, so the rotation bound faces a source that changes.
mod published;
/// Leg 1 through the assembled router, which is what says the layer is installed at all.
mod router;

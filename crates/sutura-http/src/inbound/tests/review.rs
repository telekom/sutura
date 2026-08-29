//! What review found on the first pass at leg 1, as blocking regressions.
//!
//! **Three security findings and the should-fix list from the same review**, each as a test that would
//! have failed against the code as it was. Two of them the reviewer built and ran: a valid ES256 JWT of
//! the wrong class produced a `VerifiedCaller`, and a gateway assertion with no `iat` and an `exp` ten
//! years out produced one twice on a replay of the identical token.
//!
//! Kept here rather than folded into the suite beside them, because a reviewer should be able to find,
//! run and argue with this group as a group - and because `super`'s file reached the thousand-line
//! limit. Every fixture comes from `super`, so nothing here is a second copy of a key, a token or a
//! declaration.
//!
//! **One of these asserts a LIMIT rather than a control**, and that is deliberate:
//! `a_gateway_assertion_with_no_iat_or_an_unbounded_lifetime_establishes_nobody` ends by replaying an
//! accepted assertion twice and expecting both to succeed. Nothing binds an assertion to a request, so
//! the window is what bounds a replay - and a test that pretended otherwise would be the overstatement
//! `docs/adr/0014` was corrected for.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, HeaderValue};
use sutura_config::{KeyFamily, PinnedAlgorithms, ProofLifetime, RequiredTokenType, SigningAlgorithm, TokenType};

use crate::inbound::gate::InboundGate;
use crate::inbound::keys::{
    InvalidKeySet, KeyId, KeySet, KeySetCache, KeySetUnavailable, KeyUnavailable, MAX_KEY_SET_AGE, Refreshed,
};
use crate::inbound::token::{PresentedType, TokenRejected, TokenValidator};

use super::{
    ISSUER, KID, RESOURCE, StubSource, a_token, assertion_living, bearer, cache_of_family, claims, direct, direct_pinning,
    expiry_in, gate_over, gateway_declaring, jwk, jwks, key_pair, rsa_jwks, scheme_and_token, short_lived_assertion, signed,
    signed_as, transit,
};
#[tokio::test]
async fn an_id_token_from_the_same_issuer_for_the_same_audience_establishes_nobody() {
    // **THE cross-JWT substitution regression, from review.** Every check this validator had before the
    // `typ` check passes on this token: the issuer signed it with the published key, `iss` matches,
    // `aud` matches, `sub` and `exp` are there. It is an OIDC ID TOKEN - `typ: JWT`, with `nonce`,
    // `auth_time` and `azp` - and whenever a deployment's resource identifier is also its client id,
    // which is the ordinary identity-provider arrangement, that is the same audience.
    //
    // Review built this and it produced a `VerifiedCaller`. It must not.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let id_token = signed_as(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            RESOURCE,
            ISSUER,
            r#"{"nonce":"n-0S6_WzA2Mj","auth_time":1756400000,"azp":"https://sutura.example.com"}"#,
        ),
        // What an ID token actually carries. RFC 9068 reserves `at+jwt` for an access token.
        Some("JWT"),
    );
    let rejected = gate
        .establish(&bearer(&id_token), Instant::now())
        .await
        .expect_err("an ID token is not an access token, whatever else matches");
    let TokenRejected::WrongTokenType {
        ref required,
        ref presented,
    } = rejected
    else {
        panic!("expected a class refusal, got {rejected:?}");
    };
    assert_eq!(required, TokenType::ACCESS_TOKEN);
    assert_eq!(presented.to_string(), "jwt");

    // A token with NO `typ` is refused too, which is what keeps the check from being satisfiable by
    // omission - an issuer that simply leaves it out must not get the default answer.
    let untyped = signed_as(&pair, KID, &claims("someone@example.com", RESOURCE, ISSUER, ""), None);
    assert!(
        matches!(
            gate.establish(&bearer(&untyped), Instant::now())
                .await
                .expect_err("no class is not the right class"),
            TokenRejected::WrongTokenType {
                presented: PresentedType::Absent,
                ..
            }
        ),
        "a token with no typ must not pass a class check"
    );

    // And the positive side, without which the two above are satisfied by refusing everything: an
    // `at+jwt` from the same issuer for the same audience DOES establish a caller.
    drop(
        gate.establish(&bearer(&a_token(&pair)), Instant::now())
            .await
            .expect("an access token of the right class establishes a caller"),
    );
}

#[tokio::test]
async fn the_class_check_can_be_switched_off_and_only_by_saying_so() {
    // The opt-out review asked for, and the reason it is a word rather than an absence: an issuer using
    // another profile has to be able to deploy, and a deployment that accepts every class has to be
    // visible. `RequiredTokenType::Any` is what an operator writes as `token_type: "any"`.
    let pair = key_pair();
    let permissive = direct_pinning(
        "/unread.json",
        PinnedAlgorithms::of(SigningAlgorithm::Es256),
        RequiredTokenType::Any,
    );
    let gate = gate_over(&permissive, &jwks(KID, &pair));
    for typ in [Some("JWT"), Some("at+jwt"), None] {
        let token = signed_as(&pair, KID, &claims("someone@example.com", RESOURCE, ISSUER, ""), typ);
        drop(
            gate.establish(&bearer(&token), Instant::now())
                .await
                .expect("`any` accepts every class, which is what writing it means"),
        );
    }
    // And the deployment says so on every boot, out of the config type rather than out of a log call.
    assert!(permissive.accepts_any_token_class());
}

#[tokio::test]
async fn a_gateway_assertion_with_no_iat_or_an_unbounded_lifetime_establishes_nobody() {
    // **THE replay-window regression, from review.** An assertion with no `iat` and an `exp` ten years
    // out produced a `VerifiedCaller`, twice, on a replay of the identical token - so `behind-gateway`
    // did not enforce the short-lived property its own record claimed. Two refusals close the window;
    // neither closes the replay INSIDE it, which is stated as a limit rather than claimed as a control.
    let gateway = key_pair();
    let declaration = gateway_declaring(RequiredTokenType::access_token(), ProofLifetime::default_lifetime());
    let gate = gate_over(&declaration, &jwks(KID, &gateway));

    // One: no `iat` at all. There is no lifetime to bound without it, and the library cannot require
    // it - `required_spec_claims` ignores anything but exp, nbf, aud, iss and sub.
    let unbounded = signed(
        &gateway,
        KID,
        &serde_json::json!({
            "sub": "someone@example.com",
            "aud": RESOURCE,
            "iss": ISSUER,
            "exp": expiry_in(10 * 365 * 24 * 3600),
        }),
    );
    assert!(
        matches!(
            gate.establish(&transit(&unbounded), Instant::now())
                .await
                .expect_err("an assertion with no iat establishes nobody"),
            TokenRejected::NoIssuedAt
        ),
        "an assertion this deployment cannot date is not short-lived"
    );

    // Two: an `iat` and an `exp` ten years apart. The lifetime is the component's to choose and the
    // ceiling is this deployment's.
    let decade = assertion_living(&gateway, 0, 10 * 365 * 24 * 3600);
    let rejected = gate
        .establish(&transit(&decade), Instant::now())
        .await
        .expect_err("a ten-year assertion is not short-lived");
    let TokenRejected::LifetimeTooLong { limit, .. } = rejected else {
        panic!("expected a lifetime refusal, got {rejected:?}");
    };
    assert_eq!(limit, ProofLifetime::DEFAULT_SECONDS);

    // Three: an `iat` in the future past the leeway, which would otherwise make the ceiling above
    // meaningless - a component could satisfy it by dating the assertion forward.
    let ahead = assertion_living(&gateway, -600, 60);
    assert!(
        matches!(
            gate.establish(&transit(&ahead), Instant::now())
                .await
                .expect_err("an assertion issued in the future establishes nobody"),
            TokenRejected::IssuedInTheFuture { .. }
        ),
        "a forward-dated iat must not buy a longer window"
    );

    // The positive side: an assertion inside the ceiling establishes a caller - **and replays.** That
    // is the limit review asked to have documented rather than claimed away: nothing binds an assertion
    // to a request and there is no store of what has been seen, so what bounds a replay is the window
    // and only the window.
    let short = short_lived_assertion(&gateway);
    for _ in 0..2_u8 {
        drop(
            gate.establish(&transit(&short), Instant::now())
                .await
                .expect("an assertion inside the ceiling establishes a caller, and replays inside it"),
        );
    }

    // And the ceiling is the deployment's number rather than a constant: a narrower one refuses what the
    // default accepts.
    let narrow = gateway_declaring(
        RequiredTokenType::access_token(),
        ProofLifetime::parse(30).expect("thirty seconds is a lifetime"),
    );
    let narrow = gate_over(&narrow, &jwks(KID, &gateway));
    assert!(
        matches!(
            narrow
                .establish(&transit(&short), Instant::now())
                .await
                .expect_err("sixty seconds is longer than thirty"),
            TokenRejected::LifetimeTooLong { limit: 30, .. }
        ),
        "the ceiling has to be the configured number"
    );
}

#[tokio::test]
async fn a_key_removed_from_the_set_stops_verifying_within_the_age_bound() {
    // **The revocation regression, from review.** A removed key kept verifying until an unrelated
    // unknown key id happened to arrive - and the caller presenting the revoked key presents an id this
    // deployment HAS, so nothing triggered. The age bound is what closes it, and it is asserted through
    // `key_for` rather than through the timer: what matters is that a hit is subject to it.
    let revoked = key_pair();
    let replacement = key_pair();
    let now = Instant::now();
    let (cache, source) = cache_of_family(
        &[jwks(KID, &revoked), jwks("the-next-key", &replacement)],
        KeyFamily::EllipticCurve,
        Duration::from_secs(30),
        Duration::from_millis(1),
        now,
    );
    let id = KeyId::parse(KID).expect("a test key id is a key id");
    // Fresh, so the key is served with no read.
    drop(cache.key_for(&id, now).await.expect("a fresh set serves its own key"));
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);

    // Past the age bound, the same id is re-read and is gone. This is the assertion the old code could
    // not pass: it only ever re-read on a MISS.
    let later = now + Duration::from_secs(1);
    assert!(
        matches!(
            cache.key_for(&id, later).await.map(drop).expect_err("the key was revoked"),
            KeyUnavailable::UnknownKeyId
        ),
        "a revoked key must stop verifying within the age bound"
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 2, "the age bound re-read the set");
    let (count, ids) = cache.describe().await;
    assert_eq!(count, 1);
    assert_eq!(ids, vec![String::from("the-next-key")]);
}

#[tokio::test]
async fn a_key_set_that_stops_being_usable_does_not_replace_one_that_works() {
    // The other half of the age bound, and the trade `crate::tls` already makes: adopting a broken set
    // would turn a rotation mistake into a total outage. Loud, and then carry on with what works.
    let working = key_pair();
    let now = Instant::now();
    let (cache, _) = cache_of_family(
        &[jwks(KID, &working), String::from("{\"keys\":[]}")],
        KeyFamily::EllipticCurve,
        Duration::from_secs(30),
        Duration::from_millis(1),
        now,
    );
    let later = now + Duration::from_secs(1);
    assert_eq!(cache.poll_once(later).await, Refreshed::Rejected);
    let id = KeyId::parse(KID).expect("a test key id is a key id");
    drop(
        cache
            .key_for(&id, later)
            .await
            .expect("the previous keys keep verifying when a candidate is refused"),
    );
    // And identical bytes on the next look are silent rather than a rejection once per interval: the
    // candidate was EXAMINED, which is the distinction `Renewal::seen` already carries.
    assert_eq!(cache.poll_once(later + Duration::from_secs(1)).await, Refreshed::Unchanged);
}

#[test]
fn a_key_set_whose_keys_are_the_wrong_kind_for_the_pinned_algorithms_is_refused_at_load() {
    // The refusal review asked for. The library verifies one token with one key and refuses a permitted
    // list whose family disagrees with it, so an RSA key set under `algorithms: ["ES256"]` cannot verify
    // anything - and every request would be a `401` with nothing in the log connecting the two.
    let now = Instant::now();
    let source = Arc::new(StubSource::serving(&[rsa_jwks("an-rsa-key")]));
    let refused = KeySetCache::primed_with_window(
        Box::new(Arc::clone(&source)),
        KeyFamily::EllipticCurve,
        String::from("ES256"),
        Duration::from_secs(30),
        MAX_KEY_SET_AGE,
        now,
    )
    .expect_err("an RSA key set cannot serve an elliptic-curve pin");
    let KeySetUnavailable::Invalid { ref cause, .. } = refused else {
        panic!("expected an unusable key set, got {refused:?}");
    };
    assert!(matches!(*cause, InvalidKeySet::NoKeyOfThePinnedFamily { .. }), "{cause:?}");
    assert!(refused.to_string().contains("not usable"), "{refused}");

    // And the same document under an RSA pin is fine, so the refusal is about the disagreement rather
    // than about the fixture. `holds` is at-least-one, because an issuer legitimately publishes both.
    let set = KeySet::parse(&rsa_jwks("an-rsa-key")).expect("an RSA JWK is a JWK");
    assert!(set.holds(KeyFamily::Rsa));
    assert!(!set.holds(KeyFamily::EllipticCurve));
}

#[test]
fn two_keys_under_one_id_are_refused_rather_than_last_wins() {
    // Review found a map insert making the last entry silently displace the first, so which key verifies
    // was decided by position in a JSON array - and a rotation performed by appending under the old id
    // would work while the same document written the other way round would not.
    let first = key_pair();
    let second = key_pair();
    let both = format!("{{\"keys\":[{},{}]}}", jwk(KID, &first), jwk(KID, &second));
    let refused = KeySet::parse(&both).expect_err("two keys under one id is not a key set");
    let InvalidKeySet::DuplicateKeyId { ref id } = refused else {
        panic!("expected a duplicate id, got {refused:?}");
    };
    assert_eq!(id.as_str(), KID);
    // Two keys under DIFFERENT ids is the ordinary rotation shape and is accepted.
    let rotating = format!("{{\"keys\":[{},{}]}}", jwk(KID, &first), jwk("the-next-key", &second));
    assert_eq!(KeySet::parse(&rotating).expect("a rotating set is a set").count(), 2);
}

#[tokio::test]
async fn the_bearer_scheme_is_matched_case_insensitively_and_another_scheme_says_so() {
    // RFC 9110 section 11.1 makes an authentication scheme name case-insensitive, so `bearer` is a
    // bearer token - and it was refused, as *absent*, which is the wrong outcome and the wrong
    // diagnostic on top of it.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let token = a_token(&pair);
    for scheme in ["Bearer", "bearer", "BEARER", "BeArEr"] {
        drop(
            gate.establish(&scheme_and_token(scheme, &token), Instant::now())
                .await
                .expect("the scheme name is case-insensitive"),
        );
    }
    // Another scheme is its own refusal rather than "nothing was presented": a client sending `Basic`
    // is a client configured for a different service, and a log saying nothing arrived sends whoever
    // reads it looking for a missing header.
    assert!(matches!(
        gate.establish(&scheme_and_token("Basic", &token), Instant::now())
            .await
            .expect_err("another scheme establishes nobody"),
        TokenRejected::NotABearerToken { .. }
    ));
    // A value with no space at all is the same fact.
    let mut bare = HeaderMap::new();
    drop(bare.insert("authorization", HeaderValue::from_str(&token).expect("a header value")));
    assert!(matches!(
        gate.establish(&bare, Instant::now())
            .await
            .expect_err("a token with no scheme establishes nobody"),
        TokenRejected::NotABearerToken { .. }
    ));
    // And an absent header stays `Absent`, which is what makes the two distinguishable.
    assert!(matches!(
        gate.establish(&HeaderMap::new(), Instant::now())
            .await
            .expect_err("no header establishes nobody"),
        TokenRejected::Absent { .. }
    ));
}

#[tokio::test]
async fn the_two_other_pinnable_curves_verify_end_to_end() {
    // `ES256` carries every other test here, so these two are what stop `map_algorithm` from being
    // asserted only by its own exhaustiveness. `RS*` and `PS*` are NOT covered end to end - see this
    // module's own header for why, and for what covers them instead.
    for (algorithm, pair) in [
        (
            SigningAlgorithm::Es384,
            rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P384_SHA384).expect("a P-384 pair is generatable"),
        ),
        (
            SigningAlgorithm::EdDsa,
            rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519).expect("an Ed25519 pair is generatable"),
        ),
    ] {
        let declaration = direct_pinning(
            "/unread.json",
            PinnedAlgorithms::of(algorithm),
            RequiredTokenType::access_token(),
        );
        let (cache, _) = cache_of_family(
            &[jwks(KID, &pair)],
            algorithm.family(),
            Duration::from_secs(30),
            MAX_KEY_SET_AGE,
            Instant::now(),
        );
        let gate = InboundGate::over(&declaration, cache);
        let caller = gate
            .establish(&bearer(&a_token(&pair)), Instant::now())
            .await
            .unwrap_or_else(|error| panic!("{algorithm} should verify end to end: {error:?}"));
        assert_eq!(caller.chain().subject().established(), "verified");
    }
}

#[tokio::test]
async fn the_fixtures_review_asked_for_the_audience_array_the_future_nbf_and_the_leeway_boundary() {
    // Three one-line cases that were missing, each about a claim shape a real issuer produces.
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));

    // `aud` as an ARRAY, which is what an issuer sends when a token is scoped to several resources. The
    // check is "contains ours", not "equals ours", and nothing asserted that before.
    let several = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            "https://elsewhere.example.com",
            ISSUER,
            &format!(r#"{{"aud":["https://elsewhere.example.com","{RESOURCE}"]}}"#),
        ),
    );
    drop(
        gate.establish(&bearer(&several), Instant::now())
            .await
            .expect("an audience array containing ours is our audience"),
    );
    // And an array that does NOT contain ours is refused, so the above is not "any array passes".
    let neither = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            "https://elsewhere.example.com",
            ISSUER,
            r#"{"aud":["https://elsewhere.example.com","https://third.example.com"]}"#,
        ),
    );
    assert!(
        gate.establish(&bearer(&neither), Instant::now()).await.is_err(),
        "an audience array without ours is not our audience"
    );

    // `nbf` in the future: a token that is not valid YET is not valid. The library defaults this check
    // OFF, which is why `validate_nbf` is set explicitly - and this is what asserts the setting.
    let not_yet = signed(
        &pair,
        KID,
        &serde_json::json!({
            "sub": "someone@example.com",
            "aud": RESOURCE,
            "iss": ISSUER,
            "exp": expiry_in(3600),
            "nbf": expiry_in(600),
        }),
    );
    assert!(
        gate.establish(&bearer(&not_yet), Instant::now()).await.is_err(),
        "a token that is not valid yet establishes nobody"
    );

    // The leeway boundary, from both sides: a token expired by less than the leeway is still accepted,
    // and one expired by more is not. Thirty seconds is a stated value rather than the library's
    // default sixty, so it is worth pinning at the edge rather than in the middle.
    assert_eq!(TokenValidator::leeway(), Duration::from_secs(30));
    let just_inside = signed(
        &pair,
        KID,
        &serde_json::json!({ "sub": "someone@example.com", "aud": RESOURCE, "iss": ISSUER, "exp": expiry_in(-5) }),
    );
    drop(
        gate.establish(&bearer(&just_inside), Instant::now())
            .await
            .expect("five seconds past expiry is inside thirty seconds of leeway"),
    );
    let just_outside = signed(
        &pair,
        KID,
        &serde_json::json!({ "sub": "someone@example.com", "aud": RESOURCE, "iss": ISSUER, "exp": expiry_in(-45) }),
    );
    assert!(
        gate.establish(&bearer(&just_outside), Instant::now()).await.is_err(),
        "forty-five seconds past expiry is outside thirty seconds of leeway"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_callers_past_the_same_pre_state_perform_exactly_one_read() {
    // **The second-round finding, and it broke the specific claim this module makes.** Two callers
    // both observed the old `last_attempt` in `key_for`, each entered `poll_once`, and `poll_once`
    // stamped-and-read unconditionally - so both read the source. Measured at three reads where two
    // were required. The module doc, the sequential test below and `docs/adr/0014` all state a bound
    // of one read per window, so under concurrency the code did not do what three places said.
    //
    // **Why this is deterministic and not a race that happens to lose.** The barrier releases only
    // when the SECOND task arrives, so both are provably past the point `key_for`'s pre-lock
    // comparison sits at before either goes on to `poll_once`. From there the outcome does not depend
    // on the schedule: whichever task takes the write lock first stamps inside the same acquisition it
    // compared in, so the other cannot observe the pre-state. The count is 2 for every interleaving
    // with the reservation, and was 3 for every interleaving without it - which is what makes this a
    // regression rather than a flake in either direction.
    let pair = key_pair();
    let now = Instant::now();
    let (cache, source) = cache_of_family(
        &[jwks(KID, &pair)],
        KeyFamily::EllipticCurve,
        Duration::from_millis(1),
        Duration::from_millis(1),
        now,
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 1, "priming reads it once");
    let cache = Arc::new(cache);
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let later = now + Duration::from_secs(1);

    let mut arrivals = Vec::with_capacity(2);
    for _ in 0..2_u8 {
        let cache = Arc::clone(&cache);
        let barrier = Arc::clone(&barrier);
        arrivals.push(tokio::spawn(async move {
            let _arrived = barrier.wait().await;
            cache.poll_once(later).await
        }));
    }
    let mut outcomes = Vec::with_capacity(2);
    for arrival in arrivals {
        outcomes.push(arrival.await.expect("a test task does not panic"));
    }

    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        2,
        "two concurrent callers must cost ONE read of the key set: {outcomes:?}"
    );
    // And exactly one of them was told it did not look, which is the observable half of the same
    // fact - `Refreshed::NotDue` exists so that this is assertable rather than inferred from a count.
    assert_eq!(
        outcomes.iter().filter(|outcome| **outcome == Refreshed::NotDue).count(),
        1,
        "exactly one caller loses the reservation: {outcomes:?}"
    );
}

#[tokio::test]
async fn the_timer_and_a_caller_cannot_race_into_two_reads_either() {
    // The fourth question review asked. `watch_until_shutdown` calls `poll_once` and so does
    // `key_for`, so the reservation is the same gate for both - there is no second path to fix. What
    // this asserts is that being the same path has the consequence: a look driven by the timer and a
    // look driven by a caller, at the same instant, are one read.
    //
    // The timer's own body is not called here - `poll_once` is what it calls, and asserting that a
    // `tokio::time::sleep` elapses would be asserting that a timer fires rather than that the bound
    // holds. `crate::tls::Renewal` splits the same way and for the same reason.
    let pair = key_pair();
    let now = Instant::now();
    let (cache, source) = cache_of_family(
        &[jwks(KID, &pair), jwks("the-next-key", &pair)],
        KeyFamily::EllipticCurve,
        Duration::from_millis(1),
        Duration::from_millis(1),
        now,
    );
    let id = KeyId::parse(KID).expect("a test key id is a key id");
    let later = now + Duration::from_secs(1);
    // The timer's look happens first and rotates the key away. A caller arriving at the same instant
    // then finds the key gone AND the window closed by the timer's own reservation - so it is refused
    // without reading, which is the property: the two triggers share one gate.
    assert_eq!(cache.poll_once(later).await, Refreshed::Rotated);
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    assert!(
        matches!(
            cache
                .key_for(&id, later)
                .await
                .map(drop)
                .expect_err("the timer rotated that key away"),
            KeyUnavailable::RefetchRateLimited { .. }
        ),
        "the caller must be refused by the timer's reservation rather than take one of its own"
    );
    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        2,
        "the caller must not read again behind the timer"
    );
    // Past the window it does read, and then the id is genuinely unknown rather than merely not looked
    // for - so the refusal above is the reservation and not a permanent answer.
    let even_later = later + Duration::from_secs(1);
    assert!(matches!(
        cache
            .key_for(&id, even_later)
            .await
            .map(drop)
            .expect_err("that key is gone from the set"),
        KeyUnavailable::UnknownKeyId
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 3);
}

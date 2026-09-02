//! Leg 1 against an issuer that **publishes a key set on disk**, driven through the real router.
//!
//! `super` verifies the validator and `super::router` asserts the layer is installed. What neither of
//! them does is put a **source that changes** underneath the verifier: every cache assertion in this
//! module's siblings uses a fake `crate::inbound::keys::KeySetSource` handing out prepared documents,
//! which is the right instrument for counting reads and cannot be the instrument for the one bound
//! whose failure is silent - *a key removed from the published set stops verifying*. A revoked key that
//! keeps verifying looks exactly like a working deployment.
//!
//! So this file uses `sutura_dev::issuer`: a generated key pair per test, a JWK set written to a real
//! path, and `crate::inbound::keys::FileKeySet` reading it - the one source that ships. The gate is the
//! real gate and the router is the real router; the only thing shortened is the two windows, through
//! `crate::inbound::keys::KeySetCache::primed_with_window`, because a test asserting that a window
//! *reopens* would otherwise have to sleep for a minute.
//!
//! # Why the issuer lives in another crate
//!
//! Because leg 1 is verified here, minted-for in a broker and composed in a root, and a fixture inside
//! any one of those three cannot be driven from the other two. `sutura-dev` is the test-infrastructure
//! crate and the mock issuer is a module in it, behind a default-off feature.
//!
//! # What this venue does not answer, stated where somebody reading a green run will see it
//!
//! 1. **Whether a real identity provider will mint an ID token whose `aud` is a third party's client
//!    id.** A mock answers *yes* by construction - the audience is a parameter - so the ID-token
//!    substitution asserted below says that *we* refuse such a token, and nothing about whether one
//!    can be obtained. That question has one venue and it is not this one.
//! 2. **How many times the source was read under concurrency.** A file cannot be counted, so the
//!    "exactly one read per window, whatever the interleaving" bound stays where it is measurable -
//!    `super::review::two_concurrent_callers_past_the_same_pre_state_perform_exactly_one_read`. What
//!    is asserted here is the *observable* half against a real file: a forged key id does not make the
//!    deployment look, and after the window it does.
//!
//! `docs/where-identity-is-proven.md` carries the same split for the whole identity path.

use std::time::{Duration, Instant};

use axum::http::StatusCode;
use sutura_dev::issuer::{Curve, MockIssuer, PublishedKeySet, Token};

use crate::inbound::gate::InboundGate;
use crate::inbound::keys::{FileKeySet, KeySetCache, KeyUnavailable};
use crate::inbound::token::TokenRejected;
use crate::testing::{
    Answered, accepted_by, an_issuer, asked, broker, bundle, declared_inbound, direct_overlay, fake_warehouse, serving,
    settings_with,
};

use super::{KID, direct_over};

/// The two windows every test here runs with, and they are the same value on purpose.
///
/// Both, because `KeySetCache::reserve` takes the *shorter* of the pair as its reservation window - so
/// making them equal keeps a test's arithmetic to one number.
///
/// **A whole second, which is longer than it needs to be and deliberately so.** Two of the tests below
/// assert what happens *inside* the window, and the shipped value is a minute: the flake shape is a
/// loaded machine spending longer between two statements than the window allows, and it fails as
/// *revocation was instant*, which is a confusing thing to debug. The cost of the margin is the two
/// sleeps below, and a sleep in a suite that runs its tests as processes is wall clock nobody waits on.
const WINDOW: Duration = Duration::from_secs(1);

/// Comfortably past [`WINDOW`], for the two tests that want the window reopened.
const PAST_THE_WINDOW: Duration = Duration::from_millis(1400);

/// A router whose gate reads `published`, over the real file source and the shortened windows.
fn app_reading(issuer: &MockIssuer, published: &PublishedKeySet) -> axum::Router {
    let settings = settings_with(&direct_overlay(issuer, &published.path().to_string_lossy()));
    let gate = InboundGate::over(&declared_inbound(&settings), cache_over(published));
    serving(bundle(), fake_warehouse(), broker(), settings, Some(gate))
}

/// A cache over the published file, with both windows shortened to [`WINDOW`].
///
/// `FileKeySet` and not a fake, which is the whole point of this module: what a rotation has to change
/// is the thing the shipped source reads.
fn cache_over(published: &PublishedKeySet) -> KeySetCache {
    let declaration = direct_over(&published.path().to_string_lossy());
    let requirement = declaration.requirement();
    KeySetCache::primed_with_window(
        Box::new(FileKeySet::at(published.path())),
        requirement.algorithms().family(),
        requirement.algorithms().to_string(),
        WINDOW,
        WINDOW,
        Instant::now(),
    )
    .expect("a published key set primes a cache")
}

/// A gate over the published file, for the two tests that assert on the typed refusal rather than on a
/// status code. The gate and not the router, because `TokenRejected` does not survive the `401`.
fn gate_reading(published: &PublishedKeySet) -> InboundGate {
    InboundGate::over(&direct_over(&published.path().to_string_lossy()), cache_over(published))
}

/// One question through the real router, carrying `token` if there is one.
async fn ask(app: &axum::Router, token: Option<&str>) -> Answered {
    asked(app, "POST", "/v1/query", token).await
}

/// A `HeaderMap` carrying a bearer token, for the two tests that drive the gate directly.
fn bearer(token: &str) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    let value = axum::http::HeaderValue::from_str(&format!("Bearer {token}")).expect("a test token is a header value");
    drop(headers.insert("authorization", value));
    headers
}

#[tokio::test]
async fn a_deployment_declaring_inbound_identity_answers_a_caller_it_verified_and_refuses_one_it_did_not() {
    // The composed transport over a key set an issuer PUBLISHED, which is the venue this file adds:
    // the gate read a real file through the source that ships, and the router is the real router.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "accepts").expect("the key set publishes");
    let app = app_reading(&issuer, &published);

    let token = issuer
        .mint(&accepted_by("someone@example.com"))
        .expect("the issuer signs a token");
    let answered = ask(&app, Some(&token)).await;
    assert_eq!(
        answered.status,
        StatusCode::OK,
        "a caller this deployment verified is answered"
    );

    // And a caller it did not: no token at all. A `401` with a challenge naming this deployment's own
    // realm, and NOT saying which check failed - that is the one thing a refused caller must not learn.
    let refused = ask(&app, None).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    let challenge = refused.challenge.expect("a refused caller is told where to look");
    assert!(challenge.contains(issuer.audience()), "{challenge}");
    assert!(!challenge.contains("error_description"), "{challenge}");
}

#[tokio::test]
async fn every_negative_this_issuer_can_mint_is_refused_and_none_of_them_says_which_check_failed() {
    // **The reason the issuer takes every claim as a parameter.** Each of these is the accepted token
    // with exactly one thing moved, so what is asserted is that the named check runs - and the ten
    // together assert the other half, which is that the responses are indistinguishable. An attacker
    // that could tell "your signature is wrong" from "your audience is wrong" has been told which half
    // of a forgery to fix.
    let issuer = an_issuer()
        .also_holding("never-published", Curve::P256)
        .expect("a second key pair generates");
    // Published WITHOUT the second key, so a token naming it is an unknown key id rather than a bad
    // signature. The two failures are different and both belong in this battery.
    let published = PublishedKeySet::of(&issuer, "negatives").expect("the key set publishes");
    published
        .rotate_to(&issuer.key_set_without("never-published"))
        .expect("the key set is republished");
    let app = app_reading(&issuer, &published);

    let good = accepted_by("someone@example.com");
    let mut refused: Vec<String> = Vec::new();
    for (name, token) in [
        (
            "a wrong audience",
            issuer.mint(&good.clone().for_audience("https://someone-else.example.com")),
        ),
        ("no audience at all", issuer.mint(&good.clone().for_nobody_in_particular())),
        (
            "a wrong issuer",
            issuer.mint(&good.clone().claiming_issuer("https://forger.example.com")),
        ),
        ("an expired token", issuer.mint(&good.clone().expired_since(60))),
        ("a token not yet valid", issuer.mint(&good.clone().not_before_in(3600))),
        // RFC 9068's class check, and the regression `docs/adr/0014` records by name: an OpenID Connect
        // ID token from the same issuer, for the same audience, correctly signed - and not an access
        // token. It verified once.
        (
            "an ID token where an access token is required",
            issuer.mint(&good.clone().classed(Token::ID_TOKEN)),
        ),
        ("a token with no class at all", issuer.mint(&good.clone().unclassed())),
        // The oldest JWT defect there is: `alg: none`. A verifier reading the algorithm out of the
        // header it was handed rather than out of what the deployment pinned accepts this.
        ("an unsigned token", issuer.mint_unsigned(&good)),
        // Correct in every other respect - right `kid`, right issuer, right audience - and signed by
        // somebody else. The forgery.
        ("a forged signature", issuer.mint_signed_by_a_stranger(&good)),
        ("an unknown key id", issuer.mint(&good.clone().signed_by("never-published"))),
    ] {
        let token = token.expect("the issuer mints every negative it is asked for");
        let answered = ask(&app, Some(&token)).await;
        assert_eq!(answered.status, StatusCode::UNAUTHORIZED, "{name} established a caller");
        refused.push(answered.challenge.unwrap_or_default());
    }
    // Every refusal said the same thing. Written as a set rather than pairwise so the failure message
    // names how many distinct answers there were.
    let distinct: std::collections::BTreeSet<&String> = refused.iter().collect();
    assert_eq!(distinct.len(), 1, "the refusals are distinguishable: {distinct:?}");
}

#[tokio::test]
async fn a_key_removed_from_the_published_set_stops_verifying_within_the_bound() {
    // **The bound whose failure is silent, against a source that changes.** The sibling test of this
    // name in `super::review` rotates a fake source; this one rewrites the file the shipped source
    // reads, which is what an operator actually does.
    let issuer = an_issuer()
        .also_holding("being-retired", Curve::P256)
        .expect("a second key pair generates");
    let published = PublishedKeySet::of(&issuer, "rotation").expect("the key set publishes");
    let gate = gate_reading(&published);

    let token = issuer
        .mint(&accepted_by("someone@example.com").signed_by("being-retired"))
        .expect("the issuer signs with the key being retired");
    assert!(
        gate.establish(&bearer(&token), Instant::now()).await.is_ok(),
        "a published key verifies"
    );

    published
        .rotate_to(&issuer.key_set_without("being-retired"))
        .expect("the retired key is removed from the published set");

    // **Revocation is bounded, not instant, and asserting the bound means asserting both sides of it.**
    // Inside the age window the cached key still verifies - a caller presenting a revoked key presents
    // an id the deployment holds, so nothing else would trigger a re-read, which is exactly why the age
    // bound exists.
    assert!(
        gate.establish(&bearer(&token), Instant::now()).await.is_ok(),
        "inside the age bound the cached key is still in use - which is what the bound is for"
    );

    tokio::time::sleep(PAST_THE_WINDOW).await;
    let refused = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("past the age bound the retired key verifies nothing");
    assert!(
        matches!(
            refused,
            TokenRejected::NoKey {
                cause: KeyUnavailable::UnknownKeyId
            }
        ),
        "{refused:?}"
    );

    // And the key that stayed published still works, so what was asserted above is a revocation rather
    // than a cache that stopped working.
    let survivor = issuer
        .mint(&accepted_by("someone@example.com").signed_by(KID))
        .expect("the issuer signs with the key that stayed");
    assert!(
        gate.establish(&bearer(&survivor), Instant::now()).await.is_ok(),
        "the key that was not retired still verifies"
    );
}

#[tokio::test]
async fn a_forged_key_id_does_not_make_this_deployment_read_the_published_set() {
    // The rate limit, observed through its EFFECT against a real file rather than by counting reads -
    // see this module's header for why the count stays where it is measurable. The observable form is
    // the useful one anyway: what the bound buys is that a caller cannot turn every request into a
    // source read, and what that looks like from outside is a deployment that does not notice a file
    // it was not due to look at.
    let issuer = an_issuer()
        .also_holding("rotated-in", Curve::P256)
        .expect("a second key pair generates");
    let published = PublishedKeySet::of(&issuer, "ratelimit").expect("the key set publishes");
    published
        .rotate_to(&issuer.key_set_without("rotated-in"))
        .expect("the key set is republished without the new key");
    let gate = gate_reading(&published);

    // The key arrives in the published document immediately after priming, so the only thing standing
    // between the caller and a successful verification is the window.
    published.rotate_to(&issuer.key_set()).expect("the new key is published");
    let token = issuer
        .mint(&accepted_by("someone@example.com").signed_by("rotated-in"))
        .expect("the issuer signs with the key just published");

    let refused = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("inside the window the deployment does not look");
    assert!(
        matches!(
            refused,
            TokenRejected::NoKey {
                cause: KeyUnavailable::RefetchRateLimited { .. }
            }
        ),
        "{refused:?}"
    );

    // Past the window it looks once, finds the key, and the same token verifies. Which is the other
    // half: a bound that never reopened would be an outage on every rotation.
    tokio::time::sleep(PAST_THE_WINDOW).await;
    assert!(
        gate.establish(&bearer(&token), Instant::now()).await.is_ok(),
        "past the window the newly published key is picked up"
    );
}

#[tokio::test]
async fn all_three_curves_this_issuer_can_generate_verify_off_a_published_set() {
    // **The test that makes the fixture trustworthy rather than the code under test.** The issuer writes
    // a JWK by hand, and the two shapes it can get wrong are silent: an `EC` key whose `x` and `y` are
    // the wrong spans of one point, and an `OKP` key written as if it had a `y`. A suite that only ever
    // used `P-256` would carry two untested branches of a fixture every other test here depends on.
    //
    // `RS*` and `PS*` are absent and stay absent - see this module's header and the issuer's own. Three
    // of nine, said out loud.
    for (curve, algorithm) in [
        (Curve::P256, sutura_config::SigningAlgorithm::Es256),
        (Curve::P384, sutura_config::SigningAlgorithm::Es384),
        (Curve::Ed25519, sutura_config::SigningAlgorithm::EdDsa),
    ] {
        let issuer = an_issuer()
            .also_holding("on-the-curve", curve)
            .expect("a key pair generates on every curve this issuer offers");
        let published = PublishedKeySet::of(&issuer, curve.algorithm()).expect("the key set publishes");
        // Only the key under test, so a set holding a `P-256` key as well could not verify the token by
        // accident - which is exactly what the pinned family below would otherwise allow.
        published
            .rotate_to(&issuer.key_set_without(KID))
            .expect("the key set is republished with one key");

        let declaration = super::direct_pinning(
            &published.path().to_string_lossy(),
            sutura_config::PinnedAlgorithms::of(algorithm),
            sutura_config::RequiredTokenType::access_token(),
        );
        let gate = InboundGate::from_declaration(&declaration)
            .unwrap_or_else(|error| panic!("{algorithm} should build a gate off a published set: {error:?}"));
        let token = issuer
            .mint(&accepted_by("someone@example.com").signed_by("on-the-curve"))
            .expect("the issuer signs on this curve");
        let caller = gate
            .establish(&bearer(&token), Instant::now())
            .await
            .unwrap_or_else(|error| panic!("{algorithm} should verify off a published set: {error:?}"));
        assert_eq!(caller.chain().subject().established(), "verified");
    }
}

#[tokio::test]
async fn a_published_set_this_deployment_cannot_use_starts_no_gate_at_all() {
    // Two documents that are well formed JWK sets and still unusable for a deployment pinning `ES256`:
    // one holding a SYMMETRIC key, which is algorithm confusion waiting to happen because the holder of
    // a published key could sign with it, and one holding an RSA key, which is the wrong family. Both
    // are refused while the gate is BUILT - before a listener opens - rather than answering `401` to
    // everybody while the startup log says this deployment establishes a caller identity.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "unusable").expect("the key set publishes");
    for (name, document) in [
        ("a symmetric key", MockIssuer::key_set_of_symmetric_keys()),
        ("an RSA key", MockIssuer::key_set_of_rsa_keys(KID)),
        ("one key under two entries", issuer.key_set_naming_one_key_twice(KID)),
    ] {
        published.rotate_to(&document).expect("the document publishes");
        let declaration = direct_over(&published.path().to_string_lossy());
        assert!(InboundGate::from_declaration(&declaration).is_err(), "{name} started a gate");
    }

    // And the control that makes the three above mean something: the issuer's own set does start one.
    published.rotate_to(&issuer.key_set()).expect("the good set publishes");
    let declaration = direct_over(&published.path().to_string_lossy());
    let gate = InboundGate::from_declaration(&declaration).expect("a usable published set starts a gate");
    let (count, ids) = gate.describe_keys().await;
    // Compared against what the ISSUER published rather than against a literal, so the assertion is
    // that the gate adopted the set it was pointed at rather than that both happen to spell one name.
    assert_eq!(count, issuer.key_ids().len());
    assert_eq!(ids, issuer.key_ids());
}

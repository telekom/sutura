//! Deadline and cache-age regressions over the client-credentials bearer source.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use sutura_domain::identity::Secret;

use super::{Bearer, ClientCredentials, Minted, Scopes, TokenUnavailable};
use crate::delegation::{ExchangeClient, TokenEndpoint};
use crate::test_support::{FakeServer, Scripted};
use crate::{Budget, Endpoint, ReadBounds};

fn grant_at(server: &FakeServer) -> ClientCredentials {
    let endpoint = server.endpoint();
    let bounds = ReadBounds::parse(5, 1024).expect("nonzero bounds");
    ClientCredentials::new(
        &Endpoint::parse(&endpoint).expect("loopback origin"),
        TokenEndpoint::parse(&format!("{endpoint}/token")).expect("loopback token endpoint"),
        ExchangeClient::new("catalog-reader", Secret::new("client-secret")).expect("client id"),
        Scopes::default(),
        crate::fixed(bounds, None),
    )
    .expect("same origin")
}

fn cached(grant: &ClientCredentials) {
    *grant.minted.lock() = Some(Minted {
        bearer: Bearer(Secret::new("cached-token")),
        fresh_until: Instant::now() + Duration::from_secs(60),
    });
}

#[test]
fn a_spent_budget_cannot_reuse_a_cached_client_token() {
    let server = FakeServer::start(Vec::new());
    let grant = grant_at(&server);
    cached(&grant);
    assert!(matches!(
        grant.bearer(Budget::opened(Duration::ZERO)),
        Err(TokenUnavailable::DeadlineSpent)
    ));
    assert_eq!(server.finish().len(), 0);
}

#[test]
fn waiting_for_another_token_mint_is_bounded_by_the_read_budget() {
    let server = FakeServer::start(Vec::new());
    let grant = grant_at(&server);
    cached(&grant);
    thread::scope(|scope| {
        let held = grant.minted.lock();
        let (sent, received) = mpsc::channel();
        let grant = &grant;
        let worker = scope.spawn(move || {
            let answer = grant.bearer(Budget::opened(Duration::from_millis(20)));
            sent.send(answer).expect("the result receiver remains alive");
        });
        let answer = received.recv_timeout(Duration::from_secs(1));
        drop(held);
        worker.join().expect("the token waiter finishes");
        assert!(matches!(answer, Ok(Err(TokenUnavailable::DeadlineSpent))), "{answer:?}");
    });
    assert_eq!(server.finish().len(), 0);
}

#[test]
fn minting_time_is_counted_against_the_cached_tokens_lifetime() {
    let token = |value: &str| {
        serde_json::json!({
            "access_token": value,
            "token_type": "Bearer",
            "expires_in": 61,
        })
    };
    let server = FakeServer::start(vec![
        Scripted::delayed(&token("first-token"), Duration::from_millis(1200)),
        Scripted::ok(&token("second-token")),
    ]);
    let grant = grant_at(&server);
    let first = grant.bearer(Budget::opened(Duration::from_secs(5))).expect("first mint");
    let second = grant.bearer(Budget::opened(Duration::from_secs(5))).expect("second mint");
    assert_eq!(first.header(), "Bearer first-token");
    assert_eq!(second.header(), "Bearer second-token");
    assert_eq!(server.finish().len(), 2);
}

#[test]
fn a_token_answer_after_the_read_budget_is_refused() {
    let server = FakeServer::start(vec![Scripted::delayed(
        &serde_json::json!({ "access_token": "late-token", "token_type": "Bearer", "expires_in": 3600 }),
        Duration::from_millis(100),
    )]);
    let grant = grant_at(&server);
    let answer = grant.bearer(Budget::opened(Duration::from_millis(20)));
    assert!(matches!(answer, Err(TokenUnavailable::DeadlineSpent)), "{answer:?}");
    assert_eq!(server.finish().len(), 1);
}

//! A credential a started binary is given or shown, looked for in everything the process answers and
//! writes (`telekom/sutura#1398`).
//!
//! The router tests in `sutura-http` drive the router with `oneshot`, so no connection layer and
//! none of its logging is read; the source-password cells here send no inbound token. These cells
//! start the binary at `RUST_LOG=trace`, present a deployment token or a caller token that is
//! accepted and ones that are refused, and look for any twelve characters of each in a row in the
//! status line, headers and body of every reply and in every line either stream carried.
//!
//! An absence proves nothing unless the capture is live, so a cell shows a `trace` line was
//! written, and that the accepted credential was answered.
//!
//! **The limit:** the scan reads what the process wrote until it stopped; a line written after
//! `SIGTERM` was delivered and before exit is read, one the process never flushed is not.

#[cfg(any(feature = "cube", feature = "datahub", feature = "openmetadata"))]
use sutura_config::Environment;
use sutura_dev::issuer::{PublishedKeySet, Token};
#[cfg(any(feature = "cube", feature = "datahub", feature = "openmetadata"))]
use sutura_http_client::test_support::{FakeServer, Scripted};

use crate::harness::{
    Reply, accepted_by, an_issuer, example_root, recurring_revenue_june, settings_crediting, settings_declaring_inbound,
    start_configured_with_environment, v1,
};
#[cfg(any(feature = "cube", feature = "datahub", feature = "openmetadata"))]
use crate::harness::{TOKEN, refused_to_start, written};

#[cfg(feature = "cube")]
#[path = "cube_token.rs"]
mod cube_token;

/// The subscriber reads `RUST_LOG` before the settings' filter, so this makes every line `trace`.
pub(crate) const TRACE: [(&str, &str); 1] = [("RUST_LOG", "trace")];

/// The fewest characters of a secret in a row that count as finding it: a line that wraps or chunks
/// what it writes still carries this many of a 24-character secret in one piece.
const RUN: usize = 12;

/// A `trace` event as the bunyan formatter writes its level.
const TRACE_LEVEL: &str = r#""level":10"#;

/// The name of each of `secrets` that appears, [`RUN`] characters in a row, in a reply or a line;
/// names, so a failure does not print the secret. Fails if `lines` holds no `trace` event.
///
/// Returned rather than asserted, so the cell holds its own refusal and a mutation fails the cell at
/// its own assertion.
pub(crate) fn echoed(secrets: &[(&'static str, &str)], replies: &[&Reply], lines: &[String]) -> Vec<&'static str> {
    assert!(
        lines.iter().any(|line| line.contains(TRACE_LEVEL)),
        "no `trace` line was captured, so the capture is not a record of everything logged"
    );
    secrets
        .iter()
        .filter(|&&(_, secret)| {
            (0..=secret.len().saturating_sub(RUN))
                .filter_map(|at| secret.get(at..at + RUN))
                .any(|run| {
                    lines.iter().any(|line| line.contains(run))
                        || replies
                            .iter()
                            .any(|reply| reply.head.contains(run) || reply.body.contains(run))
                })
        })
        .map(|&(name, _)| name)
        .collect()
}

/// A catalog reader's bearer `token`, sent to a fake that answers `accepted` and then to one that
/// refuses it with a body naming it, and looked for in the listing the first deployment serves and in
/// every line both processes wrote.
///
/// `settings` declares the deployment over the fake it is given, reading `token` from its token
/// file. Each fake must have been sent `token` as a bearer, or the absence below proves nothing.
#[cfg(any(feature = "cube", feature = "datahub", feature = "openmetadata"))]
pub(crate) fn bearer_echoed(
    case: &str,
    token: &str,
    accepted: Vec<Scripted>,
    settings: impl Fn(&FakeServer) -> String,
) -> Vec<&'static str> {
    let accepting = FakeServer::start(accepted);
    let mut running = start_configured_with_environment(case, &settings(&accepting), None, &TRACE, &[]);
    let listing = running.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
    running.terminate();
    let mut lines = running.log();
    assert_eq!(listing.status, 200, "the accepted catalog must be served: {}", listing.body);

    let refusing = FakeServer::start(vec![Scripted::status(
        401,
        &format!(r#"{{"message":"unauthorized {token}"}}"#),
    )]);
    let refused = refused_to_start(
        Environment::Development,
        written(&format!("{case}-refused"), &settings(&refusing)),
        &TRACE,
    );
    lines.extend(refused);

    let bearer = format!("Bearer {token}");
    for (fake, sent) in [("accepting", accepting.finish()), ("refusing", refusing.finish())] {
        assert!(
            sent.iter().any(|request| request.authorization() == Some(bearer.as_str())),
            "the {fake} fake was never sent the token, so its absence proves nothing: {sent:?}"
        );
    }
    echoed(&[("the catalog token", token)], &[&listing], &lines)
}

/// The payload and signature of a signed token: the part no log has a reason to carry, unlike the
/// header every token of this issuer shares.
fn signed_part(token: &str) -> &str {
    token.split_once('.').map_or(token, |(_, rest)| rest)
}

#[test]
fn a_deployment_token_reaches_no_reply_and_no_log_line_of_a_started_binary() {
    const DEPLOYMENT: &str = "3f9a1c77d0e24b68a5c2e9d1f6b30874c1d9e5a2";
    const REFUSED: &str = "8d12c5e0a97b4f31ce6b07a4d95e1f3820c6b7a9";

    let mut served = start_configured_with_environment(
        "no-echo-deployment-token",
        &settings_crediting(&example_root(), &format!("  access_token: \"{DEPLOYMENT}\"\n")),
        None,
        &TRACE,
        &[],
    );
    let question = recurring_revenue_june();
    let query = v1(sutura_http::constants::base_paths::QUERY);
    let accepted = served.post(&query, Some(DEPLOYMENT), &question);
    let refused = served.post(&query, Some(REFUSED), &question);
    let listing = served.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(REFUSED));
    served.terminate();
    let lines = served.log();

    assert_eq!(
        accepted.status, 200,
        "the deployment token must be accepted: {}",
        accepted.body
    );
    assert!(
        refused.status == 401 && listing.status == 401,
        "a token that is not the deployment's must be refused: {} {}",
        refused.status,
        listing.status
    );
    let leaked = echoed(
        &[("the deployment token", DEPLOYMENT), ("the refused token", REFUSED)],
        &[&accepted, &refused, &listing],
        &lines,
    );
    assert!(leaked.is_empty(), "a credential was sent back or logged: {leaked:?}");
}

#[test]
fn a_caller_token_reaches_no_reply_and_no_log_line_of_a_started_binary() {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "no-echo-caller-token").expect("the key set publishes");
    let mut served = start_configured_with_environment(
        "no-echo-caller-token",
        &settings_declaring_inbound(&example_root(), &issuer, published.path()),
        None,
        &TRACE,
        &[],
    );
    let accepted_token = issuer
        .mint(&accepted_by("caller-a@example.com"))
        .expect("the issuer mints an accepted token");
    let expired_token = issuer
        .mint(&Token::for_subject("caller-b@example.com").expired_since(3600))
        .expect("the issuer mints an expired token");
    let stranger_token = issuer
        .mint_signed_by_a_stranger(&Token::for_subject("caller-c@example.com"))
        .expect("a stranger signs a token");

    let question = recurring_revenue_june();
    let query = v1(sutura_http::constants::base_paths::QUERY);
    let accepted = served.post(&query, Some(&accepted_token), &question);
    let expired = served.post(&query, Some(&expired_token), &question);
    let stranger = served.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(&stranger_token));
    served.terminate();
    let lines = served.log();

    assert_eq!(
        accepted.status, 200,
        "the accepted caller must be answered: {}",
        accepted.body
    );
    assert!(
        expired.status == 401 && stranger.status == 401,
        "an expired or forged token must be refused: {} {}",
        expired.status,
        stranger.status
    );
    let leaked = echoed(
        &[
            ("the accepted token", signed_part(&accepted_token)),
            ("the expired token", signed_part(&expired_token)),
            ("the forged token", signed_part(&stranger_token)),
        ],
        &[&accepted, &expired, &stranger],
        &lines,
    );
    assert!(leaked.is_empty(), "a credential was sent back or logged: {leaked:?}");
}

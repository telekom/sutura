//! A loopback bind that declares callers from beyond this host. `/v1` and `/metrics` ask the same
//! off-host predicate `/mcp` asks, so a declared proxy hop, TLS terminator or host name needs the
//! deployment token, the metrics token and the limiter, whichever surface is mounted.
//!
//! Each cell carries its guarded twin, so a refusal cannot be satisfied by refusing every deployment
//! that declares the key.

use super::{METRICS_TOKEN, TOKEN};
use crate::proxy::ClientAddressSource;
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources, TokenRequiredBy};

const PROXY: &str = "  client_address: \"forwarded\"\n  trusted_proxies: [\"127.0.0.1\"]\n";

/// What an unguarded off-host deployment with no agent surface is refused with, in order.
const UNGUARDED: [NotFitToServe; 3] = [
    NotFitToServe::AccessTokenRequired {
        because: TokenRequiredBy::OffHost,
    },
    NotFitToServe::MetricsTokenRequired {
        because: TokenRequiredBy::OffHost,
    },
    NotFitToServe::RateLimitingDisabled {
        because: TokenRequiredBy::OffHost,
    },
];

/// What `overlay` is refused with, or nothing where it serves.
fn refusals(overlay: &str) -> Vec<NotFitToServe> {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .err()
        .map_or_else(Vec::new, |error| {
            let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
                panic!("expected a posture refusal, got {error:?}");
            };
            refusals.clone()
        })
}

/// `name:` over `lines`, or nothing where there are none.
fn group(name: &str, lines: &str) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{name}:\n{lines}")
    }
}

/// A loopback deployment with `server:`, `security:` and `rate_limit:` lines of the caller's choosing,
/// with or without the three guards.
fn loopback(server: &str, security: &str, rate_limit: &str, guarded: bool) -> String {
    let (tokens, limiter) = if guarded {
        (
            format!("  access_token: \"{TOKEN}\"\n  metrics_token: \"{METRICS_TOKEN}\"\n"),
            "  enabled: true\n",
        )
    } else {
        (String::new(), "")
    };
    format!(
        "{}{}{}",
        group("server", server),
        group("security", &format!("{tokens}{security}")),
        group("rate_limit", &format!("{limiter}{rate_limit}"))
    )
}

/// Refused with exactly `UNGUARDED` bare, and served with the three guards.
fn off_host(server: &str, security: &str, rate_limit: &str) {
    let bare = loopback(server, security, rate_limit, false);
    assert_eq!(refusals(&bare), UNGUARDED, "{bare}");
    let guarded = loopback(server, security, rate_limit, true);
    assert_eq!(refusals(&guarded), Vec::new(), "{guarded}");
}

#[test]
fn a_loopback_api_behind_a_declared_proxy_needs_the_tokens_and_the_limiter() {
    off_host("", "", PROXY);
    let refused = refusals(&loopback("", "", PROXY, false));
    let rendered = refused.first().expect("the token refusal").to_string();
    assert!(rendered.contains("rate_limit.trusted_proxies"), "{rendered}");
}

#[test]
fn a_loopback_api_behind_a_declared_tls_terminator_needs_the_tokens_and_the_limiter() {
    for terminator in ["sidecar", "ingress"] {
        off_host("", &format!("  tls_termination: \"{terminator}\"\n"), "");
    }
    // `none` names no terminator, so a loopback bind that says it is still this host only.
    assert_eq!(
        refusals(&loopback("", "  tls_termination: \"none\"\n", "", false)),
        Vec::new()
    );
}

#[test]
fn a_loopback_api_answering_a_host_name_needs_the_tokens_and_the_limiter() {
    // A loopback address beside a name does not excuse the name, and `localhost` is a name.
    for hosts in [
        r#""sutura.example.com""#,
        r#""localhost""#,
        r#""127.0.0.1", "sutura.example.com""#,
    ] {
        off_host(&format!("  allowed_hosts: [{hosts}]\n"), "", "");
    }
    assert_eq!(
        refusals(&loopback("  allowed_hosts: [\"127.0.0.1\", \"::1\"]\n", "", "", false)),
        Vec::new()
    );
}

#[test]
fn a_trusted_proxy_list_that_nothing_reads_is_refused() {
    // The other direction, and it is refused for the reason every unknown key here is an error: a
    // list that has no effect reads as a control that is in place. The declared hop is still
    // off-host, so the three off-host guards are asked for too.
    let unread = NotFitToServe::TrustedProxiesWithoutForwarding { count: 2 };
    assert_eq!(
        refusals("rate_limit:\n  trusted_proxies:\n    - \"10.0.0.0/8\"\n    - \"10.1.0.0/16\"\n"),
        [vec![unread], UNGUARDED.to_vec()].concat()
    );
}

#[test]
fn forwarded_keying_with_a_named_hop_loads() {
    // The positive case, without which `settings::tests::a_forwarded_header_is_not_believed_without_a_named_hop`
    // and the refusal above are satisfied by refusing every forwarded configuration there is. The
    // hop forwards callers from beyond this host, so it loads behind the three off-host guards.
    let hop = "  client_address: \"forwarded\"\n  trusted_proxies:\n    - \"10.0.0.0/8\"\n";
    off_host("", "", hop);
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(loopback("", "", hop, true)))
        .expect("a named hop and a header source load");
    assert_eq!(settings.rate_limit().client_address(), ClientAddressSource::Forwarded);
    assert!(
        settings
            .rate_limit()
            .trusted_proxies()
            .trusts("10.1.2.3".parse().expect("a test address is an address"))
    );
}

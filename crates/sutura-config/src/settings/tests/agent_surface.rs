//! `server.agent_surface.enabled` with no `security.inbound`: the two boot refusals
//! `Settings::agent_surface_refusals` returns, and the deployments that serve `/mcp` without leg 1.
//!
//! A file of its own because `settings/tests.rs` is at the thousand-line limit `cargo xtask
//! max-lines` enforces. Each cell carries its own serving twin, so a refusal cannot be satisfied by
//! refusing every agent surface.

use sutura_domain::model::SourceName;

use super::inbound::DIRECT_INBOUND;
use super::{METRICS_TOKEN, TOKEN};
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources, TokenRequiredBy};

const AGENT: &str = "server:\n  agent_surface:\n    enabled: true\n";
const LIMITED: &str = "rate_limit:\n  enabled: true\n";
const SINGLE_USER: &str = "  identity: \"single-user\"\n  single_user_because: \"one operator, their own files\"\n";

/// The settings an overlay loads to.
fn serves(overlay: &str) -> Settings {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect("this deployment is fit to serve")
}

/// The posture refusals an overlay is refused with.
fn refused(overlay: &str) -> Vec<NotFitToServe> {
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect_err("this deployment is not fit to serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    refusals.clone()
}

/// An off-host single-user agent deployment, with `security:` lines and a `rate_limit:` group of the
/// caller's choosing.
fn off_host(security: &str, rate_limit: &str) -> String {
    format!(
        "server:\n  host: \"0.0.0.0\"\n  agent_surface:\n    enabled: true\nsecurity:\n{SINGLE_USER}  \
         tls_termination: \"ingress\"\n  metrics_token: \"{METRICS_TOKEN}\"\n{security}{rate_limit}"
    )
}

/// A loopback single-user agent deployment, with `server:` lines, `security:` lines and further
/// groups of the caller's choosing.
fn loopback(server: &str, security: &str, rest: &str) -> String {
    format!("server:\n{server}  agent_surface:\n    enabled: true\nsecurity:\n{SINGLE_USER}{security}{rest}")
}

/// Two sources: `local` runs as the asking subject, `shared` as one identity.
fn two_sources() -> &'static str {
    "sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    \
     posture: \"impersonation-at-source\"\n    workload_identity:\n      \
     audience: \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n  shared:\n    kind: \"files\"\n    \
     data_dir: \"/srv/sutura/shared\"\n    posture: \"shared-service-user\"\n"
}

#[test]
fn an_agent_surface_with_no_inbound_identity_in_multi_user_mode_is_not_fit_to_serve() {
    let refusals = refused(&format!("{AGENT}security:\n  identity: \"multi-user\"\n"));
    assert_eq!(refusals, vec![NotFitToServe::AgentSurfaceWithoutInboundIdentity]);
    let rendered = refusals.first().expect("one refusal").to_string();
    for key in ["server.agent_surface.enabled", "security.inbound", "single-user"] {
        assert!(rendered.contains(key), "{key} missing from: {rendered}");
    }
    // Leg 1 declared, the same multi-user deployment serves.
    serves(&format!("{AGENT}security:\n  identity: \"multi-user\"\n{DIRECT_INBOUND}"));
}

#[test]
fn an_agent_surface_with_no_inbound_identity_and_no_declared_mode_is_not_fit_to_serve() {
    // No mode is not single-user: a deployment that says nothing keeps the rule.
    assert_eq!(refused(AGENT), vec![NotFitToServe::AgentSurfaceWithoutInboundIdentity]);
    // And single-user is reached only through its written reason.
    let error = Settings::load(
        &Sources::defaults(Environment::Development).with_overlay(format!("{AGENT}security:\n  identity: \"single-user\"\n")),
    )
    .expect_err("single-user with no reason is not a mode");
    assert!(matches!(*error.reason(), SettingsError::Identity { .. }), "{error:?}");
    let settings = serves(&format!("{AGENT}security:\n{SINGLE_USER}"));
    assert_eq!(settings.agent_surface_refusals(), Vec::new());
}

#[test]
fn an_off_host_agent_surface_with_no_inbound_identity_needs_the_token_and_the_limiter() {
    let token = format!("  access_token: \"{TOKEN}\"\n");
    assert_eq!(
        refused(&off_host(&token, "rate_limit:\n  enabled: false\n")),
        vec![
            NotFitToServe::AgentSurfaceWithoutInboundIdentity,
            NotFitToServe::RateLimitingDisabled {
                because: TokenRequiredBy::OffHost
            },
        ]
    );
    assert_eq!(
        refused(&off_host("", LIMITED)),
        vec![
            NotFitToServe::AccessTokenRequired {
                because: TokenRequiredBy::OffHost
            },
            NotFitToServe::AgentSurfaceWithoutInboundIdentity,
        ]
    );
    serves(&off_host(&token, LIMITED));
}

#[test]
fn an_agent_surface_over_an_impersonating_source_with_no_inbound_identity_is_not_fit_to_serve() {
    let security = format!("{AGENT}security:\n{SINGLE_USER}");
    let refusals = refused(&format!("{security}{}", two_sources()));
    // Per source, and only the one that runs as the asker.
    assert_eq!(
        refusals,
        vec![NotFitToServe::AgentSurfaceOverAnImpersonatingSource {
            alias: SourceName::parse("local").expect("a legal identifier")
        }]
    );
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("source `local` is `impersonation-at-source`"), "{rendered}");
    serves(&format!("{security}{DIRECT_INBOUND}{}", two_sources()));
}

#[test]
fn a_loopback_agent_surface_behind_a_declared_proxy_counts_as_off_host() {
    let proxy = "  client_address: \"forwarded\"\n  trusted_proxies: [\"127.0.0.1\"]\n";
    let refusals = refused(&loopback("", "", &format!("rate_limit:\n{proxy}")));
    assert_eq!(refusals, vec![NotFitToServe::AgentSurfaceWithoutInboundIdentity]);
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("rate_limit.trusted_proxies"), "{rendered}");
    // Off-host is met as off-host is: the deployment token and the limiter.
    serves(&loopback(
        "",
        &format!("  access_token: \"{TOKEN}\"\n"),
        &format!("rate_limit:\n  enabled: true\n{proxy}"),
    ));
}

#[test]
fn a_loopback_agent_surface_answering_a_host_name_counts_as_off_host() {
    // Every entry is read: a loopback address does not excuse a name before it, after it or between two.
    for hosts in [
        r#""sutura.example.com""#,
        r#""localhost""#,
        r#""10.0.0.5""#,
        r#""127.0.0.1", "sutura.example.com""#,
        r#""sutura.example.com", "127.0.0.1""#,
        r#""127.0.0.1", "sutura.example.com", "::1""#,
    ] {
        let named = format!("  allowed_hosts: [{hosts}]\n");
        let refusals = refused(&loopback(&named, "", ""));
        assert_eq!(refusals, vec![NotFitToServe::AgentSurfaceWithoutInboundIdentity], "{hosts}");
        let rendered = refusals.first().expect("one refusal").to_string();
        assert!(rendered.contains("server.allowed_hosts"), "{rendered}");
        serves(&loopback(&named, &format!("  access_token: \"{TOKEN}\"\n"), LIMITED));
    }
    // A loopback address names no caller from beyond this host.
    serves(&loopback("  allowed_hosts: [\"127.0.0.1\", \"::1\"]\n", "", ""));
}

#[test]
fn a_loopback_agent_surface_behind_a_declared_tls_terminator_counts_as_off_host() {
    for terminator in ["sidecar", "ingress"] {
        let declared = format!("  tls_termination: \"{terminator}\"\n");
        let refusals = refused(&loopback("", &declared, ""));
        assert_eq!(
            refusals,
            vec![NotFitToServe::AgentSurfaceWithoutInboundIdentity],
            "{terminator}"
        );
        let rendered = refusals.first().expect("one refusal").to_string();
        assert!(rendered.contains("security.tls_termination"), "{rendered}");
        serves(&loopback("", &format!("{declared}  access_token: \"{TOKEN}\"\n"), LIMITED));
    }
    // `none` names no terminator, so a loopback bind that says it is still this host only.
    serves(&loopback("", "  tls_termination: \"none\"\n", ""));
}

//! `server.role: worker`: one boot refusal per thing a worker may not declare, the worker that is
//! fit, and the `serving` default.
//!
//! `InProcessTlsNotCompiledIn` is dropped from every list: a worker needs in-process TLS, and
//! whether the binary can terminate it is the build's answer, not the role's -
//! `in_process_tls_is_refused_when_the_binary_cannot_do_it` holds both arms of that.

use crate::server::ServerRole;
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

const PAIR: &str = "  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \"/tls/key.pem\"\n";
const AUTHORITY: &str = "  tls_node_authority: \"/tls/nodes.pem\"\n";

/// A loopback worker with in-process TLS, `server:` lines and further groups of the caller's choosing.
fn worker(server: &str, rest: &str) -> String {
    format!("server:\n  role: \"worker\"\n{PAIR}{server}security:\n  tls_termination: \"in-process\"\n{rest}")
}

/// Every posture refusal `sources` loads to, less the build's own.
fn refusals(sources: &Sources) -> Vec<NotFitToServe> {
    let Err(error) = Settings::load(sources) else {
        return Vec::new();
    };
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    refusals
        .iter()
        .filter(|&refusal| *refusal != NotFitToServe::InProcessTlsNotCompiledIn)
        .cloned()
        .collect()
}

fn overlaid(overlay: &str) -> Vec<NotFitToServe> {
    refusals(&Sources::defaults(Environment::Development).with_overlay(overlay))
}

#[test]
fn a_worker_that_declares_a_source_is_not_fit_to_serve() {
    let source = "sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    \
                  posture: \"shared-service-user\"\n";
    let found = overlaid(&worker(AUTHORITY, source));
    assert_eq!(
        found,
        vec![
            NotFitToServe::DeploymentIdentityUndeclared { count: 1 },
            NotFitToServe::WorkerDeclaresSources { count: 1 },
        ]
    );
    let rendered = found.last().expect("the worker refusal").to_string();
    assert!(rendered.contains("1 source(s)"), "{rendered}");
    assert_eq!(overlaid(&worker(AUTHORITY, "")), Vec::new());
}

#[test]
fn a_worker_whose_file_declares_a_catalog_is_not_fit_to_serve() {
    let catalog = "catalogs:\n  - name: model\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n";
    let dir = std::env::temp_dir().join(format!("sutura-config-worker-catalog-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
    std::fs::write(dir.join("base.yaml"), worker(AUTHORITY, catalog)).expect("a scratch file is writable");
    let declared = refusals(&Sources::defaults(Environment::Development).with_directory(dir.clone()));
    std::fs::write(dir.join("base.yaml"), worker(AUTHORITY, "")).expect("a scratch file is writable");
    let undeclared = refusals(&Sources::defaults(Environment::Development).with_directory(dir.clone()));
    drop(std::fs::remove_dir_all(&dir));
    assert_eq!(declared, vec![NotFitToServe::WorkerDeclaresCatalogs]);
    // The embedded defaults' catalog is in every configuration, and it is not the worker's own.
    assert_eq!(undeclared, Vec::new());
}

#[test]
fn a_worker_without_a_node_authority_is_not_fit_to_serve() {
    assert_eq!(overlaid(&worker("", "")), vec![NotFitToServe::WorkerWithoutNodeAuthority]);
    // No pair at all is the same finding.
    assert_eq!(
        overlaid("server:\n  role: \"worker\"\n"),
        vec![NotFitToServe::WorkerWithoutNodeAuthority]
    );
}

#[test]
fn a_worker_with_the_agent_surface_is_not_fit_to_serve() {
    assert_eq!(
        overlaid(&worker(&format!("{AUTHORITY}  agent_surface:\n    enabled: true\n"), "")),
        vec![
            NotFitToServe::AgentSurfaceWithoutInboundIdentity,
            NotFitToServe::WorkerWithAgentSurface,
        ]
    );
}

#[test]
fn a_worker_with_the_flight_surface_is_not_fit_to_serve() {
    assert_eq!(
        overlaid(&worker(&format!("{AUTHORITY}  flight_surface:\n    enabled: true\n"), "")),
        vec![
            NotFitToServe::FlightSurfaceWithoutDirectInbound,
            NotFitToServe::WorkerWithFlightSurface,
        ]
    );
}

#[test]
fn an_unknown_server_role_is_refused_naming_the_roles() {
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay("server:\n  role: \"workers\"\n"))
        .expect_err("a misspelt role is not a role");
    let SettingsError::Role { ref cause } = *error.reason() else {
        panic!("expected a role refusal, got {error:?}");
    };
    assert_eq!(
        cause.to_string(),
        "`workers` is not a server role - use one of: serving, worker"
    );
    assert_eq!(ServerRole::parse(" Worker "), Ok(ServerRole::Worker));
}

#[test]
fn the_default_role_is_serving_and_asks_for_no_worker_shape() {
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert_eq!(settings.server().role(), ServerRole::Serving);
    // Written out, it still needs none of what a worker does: no node authority here.
    assert_eq!(overlaid("server:\n  role: \"serving\"\n"), Vec::new());
}

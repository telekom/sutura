//! `server.role: reading-worker`: its spelling, and the two refusals only it raises.

use crate::server::ServerRole;
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

const READING: &str = "server:\n  role: \"reading-worker\"\n  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \
                       \"/tls/key.pem\"\n  tls_node_authority: \"/tls/nodes.pem\"\nsecurity:\n  tls_termination: \
                       \"in-process\"\n  identity: \"single-user\"\n  single_user_because: \"one operator\"\n";
const SOURCE: &str = "sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    posture: \
                      \"shared-service-user\"\n";
const CATALOG: &str =
    "catalogs:\n  - name: model\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n";

/// The posture refusals an overlay on the defaults loads to, less the build's own TLS answer.
fn refused(overlay: &str) -> Vec<NotFitToServe> {
    let Err(error) = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)) else {
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

#[test]
fn reading_worker_is_its_own_role_and_no_other_spelling_is() {
    assert_eq!(ServerRole::parse(" Reading-Worker "), Ok(ServerRole::ReadingWorker));
    assert_eq!(
        ServerRole::parse("reading_worker")
            .expect_err("an underscore is not the role's spelling")
            .to_string(),
        "`reading_worker` is not a server role - use one of: serving, worker, reading-worker"
    );
}

#[test]
fn a_reading_worker_without_a_catalog_is_not_fit_to_serve() {
    let found = refused(&format!("{READING}{SOURCE}"));
    assert_eq!(found, vec![NotFitToServe::ReadingWorkerWithoutCatalog]);
    // An overlay is stamped like the embedded defaults, so a catalog written there is not the worker's.
    assert_eq!(refused(&format!("{READING}{SOURCE}{CATALOG}")), found);
}

#[test]
fn a_reading_worker_without_a_source_is_not_fit_to_serve() {
    assert_eq!(
        refused(READING),
        vec![
            NotFitToServe::ReadingWorkerWithoutSources,
            NotFitToServe::ReadingWorkerWithoutCatalog,
        ]
    );
}

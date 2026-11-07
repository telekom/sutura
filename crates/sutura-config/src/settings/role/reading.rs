//! `server.role: reading-worker`: its spelling, and the refusals only it raises.

use sutura_domain::model::SourceName;

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

/// `#828`: a reading worker's own entry opts its source into hash buckets, and it declares one
/// source and reads for others, so neither hash-bucket boot refusal is its to raise - only the
/// refusals a reading worker always has.
#[test]
fn a_reading_worker_opting_its_own_source_into_hash_buckets_is_not_refused_for_it() {
    let source = "sources:\n  warehouse:\n    kind: \"clickhouse\"\n    host: \"127.0.0.1\"\n    port: 8123\n    \
                  user: \"sutura\"\n    password_file: \"/run/secrets/ch-password\"\n    transport_mode: \"plaintext\"\n    \
                  posture: \"shared-service-user\"\n    hash_buckets_acknowledged_because: \"each key's rows are read together\"\n";
    assert_eq!(
        refused(&format!("{READING}{source}")),
        vec![NotFitToServe::ReadingWorkerWithoutCatalog]
    );
}

/// A shared `BigQuery` source and a `BigQuery` source at a pool.
const BIGQUERY_SOURCES: &str = "  warehouse:\n    kind: \"bigquery\"\n    billing:\n      service_account_project: \
                                \"acme-analytics\"\n    max_bytes_billed: 1073741824\n    posture: \
                                \"shared-service-user\"\n  pooled:\n    kind: \"bigquery\"\n    max_bytes_billed: \
                                1073741824\n    posture: \"impersonation-at-source\"\n    workload_identity:\n      \
                                audience: \"//iam.googleapis.com/projects/acme-analytics/locations/global/\
                                workloadIdentityPools/analysts/providers/sso\"\n";

#[test]
fn a_reading_worker_declaring_a_bigquery_source_outside_a_pool_does_not_start() {
    let found = refused(&format!("{READING}{SOURCE}{BIGQUERY_SOURCES}"));
    let outside = |alias: &str| NotFitToServe::ReadingWorkerBigQueryOutsideAPool {
        alias: SourceName::parse(alias).expect("a test alias is a source name"),
    };
    assert!(
        found.contains(&outside("warehouse")),
        "the shared BigQuery source is refused: {found:?}"
    );
    assert!(
        !found.contains(&outside("pooled")),
        "a BigQuery source at a pool is not: {found:?}"
    );
    assert!(!found.contains(&outside("local")), "a files source is not: {found:?}");
    let rendered = outside("warehouse").to_string();
    assert!(
        rendered.contains("sources.warehouse") && rendered.contains("workload_identity"),
        "the refusal names the source and the key to declare: {rendered}"
    );
}

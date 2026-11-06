//! `server.role: reading-worker` over a `BigQuery` source: it starts only where every such source
//! federates at a workload identity pool.

use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

const READING_WORKER: &str = "server:\n  role: \"reading-worker\"\n  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \
                              \"/tls/key.pem\"\n  tls_node_authority: \"/tls/nodes.pem\"\nsecurity:\n  \
                              tls_termination: \"in-process\"\n  identity: \"single-user\"\n  single_user_because: \
                              \"one operator\"\n";

/// A files source, a shared `BigQuery` source and a `BigQuery` source at a pool.
const THREE_SOURCES: &str = "sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    \
                             posture: \"shared-service-user\"\n  warehouse:\n    kind: \"bigquery\"\n    billing:\n      \
                             service_account_project: \"acme-analytics\"\n    max_bytes_billed: 1073741824\n    \
                             posture: \"shared-service-user\"\n  pooled:\n    kind: \"bigquery\"\n    \
                             max_bytes_billed: 1073741824\n    posture: \"impersonation-at-source\"\n    \
                             workload_identity:\n      audience: \
                             \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n";

/// The posture refusals `overlay` on the defaults loads to.
fn not_fit(overlay: &str) -> Vec<NotFitToServe> {
    let Err(error) = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)) else {
        return Vec::new();
    };
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    refusals.clone()
}

#[test]
fn a_reading_worker_declaring_a_bigquery_source_outside_a_pool_does_not_start() {
    let found = not_fit(&format!("{READING_WORKER}{THREE_SOURCES}"));
    let outside = |alias: &str| NotFitToServe::ReadingWorkerBigQueryOutsideAPool { alias: alias.to_owned() };
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

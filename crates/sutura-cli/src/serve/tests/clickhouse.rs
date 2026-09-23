//! `clickhouse`-kind sources at `sutura serve`'s own root: the refusal for a build that did not
//! link the adapter, and - on a build that did - the furthest a fixture with no server can reach,
//! the declared password file, alone and beside a `files` source.

#[cfg(feature = "clickhouse")]
use super::{ENGINE_SOURCE, entry};
use super::{bundle_over, default_timeout, one_worker, open_engine, refusal, registry};

/// One `clickhouse` entry whose password file is not there, so a refusal naming that key is proof
/// the composition reached the credential step.
fn clickhouse_entry(alias: &str) -> String {
    format!(
        "  {alias}:\n    kind: \"clickhouse\"\n    host: \"127.0.0.1\"\n    port: 8123\n    user: \"sutura\"\n    \
         password_file: \"/nonexistent/sutura-test-clickhouse-password\"\n    transport_mode: \"plaintext\"\n    \
         posture: \"shared-service-user\"\n"
    )
}

/// **Refused at startup and naming the source and the feature, not skipped**: a default build
/// links no `ClickHouse` adapter. Compiled by `cargo xtask check-default-features` and RUN by
/// `cargo xtask check-default-feature-tests`; `--all-features` makes this cfg false.
#[test]
#[cfg(not(feature = "clickhouse"))]
fn a_clickhouse_source_is_refused_by_a_build_that_did_not_link_the_adapter() {
    let error = refusal(
        open_engine(
            &bundle_over(&[("customers", "warehouse", "dim_customer")]),
            &registry(&clickhouse_entry("warehouse")),
            one_worker(),
            default_timeout(),
            None,
        ),
        "a kind this binary linked no adapter for must not start",
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        error.contains("--features clickhouse"),
        "the refusal must say what to build: {error}"
    );
}

#[test]
#[cfg(feature = "clickhouse")]
fn a_clickhouse_source_reaches_the_password_file_the_deployment_declared() {
    let error = refusal(
        open_engine(
            &bundle_over(&[("customers", "warehouse", "dim_customer")]),
            &registry(&clickhouse_entry("warehouse")),
            one_worker(),
            default_timeout(),
            None,
        ),
        "the declared password file is not there, so this deployment does not start",
    );
    assert!(error.contains("warehouse.password_file"), "{error}");
}

/// The mixed registry's `clickhouse` group: the `files` half opens, then the `ClickHouse` half is
/// refused by its own credential read rather than by the kind or the mix.
#[test]
#[cfg(feature = "clickhouse")]
fn a_catalog_spanning_files_and_clickhouse_reaches_the_clickhouse_credential() {
    let both = format!(
        "{}{}",
        entry(ENGINE_SOURCE, "shared-service-user", ""),
        clickhouse_entry("warehouse")
    );
    let error = refusal(
        open_engine(
            &bundle_over(&[
                ("customers", ENGINE_SOURCE, "dim_customer"),
                ("products", "warehouse", "dim_product"),
            ]),
            &registry(&both),
            one_worker(),
            default_timeout(),
            None,
        ),
        "a mixed catalog opens each kind and reaches the clickhouse arm's own credential refusal",
    );
    assert!(error.contains("warehouse.password_file"), "{error}");
}

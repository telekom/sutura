//! The `ClickHouse` half of this composition root: one declared source becomes an open
//! `Warehouse`.
//!
//! **Its own file beside `bigquery`'s and `postgres`' halves**, for the reason those two state: the
//! dispatcher stays in the composition root and each kind's open-and-refuse pair lives in a file of
//! its own, under the unexemptable 1000-line cap. Both `open_clickhouse` definitions are here - the
//! one that opens the adapter and the refusal for a build that linked none - because the compiler
//! picks between them at the one call site.
//!
//! **What is NOT here is the per-source BUILD**, and that is the difference from its two siblings:
//! it lives once in `crate::clickhouse`, shared with `crate::sources`' own root, so the posture
//! cross-check, the credential read and the channel resolution cannot differ between the two
//! composition roots. `crates/sutura-cli/src/sources/postgres.rs` names that sharing as what issue
//! 121 asks for and does not have for `postgres`; this kind arrives with it.

/// Opens one `ClickHouse` adapter per declared source, over its HTTP interface, under the declared
/// credential.
///
/// **Nothing is attached and nothing is registered** - the tables live in the database, the same
/// difference from `open_files` that `open_postgres` documents. What this function does instead is
/// everything that can fail before a listener is bound, through [`crate::clickhouse::build`]: the
/// posture cross-check, the password file and the TLS material.
#[cfg(feature = "clickhouse")]
pub(crate) fn open_clickhouse(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::ClickHouseSource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = crate::clickhouse::build(source, configured)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns. Written as a fallback for
    // the reason `open_files` gives - the workspace denies `unwrap` and `expect`.
    engines
        .map(super::OpenedSources::ClickHouse)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the `ClickHouse` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, for the reason
/// `open_bigquery` gives at the same shape: the dispatcher has exactly one call and the compiler
/// decides which of these it reaches, keeping both signatures identical under `dead_code = "deny"`.
#[cfg(not(feature = "clickhouse"))]
pub(crate) fn open_clickhouse(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: clickhouse`, and this binary was built without the `clickhouse` \
         feature - so it links no ClickHouse adapter. Build `sutura-cli` with \
         `--features clickhouse`, or declare a `files` source"
    ))
}

/// `sutura serve`'s own `clickhouse` cells: the refusal for a build that did not link the adapter
/// and, on a build that did, the furthest a fixture with no server reaches - the declared password
/// file, alone and beside a `files` source.
#[cfg(test)]
mod tests {
    #[cfg(feature = "clickhouse")]
    use crate::serve::ENGINE_SOURCE;
    use crate::serve::open_engine;
    #[cfg(feature = "clickhouse")]
    use crate::serve::tests::entry;
    use crate::serve::tests::{bundle_over, default_timeout, one_worker, refusal, registry};

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
}

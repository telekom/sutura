//! The `DuckDB` half of this composition root: one declared database file becomes an open
//! `Warehouse`.
//!
//! `clickhouse`'s shape: the per-source build lives once in `crate::duckdb`, shared with
//! `crate::sources`' root, and both `open_duckdb` definitions are here because the compiler picks
//! between them for `open_engine`'s single-kind arm and `kind::duckdb_group`.

/// Opens one `DuckDB` adapter per declared source, read-only, through [`crate::duckdb::build`].
///
/// Nothing is attached - the tables live in the file - the difference from `open_files` that
/// `open_postgres` documents.
#[cfg(feature = "duckdb")]
pub(crate) fn open_duckdb(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::DuckdbSource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = crate::duckdb::build(source, configured, crate::sources::working_set(runtime))?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns - `open_clickhouse`'s reason.
    engines
        .map(super::OpenedSources::Duckdb)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the `DuckDB` adapter - `open_clickhouse`'s two
/// definitions of one signature, for its reason.
#[cfg(not(feature = "duckdb"))]
pub(crate) fn open_duckdb(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
    _runtime: sutura_config::RuntimeSettings,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: duckdb`, and this binary was built without the `duckdb` \
         feature - so it links no DuckDB adapter. Build `sutura-cli` with `--features duckdb`, \
         or declare a `files` source"
    ))
}

#[cfg(test)]
#[cfg(not(feature = "duckdb"))]
mod tests {
    use crate::serve::open_engine;
    use crate::serve::tests::{bundle_over, default_timeout, one_worker, registry};

    /// One `duckdb` entry over `database_file`.
    fn duckdb_entry(alias: &str, database_file: &str) -> String {
        format!(
            "  {alias}:\n    kind: \"duckdb\"\n    database_file: \"{database_file}\"\n    posture: \"shared-service-user\"\n"
        )
    }

    /// **Refused at startup and naming the source and the feature, not skipped**: a default build
    /// links no `DuckDB` adapter. Compiled by `cargo xtask check-default-features` and RUN by
    /// `cargo xtask check-default-feature-tests`; `--all-features` makes this cfg false.
    #[test]
    #[cfg(not(feature = "duckdb"))]
    fn a_duckdb_source_is_refused_by_a_build_that_did_not_link_the_adapter() {
        let error = crate::serve::tests::refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&duckdb_entry("warehouse", "/nonexistent/sutura-test.duckdb")),
                one_worker(),
                default_timeout(),
                None,
            ),
            "a kind this binary linked no adapter for must not start",
        );
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(
            error.contains("--features duckdb"),
            "the refusal must say what to build: {error}"
        );
    }
}

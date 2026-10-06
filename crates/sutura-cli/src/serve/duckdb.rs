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

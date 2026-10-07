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

    /// **Two `duckdb` sources are one kind, so they are never erased into `AnyWarehouse`** and both
    /// answer `ACCEPTS_RAW_STATEMENTS = true`. What refuses the raw tool is `run_sql`'s one-source
    /// count alone; without it the caller's statement would run on whichever source came first.
    #[test]
    #[cfg(feature = "duckdb")]
    fn two_duckdb_sources_are_refused_the_raw_tool_rather_than_one_picked() {
        use std::time::{Duration, Instant};

        use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};
        use sutura_domain::model::TableName;
        use sutura_domain::raw::RawStatement;
        use sutura_domain::warehouse::deadline::{Budget, Deadline};

        let dir = std::env::temp_dir().join(format!("sutura-cli-two-duckdb-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("the scratch directory is created");
        let csv = dir.join("t.csv");
        std::fs::write(&csv, "id\n1\n").expect("the table's CSV is written");
        let mut entries = String::new();
        for alias in ["first", "second"] {
            let file = dir.join(format!("{alias}.duckdb"));
            sutura_exec_duckdb::write_database(&file, &[(TableName::parse("t").expect("t is a table"), csv.clone())])
                .expect("the fixture database is written");
            entries.push_str(&duckdb_entry(alias, &file.display().to_string()));
        }
        let declared = registry(&entries);
        let opened = open_engine(
            &bundle_over(&[("first_t", "first", "t"), ("second_t", "second", "t")]),
            &declared,
            one_worker(),
            default_timeout(),
            None,
        );
        let engines = match opened {
            Ok(crate::serve::OpenedSources::Duckdb(engines)) => engines,
            Ok(_) => panic!("two duckdb sources open as one kind, not as a mix"),
            Err(error) => panic!("two duckdb sources must open: {error}"),
        };
        let answered = sutura_app::run_sql(
            &RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself)),
            &RawStatement::parse("SELECT 1").expect("a test statement is a statement"),
            &sutura_config::StaticCredentialBroker::from_registry(&declared),
            &engines,
            Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(30)).expect("a budget")),
        );
        drop(std::fs::remove_dir_all(&dir));
        assert!(
            matches!(answered, Err(sutura_app::RunSqlError::NoAcceptingSource)),
            "two raw-capable sources must have no raw tool, got {answered:?}"
        );
    }
}

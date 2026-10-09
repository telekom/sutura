//! The `DuckDB` half of the command root: one declared database file becomes an open `Warehouse`.
//!
//! `clickhouse.rs`'s shape: the build lives once in `crate::duckdb`, and what is here is this root's
//! registry wrapper and the refusal for a build that linked no adapter.

use sutura_domain::model::SourceName;

use crate::sources::Opened;
#[cfg(feature = "duckdb")]
use crate::sources::OpenedWith;

/// Opens one `DuckDB` source, read-only, under the identity the deployment declared.
///
/// **Nothing is attached** - the tables live in the file - so [`OpenedWith::attached`] is `None`.
///
/// # Errors
///
/// Everything [`crate::duckdb::build`] refuses.
#[cfg(feature = "duckdb")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
    working_set: sutura_exec_datafusion::WorkingSet,
) -> Result<Opened, String> {
    Ok(Opened::Duckdb(OpenedWith {
        engines: sutura_app::Warehouses::of(crate::duckdb::build(source, configured, working_set)?),
        attached: None,
        broker: sutura_config::StaticCredentialBroker::from_registry(registry),
    }))
}

/// The refusal for a build that linked no `DuckDB` adapter, naming the feature.
#[cfg(not(feature = "duckdb"))]
pub(super) fn open(
    source: &SourceName,
    _configured: &sutura_config::ConfiguredSource,
    _registry: &sutura_config::SourceRegistry,
    _working_set: sutura_exec_datafusion::WorkingSet,
) -> Result<Opened, String> {
    Err(format!(
        "`sources.{source}` is `kind: duckdb`, and this binary was built without the `duckdb` \
         feature - so it links no DuckDB adapter. Build `sutura-cli` with `--features duckdb`, or \
         declare a `files` source"
    ))
}

#[cfg(test)]
mod tests {
    use crate::sources::{bundle_naming, declaring, open_engine, runtime, timeout};

    /// One `sources:` entry for a `DuckDB` database that is not there, so a refusal naming the open is
    /// proof the composition reached the driver.
    fn declaring_duckdb(posture: &str, extra: &str) -> sutura_config::SourceRegistry {
        declaring(
            "warehouse",
            &format!("    kind: duckdb\n    database_file: \"/nonexistent/sutura-cli-test.duckdb\"\n{extra}"),
            posture,
        )
    }

    fn opened(registry: &sutura_config::SourceRegistry) -> String {
        open_engine(&bundle_naming("warehouse"), registry, runtime(), timeout(), None, None)
            .map(|_| ())
            .expect_err("the declared file is not there, or the build cannot open it")
    }

    /// The feature-off half, run by `cargo xtask check-default-feature-tests`.
    #[test]
    #[cfg(not(feature = "duckdb"))]
    fn a_duckdb_source_on_a_build_without_the_feature_is_refused_by_name() {
        let error = opened(&declaring_duckdb("shared-service-user", ""));
        assert!(error.contains("kind: duckdb"), "the kind is not named: {error}");
        assert!(
            error.contains("--features duckdb"),
            "the refusal must say what to build: {error}"
        );
    }

    /// The kind dispatched to this adapter, the posture passed its own `IMPERSONATION`, and the open
    /// was read-only - a missing file is refused, not created.
    #[test]
    #[cfg(feature = "duckdb")]
    fn a_declared_duckdb_source_reaches_a_read_only_open_of_its_file() {
        let error = opened(&declaring_duckdb("shared-service-user", ""));
        assert!(error.contains("did not open"), "the refusal must be the open's: {error}");
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(!std::path::Path::new("/nonexistent/sutura-cli-test.duckdb").exists());
    }

    /// `DuckDbWarehouse::IMPERSONATION` is `NoPlaceForASubject`, checked before the file is opened.
    #[test]
    #[cfg(feature = "duckdb")]
    fn a_declared_duckdb_source_configured_to_impersonate_refuses_at_boot() {
        let wif = "    workload_identity:\n      audience: \"//iam.googleapis.com/projects/1/locations/global/\
                   workloadIdentityPools/p/providers/sso\"\n";
        let error = opened(&declaring_duckdb("impersonation-at-source", wif));
        assert!(error.contains("per-subject credential"), "{error}");
        assert!(
            !error.contains("did not open"),
            "the cross-check fires before the open: {error}"
        );
    }
}

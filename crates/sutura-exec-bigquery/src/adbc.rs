//! The ADBC transport: loads the self-built `libadbc_driver_bigquery.so`
//! (`nix/bigquery-adbc.nix`) through `adbc_core` + `adbc_driver_manager` and
//! decodes its Arrow result sets.
//!
//! ```text
//! adbc_core + adbc_driver_manager → C ABI → libadbc_driver_bigquery.so
//!   → BigQuery → Arrow RecordBatchReader → decode::job_rows → RowSet
//! ```
//!
//! Behind the crate's default-off `adbc` feature, like the `wire`: the native
//! driver and its Arrow graph are a per-triple addition a lean build should not
//! link. Off is not hidden — every gate passes `--all-features`.

// The ADBC traits below are imported anonymously (`as _`) because they exist only
// to resolve those types' methods and are never named directly.
pub mod decode;

use adbc_core::error::Error as CoreError;
use adbc_core::options::{AdbcVersion, OptionDatabase, OptionValue};
use adbc_core::{Connection as _, Database as _, Driver as _, Statement as _};
use adbc_driver_manager::{ManagedDriver, ManagedStatement};
use arrow_array::RecordBatchReader as _;

use crate::transport::{DatasetAddress, DryRunEstimate, HeldTables, JobRequest, JobRows, JobTransport};
pub use decode::{Reported, job_rows};

/// Why the ADBC transport could not answer.
#[derive(Debug, thiserror::Error)]
pub enum AdbcError {
    /// The driver `.so` could not be loaded.
    #[error("could not load the BigQuery ADBC driver: {0}")]
    Load(#[source] CoreError),
    /// An ADBC call (connect, prepare, execute) failed.
    #[error("ADBC call failed: {0}")]
    Adbc(#[source] CoreError),
    /// A result batch could not be read from the stream.
    #[error("could not read a result batch: {0}")]
    Batch(#[source] arrow_schema::ArrowError),
    /// The result set could not be decoded into the adapter's own shape.
    #[error("could not decode the ADBC result set: {0}")]
    Decode(#[source] decode::Decode),
    /// ADBC does not yet cover a port method this transport was asked for.
    #[error("ADBC transport cannot yet {0}")]
    Uncovered(&'static str),
}

/// A driver handle and a prepared statement, the shape one job needs.
type Connected = (ManagedDriver, ManagedStatement);

/// A `BigQuery` endpoint over ADBC.
pub struct AdbcBigQuery {
    driver_path: String,
}

impl AdbcBigQuery {
    /// Names the driver `.so` a composition root resolves to load.
    pub fn new(driver_path: impl Into<String>) -> Self {
        Self {
            driver_path: driver_path.into(),
        }
    }

    /// Loads the driver, connects, and prepares the request's statement.
    fn connect(&self, request: &JobRequest<'_>) -> Result<Connected, AdbcError> {
        let mut driver = ManagedDriver::load_dynamic_from_filename(&self.driver_path, None, AdbcVersion::default())
            .map_err(AdbcError::Load)?;
        let opts = [
            (
                OptionDatabase::Other("bigquery.project_id".into()),
                OptionValue::String(request.billing_project().as_str().to_owned()),
            ),
            (
                OptionDatabase::Other("bigquery.dataset_id".into()),
                OptionValue::String(request.default_dataset().as_str().to_owned()),
            ),
        ];
        let db = driver.new_database_with_opts(opts).map_err(AdbcError::Adbc)?;
        let mut conn = db.new_connection().map_err(AdbcError::Adbc)?;
        let mut stmt = conn.new_statement().map_err(AdbcError::Adbc)?;
        stmt.set_sql_query(request.statement()).map_err(AdbcError::Adbc)?;
        Ok((driver, stmt))
    }
}

impl JobTransport for AdbcBigQuery {
    type Error = AdbcError;

    fn run(&self, request: &JobRequest<'_>) -> Result<JobRows, Self::Error> {
        let (_driver, mut stmt) = self.connect(request)?;
        let reader = stmt.execute().map_err(AdbcError::Adbc)?;
        let schema = reader.schema();
        let mut batches = Vec::new();
        for batch in reader {
            batches.push(batch.map_err(AdbcError::Batch)?);
        }
        // A full drain IS completeness for ADBC: the Storage Read API streams the
        // whole result. The `reported` total is only checked when the driver
        // reports one (schema-metadata keys measured in the provisioned leg).
        decode::job_rows(&schema, &batches, Reported::Unreported).map_err(AdbcError::Decode)
    }

    fn validate(&self, _request: &JobRequest<'_>) -> Result<DryRunEstimate, Self::Error> {
        // No ADBC call maps onto BigQuery's free `dryRun`; the port keeps this
        // honest rather than billing a guess.
        Err(AdbcError::Uncovered("dry-run a statement"))
    }

    fn list_tables(&self, _at: &DatasetAddress) -> Result<HeldTables, Self::Error> {
        // ADBC `GetObjects` is unverified for this driver; the port keeps
        // "cannot list" distinct from "table absent".
        Err(AdbcError::Uncovered("list tables"))
    }

    #[cfg(feature = "fixtures")]
    fn apply(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(AdbcError::Uncovered("bulk-load fixtures"))
    }
}

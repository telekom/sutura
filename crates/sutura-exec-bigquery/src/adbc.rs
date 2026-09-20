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
//! link. Off is not hidden - every gate passes `--all-features`.
//!
//! # Who a job runs as
//!
//! One of the driver's own options, and [`identity`] is where the whole decision lives and is
//! asserted: a [`JobIdentity::AsPrincipal`](crate::transport::JobIdentity::AsPrincipal) becomes
//! `bigquery.impersonate.target_principal`, so the data system evaluates the statement as the
//! account a source declared for that subject. A shared leg sends no impersonation option and runs
//! as the process. A bearer is REFUSED - the pinned driver has nowhere to put one - rather than
//! dropped, which would run somebody else's question under this deployment's identity.
//!
//! **Nothing is shared between two jobs.** The driver handle, the database and the connection are
//! locals of [`AdbcBigQuery::connect`], built from one request's own options; the endpoint itself
//! owns only a path and a declaration. That, and not a check, is what keeps two concurrent subjects apart
//! here - stated with its limit in [`AdbcBigQuery`]'s own documentation.

pub mod decode;
mod identity;
pub use identity::{Impersonation, ImpersonationScopes, TargetAccount, UnusableIdentityOption};

// The ADBC traits below are imported anonymously (`as _`) because they exist only
// to resolve those types' methods and are never named directly.
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
    /// A leg named a principal that is not an account this transport can ask the driver to become.
    ///
    /// **Its own variant rather than an [`Self::Uncovered`] string**, because the two say different
    /// things to whoever reads them: `Uncovered` is *this transport does not do that*, which is a
    /// fact about the driver, and this is *the declaration named something unsendable*, which is a
    /// fact about a settings file. The cause carries a position and never the value - see
    /// [`identity::UnusableIdentityOption`].
    #[error("a leg named a principal this transport cannot ask the driver to become")]
    UnusableTarget {
        #[source]
        cause: UnusableIdentityOption,
    },
}

/// A driver handle and a prepared statement, the shape one job needs.
type Connected = (ManagedDriver, ManagedStatement);

/// A `BigQuery` endpoint over ADBC.
///
/// **Two owned values and nothing else, which is load-bearing rather than tidy.** There is no
/// connection here, no database handle and no token: [`Self::connect`] builds all three per job from
/// that job's own request and drops them with it. That is what keeps one subject's principal off
/// another subject's query - not a check, but the absence of anything two jobs could share.
///
/// **The limit beside it:** nothing in the type system forbids a future field from holding a
/// connection, and a pool keyed on anything but the identity would be exactly the cross-user leak
/// this shape avoids. `identity::tests::one_subjects_principal_never_appears_in_the_next_subjects_options`
/// is the cell that dies if the option list starts being memoised; a *connection* cache would need
/// its own.
pub struct AdbcBigQuery {
    driver_path: String,
    /// Whether this source impersonates, and at what scope - decided at composition from what the
    /// source declared, never per request. See [`Impersonation`] for why it is not an `Option`.
    impersonation: Impersonation,
}

impl AdbcBigQuery {
    /// Names the driver `.so` a composition root resolves to load, and whether this source
    /// impersonates.
    pub fn new(driver_path: impl Into<String>, impersonation: Impersonation) -> Self {
        Self {
            driver_path: driver_path.into(),
            impersonation,
        }
    }

    /// Decides who the job runs as, loads the driver, connects, and prepares the statement.
    ///
    /// **Every refusal this transport makes about a request is here, before the `.so` is loaded**,
    /// and that is deliberate: `run` is not the only caller a future port method could have, and a
    /// guard per caller is a guard somebody forgets. The identity decision is first, so a request
    /// this transport cannot execute as the right principal never opens a connection it could
    /// execute as the wrong one.
    fn connect(&self, request: &JobRequest<'_>) -> Result<Connected, AdbcError> {
        // WHO first. `identity_options` is empty for a shared leg and refuses a bearer; a refused
        // identity must not reach `load_dynamic_from_filename`, because a loaded driver with no
        // impersonation option is a connection as the deployment itself.
        let identity = identity::identity_options(request.identity(), &self.impersonation)?;
        // This transport cannot bind parameters, and the rendered statement carries positional `?`
        // - so an unbound statement is a runtime error at the driver rather than a wrong answer.
        // Refused here for the same reason the identity is: one guard, before anything is opened.
        if !request.params().is_empty() {
            return Err(AdbcError::Uncovered("bind statement parameters"));
        }
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
        let db = driver
            .new_database_with_opts(opts.into_iter().chain(identity))
            .map_err(AdbcError::Adbc)?;
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
        // "cannot list" distinct from "table absent". Note what that means for a
        // composed warehouse: `Warehouse::preflight` calls this at boot, so a
        // non-empty bundle under this transport FAILS preflight (an outage), not
        // a silent empty grant. Do not select this transport at composition until
        // `GetObjects` is bound - that binding is the provisioned follow-on.
        Err(AdbcError::Uncovered("list tables"))
    }

    #[cfg(feature = "fixtures")]
    fn apply(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(AdbcError::Uncovered("bulk-load fixtures"))
    }
}

#[cfg(test)]
mod tests;

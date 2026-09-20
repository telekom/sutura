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
//! Two declared postures, XOR, decided at composition and never per request: a `shared-service-user`
//! source runs on the deployment's own application default credentials and impersonates nothing, and
//! an `impersonation-at-source` source federates the asking subject's own assertion against the pool
//! it declares. `identity` is where that decision lives and is asserted, and `subject` is where the
//! federation's mechanism and its exposure are written down.
//!
//! **A subject at a source with no pool is REFUSED** rather than answered as the deployment - the
//! one thing this transport must never do, and the reason there is no third state. The mechanism
//! this replaced - an impersonation target minted from the deployment's own credentials - has no
//! spelling in `crate::transport::JobIdentity` any more, so it is unrepresentable rather than
//! refused.
//!
//! # The values
//!
//! Bound as one Arrow batch of one row, per [`bind`] - which is where the driver's own per-row
//! execution loop is read off the pinned source and why one row is the only correct count. The
//! statement carries positional `?` and nothing is ever interpolated into it.
//!
//! **Nothing is shared between two jobs.** The driver handle, the database and the connection are
//! locals of [`AdbcBigQuery::connect`], built from one request's own options; the endpoint itself
//! owns only a path and a declaration. That, and not a check, is what keeps two concurrent subjects apart
//! here - stated with its limit in [`AdbcBigQuery`]'s own documentation.

mod bind;
pub mod decode;
mod identity;
mod subject;
pub use identity::Impersonation;
pub use subject::{UnusablePool, WorkloadPool};

// The ADBC traits below are imported anonymously (`as _`) because they exist only
// to resolve those types' methods and are never named directly - except `Statement`, which
// `prepared` names as a BOUND so a fake implementor can stand in for the driver's own.
use adbc_core::error::Error as CoreError;
use adbc_core::options::{AdbcVersion, OptionDatabase, OptionValue};
use adbc_core::{Connection as _, Database as _, Driver as _, Statement};
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
    /// The plan's values could not be assembled as the batch this driver binds them from.
    ///
    /// Its own variant rather than an [`Self::Adbc`], because the failure is on THIS side of the C
    /// ABI: nothing has been sent, and what went wrong is an Arrow batch this transport built. The
    /// cause is Arrow's own, kept as a `#[source]` so the chain still walks.
    #[error("the plan's values could not be assembled for binding")]
    Parameters {
        #[source]
        cause: arrow_schema::ArrowError,
    },
    /// The loopback source a subject's assertion is served over could not be opened.
    ///
    /// Its own variant rather than an [`Self::Adbc`], because nothing has been sent and the failure
    /// is this process's own: a host with no usable loopback interface cannot serve an impersonated
    /// question, and saying that is better than a driver failing to fetch a token for reasons of
    /// its own.
    #[error("the loopback source for the asking subject's assertion could not be opened")]
    SubjectSource {
        #[source]
        cause: std::io::Error,
    },
    /// The operating system would not supply the randomness this request's two secrets need.
    ///
    /// **A refusal and not a fallback**, and `subject::unguessable`'s own doc carries why: every
    /// constant available here would be written into the same document the driver reads, so the
    /// fetch would authenticate against a value any local process could guess.
    #[error("this host would not supply the randomness a subject's loopback source needs: {cause}")]
    NoRandomness {
        /// Carried by value rather than as a `#[source]`: `getrandom::Error` is an opaque code with
        /// a `Display` and no `Error` impl, so there is no chain to walk - and inventing a wrapper
        /// for it would add a name that tells a reader nothing the message does not.
        cause: getrandom::Error,
    },
}

/// Puts the statement and its values on one prepared statement.
///
/// **Generic in `S: Statement` so a FAKE can hold it, and that is the whole reason it is not inline
/// in [`AdbcBigQuery::connect`].** It was, and review measured what that cost: replacing the `bind`
/// call with `drop(bound)` left the clippy leg clean and the suite green, because reaching that line
/// at all needs a real driver. `adbc_core::Statement` is a trait, so a recording implementor is
/// cheaper than the hosted leg and asserts the one thing that matters - that the batch this
/// transport built is the batch the statement was bound with.
///
/// **The values travel in a bound batch and never in the text**, which is the no-injection
/// invariant at the one boundary this adapter could break it at. `bigquery.query.parameter_mode` is
/// left at the driver's own default of `positional`, which is what the rendered `?` placeholders and
/// `JobRequest::PARAMETER_MODE` both say: a value's identity in a plan is its position, so there is
/// no name to send.
///
/// `None` binds nothing at all rather than an empty batch - [`bind`]'s own header has the driver's
/// two code paths.
fn prepared<S>(stmt: &mut S, request: &JobRequest<'_>, bound: Option<arrow_array::RecordBatch>) -> Result<(), AdbcError>
where
    S: Statement,
{
    stmt.set_sql_query(request.statement()).map_err(AdbcError::Adbc)?;
    if let Some(batch) = bound {
        stmt.bind(batch).map_err(AdbcError::Adbc)?;
    }
    Ok(())
}

/// A driver handle, a prepared statement, and the loopback source they may still fetch from.
///
/// **The third element is the lifetime fix rather than a convenience.** `externalaccount`'s token
/// provider is CACHED, so the driver fetches the subject token lazily - after `connect` returns -
/// and may fetch again if the credential expires mid-query. Returning the source makes `run` its
/// owner, so the endpoint closes when the job ends and not before.
type Connected = (ManagedDriver, ManagedStatement, Option<subject::SubjectSource>);

/// The project id [`AdbcBigQuery::probe`] hands the driver, which reaches no request.
///
/// A named constant and an obviously unusable value, so that nothing reads it as a fallback: the
/// probe opens no connection, so this is never sent anywhere. `.invalid` is RFC 2606's reserved
/// never-resolvable name.
const PROBE_PROJECT: &str = "sutura-driver-probe.invalid";

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

    /// Does the driver at this path load and initialise at all?
    ///
    /// **The one thing a boot path or a diagnostic can find out about the `.so` without a project**,
    /// and it is worth more than reading the environment variable: `dlopen` of this driver runs the
    /// GO RUNTIME's own initialisation inside this process, beside tokio and beside the allocator a
    /// release build links. That is the coexistence nobody could assert while the only caller was a
    /// question - so a link-success check would have passed and been wrong, and this executes instead.
    ///
    /// It opens a DATABASE and stops there, deliberately. `new_database_with_opts` is option-setting
    /// on the Go side and reaches no network; `new_connection` is where the driver builds its client
    /// and looks for application default credentials, which on a host with none is a metadata-server
    /// probe this has no business making. So what a success means is exactly *the `.so` is this ABI
    /// and its runtime started*, and nothing about whether a question could be answered.
    ///
    /// # Errors
    ///
    /// [`AdbcError::Load`] where the `.so` is absent, is not this ABI, or cannot be loaded at all -
    /// which is what a static-musl binary answers, because it has no dynamic loader.
    /// [`AdbcError::Adbc`] where the driver loaded and refused the database.
    pub fn probe(driver_path: &str) -> Result<(), AdbcError> {
        let mut driver =
            ManagedDriver::load_dynamic_from_filename(driver_path, None, AdbcVersion::default()).map_err(AdbcError::Load)?;
        // A project id is set because the driver's own option map is what is being exercised; the
        // value reaches no request, because no connection is opened. `PROBE_PROJECT` is a name
        // rather than a literal so nobody reads it as a default for anything.
        let opts = [(
            OptionDatabase::Other("bigquery.project_id".into()),
            OptionValue::String(String::from(PROBE_PROJECT)),
        )];
        drop(driver.new_database_with_opts(opts).map_err(AdbcError::Adbc)?);
        Ok(())
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
        let authentication = identity::authenticate(request.identity(), &self.impersonation)?;
        // The values, as the one batch this driver binds from - assembled before the `.so` is
        // loaded for the identity's reason: a request this transport cannot assemble must not open a
        // connection. `None` where a call carries no values, which is every boot-path call and a
        // different code path inside the driver - see [`bind`].
        let bound = bind::parameter_batch(request.params())?;
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
            .new_database_with_opts(opts.into_iter().chain(authentication.options))
            .map_err(AdbcError::Adbc)?;
        let mut conn = db.new_connection().map_err(AdbcError::Adbc)?;
        let mut stmt = conn.new_statement().map_err(AdbcError::Adbc)?;
        prepared(&mut stmt, request, bound)?;
        Ok((driver, stmt, authentication.source))
    }
}

impl JobTransport for AdbcBigQuery {
    type Error = AdbcError;

    fn run(&self, request: &JobRequest<'_>) -> Result<JobRows, Self::Error> {
        // `_source` is BOUND rather than discarded, and the underscore is the only thing about it
        // that is cosmetic: dropping it here would close the subject-token endpoint before the
        // driver's own lazy fetch reached it. It lives to the end of this function.
        let (_driver, mut stmt, _source) = self.connect(request)?;
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
        // ADBC `GetObjects` is unverified for this driver; the port keeps "cannot list" distinct
        // from "table absent", so this is an `Err` and never an empty set - an empty set would
        // report every table in the bundle absent and refuse a correct deployment.
        //
        // **WHAT THAT MEANS AT BOOT, corrected: the deployment SERVES.** This comment used to say a
        // non-empty bundle FAILS preflight under this transport, which was false and is the kind of
        // false a startup claim must not be. `listing_was_refused` answers `false` here - see its
        // own override below - so `sutura_app::preflight` produces `Notice::Unverified` and
        // `sutura-cli`'s `serve::boot::refuse_absent_tables` takes the WARN arm and binds the
        // listener.
        //
        // **The consequence, stated where the limit is:** on a `bigquery` source a mistyped
        // `table:` is not caught at boot. It fails the first question asked against that model,
        // which is exactly the asymmetry `telekom/sutura#120` is about and which a `files`
        // deployment does not have. Binding `GetObjects` is what closes it.
        Err(AdbcError::Uncovered("list tables"))
    }

    /// `false`, always, and the two ways that could have been wrong are both worse.
    ///
    /// **This is an OVERRIDE that restates the trait default on purpose**, because the default is
    /// the right answer for the wrong reason and a reader has to be able to see which. The question
    /// this predicate asks is *did the data system REFUSE the listing* - an authorization failure
    /// whose fix is one grant, and which `serve::boot` turns into a startup refusal naming
    /// `bigquery.tables.list`. Nothing here was refused by anything: the transport did not ask.
    ///
    /// So `true` would send an operator to grant a permission that is not missing, and the honest
    /// `false` costs the boot check for this source - which [`Self::list_tables`] states above.
    ///
    /// **What is pinned, and it is NOT "both directions".** An earlier version of this sentence
    /// claimed both; review answered that a constant `false` has no second direction to pin, and
    /// that is right. Two cells hold two different things instead:
    /// `the_listing_this_transport_cannot_do_is_not_an_authorization_refusal` reads the value here
    /// and dies if it flips - which is what the trait default left unheld, measured as a green
    /// suite - and `a_listing_this_transport_cannot_do_is_a_warning_and_not_a_startup_refusal` in
    /// `crate::tests` reads the OUTCOME one layer up, where `sutura_app::preflight` turns this
    /// answer into the verdict `serve::boot` acts on. The second is the direction that matters to a
    /// deployment: it is the one that would change if this constant were ever right to be `true`.
    fn listing_was_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    #[cfg(feature = "fixtures")]
    fn apply(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(AdbcError::Uncovered("bulk-load fixtures"))
    }
}

#[cfg(test)]
mod tests;

//! The ADBC transport: opens the self-built `BigQuery` driver
//! (`nix/bigquery-adbc.nix`) through `adbc_core` + `adbc_driver_manager` and
//! decodes its Arrow result sets.
//!
//! ```text
//! adbc_core + adbc_driver_manager → C ABI → the `BigQuery` ADBC driver
//!   → `BigQuery` → Arrow RecordBatchReader → decode::Decoding → RowSet
//! ```
//!
//! **Two routes to that driver and one type deciding between them** - [`DriverLocation`], resolved
//! once at composition. A release artefact carries the driver in its own link (the `c-archive`
//! half of one nix derivation), which is the only route a STATIC musl binary has; a source build
//! opens a `.so` a deployment mounted. `location` carries why that is a parsed value rather than
//! the arbitrary path `telekom/sutura#929`'s sixth finding named.
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
mod ceiling;
mod identity;
// The driver this artefact carries, and the one `unsafe` in this workspace. `cfg`-gated by
// `../../build.rs`, so a source build does not compile it - `cargo xtask check-unsafe` is what
// reads it regardless of that, and the module header carries the limit.
#[cfg(adbc_driver_linked)]
mod linked;
mod location;
mod subject;
pub use ceiling::{BytesBilledCeiling, UnusableCeiling};
pub use identity::Impersonation;
pub use location::{DriverLocation, UnusableDriverPath};
pub use subject::{UnusablePool, WorkloadPool};

// The ADBC traits below are imported anonymously (`as _`) because they exist only
// to resolve those types' methods and are never named directly - except `Statement`, which
// `prepared` names as a BOUND so a fake implementor can stand in for the driver's own.
use adbc_core::error::Error as CoreError;
use adbc_core::options::{AdbcVersion, OptionDatabase, OptionStatement, OptionValue};
use adbc_core::{Connection as _, Database as _, Driver as _, Statement};
use adbc_driver_manager::{ManagedDriver, ManagedStatement};
use arrow_array::RecordBatchReader as _;

use sutura_domain::warehouse::{Accumulating, ResultBatches, UnannouncedBatch};

use crate::transport::{DatasetAddress, DryRunEstimate, HeldTables, JobRequest, JobTransport};

/// How many rows this transport will materialise from one result stream before refusing.
///
/// **A ceiling on ROWS and not on BYTES, which is the limit this sentence used to overstate.** It
/// was called a ceiling on this process's memory; a row count times an unbounded row width is not a
/// memory bound, and what decides the width is the plan's projection - a property of the plans a
/// deployment can ask, not of this constant. So what it holds is that a stream is FINITE: an
/// unending driver is refused, a million very wide rows are not.
///
/// It is not a cap on an answer either, and that distinction decides the value.
/// `sutura_domain::plan::MAX_ROWS` caps an answer and travels in the statement's own `LIMIT`; a
/// federation LEG carries no `LIMIT` at all - `sutura_domain::plan::leg`'s header says so, because a
/// leg is not an answer - so for a leg there is nothing in the statement bounding what the source
/// may stream back, and the only thing between a driver that streams without end and this process is
/// a number here. The refusal fires WHILE reading, in `Accumulating::push`, so it cannot be reached
/// by first materialising the whole stream.
///
/// Two orders of magnitude above `MAX_ROWS`, because it has to refuse only a stream no plan could
/// have asked for: a leg legitimately returns more rows than the one answer re-aggregated above it
/// keeps. **The VALUE is held rather than commented** - review measured that raising it to
/// `usize::MAX` left the whole suite green, because `delivered + n > usize::MAX` is never true and
/// the refusal test passes its own ceiling in. Both bounds of that sentence are asserted by
/// `tests::the_transports_own_ceiling_is_two_orders_of_magnitude_above_the_answer_cap`.
///
/// **The engine passes `usize::MAX` deliberately**, and the contrast is the reason this is the
/// caller's argument rather than the guard's default: `sutura-exec-datafusion` produces its own
/// batches from its own plan and is bounded by its memory pool, which is where `docs/adr/0009`
/// puts it. A foreign driver is what a row ceiling exists for.
pub const MOST_RESULT_ROWS: usize = 1_000_000;

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
    /// A batch did not carry the fields the driver's own announced schema said it would.
    ///
    /// **The check is `sutura_domain::warehouse::Accumulating`'s and not this crate's**, which is
    /// `docs/adr/0039`'s point: a foreign driver streaming over a C ABI is exactly the case to
    /// refuse rather than trust, and the obligation is the same for every adapter that has one.
    /// This variant also carries the row ceiling being reached - see [`MOST_RESULT_ROWS`].
    #[error("the ADBC result stream did not match its announced schema: {0}")]
    Unannounced(#[source] UnannouncedBatch),
    /// ADBC does not yet cover a port method this transport was asked for.
    #[error("ADBC transport cannot yet {0}")]
    Uncovered(&'static str),
    /// There is no ADBC call that prices a statement without running it.
    ///
    /// **Its own variant rather than an [`Self::Uncovered`], because one caller has to be able to
    /// tell it apart and a `&'static str` is not something to match on.**
    /// [`JobTransport::declined_to_dry_run`] reads this variant and nothing else, which is what
    /// lets the adapter answer `PreFlight::NotAsked` for a dry run nobody made while a dry run that
    /// was made and failed stays a failure.
    #[error("ADBC has no call that prices a statement without running it")]
    NoDryRun,
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
    /// A declared impersonation target is not one this transport will name in a request.
    ///
    /// **Its own variant rather than an [`Self::Uncovered`], because it is a refusal about a
    /// VALUE and the string in that one is a missing capability.** The account rides into one path
    /// segment of `service_account_impersonation_url`, which decides which account the question
    /// runs as, and `cloud.google.com/go/auth`'s impersonation provider POSTs that URL verbatim
    /// with no shape check at all. `sutura_domain::identity::PrincipalName`'s parser is the
    /// domain's shared one and accepts `/`, so the narrowing is this crate's.
    ///
    /// **It carries nothing**, deliberately: the shipped broker parses the same rule at boot and
    /// names the value there, where an operator can act on it, and this arm is reachable only from
    /// a broker that built a `Presented` by hand. A refusal at the send boundary that echoed the
    /// value would put a caller-influenced string into an error that reaches a log.
    #[error("the account declared for this subject is not one this transport will name in an impersonation request")]
    UnusableTarget,
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
///
/// **It also carries the MONEY bound, and this is the one place every statement passes through** -
/// `AdbcBigQuery`'s `run` is the only caller of its [`connect`](AdbcBigQuery::connect), so a leg, a verified
/// anchor, an identity read and a fixture load all arrive here and none of them can opt out. Sent
/// rather than checked: see [`MAX_BYTES_BILLED_OPTION`] for the key and [`BytesBilledCeiling`] for
/// what the service does with it, and what it does not bound.
fn prepared<S>(
    stmt: &mut S,
    request: &JobRequest<'_>,
    bound: Option<arrow_array::RecordBatch>,
    max_bytes_billed: BytesBilledCeiling,
) -> Result<(), AdbcError>
where
    S: Statement,
{
    stmt.set_option(
        OptionStatement::Other(MAX_BYTES_BILLED_OPTION.to_owned()),
        OptionValue::Int(max_bytes_billed.as_int()),
    )
    .map_err(AdbcError::Adbc)?;
    stmt.set_sql_query(request.statement()).map_err(AdbcError::Adbc)?;
    if let Some(batch) = bound {
        stmt.bind(batch).map_err(AdbcError::Adbc)?;
    }
    Ok(())
}

/// Loads the driver, by whichever of the two routes this artefact has.
///
/// **One function for both call sites, and that is the point rather than tidiness.** `probe` and
/// `connect` are the only two places a driver is opened, and before this each named its own
/// constructor - so a build that linked the archive could have had one of them still looking for a
/// file. The route is a property of the [`DriverLocation`] the composition root resolved, so
/// neither caller decides it.
fn load(at: &DriverLocation) -> Result<ManagedDriver, AdbcError> {
    if let Some(path) = at.mounted() {
        return ManagedDriver::load_dynamic_from_filename(path, None, AdbcVersion::default()).map_err(AdbcError::Load);
    }
    carried()
}

/// The driver this artefact's own link carries - the only route a STATIC musl binary has, because
/// it has no dynamic loader at all. `linked`'s header carries what makes it sound.
#[cfg(adbc_driver_linked)]
fn carried() -> Result<ManagedDriver, AdbcError> {
    linked::driver().map_err(AdbcError::Load)
}

/// Unreachable in a build that linked no archive, because [`DriverLocation::linked_in`] is the only
/// constructor of the location this serves and it answers `None` there.
///
/// An `Err` and not an `unreachable!`: a refusal a caller can render beats a panic in a boot path,
/// and the two `cfg` halves then have one signature, so [`load`] needs no branch of its own.
#[cfg(not(adbc_driver_linked))]
fn carried() -> Result<ManagedDriver, AdbcError> {
    Err(AdbcError::Load(CoreError::with_message_and_status(
        "this build linked no `BigQuery` driver",
        adbc_core::error::Status::NotFound,
    )))
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

/// The pinned driver's own name for `BigQuery`'s `maximumBytesBilled` job configuration.
///
/// **Read off the driver's option table rather than guessed** - `go/driver.go`'s
/// `OptionQueryMaxBytesBilled`, which `go/statement.go`'s `SetOptionInt` assigns to
/// `queryConfig.MaxBytesBilled`. The flake pins that source (`bigquery-adbc-src`, tag
/// `go/v1.13.0`), so the string and the version it is true of move together.
///
/// **An INTEGER option, which decides how it is sent.** `adbc_ffi` routes an
/// `OptionValue::Int` to `StatementSetOptionInt` only at ADBC 1.1.0, and the driver is opened at
/// `AdbcVersion::default()` - which is that revision. A string here would reach the driver's
/// `SetOptionString`, whose own match does not carry this key, and come back
/// `NotImplemented`.
const MAX_BYTES_BILLED_OPTION: &str = "bigquery.query.max_bytes_billed";

/// A `BigQuery` endpoint over ADBC.
///
/// **Three owned values and nothing else, which is load-bearing rather than tidy.** There is no
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
    driver: DriverLocation,
    /// Whether this source impersonates, and at what scope - decided at composition from what the
    /// source declared, never per request. See [`Impersonation`] for why it is not an `Option`.
    impersonation: Impersonation,
    /// The money bound every statement this transport submits carries.
    ///
    /// Held here rather than on a [`JobRequest`] for [`Impersonation`]'s reason: it is a property
    /// of the SOURCE the deployment declared, so no request decides it and no request can omit it.
    /// Parsed at composition, so a value that is not a ceiling fails before a listener is bound.
    max_bytes_billed: BytesBilledCeiling,
}

impl AdbcBigQuery {
    /// Takes the driver a composition root resolved, whether this source impersonates, and the
    /// money bound every job it submits is capped at.
    ///
    /// **A [`DriverLocation`] and not a path**, which is `telekom/sutura#929`'s sixth finding: the
    /// driver a release artefact carries has no path, and a mounted one has been parsed before it
    /// gets here.
    ///
    /// **`max_bytes_billed` has no default, and that is the same argument
    /// `crate::BigQueryWarehouse::new` makes about its own arguments**: a defaulted ceiling is
    /// money somebody else pays. It is also why the type is [`BytesBilledCeiling`] and not a
    /// number - `sources.<alias>.max_bytes_billed` is required at boot, and for the length of one
    /// review round it was required, unparsed and sent nowhere, so a declared `0` and a declared
    /// `u64::MAX` both booted green over a source with no bound on bytes scanned at all.
    pub const fn new(driver: DriverLocation, impersonation: Impersonation, max_bytes_billed: BytesBilledCeiling) -> Self {
        Self {
            driver,
            impersonation,
            max_bytes_billed,
        }
    }

    /// Does this artefact's driver load and initialise at all?
    ///
    /// **The one thing a boot path or a diagnostic can find out about the driver without a
    /// project**, and it is worth more than reading a manifest or an environment variable:
    /// initialising this driver runs the GO RUNTIME inside this process, beside tokio and beside
    /// the allocator a release build links. That is the coexistence nobody could assert while the
    /// only caller was a question - so a link-success check would have passed and been wrong, and
    /// this executes instead. `nix/bigquery-driver-check.sh` is the venue that runs it against the
    /// release artefacts, and it is the whole of what makes the driver *carried* rather than
    /// *built*.
    ///
    /// It opens a DATABASE and stops there, deliberately. `new_database_with_opts` is option-setting
    /// on the Go side and reaches no network; `new_connection` is where the driver builds its client
    /// and looks for application default credentials, which on a host with none is a metadata-server
    /// probe this has no business making. So what a success means is exactly *this driver is this ABI
    /// and its runtime started*, and nothing about whether a question could be answered.
    ///
    /// # Errors
    ///
    /// [`AdbcError::Load`] where a mounted `.so` is absent, is not this ABI, or cannot be loaded at
    /// all, and where a linked-in driver's own initialisation refused.
    /// [`AdbcError::Adbc`] where the driver loaded and refused the database.
    pub fn probe(driver: &DriverLocation) -> Result<(), AdbcError> {
        let mut driver = load(driver)?;
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
        // identity must not reach `load`, because a loaded driver with no
        // impersonation option is a connection as the deployment itself.
        let authentication = identity::authenticate(request.identity(), &self.impersonation)?;
        // The values, as the one batch this driver binds from - assembled before the `.so` is
        // loaded for the identity's reason: a request this transport cannot assemble must not open a
        // connection. `None` where a call carries no values, which is every boot-path call and a
        // different code path inside the driver - see [`bind`].
        let bound = bind::parameter_batch(request.params())?;
        let mut driver = load(&self.driver)?;
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
        prepared(&mut stmt, request, bound, self.max_bytes_billed)?;
        Ok((driver, stmt, authentication.source))
    }
}

impl JobTransport for AdbcBigQuery {
    type Error = AdbcError;

    fn run(&self, request: &JobRequest<'_>) -> Result<ResultBatches, Self::Error> {
        // `_source` is BOUND rather than discarded, and the underscore is the only thing about it
        // that is cosmetic: dropping it here would close the subject-token endpoint before the
        // driver's own lazy fetch reached it. It lives to the end of this function.
        let (_driver, mut stmt, _source) = self.connect(request)?;
        let reader = stmt.execute().map_err(AdbcError::Adbc)?;
        let announced = reader.schema();
        // **Checked as each batch arrives, under a ceiling, and nothing collects the stream first.**
        // This used to push every `RecordBatch` into a `Vec` and then decode every row beside it -
        // two materialisations of the same result, neither bounded, both before anything downstream
        // could look at the working set. Review round 4 of `telekom/sutura#929` named exactly that.
        // Now a batch whose own schema is not the announced one is refused before its values are
        // read, and a stream past [`MOST_RESULT_ROWS`] is refused at the batch that crosses the
        // line - so the rows past the ceiling are never held at all.
        //
        // **What changed with `docs/adr/0039`:** the batches are KEPT as Arrow rather than decoded
        // into this crate's own text rows. The check is the same check, in the interior now, so the
        // engine's own collection goes through it too and there is no lenient second copy of it.
        //
        // **The limit beside it:** the ceiling is a bound on THIS PROCESS, not the working-set
        // check `sutura_app` makes above. A leg carries no `LIMIT`, so this is the only number
        // standing between a driver that streams without end and this process's memory.
        let mut accumulating = Accumulating::announcing(announced, MOST_RESULT_ROWS);
        for batch in reader {
            accumulating
                .push(batch.map_err(AdbcError::Batch)?)
                .map_err(AdbcError::Unannounced)?;
        }
        // **A full drain IS completeness for ADBC.** The wire transport that used to be here
        // refused a first page by comparing a delivered count against the endpoint's own
        // `totalRows`; an ADBC read streams the whole result, so completeness is the loop above
        // consuming the reader to exhaustion and turning any error on the way into an `Err`
        // instead of a short answer. Nothing reads the driver's schema metadata, so there is no
        // reported total to compare against - and the `Reported`/`Incomplete` pair that existed to
        // hold one is gone rather than left reachable only from its own tests.
        Ok(accumulating.finish())
    }

    fn validate(&self, _request: &JobRequest<'_>) -> Result<DryRunEstimate, Self::Error> {
        // No ADBC call maps onto BigQuery's free `dryRun`; the port keeps this honest rather than
        // billing a guess. `Ok(None)` was the other option and is refused: it would mean *the
        // endpoint accepted this statement*, which nobody asked, and the conformance pack's
        // `estimated_bytes.is_some() == PRICES_DRY_RUN` comparison would fail on it too.
        Err(AdbcError::NoDryRun)
    }

    /// `true` for [`AdbcError::NoDryRun`] alone, which is what makes a question answerable here.
    ///
    /// **Until this existed a configured ADBC source could not answer ANYTHING**, and the path is
    /// worth naming because nothing in this crate showed it: `sutura_app::answer` calls
    /// `Warehouse::dry_run` before `execute`, and an `Err` that is neither a spent deadline nor a
    /// source refusal becomes `ServiceError::Warehouse` - a service error, on every question.
    /// Round 4 of `telekom/sutura#929`'s review measured it as *adoption scaffolding, not an
    /// adopted transport*.
    ///
    /// Matched on the VARIANT and never on [`AdbcError::Uncovered`]'s text: `list_tables` also
    /// answers `Uncovered`, and a predicate keyed on a `&'static str` would read a listing this
    /// transport cannot do as a dry run it declined.
    fn declined_to_dry_run(&self, error: &Self::Error) -> bool {
        matches!(*error, AdbcError::NoDryRun)
    }

    /// `true` for the ROW CEILING alone, which is the one failure here that is a result not fitting.
    ///
    /// **The port's default is `false` and that was wrong for this transport once
    /// [`MOST_RESULT_ROWS`] existed.** A stream refused for crossing the ceiling is exactly *the
    /// result did not fit*: the caller cannot get it whatever it retries, and it is a governance
    /// outcome rather than an outage - so leaving it at the default reached a caller as a `503`
    /// inviting a retry that returns the same stream. The predicate is what makes
    /// `BigQueryWarehouse::result_did_not_fit`'s own doc true, which is why it is here and not a
    /// sentence there.
    ///
    /// Every other [`UnannouncedBatch`] is `false`, exhaustively and by NAME: a mislabelled or
    /// mis-width batch is a driver disagreeing with its own announced schema, which no narrower
    /// request fixes and which a retry may not repeat.
    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        match *error {
            AdbcError::Unannounced(ref cause) => match *cause {
                UnannouncedBatch::OverBound { .. } => true,
                UnannouncedBatch::Width { .. } | UnannouncedBatch::Mislabelled { .. } => false,
            },
            AdbcError::Load(_)
            | AdbcError::Adbc(_)
            | AdbcError::Batch(_)
            | AdbcError::Uncovered(_)
            | AdbcError::NoDryRun
            | AdbcError::Parameters { .. }
            | AdbcError::SubjectSource { .. }
            | AdbcError::UnusableTarget
            | AdbcError::NoRandomness { .. } => false,
        }
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
    /// `crate::adbc::tests` (which is where nextest reports it, and this said `crate::tests`) reads
    /// the OUTCOME one layer up, where `sutura_app::preflight` turns this
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

/// The account every cell in this module tree declares for its subject.
///
/// **One definition for three suites** (`adbc::tests`, `adbc::identity::tests`,
/// `adbc::subject::tests`), because three copies of the same fixture address is what
/// `check-jscpd` is for. A real service-account address rather than a placeholder, so a cell
/// asserting the URL asserts the shape an operator actually declares.
#[cfg(test)]
pub(crate) fn a_declared_account() -> sutura_domain::identity::PrincipalName {
    sutura_domain::identity::PrincipalName::parse("bq-analyst@acme.iam.gserviceaccount.com")
        .expect("a fixture account is an account")
}

#[cfg(test)]
mod tests;

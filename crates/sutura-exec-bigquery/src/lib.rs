//! A [`Warehouse`] adapter over `BigQuery`: render the plan, push it down, map the rows back.
//!
//! `BigQuery` is the first source in this repository that is a **network service with its own
//! authorization**, which is what makes it the interesting one: a file engine has nobody else to be,
//! and a dataset has grants that belong to somebody. Everything about that is declared here rather
//! than assumed - see *identity*, below.
//!
//! # What is built, and what is NOT
//!
//! **This crate does not contain an HTTP client, and that is a decision rather than an omission.**
//! What it contains is everything this adapter DECIDES:
//!
//! - the credential match and the posture agreement, so a leg cannot execute as an identity nobody
//!   declared;
//! - the rendering, through `sutura-sql` in [`Dialect::BigQuery`], so no second set of quoting and
//!   placeholder decisions exists here;
//! - the refusal of a federated leg, because there is no combiner above it;
//! - the value mapping, which is where a wrong number would come from;
//! - the boot pre-flight, which asks each dataset once - not once per model - whether it holds the
//!   tables the bundle names, so a mistyped table name costs a boot refusal here as it already does
//!   on a `files` deployment rather than a failed answer for whoever asks first.
//!
//! **A limit of that mapping, stated because it decides what a time column on this source is:**
//! [`transport::FieldType`] reads `DATE` and refuses `TIMESTAMP` and `DATETIME` - a timestamp arrives
//! as epoch-seconds text the `Date` arm cannot parse, so either comes back `Unmapped` and fails the
//! answer, which is the correct and loud outcome. A time column therefore has to be a `DATE` here.
//!
//! The **wire** - one [`transport::JobTransport`] that speaks to the endpoint - is [`wire`], behind
//! the default-off `wire` feature. `docs/adr/0018` is the decision that produced it and prices what
//! it costs; the two reasons it was absent are answered rather than repealed:
//!
//! 1. The dependency addition turned out to be **zero new packages in `Cargo.lock`**, measured:
//!    `ureq` at the resolved version and features is already in the graph under `libduckdb-sys`. The
//!    feature is default-off anyway, so which side of the build its TLS stack is compiled on stays a
//!    decision a composition root makes in a manifest line.
//! 2. **Nothing in CI can verify it; a developer's own project now has.** On 2026-08-30 the three
//!    `#[ignore]`d tests in `tests/acceptance.rs` passed against a real dataset under a
//!    service-account key - the first statement this repository generated to be accepted by
//!    `BigQuery`. **What that one is, exactly:** one hand-built `SUM` over a two-column
//!    fixture, so it says nothing about a join, `COUNT(DISTINCT`, `CASE WHEN`, a `NULLIF` ratio or
//!    `ISOWEEK` - and the last is one of the two constructs `docs/adr/0017` measured the parse check
//!    to be blind about. **The corpus-wide leg is `tests/corpus.rs`**, behind the default-off
//!    `fixtures` feature: it loads the example fixtures into four tables through
//!    [`BigQueryWarehouse::load_fixture`], runs the corpus questions, and compares its rows with the
//!    engine's for the same plan. That is where the join, the ratio and `ISOWEEK` are reached.
//!
//! So nothing here may be cited as an invariant. `sutura-serve` DOES link this adapter and dispatch
//! `kind: bigquery` behind its default-off `bigquery` feature - `docs/adr/0017`'s second amendment
//! records the day the last *not wired* was spent. A default build links none of it, and
//! the `data_systems:` axis of the golden matrix gains no entry - because a cell in that registry
//! runs inside `just test` and this one cannot: the nix sandbox has no network, so acceptance is a
//! `nix run` app and not a `checks.*` output.
//!
//! # Identity
//!
//! [`BigQueryWarehouse::IMPERSONATION`] is `PerSubjectCredential`, which is what makes a source
//! executed as the asking subject representable here: the credential a broker mints for the asker is
//! carried as a [`Presented::SubjectToken`] and sent as this job's bearer, so the dataset evaluates
//! the statement under whoever that token is. The [`wire`]'s own credential source stays for the
//! shared posture. Per-subject execution still needs a broker that mints a per-leg credential through
//! a token exchange - this crate performs no exchange, it presents one - and that broker lives beside
//! the composition root that links this adapter, which is the half `docs/implementation-plan-bigquery.md`
//! describes as not wired.
//!
//! **ONE of the two subject shapes, and the other is refused rather than degraded.** A
//! [`Presented::SubjectPrincipal`] is a principal the data system switches to on a connection the
//! DEPLOYMENT authenticated, and `BigQuery` has no such mechanism; it is the same POSTURE as a
//! subject token, so [`Presented::agrees_with`] passes it
//! and only this adapter can say it has nowhere to put it. [`BigQueryError::NoPrincipalSwitch`] is
//! that refusal, and the reason it is a refusal is the reason the whole-shape `NoPlaceForASubject`
//! it replaced existed: a leg accepted here would be submitted under the transport's own credential
//! while provenance, read off this source's posture, reported the answer as impersonated.
//!
//! **What no version of this is:** a deployment where a served source executes as its asker.
//! `sutura-serve` refuses an `impersonation-at-source` `bigquery` entry by name, because no broker
//! that exchanges is attached to a served source yet - see that crate's `build_bigquery`.
//!
//! # Two things this adapter deliberately does not offer
//!
//! **No arbitrary SQL entry point.** [`BigQueryWarehouse::execute`] takes an [`Executable`] and
//! renders the statement itself; `transport::JobRequest::new` is `pub(crate)`, so there is no way to
//! hand a statement to a transport from outside this crate. **The `fixtures` feature does not open
//! one:** `load_fixture` takes a table name and a path, and `crate::importer` renders the statement
//! from names that parsed and cells that parsed - refusing, rather than escaping, a cell that could
//! close a literal.
//!
//! **No result caching.** Under row-level security a query-keyed cache is a cross-user leak, and this
//! is the first adapter where there would be row-level security to leak through.

use std::collections::BTreeSet;

use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture};
#[cfg(feature = "fixtures")]
use sutura_domain::model::TableName;
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::preflight::TablesPresent;
use sutura_domain::warehouse::{AnchorRows, MalformedRowSet, NotFinite, PreFlight, RowSet, Warehouse};
use sutura_sql::generate::generate;
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};

mod rowset;

mod preflight;

pub mod transport;
#[cfg(feature = "wire")]
pub mod wire;

mod identity_read;
pub use identity_read::SessionUser;

// The fixture loader, behind the default-off `fixtures` feature. `Cargo.toml` carries the argument
// for why it is a feature and not simply a `#[cfg(test)]` helper: an INTEGRATION test target is a
// separate crate, so it cannot reach a `#[cfg(test)]` item here, and a method that issues
// `CREATE OR REPLACE TABLE` is one no shipped build should contain.
#[cfg(feature = "fixtures")]
mod importer;
#[cfg(feature = "fixtures")]
pub use crate::importer::{Dropped, FixtureNotLoaded, FixtureNotUsable, Loaded};

mod sts;
pub use sts::{StsCredential, StsExchange, SystemClock, UnixClock, WorkloadIdentity, WorkloadIdentityBroker};

use crate::transport::{DatasetId, JobRequest, JobRows, JobTransport, ProjectId};

/// One fallible step of this adapter.
///
/// Named because `Result<V, BigQueryError<T::Error>>` is over the `type_complexity` threshold this
/// workspace tightened, and because the generic error is the point: erasing it would lose which
/// transport failed.
type Mapped<V, E> = Result<V, BigQueryError<E>>;

/// Why this data system could not answer.
///
/// Generic in the transport's own error, for the reason `sutura_app::ServiceError` is generic in the
/// adapter's: erasing it here would mean a caller that knows which transport is installed could no
/// longer tell a refused credential from a dropped connection. The chain still walks - the cause is
/// an owned `#[source]`.
#[derive(Debug, thiserror::Error)]
pub enum BigQueryError<E>
where
    E: core::error::Error + 'static,
{
    /// The endpoint did not answer.
    #[error("the data system did not answer")]
    Endpoint {
        #[source]
        cause: E,
    },
    /// The plan would not render.
    #[error("the plan could not be rendered as GoogleSQL")]
    Render {
        #[source]
        cause: GenerateError,
    },
    /// A federated leg arrived, and there is nothing above it to combine legs.
    ///
    /// **A refusal to execute rather than an execution**, worded as `sutura-exec-duckdb` words it: a
    /// leg run with nothing above it returns rows at a finer grouping than the question asked for,
    /// which is a wrong number under a certified name.
    #[error("a leg of a federated plan over {table} arrived, and there is no combiner above it")]
    LegWithoutCombiner { table: String },
    /// The leg presents a principal for the data system to switch to, and there is no such
    /// mechanism here.
    ///
    /// **The narrow half of a refusal that used to be wholesale, and it has to stay refused.** This
    /// adapter declares `PerSubjectCredential` and delivers exactly one of the two subject shapes: a
    /// [`SubjectToken`](sutura_domain::identity::Presented::SubjectToken) rides as this job's bearer, so the
    /// dataset evaluates the statement under whoever the token is. `BigQuery` has no proxy-user or
    /// `SET ROLE` equivalent for a [`SubjectPrincipal`](sutura_domain::identity::Presented::SubjectPrincipal),
    /// so a leg carrying one has no material to send - and
    /// [`agrees_with`](sutura_domain::identity::Presented::agrees_with) passes it, because the two shapes are
    /// the same POSTURE. Accepting
    /// it would submit the job under the credential the transport already holds while provenance,
    /// read off this source's posture, reported the answer as impersonated: every row as the
    /// process, recorded as the asker.
    #[error("{presented} was minted for {at}, and this adapter can only send a subject's own bearer token")]
    NoPrincipalSwitch { at: String, presented: &'static str },
    /// The leg's credential and this source's declared posture do not agree.
    #[error("the credential presented for this source does not agree with the posture it was opened under")]
    PresentedDisagreesWithPosture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
    /// A column came back as a type this adapter does not map.
    ///
    /// It NAMES the type rather than answering null, which is the whole reason
    /// [`FieldType::Unmapped`](crate::transport::FieldType::Unmapped) carries the endpoint's own
    /// spelling.
    #[error("column {column} came back as {named}, which this adapter does not map")]
    UnmappedType { column: String, named: String },
    /// A cell declared `INT64` did not parse as one.
    ///
    /// **Two variants rather than one carrying a `&'static str`, because the CAUSE differs.** The
    /// endpoint sends every value as text, so "declared an integer" and "parses as an integer" are
    /// two facts, and the standard-library error that says why is worth keeping on the chain.
    #[error("column {column} is declared INT64 and its value did not parse as one")]
    NotAnInteger {
        column: String,
        #[source]
        cause: core::num::ParseIntError,
    },
    /// A cell declared `FLOAT64` did not parse as one.
    #[error("column {column} is declared FLOAT64 and its value did not parse as one")]
    NotADouble {
        column: String,
        #[source]
        cause: core::num::ParseFloatError,
    },
    /// A cell declared `BOOL` was neither `true` nor `false`.
    ///
    /// No `#[source]`: there is no parse behind it, because the check is a comparison against the two
    /// spellings the endpoint documents. A variant with an invented cause would be worse than none.
    #[error("column {column} is declared BOOL and its value was neither true nor false")]
    NotABool { column: String },
    /// A double came back non-finite.
    ///
    /// **What this arm actually guards, on THIS target, is narrower than the two SQL adapters
    /// agreeing.** In `GoogleSQL` the `/` operator raises on a zero divisor for every numeric type -
    /// only `IEEE_DIVIDE` answers `inf`/`NaN` - so an unguarded zero-division ratio fails at the
    /// service first, as [`Self::Endpoint`] with the same `503` as a dead data system. What reaches
    /// this arm is a non-finite value STORED in a `FLOAT64` column, and the check keeps that stored
    /// `Infinity` from answering a real under a certified metric name. It is `sutura-exec-duckdb`'s
    /// same arm that gives `zero_denominator: fails` its meaning, because there the unguarded `/`
    /// does answer `inf`; the sentence that credits this arm with the ratio case belongs to `DuckDB`.
    #[error("column {column} came back as a non-finite number")]
    NotFinite {
        column: String,
        #[source]
        cause: NotFinite,
    },
    /// A cell declared as a date did not parse as one.
    #[error("column {column} is declared a date and its value did not parse as one")]
    NotADate {
        column: String,
        #[source]
        cause: sutura_domain::calendar::InvalidDate,
    },
    /// A row had more or fewer cells than the schema had columns.
    ///
    /// Distinct from [`Self::Shape`]: this one is the ENDPOINT disagreeing with itself, caught before
    /// a row is built, so the position of the offending row is reportable.
    #[error("row {row} came back with {cells} cells and the schema declared {columns} columns")]
    RowWidth { row: usize, cells: usize, columns: usize },
    /// The endpoint delivered a page whose row count is not what it reported as total.
    ///
    /// `jobs.query` answers one page at a time, and completeness is stated as `totalRows` beside the
    /// rows - never by the rows alone. A first page, or an incomplete job's empty `rows`, would read
    /// to `answer()` as *under the cap, not truncated*: a wrong number under a certified name, through
    /// the exact row the row-cap invariant exists to hold. So a delivered count that does not equal the
    /// reported total is refused here, at the seam, rather than certified.
    #[error("the endpoint delivered {delivered} rows and reported {total} total")]
    Incomplete { delivered: usize, total: usize },
    /// The identity read came back as something other than one row of one text cell.
    ///
    /// Its own variant rather than [`Self::RowWidth`] or [`Self::Shape`], because what a caller does
    /// about it is different: those two are a result set this adapter could not map, and this is
    /// *the endpoint did not tell us who ran the job* - which for the one caller that asks
    /// ([`BigQueryWarehouse::session_user`](crate::BigQueryWarehouse::session_user)) is the whole
    /// answer rather than a cell of it.
    ///
    /// **It carries the SHAPE and never the value**, deliberately. The one thing this answer can
    /// contain is an account identifier, and the venue that reads it writes to a public log - so a
    /// refusal that quoted what came back would be the disclosure the read exists to check for.
    #[error("the identity read answered {rows} row(s) of {columns} column(s), which is not one identity")]
    NoIdentityInTheAnswer { rows: usize, columns: usize },
    /// The result set could not be built.
    #[error("the rows did not form a result set")]
    Shape {
        #[source]
        cause: MalformedRowSet,
    },
}

/// A `BigQuery` dataset, behind the [`Warehouse`] port.
///
/// Generic in its transport rather than holding a boxed one: there is one per process, it is chosen at
/// composition, and a generic keeps the transport's own error type visible in [`BigQueryError`].
#[derive(Debug)]
pub struct BigQueryWarehouse<T> {
    source: SourceName,
    /// Which identity a query reaches this source as, handed over at construction.
    ///
    /// The deployment declares the posture and the adapter declares its *capability* - see
    /// [`Warehouse::IMPERSONATION`]. Kept so provenance is read off the thing that executed.
    posture: SourcePosture,
    billing_project: ProjectId,
    default_dataset: DatasetId,
    transport: T,
}

impl<T> BigQueryWarehouse<T>
where
    T: JobTransport,
{
    /// Opens a dataset.
    ///
    /// **Every argument is required and none has a default**, which is the shape the port asks for and
    /// the reason is different for each: a defaulted posture would be a claim about who a query runs
    /// as that nobody made, and a defaulted billing project would be a project somebody else pays
    /// for. The billing project is the caller's to supply because there is nothing to infer it from -
    /// it is a path segment of the request that submits a job, and a federated identity has no project
    /// of its own.
    pub const fn new(
        source: SourceName,
        posture: SourcePosture,
        billing_project: ProjectId,
        default_dataset: DatasetId,
        transport: T,
    ) -> Self {
        Self {
            source,
            posture,
            billing_project,
            default_dataset,
            transport,
        }
    }

    /// Whether this leg's credential agrees with how the source was declared.
    ///
    /// **One exhaustive match, called by every port method that takes a credential**, for the reason
    /// `sutura-exec-duckdb` gives: a copy per method is two places for the arms to disagree, and the
    /// pre-flight is the call where a missing check would matter least and be noticed least.
    ///
    /// **Two questions in order, and they are different questions.** The first is *can this adapter
    /// deliver the SHAPE it was handed*, which [`Presented::agrees_with`] cannot answer - see
    /// [`BigQueryError::NoPrincipalSwitch`]. The second is *does the shape agree with how the source
    /// was DECLARED*, which is `agrees_with`'s exhaustive match over the pair.
    ///
    /// The order is the point rather than an accident: a principal switch at a source declared
    /// shared is refused as the disagreement it is, because that is what an operator would fix,
    /// which is why the shape check reads the posture too rather than the variant alone.
    fn deliverable(&self, presented: &Presented) -> Mapped<(), T::Error> {
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| BigQueryError::PresentedDisagreesWithPosture { cause })?;
        match *presented {
            Presented::SubjectToken { .. } | Presented::SharedServiceUser { .. } => Ok(()),
            Presented::SubjectPrincipal { .. } => Err(BigQueryError::NoPrincipalSwitch {
                at: String::from(self.source.as_str()),
                presented: presented.as_str(),
            }),
        }
    }

    /// The plan, rendered as one `GoogleSQL` statement.
    ///
    /// The dialect is not a parameter: a `BigQuery` adapter renders `BigQuery`. One exhaustive match,
    /// so a third plan shape cannot be answered by accident, and the leg arm refuses rather than
    /// renders.
    fn render(executable: Executable<'_>) -> Mapped<GeneratedQuery, T::Error> {
        match executable {
            Executable::Query(plan) => generate(plan, Dialect::BigQuery).map_err(|cause| BigQueryError::Render { cause }),
            Executable::Leg(leg) => Err(BigQueryError::LegWithoutCombiner {
                table: leg.table().to_string(),
            }),
        }
    }

    /// The credential one leg presents, as a bearer this job may send.
    ///
    /// `None` for the shared posture, which carries no material: that leg runs under the identity the
    /// transport already holds. The principal-switch shape never reaches here - [`Self::deliverable`]
    /// refuses it as [`BigQueryError::NoPrincipalSwitch`] - and its arm stays exhaustive rather than
    /// wildcarded so a fourth presented shape is a compile error at this line.
    const fn subject_bearer(presented: &Presented) -> Option<&sutura_domain::identity::Secret> {
        match presented {
            Presented::SubjectToken { material } => Some(material),
            Presented::SubjectPrincipal { .. } | Presented::SharedServiceUser { .. } => None,
        }
    }

    /// The request one rendered statement becomes.
    ///
    /// Named rather than inlined at three call sites, because the thing it decides is that the
    /// statement and its values travel in SEPARATE fields - which is the no-injection invariant at the
    /// point where this adapter would be the one to break it - and that the asking subject's bearer
    /// (where there is one) rides beside them rather than in the SQL.
    ///
    /// `subject_bearer` is the [`Presented::SubjectToken`]'s material extracted by [`Self::subject_bearer`],
    /// and `None` at the boot path, where there is no caller to present one.
    fn request<'job>(
        &'job self,
        query: &'job GeneratedQuery,
        subject_bearer: Option<&'job sutura_domain::identity::Secret>,
    ) -> JobRequest<'job> {
        JobRequest::new(
            query.sql(),
            query.params(),
            &self.billing_project,
            &self.default_dataset,
            subject_bearer,
        )
    }

    /// Replaces one table in the connection's dataset with the rows of a committed fixture CSV.
    ///
    /// **The mirror of #78's `PostgresWarehouse::load_csv`, and it exists for the reason that one
    /// does: a relational data system has to be GIVEN tables before a corpus can be run against it,
    /// and the example models are files.** The differences from the Postgres shape are in
    /// [`crate::importer`]'s header - there is no `COPY`, so the rows travel inside the statement and
    /// every cell is re-rendered from a parsed value.
    ///
    /// **Behind the `fixtures` feature, so no shipped build holds it.** `Cargo.toml` carries that
    /// argument. What it buys over a `#[cfg(test)]` helper is that the acceptance leg is an
    /// INTEGRATION target - a separate crate - which cannot reach a test-gated item here.
    ///
    /// It takes a table name and a path and never a statement, which is what keeps *no arbitrary SQL
    /// entry point* true of this crate: the statement is rendered from names that parsed and cells
    /// that parsed.
    ///
    /// **In THIS impl block rather than in the module that renders the statement**, because
    /// `clippy::multiple_inherent_impl` is denied here and it is right to be: a type whose inherent
    /// methods are spread over files is one whose surface nobody can read in one place.
    ///
    /// Returns how many data rows the fixture carried, so a caller can assert the load moved what the
    /// file holds rather than trusting a green.
    #[cfg(feature = "fixtures")]
    pub fn load_fixture(&self, table: &TableName, csv: &std::path::Path) -> Loaded<T::Error> {
        let text = std::fs::read_to_string(csv).map_err(|cause| FixtureNotLoaded::Unreadable {
            path: csv.display().to_string(),
            cause,
        })?;
        let fixture = crate::importer::read_fixture(&text).map_err(|cause| FixtureNotLoaded::NotUsable {
            path: csv.display().to_string(),
            cause,
        })?;
        let statement = fixture.create_statement(table);
        // No parameters, deliberately, and the no-injection invariant is about exactly this
        // position: a bind parameter carries a VALUE FROM A QUESTION, and there is no question here.
        // What makes the literals safe is that each one was parsed - see `crate::importer`.
        // And no subject bearer for the same reason: a `CREATE OR REPLACE TABLE` is a thing the
        // identity this transport already holds does to its own dataset, so handing it a subject's
        // exchanged token would run a write under whoever last asked a question.
        let request = JobRequest::new(&statement, &[], &self.billing_project, &self.default_dataset, None);
        self.transport
            .apply(&request)
            .map_err(|cause| FixtureNotLoaded::Endpoint { cause })?;
        Ok(fixture.rows())
    }

    /// Removes one table from the connection's dataset.
    ///
    /// **The tidy half of per-run fixture cleanup.** A run names its tables with a per-run suffix
    /// (see `tests/corpus.rs`), so what it removes is its OWN tables and never a colleague's. The
    /// [`crate::importer`] header states the guarantee half - every `CREATE` also carries a 24-hour
    /// expiration, because `panic = "abort"` means a cancelled runner never reaches this method and
    /// the expiration is what still cleans up after it.
    ///
    /// It takes a table name and never a statement, for the same reason `load_fixture` does: the
    /// statement is rendered from a name that parsed, and *no arbitrary SQL entry point* stays true.
    ///
    /// Behind the same `fixtures` feature and in the same impl block, for `load_fixture`'s reasons.
    #[cfg(feature = "fixtures")]
    pub fn drop_table(&self, table: &TableName) -> Dropped<T::Error> {
        let statement = crate::importer::Fixture::drop_statement(table);
        // No parameters and no subject bearer, exactly as the load: a DROP is a thing the identity
        // this transport already holds does to its own dataset, like the `CREATE` that built it.
        let request = JobRequest::new(&statement, &[], &self.billing_project, &self.default_dataset, None);
        self.transport
            .apply(&request)
            .map_err(|cause| FixtureNotLoaded::Endpoint { cause })
    }

    /// Who this data system says the leg presenting `presented` is executing AS.
    ///
    /// **The observable for the claim this adapter's `IMPERSONATION` constant makes.** A
    /// [`Presented::SubjectToken`] rides as this job's own bearer, so what the endpoint resolves
    /// that bearer to IS the identity the source executed under - and asking the source rather than
    /// asserting it is the difference between evidence and a comment. `docs/adr/0008` names
    /// `SESSION_USER()` as the primitive; `SESSION_USER` is the only statement this can issue.
    ///
    /// It goes through [`Self::deliverable`] like every other credential-taking method, so a leg
    /// whose credential disagrees with the source's posture is refused here too rather than being
    /// answered by a read that looks harmless. The [`SessionUser`] answer redacts under `Debug`;
    /// explicit access and `Display` still reveal it. Neither this read nor its return type
    /// establishes how the bearer was obtained.
    ///
    /// **Not part of the [`Warehouse`] port, and that is a decision rather than an omission.** No
    /// other adapter can answer it - `sutura-exec-datafusion` and `sutura-exec-duckdb` execute in
    /// process under one identity, so a defaulted method would answer *the process* and read as
    /// though it had asked. An inherent method is reachable by the one venue that needs it and by
    /// nothing that federates.
    ///
    /// # Errors
    ///
    /// [`BigQueryError::Endpoint`] where the endpoint did not answer,
    /// [`BigQueryError::Incomplete`] where the page and the reported total disagree, and
    /// [`BigQueryError::NoIdentityInTheAnswer`] where the answer is not one row of one text cell.
    /// Nothing here quotes what came back: see that variant.
    pub fn session_user(&self, presented: &Presented) -> Mapped<SessionUser, T::Error> {
        identity_read::session_user(self, presented)
    }
    /// A job's result, as a domain result set. The mapping itself is [`crate::rowset`], which is
    /// where a wrong number would come from; this is the seam the port's methods call.
    fn rows(answered: &JobRows) -> Mapped<RowSet, T::Error> {
        rowset::rows(answered)
    }

    /// Whether an adapter error wraps the configured transport REFUSING: answered for the
    /// [`BigQueryError::Endpoint`] variant by `transport_says` and `false` for everything else -
    /// the ONE place this adapter decides a non-`Endpoint` error is never a transport refusal.
    /// [`Self::preflight_was_refused`] and [`Self::source_refused`] delegate here, so
    /// boot-time and query-time cannot diverge.
    fn refused_via(error: &BigQueryError<T::Error>, transport_says: impl Fn(&T::Error) -> bool) -> bool {
        match *error {
            BigQueryError::Endpoint { ref cause } => transport_says(cause),
            BigQueryError::NoIdentityInTheAnswer { .. }
            | BigQueryError::Render { .. }
            | BigQueryError::LegWithoutCombiner { .. }
            | BigQueryError::NoPrincipalSwitch { .. }
            | BigQueryError::PresentedDisagreesWithPosture { .. }
            | BigQueryError::UnmappedType { .. }
            | BigQueryError::NotAnInteger { .. }
            | BigQueryError::NotADouble { .. }
            | BigQueryError::NotABool { .. }
            | BigQueryError::NotFinite { .. }
            | BigQueryError::NotADate { .. }
            | BigQueryError::RowWidth { .. }
            | BigQueryError::Incomplete { .. }
            | BigQueryError::Shape { .. } => false,
        }
    }
}

impl<T> Warehouse for BigQueryWarehouse<T>
where
    T: JobTransport,
{
    type Error = BigQueryError<T::Error>;

    /// **How this adapter can carry a subject.** A leg presenting [`Presented::SubjectToken`] or
    /// [`Presented::SubjectPrincipal`] has somewhere to go: the token rides as this job's bearer so
    /// the endpoint evaluates under the asker, and a principal name is what the endpoint switches to.
    /// So a source declared `impersonation-at-source` can be opened here, and [`Self::deliverable`]
    /// accepts the two subject shapes instead of refusing them.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    /// Validates the statement without reading data, as the identity this leg presents.
    ///
    /// **Answers [`PreFlight::Accepted`] because it really asked**, which is the one thing the port's
    /// default cannot honestly return. A dry run at this endpoint validates the query and returns a
    /// result schema without using slots and without being charged, so the round trip is worth making
    /// here in a way it is not for an in-process engine - the port's own documentation draws exactly
    /// that line.
    fn dry_run(&self, executable: Executable<'_>, presented: &Presented) -> Result<PreFlight, Self::Error> {
        self.deliverable(presented)?;
        let query = Self::render(executable)?;
        self.transport
            .validate(&self.request(&query, Self::subject_bearer(presented)))
            .map_err(|cause| BigQueryError::Endpoint { cause })?;
        Ok(PreFlight::Accepted)
    }

    fn execute(&self, executable: Executable<'_>, presented: &Presented) -> Result<RowSet, Self::Error> {
        self.deliverable(presented)?;
        let query = Self::render(executable)?;
        let answered = self
            .transport
            .run(&self.request(&query, Self::subject_bearer(presented)))
            .map_err(|cause| BigQueryError::Endpoint { cause })?;
        Self::rows(&answered)
    }

    /// Re-runs an anchor's plan, under the one identity this adapter was configured with.
    ///
    /// It takes no credential because there is no caller at boot, and an [`AnchorPlan`] rather than a
    /// bare plan, so the one method here needing no credential cannot be handed a caller's question.
    /// [`AnchorRows`] is what keeps the result from being handed back to one as an answer.
    ///
    /// **What an executed anchor proves here is narrower than on a file engine, and this is the first
    /// adapter where that bites:** a dataset has grants, so these numbers reproduced *for the identity
    /// this adapter holds*. Under row-level security that is not necessarily any caller's.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        let query = generate(plan.plan(), Dialect::BigQuery).map_err(|cause| BigQueryError::Render { cause })?;
        let answered = self
            .transport
            .run(&self.request(&query, None))
            .map_err(|cause| BigQueryError::Endpoint { cause })?;
        Self::rows(&answered).map(AnchorRows::of)
    }

    /// Asks the dataset which of the bundle's tables it holds. [`crate::preflight`] is the whole
    /// answer, including what a short listing does and does not establish.
    fn preflight(&self, tables: &BTreeSet<QualifiedTable>) -> Result<TablesPresent, Self::Error> {
        crate::preflight::preflight(self, tables)
    }

    /// Whether the endpoint REFUSED to list a dataset, rather than failing to answer about one.
    ///
    /// **The reason for the split is in the port's own documentation:** a `403` on `tables.list` is a
    /// grant an operator can add and will fail identically on every boot, while a `503` from the
    /// endpoint is a condition that passes - and before this they were the same permanent warning in
    /// the deployment least likely to read a startup log.
    ///
    /// It asks the TRANSPORT, for [`Self::result_did_not_fit`]'s reason: the HTTP status is a fact
    /// about the wire and `T::Error` is the transport's own type. Every other variant is `false`
    /// exhaustively rather than through a wildcard, so a variant added to [`BigQueryError`] has to be
    /// decided here - and `false` is the reading that keeps a deployment serving.
    fn preflight_was_refused(&self, error: &Self::Error) -> bool {
        Self::refused_via(error, |cause| self.transport.listing_was_refused(cause))
    }

    /// Was this [`execute`](Warehouse::execute) failure the endpoint REFUSING the statement at the
    /// identity/authorization level? The query-time sibling of [`Self::preflight_was_refused`]: an
    /// authorization refusal used to leave [`BigQueryError::Endpoint`] as a `503`. It asks the
    /// TRANSPORT through [`JobTransport::job_was_refused`]; the classification is [`Self::refused_via`].
    fn source_refused(&self, error: &Self::Error) -> bool {
        Self::refused_via(error, |cause| self.transport.job_was_refused(cause))
    }

    // `working_set_exhausted` is deliberately NOT overridden. The port's default is `None`, and that
    // is the honest answer for an adapter with no local memory pool to bound: the work happens at the
    // endpoint, and a query refused there for its own resource reasons is not this deployment's
    // configured ceiling refusing a reservation. Answering otherwise would tell a caller not to retry
    // something a retry would have answered.

    /// Whether the endpoint declined to return the whole result at once.
    ///
    /// **This IS overridden, and it is the one place this adapter has a governance outcome the port's
    /// default would report as an outage.** `jobs.query` answers one page - as many rows as fit the
    /// maximum permitted reply size - so a result under the row cap can still be over the reply
    /// bound, and both of the shapes that says so used to leave here as `BigQueryError` and reach a
    /// caller as `503`: a status that invites a retry returning the same page.
    ///
    /// Two arms answer `true`, and the third case in the same variant deliberately does not:
    ///
    /// - `Endpoint` asks the transport, because the page token is a fact about the wire document and
    ///   `T::Error` is the transport's own type. See `JobTransport::result_did_not_fit`.
    /// - `Incomplete` where the delivered count is **below** the reported total: this is NOT the
    ///   documented paging shape - that is `MoreThanOnePage`, which `complete` refuses at the wire.
    ///   It is a reply that states *total N*, carries no page token, and delivered fewer - the
    ///   endpoint contradicting itself. Answered `true` defensively, because the caller cannot get
    ///   the rest of this reply whatever it retries.
    /// - `Incomplete` where delivered is **above** the total is NOT this, and calling it a governance
    ///   refusal would tell a caller not to retry a defect a retry might well not repeat.
    ///
    /// Exhaustive with no wildcard arm, so a variant added to `BigQueryError` has to be decided here
    /// rather than inheriting `false`.
    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        match *error {
            BigQueryError::Endpoint { ref cause } => self.transport.result_did_not_fit(cause),
            BigQueryError::Incomplete { delivered, total } => delivered < total,
            // `NoIdentityInTheAnswer` joins the `false` group rather than getting an arm of its
            // own: the identity read projects ONE cell, so there is no narrower page to ask for and
            // a retry returns the same shape. `clippy::match_same_arms` is denied here and is right
            // to be - an arm whose body is identical to the group's is a distinction a reader is
            // invited to look for and will not find.
            BigQueryError::NoIdentityInTheAnswer { .. }
            | BigQueryError::Render { .. }
            | BigQueryError::LegWithoutCombiner { .. }
            | BigQueryError::NoPrincipalSwitch { .. }
            | BigQueryError::PresentedDisagreesWithPosture { .. }
            | BigQueryError::UnmappedType { .. }
            | BigQueryError::NotAnInteger { .. }
            | BigQueryError::NotADouble { .. }
            | BigQueryError::NotABool { .. }
            | BigQueryError::NotFinite { .. }
            | BigQueryError::NotADate { .. }
            | BigQueryError::RowWidth { .. }
            | BigQueryError::Shape { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests;

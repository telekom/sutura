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
//! The **transport** - one [`transport::JobTransport`] that executes the statement - is [`adbc`],
//! behind the default-off `adbc` feature: it loads the self-built `libadbc_driver_bigquery.so`
//! (`nix/bigquery-adbc.nix`), runs the query through the driver, and decodes the Arrow result. The
//! driver owns the HTTP transport and its own authentication, so this crate ships no TLS stack and
//! reads no credential file - the previous HTTP `wire` transport and its STS/credential machinery
//! were removed when ADBC became this adapter's only mode.
//!
//! So nothing here may be cited as a round-tripped invariant. `sutura serve` links this adapter and
//! dispatches `kind: bigquery` behind its default-off `bigquery` feature, but the ADBC driver path is
//! not yet a shipped artefact and no live acceptance leg against a real dataset is wired under it -
//! the `wire`-era acceptance/corpus/differential legs went away with the transport. A default build
//! links none of this.
//!
//! # Identity
//!
//! [`BigQueryWarehouse::IMPERSONATION`] is `PerSubjectCredential`, and it is now true by
//! construction rather than by this paragraph: the asking subject's **own verified assertion** is
//! what the data system authenticates. [`adbc`] hands the driver a workload-identity credential
//! document naming a loopback source for that assertion, so Google's token service verifies it
//! against the pool the source declares and the question executes as whatever principal that pool
//! resolves the subject to. `adbc`'s own `subject` module carries the mechanism and its exposure.
//!
//! **Two modes, and they are XOR rather than a ladder.** A source declares one posture and gets one
//! mechanism:
//!
//! | declared posture | mechanism | identity at the source |
//! | --- | --- | --- |
//! | `shared-service-user` | the deployment's own application default credentials, **mandatory** for this posture, impersonating nothing | the deployment |
//! | `impersonation-at-source` | the subject's own assertion, federated against the declared pool | the asking subject |
//!
//! **What does not exist is a path from the second to the first**, and that is the whole point: an
//! impersonating source whose subject cannot be federated is REFUSED
//! (`adbc::identity::authenticate`'s third arm), never answered as the deployment. The reverse is
//! refused one layer up, by [`Presented::agrees_with`]: a shared source handed a subject's
//! credential is a posture disagreement before any transport sees it.
//!
//! **The mechanism this replaced is deleted rather than kept beside it.** For two rounds this
//! adapter set the driver's `bigquery.impersonate.target_principal` from the DEPLOYMENT's own
//! credentials - so the connection was the deployment's, the subject's credential was nowhere in
//! the chain, and leg 1 was the only barrier. `docs/adr/0018`'s fifth amendment records it, and
//! `JobIdentity` has no spelling for it: a principal switch is unrepresentable here, not refused at
//! runtime.
//!
//! **So a [`Presented::SubjectPrincipal`] is refused.** It is the same POSTURE as an assertion to
//! the domain, so [`Presented::agrees_with`] passes it and only this adapter can say it has no
//! mechanism - [`BigQueryError::NoPrincipalSwitch`]. `GoogleSQL` has no proxy-user or `SET ROLE`
//! equivalent either, so there is nothing to build it out of.
//!
//! **And because leg 1 still gates who may be federated at all, two of ITS limits bound this.**
//! `sutura-http`'s inbound gate bounds a gateway assertion's replay *window* and binds nothing to a
//! request (`within_the_lifetime_ceiling`'s own doc), so inside that window a captured assertion is
//! replayable - and Google would accept it, because it is the caller's real document. And the
//! signing-key age bound `KeySetCache::stale_for` measures excludes a failing refresh. Neither is
//! this crate's to fix; both are cited here because *the limit belongs beside the claim*.
//!
//! **What is still unproven, and no green run here changes it:** nothing reachable from this
//! repository shows Google ACCEPTING the assertion. The document is built and the loopback source
//! is asserted against a real socket; the exchange needs a pool, a project and a hosted run.
//! `docs/where-identity-is-proven.md` records that venue as `wired` and undispatched, so **leg 2 is
//! not proven.**
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
use sutura_domain::plan::{AnchorPlan, Executable, QueryPlan};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::preflight::TablesPresent;
use sutura_domain::warehouse::{AnchorRows, MalformedRowSet, NotFinite, PreFlight, RowSet, Warehouse};
use sutura_sql::generate::generate;
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};

mod rowset;

mod preflight;

#[cfg(feature = "adbc")]
pub mod adbc;

mod resolve;
pub use resolve::UnresolvableConnection;

pub mod transport;

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

/// The broker a served impersonating `BigQuery` source is answered through, and now the only one
/// this crate carries: the exchanging `WorkloadIdentityBroker` was deleted with its HTTP hops
/// (`docs/adr/0018`, eighth amendment), since the ADBC path federates the asker's own assertion at
/// Google's token service instead of exchanging it here.
mod principal;
pub use principal::{DeclaredPrincipalBroker, DeclaredPrincipals, DeclaredPrincipalsUnusable, NoDeclaredPrincipals};

use crate::transport::{DatasetId, JobDeadline, JobIdentity, JobRequest, JobRows, JobTransport, ProjectId};

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
    /// A table path could not be resolved against this connection, or the resolved tables -
    /// taken together - answer to one identifier the statement cannot tell apart.
    ///
    /// **Unreachable in practice**: see [`UnresolvableConnection`]'s own documentation for why. A
    /// typed variant rather than a panic for the same reason every other "unreachable" case in
    /// this workspace is one - the input reaching it is not bounded by the type system alone.
    #[error("a table path could not be resolved for this connection")]
    UnresolvableConnection {
        #[source]
        cause: sutura_domain::plan::ResolveTablesError<UnresolvableConnection>,
    },
    /// A federated leg arrived, and there is nothing above it to combine legs.
    ///
    /// **A refusal to execute rather than an execution**, worded as `sutura-exec-duckdb` words it: a
    /// leg run with nothing above it returns rows at a finer grouping than the question asked for,
    /// which is a wrong number under a certified name.
    #[error("a leg of a federated plan over {table} arrived, and there is no combiner above it")]
    LegWithoutCombiner { table: String },
    /// The leg presents a principal for the data system to switch to, and no transport here has a
    /// spelling for one.
    ///
    /// **Reinstated, and the reason is the mechanism reversal rather than a revert.** For two rounds
    /// this adapter delivered exactly that shape, through the driver's `target_principal` option and
    /// the deployment's own application default credentials - so the connection was the
    /// deployment's and the subject's credential was nowhere in the chain. The owner rejected it,
    /// so [`crate::transport::JobIdentity`] carries one subject arm now and this refusal is what a
    /// broker presenting the other one gets. Accepting it would submit the job under the
    /// credential the transport already holds while provenance, read off this source's posture,
    /// reported the answer as impersonated.
    #[error("{presented} was minted for {at}, and this adapter federates a subject's own assertion instead")]
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

    /// Whether an accepted pre-flight's own estimate agrees with what [`Warehouse::PRICES_DRY_RUN`]
    /// declares.
    ///
    /// **The same comparison `sutura_conformance::execute`'s pack now also makes over this
    /// adapter** (`telekom/sutura#710`, `crates/sutura-exec-bigquery/tests/conformance.rs`) - kept
    /// here as well so it can be checked against this adapter's own dry-run path without going
    /// through a fixture at all. A live endpoint's own guarantee that it always prices one is
    /// still unverified by either cell; this only compares what an already-answered pre-flight
    /// carried against the declaration.
    #[must_use]
    pub const fn dry_run_estimate_agrees_with_its_declaration(estimated_bytes: Option<EstimatedBytes>) -> bool {
        estimated_bytes.is_some() == <Self as Warehouse>::PRICES_DRY_RUN
    }

    /// Whether this leg's credential agrees with how the source was declared.
    ///
    /// **One exhaustive match, called by every port method that takes a credential**, for the reason
    /// `sutura-exec-duckdb` gives: a copy per method is two places for the arms to disagree, and the
    /// pre-flight is the call where a missing check would matter least and be noticed least.
    ///
    /// **One question here now, and the second one moved to where it can be answered.** This used
    /// to ask *does the shape agree with the posture* and then *can this adapter deliver the shape*,
    /// because the adapter itself refused a principal switch outright. It no longer does: the
    /// adapter is generic in its transport, and *which shapes can be delivered* moved to
    /// [`Self::job_identity`] - which refuses [`Presented::SubjectPrincipal`], the shape whose
    /// mechanism was deleted. An adapter-level match over postures AND shapes would have to be
    /// changed for either, which is exactly the coupling `JobTransport` exists to remove.
    fn deliverable(&self, presented: &Presented) -> Mapped<(), T::Error> {
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| BigQueryError::PresentedDisagreesWithPosture { cause })
    }

    /// One query plan, resolved against this connection and rendered as one `GoogleSQL` statement.
    ///
    /// **The resolve step, and why it sits here rather than inside `generate`.** A path this plan
    /// carries that names a dataset and no project is exactly the path `sutura_sql::generate`
    /// would render as literal text, leaving `BigQuery`'s own request-level default to fill the
    /// missing project in - silently, and not necessarily where the model actually lives.
    /// [`crate::resolve::resolve`] closes that gap on the plan itself, before anything renders, so
    /// the statement says which project rather than depending on a request field beside it.
    ///
    /// Shared with [`Self::verify_anchor`]'s own plan, so the boot-time reproduction and a live
    /// question resolve identically rather than one of them keeping the old, unresolved behaviour.
    fn render_query(&self, plan: &QueryPlan) -> Mapped<GeneratedQuery, T::Error> {
        let resolved = plan
            .clone()
            .resolve_tables(|table| resolve::resolve(table, &self.billing_project))
            .map_err(|cause| BigQueryError::UnresolvableConnection { cause })?;
        generate(&resolved, Dialect::BigQuery).map_err(|cause| BigQueryError::Render { cause })
    }

    /// The plan, rendered as one `GoogleSQL` statement.
    ///
    /// The dialect is not a parameter: a `BigQuery` adapter renders `BigQuery`. One exhaustive match,
    /// so a third plan shape cannot be answered by accident, and the leg arm refuses rather than
    /// renders.
    fn render(&self, executable: Executable<'_>) -> Mapped<GeneratedQuery, T::Error> {
        match executable {
            Executable::Query(plan) => self.render_query(plan),
            Executable::Leg(leg) => Err(BigQueryError::LegWithoutCombiner {
                table: leg.table().to_string(),
            }),
        }
    }

    /// What one leg's presented credential means to a transport: two arms, or a refusal.
    ///
    /// **Exhaustive over three shapes and never wildcarded**, so a fourth [`Presented`] variant is a
    /// compile error at this line rather than a shape that falls through to whatever the last arm
    /// was. Two shapes have a [`JobIdentity`](crate::transport::JobIdentity) spelling; the third is
    /// refused HERE and not by a transport, because the mechanism it names was deleted from every
    /// transport in this crate.
    fn job_identity<'leg>(presented: &'leg Presented, source: &SourceName) -> Mapped<JobIdentity<'leg>, T::Error> {
        match presented {
            Presented::SubjectToken { material } => Ok(JobIdentity::AsSubject(material)),
            // **The weaker subject shape, refused because no transport here has a spelling for
            // it.** A principal switch runs the question on a connection the DEPLOYMENT
            // authenticated - this deployment vouching for a subject - and the owner rejected that
            // mechanism, so `JobIdentity` no longer carries it. `agrees_with` passes this shape
            // (both are one POSTURE), so only the adapter can say it cannot be delivered.
            Presented::SubjectPrincipal { .. } => Err(BigQueryError::NoPrincipalSwitch {
                at: String::from(source.as_str()),
                presented: presented.as_str(),
            }),
            Presented::SharedServiceUser { .. } => Ok(JobIdentity::Transport),
        }
    }

    /// The request one rendered statement becomes.
    ///
    /// Named rather than inlined at three call sites, because the thing it decides is that the
    /// statement and its values travel in SEPARATE fields - which is the no-injection invariant at the
    /// point where this adapter would be the one to break it - and that who the job runs as rides
    /// beside them rather than in the SQL.
    ///
    /// `identity` is [`Self::job_identity`]'s reading of what the broker presented, and
    /// [`JobIdentity::Transport`] at the boot path, where there is no caller to present anything.
    fn request<'job>(
        &'job self,
        query: &'job GeneratedQuery,
        identity: JobIdentity<'job>,
        deadline: JobDeadline,
    ) -> JobRequest<'job> {
        JobRequest::new(
            query.sql(),
            query.params(),
            &self.billing_project,
            &self.default_dataset,
            identity,
            deadline,
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
        // And `JobIdentity::Transport` for the same reason: a `CREATE OR REPLACE TABLE` is a thing
        // the identity this transport already holds does to its own dataset, so naming a subject
        // here would run a write as whoever last asked a question. And no port
        // `Deadline`: a fixture load has no caller and no request timeout - the boot path's own shape.
        let request = JobRequest::new(
            &statement,
            &[],
            &self.billing_project,
            &self.default_dataset,
            JobIdentity::Transport,
            JobDeadline::Boot,
        );
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
        // No parameters and `JobIdentity::Transport`, exactly as the load: a DROP is a thing the
        // identity this transport already holds does to its own dataset, like the `CREATE` that
        // built it. And
        // no port `Deadline`, for the same reason `load_fixture` carries none.
        let request = JobRequest::new(
            &statement,
            &[],
            &self.billing_project,
            &self.default_dataset,
            JobIdentity::Transport,
            JobDeadline::Boot,
        );
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
            | BigQueryError::NoPrincipalSwitch { .. }
            | BigQueryError::Render { .. }
            | BigQueryError::UnresolvableConnection { .. }
            | BigQueryError::LegWithoutCombiner { .. }
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

/// A warehouse whose transport is the ADBC driver (telekom/sutura#913).
///
/// Reaching this constructor is what makes the `adbc` module first-party:
/// `BigQueryWarehouse<adbc::AdbcBigQuery>` is a concrete, constructible transport
/// the composition root can pick. Default-off (`adbc` feature), for the reason
/// any out-of-process transport is: the native driver is a per-triple addition.
#[cfg(feature = "adbc")]
impl BigQueryWarehouse<adbc::AdbcBigQuery> {
    /// Opens a dataset over the ADBC transport.
    ///
    /// `driver_path` is the on-disk location of the self-built
    /// `libadbc_driver_bigquery.so` (one per release triple, see
    /// `nix/bigquery-adbc.nix`).
    ///
    /// `impersonation` is whether this source impersonates and at what scope - the source's declared
    /// `workload_identity.scope`, or [`adbc::Impersonation::Disabled`] for a shared one. Taken here
    /// rather than read per request because it is a property of the source, and a declared scope the
    /// driver would refuse then fails before a listener is bound.
    #[must_use]
    pub fn over_adbc(
        source: SourceName,
        posture: SourcePosture,
        billing_project: ProjectId,
        default_dataset: DatasetId,
        driver_path: impl Into<String>,
        impersonation: adbc::Impersonation,
    ) -> Self {
        Self::new(
            source,
            posture,
            billing_project,
            default_dataset,
            adbc::AdbcBigQuery::new(driver_path, impersonation),
        )
    }
}

impl<T> Warehouse for BigQueryWarehouse<T>
where
    T: JobTransport,
{
    type Error = BigQueryError<T::Error>;

    /// **The variant's own name is what this mechanism is, which it was not for two rounds.** The
    /// asking subject's own verified assertion is what reaches the data system: [`Self::job_identity`]
    /// maps [`Presented::SubjectToken`] onto [`JobIdentity::AsSubject`], and [`adbc`] federates it
    /// through an `external_account` credential document so Google's token service verifies it. A
    /// per-subject credential, presented per subject - so `PerSubjectCredential` is the accurate
    /// value and not merely the only workable one.
    ///
    /// **Corrected**: a round of this doc said "nothing a subject possesses arrives, only a
    /// principal the deployment becomes on that subject's behalf", which described the deleted
    /// principal switch. That mechanism is gone and its `JobIdentity` arm with it;
    /// [`Presented::SubjectPrincipal`] is what this adapter now refuses.
    ///
    /// **The limit beside the claim**: this constant says a subject's own credential has somewhere
    /// to go here. It does not say Google has ever accepted one - that is leg 2, it needs a hosted
    /// run, and `docs/where-identity-is-proven.md` records the venue as `wired`.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;

    /// **This declares what this adapter's DATA SYSTEM can do, and the shipped transport has no
    /// call that does it.** Both halves are load-bearing and the second one is new. `BigQuery`'s
    /// `dryRun` uses no slots and is not charged, which is the property `docs/adr/0030` built the
    /// spend ledger on, and it is still true of the endpoint. What the ADBC adoption changed is the
    /// TRANSPORT: there is no ADBC call that prices a statement without running it, so
    /// `crate::adbc::AdbcBigQuery::validate` declines and [`Self::dry_run`] answers
    /// `PreFlight::NotAsked` - never `Accepted` carrying a number nothing computed.
    ///
    /// **So the limit belongs here: on a served deployment nothing prices a statement today.**
    /// `sutura_app`'s spend ledger charges a `NotAsked` pre-flight nothing - *not counted*, never
    /// *free* - so `governance.per_replica_spend_ceiling` does not bound a `BigQuery` source over
    /// ADBC at all. The prose that said *every adapter but `BigQuery`* answers `None` is corrected in
    /// `.agents/skills/sutura/invariants/SKILL.md` rather than left to be inferred from here.
    ///
    /// **Why it stays `true` rather than flipping with the transport.** It is a `Warehouse`
    /// associated constant, so it cannot vary with `T`, and `false` would declare that `BigQuery`
    /// cannot price a dry run - which is not true, and which would refuse
    /// `sutura_conformance::execute`'s own accepted-with-an-estimate case, the pack this adapter is
    /// bound to over a transport that does price (`tests/conformance.rs`'s `Canned`).
    const PRICES_DRY_RUN: bool = true;

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
    /// that line. `estimated_bytes` carries whatever `totalBytesProcessed` the endpoint reported for
    /// THIS statement - `docs/adr/0030` decides the shape; nothing here sums or refuses against it.
    ///
    /// **The deadline is now what `timeoutMs`/`jobTimeoutMs` derive from; `docs/adr/0029`.** This
    /// call's `CallDeadline` opens from what the port's own `Deadline` says is left, read at the
    /// instant this call reaches the wire - not from this adapter's own configured job bounds, which
    /// stay only for the boot path and the socket's own backstop ceiling. See
    /// (the `wire` transport that used to do this was removed with the ADBC adoption).
    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.deliverable(presented)?;
        let query = self.render(executable)?;
        let asked = self.transport.validate(&self.request(
            &query,
            Self::job_identity(presented, &self.source)?,
            JobDeadline::Port(deadline),
        ));
        let estimated_bytes = match asked {
            Ok(estimate) => estimate,
            // **A transport with no dry run did not ask, and that is not a question that failed.**
            // `sutura_app::answer` turns any other `Err` here into a service error on every
            // question, so before this arm existed an ADBC-backed source could answer nothing at
            // all - `JobTransport::declined_to_dry_run` carries the measurement.
            //
            // `NotAsked` and never `Accepted { estimated_bytes: None }`: the port's own
            // documentation is that a defaulted pre-flight reads as *this subject may run this
            // plan*, and the conformance pack compares `estimated_bytes.is_some()` against
            // `PRICES_DRY_RUN` on an accepted one. **The limit, where the claim is:** nothing
            // prices such a statement, so `sutura_app`'s spend ledger charges this source nothing
            // and the per-subject byte ceiling does not bound it.
            Err(ref cause) if self.transport.declined_to_dry_run(cause) => return Ok(PreFlight::NotAsked),
            Err(cause) => return Err(BigQueryError::Endpoint { cause }),
        };
        Ok(PreFlight::Accepted { estimated_bytes })
    }

    /// Derives the job's own bounds from the port's `Deadline`; see [`Self::dry_run`]'s note and
    /// `docs/adr/0029`.
    fn execute(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<RowSet, Self::Error> {
        self.deliverable(presented)?;
        let query = self.render(executable)?;
        let answered = self
            .transport
            .run(&self.request(
                &query,
                Self::job_identity(presented, &self.source)?,
                JobDeadline::Port(deadline),
            ))
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
    ///
    /// **No port `Deadline`, and `docs/adr/0029` says why: this is the boot path.** There is no
    /// caller and no request timeout to read one from, so the job is bounded by this adapter's own
    /// configured job bounds instead - unchanged by this record.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        let query = self.render_query(plan.plan())?;
        let answered = self
            .transport
            .run(&self.request(&query, JobIdentity::Transport, JobDeadline::Boot))
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

    /// Was this failure the port's own `Deadline` running out, either found spent before the job was
    /// sent or the service stopping it at `jobTimeoutMs`? Delegates to the TRANSPORT, for the same
    /// reason [`Self::result_did_not_fit`] and [`Self::source_refused`] do: `Self::Error` is
    /// `BigQueryError::Endpoint` wrapping the transport's own type, and only the transport can read
    /// the wire-level shape. Every other variant is `false`, exhaustively: the transport is the
    /// ADBC driver now, and only it can read its own error shape for the port's deadline model.
    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        match *error {
            BigQueryError::Endpoint { ref cause } => self.transport.deadline_exceeded(cause),
            BigQueryError::NoIdentityInTheAnswer { .. }
            | BigQueryError::NoPrincipalSwitch { .. }
            | BigQueryError::Render { .. }
            | BigQueryError::UnresolvableConnection { .. }
            | BigQueryError::LegWithoutCombiner { .. }
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
            | BigQueryError::NoPrincipalSwitch { .. }
            | BigQueryError::Render { .. }
            | BigQueryError::UnresolvableConnection { .. }
            | BigQueryError::LegWithoutCombiner { .. }
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

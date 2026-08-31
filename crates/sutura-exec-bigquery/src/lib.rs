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
//! - the value mapping, which is where a wrong number would come from.
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
//!    `BigQuery`. **What that one is, exactly:** one hand-built `SUM` over a two-column fixture, so it
//!    says nothing about a join, `COUNT(DISTINCT`, `CASE WHEN`, a `NULLIF` ratio or `ISOWEEK` - and
//!    the last is one of the two constructs `docs/adr/0017` measured the parse check to be blind
//!    about. **The corpus-wide leg that record specifies is `tests/corpus.rs`, beside it**, behind the
//!    default-off `fixtures` feature: it loads the example fixtures into four tables through
//!    [`BigQueryWarehouse::load_fixture`], runs the corpus questions, and compares its rows with the
//!    engine's for the same plan. That is where the join, the ratio and `ISOWEEK` are reached.
//!
//! So this crate is still in AGENTS.md's *Built And Not Wired* section, and nothing here may be cited
//! as an invariant. `sutura-serve` links no `BigQuery` adapter and refuses `kind: bigquery` by name,
//! and the `data_systems:` axis of the golden matrix still gains no entry - **and the reason for that
//! last one has changed rather than gone away.** It was *a cell that has never executed reads as
//! coverage*; the corpus leg executes, so what keeps the entry out now is that a cell in that registry
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

use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture};
use sutura_domain::model::SourceName;
#[cfg(feature = "fixtures")]
use sutura_domain::model::TableName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::{AnchorRows, MalformedRowSet, NotFinite, PreFlight, Real, RowSet, Value, Warehouse};
use sutura_sql::generate::generate;
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};

pub mod transport;
#[cfg(feature = "wire")]
pub mod wire;

// The fixture loader, behind the default-off `fixtures` feature. `Cargo.toml` carries the argument
// for why it is a feature and not simply a `#[cfg(test)]` helper: an INTEGRATION test target is a
// separate crate, so it cannot reach a `#[cfg(test)]` item here, and a method that issues
// `CREATE OR REPLACE TABLE` is one no shipped build should contain.
#[cfg(feature = "fixtures")]
mod importer;
#[cfg(feature = "fixtures")]
pub use crate::importer::{FixtureNotLoaded, FixtureNotUsable, Loaded};

mod sts;
pub use sts::{StsCredential, StsExchange, WorkloadIdentity, WorkloadIdentityBroker};

use crate::transport::{Cell, DatasetId, Field, FieldType, JobRequest, JobRows, JobTransport, ProjectId};

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
    /// The leg's credential and this source's declared posture do not agree.
    #[error("the credential presented for this source does not agree with the posture it was opened under")]
    PresentedDisagreesWithPosture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
    /// A column came back as a type this adapter does not map.
    ///
    /// It NAMES the type rather than answering null, which is the whole reason
    /// [`FieldType::Unmapped`] carries the endpoint's own spelling.
    #[error("column {column} came back as {named}, which this adapter does not map")]
    UnmappedType { column: String, named: String },
    /// A cell declared `INT64` did not parse as one.
    ///
    /// **Two variants rather than one carrying a `&'static str`, because the CAUSE differs.** The
    /// endpoint sends every value as text, so "declared an integer" and "parses as an integer" are two
    /// facts, and the standard-library error that says why is worth keeping on the chain - `#[source]`
    /// is not wired for you.
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
    /// **The whole of the match is [`Presented::agrees_with`]** - a subject's credential for a source
    /// opened impersonating passes, a shared leg for one passes, and the two shapes on the opposite
    /// posture (and a shared witness that is another source's) are refused with the typed
    /// [`PresentedDisagreesWithPosture`]. This adapter declares it can carry a subject, so
    /// `agrees_with` decides which shape, and no arm here refuses subject material a this-adapter
    /// used to invent.
    fn deliverable(&self, presented: &Presented) -> Mapped<(), T::Error> {
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| BigQueryError::PresentedDisagreesWithPosture { cause })
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
    /// `None` for every shape but the asker's own token: a principal-name switch is a value the
    /// endpoint resolves on a connection the deployment authenticated, and the shared posture has no
    /// material to send - each is the transport's business discussed above.
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
        // No parameters, deliberately, and it is worth naming because the no-injection invariant is
        // about exactly this position: a bind parameter carries a VALUE FROM A QUESTION, and there is
        // no question here. What makes the literals safe is that each one was parsed - see the header
        // of `crate::importer`.
        // And no subject bearer, for the same reason there are no parameters: a fixture load is not
        // an answer. There is no asker to present one, and a `CREATE OR REPLACE TABLE` is a thing the
        // identity this transport already holds does to its own dataset - handing it a subject's
        // exchanged token would run a write under whoever last asked a question.
        let request = JobRequest::new(&statement, &[], &self.billing_project, &self.default_dataset, None);
        self.transport
            .apply(&request)
            .map_err(|cause| FixtureNotLoaded::Endpoint { cause })?;
        Ok(fixture.rows())
    }

    /// One cell, as the domain names it.
    ///
    /// **The mapping is deliberately the same as `sutura-exec-duckdb`'s wherever both can answer**, and
    /// that agreement is a correctness property rather than tidiness: one plan answered by two adapters
    /// has to produce one number, or an anchor certified against one stops reproducing against the
    /// other. The three arms where that matters are marked below.
    fn cell(field: &Field, value: Cell) -> Mapped<Value, T::Error> {
        let column = || String::from(field.name());
        let text = match value {
            // A null is a null whatever the column is declared as, so it is answered before the type
            // is read. **A comment here used to conclude from that "an unmapped type holding only
            // nulls is still an error", and the code did the opposite** - which is why the schema is
            // now checked by [`Self::mappable`] before any row is read. This arm is the belt: it
            // cannot fire for a result that came through `rows`, and it stays because `cell` is
            // reachable from a test on its own and because a null-answered type would be a wrong
            // number rather than a refusal.
            Cell::Null => return Ok(Value::Null),
            Cell::Text(text) => text,
        };
        match *field.kind() {
            FieldType::Int64 => text
                .parse::<i64>()
                .map(Value::Integer)
                .map_err(|cause| BigQueryError::NotAnInteger { column: column(), cause }),
            // CHECKED, not taken - see `BigQueryError::NotFinite`. Both SQL adapters have this arm.
            FieldType::Float64 => {
                let parsed = text
                    .parse::<f64>()
                    .map_err(|cause| BigQueryError::NotADouble { column: column(), cause })?;
                Real::parse(parsed)
                    .map(Value::Real)
                    .map_err(|cause| BigQueryError::NotFinite { column: column(), cause })
            }
            // **Both stay TEXT, and the arms are joined because the behaviour really is one arm.** For
            // a string that is trivial; for an exact decimal it is the whole point - turning `NUMERIC`
            // into an `f64` here is how a total that was correct in the data system stops being
            // correct in an answer, which is the same sentence `sutura-exec-duckdb` carries on its own
            // `Decimal` arm.
            FieldType::Numeric | FieldType::String => Ok(Value::Text(text)),
            // `Integer(0 | 1)`, because the domain's `Value` has no boolean and `sutura-exec-duckdb`
            // answers a `BOOLEAN` the same way. Agreeing matters here: the example catalog counts a
            // `churned_in_month` flag, so the two adapters would otherwise disagree about a metric.
            FieldType::Bool => match text.as_str() {
                "true" => Ok(Value::Integer(1)),
                "false" => Ok(Value::Integer(0)),
                _ => Err(BigQueryError::NotABool { column: column() }),
            },
            // Parsed and re-rendered rather than passed through, so a malformed date is an error here
            // instead of text that looks like a date downstream. `sutura-exec-duckdb` reaches the same
            // `Value::Text(date.to_iso())` from a day count.
            FieldType::Date => sutura_domain::calendar::Date::parse(&text)
                .map(|date| Value::Text(date.to_iso()))
                .map_err(|cause| BigQueryError::NotADate { column: column(), cause }),
            FieldType::Unmapped(ref named) => Err(BigQueryError::UnmappedType {
                column: column(),
                named: named.clone(),
            }),
        }
    }

    /// Every column the endpoint declared is one this adapter maps, or the first one that is not.
    ///
    /// **A schema-wide pass, and it runs BEFORE any row is read - which is the whole point.** The
    /// per-cell check in [`Self::cell`] can only see a column that HOLDS something: a null is answered
    /// before the type is read, so a result with no rows never reached the check at all and a result
    /// whose unmapped column happened to be entirely null passed it. A `TIMESTAMP` or a `BYTES` column
    /// therefore came back as a successful `RowSet`, and whether this adapter mapped a type depended on
    /// what the data happened to be. Review found it; a test pins both shapes.
    ///
    /// It reads the SCHEMA and nothing else, so the answer does not vary with the page.
    fn mappable(answered: &JobRows) -> Mapped<(), T::Error> {
        for field in answered.fields() {
            if let FieldType::Unmapped(ref named) = *field.kind() {
                return Err(BigQueryError::UnmappedType {
                    column: String::from(field.name()),
                    named: named.clone(),
                });
            }
        }
        Ok(())
    }

    /// A job's result, as a domain result set.
    fn rows(answered: &JobRows) -> Mapped<RowSet, T::Error> {
        // The schema first, because it is the one check whose answer does not depend on the rows -
        // see `Self::mappable`. A page this adapter could not read whatever it contained is refused
        // before its count is compared against anything.
        Self::mappable(answered)?;
        // Then the count, which refuses a wrong number before any cell work. A page whose delivered
        // count is not what the endpoint reported is refused here rather than read as *under the cap,
        // not truncated* - see `BigQueryError::Incomplete`.
        if answered.rows().len() != answered.total_rows() {
            return Err(BigQueryError::Incomplete {
                delivered: answered.rows().len(),
                total: answered.total_rows(),
            });
        }
        let columns: Vec<String> = answered.fields().iter().map(|f| String::from(f.name())).collect();
        let mut out: Vec<Vec<Value>> = Vec::with_capacity(answered.rows().len());
        for (index, row) in answered.rows().iter().enumerate() {
            // Checked here rather than left to `RowSet::new`, so the refusal can name WHICH row the
            // endpoint sent at the wrong width. `RowSet::new` catches it too, and that is the belt:
            // this is the one that produces a usable message.
            if row.len() != columns.len() {
                return Err(BigQueryError::RowWidth {
                    row: index,
                    cells: row.len(),
                    columns: columns.len(),
                });
            }
            let mut cells: Vec<Value> = Vec::with_capacity(row.len());
            for (field, value) in answered.fields().iter().zip(row.iter()) {
                cells.push(Self::cell(field, value.clone())?);
            }
            out.push(cells);
        }
        RowSet::new(columns, out).map_err(|cause| BigQueryError::Shape { cause })
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

    // `working_set_exhausted` is deliberately NOT overridden. The port's default is `None`, and that
    // is the honest answer for an adapter with no local memory pool to bound: the work happens at the
    // endpoint, and a query refused there for its own resource reasons is not this deployment's
    // configured ceiling refusing a reservation. Answering otherwise would tell a caller not to retry
    // something a retry would have answered.
}

#[cfg(test)]
mod tests;

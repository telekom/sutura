#![forbid(unsafe_code)]
//! A [`Warehouse`](sutura_domain::warehouse::Warehouse) adapter over PostgreSQL, through the ADBC
//! driver - [`adbc::AdbcPostgres`], one connection string under the deployment's declared identity
//! (`SharedServiceUser`). The static half of Postgres: no OAuth, no impersonation.
//!
//! **ADBC is the only transport** (`telekom/sutura#913` stage 2). The driver is the self-built
//! `libadbc_driver_postgresql` (`nix/postgres-adbc.nix`): linked into every musl release, mounted
//! from `SUTURA_POSTGRES_ADBC_DRIVER` everywhere else. SQL renders through `sutura-sql`
//! (`Dialect::Postgres`); nothing here is compiled or translated. `adbc`'s header says what holds
//! the channel, the deadline and the single-statement guarantee, and what does not.
//!
//! ## Limits
//!
//! - **One declared identity, only.** A Postgres source signs in solely as the deployment's
//!   declared shared service account: a password, or one Kerberos principal from the deployment's
//!   keytab. OAuth and per-caller sign-in are not supported: `AdbcPostgres::IMPERSONATION` is
//!   `NoPlaceForASubject`, and `Conninfo` pins `require_auth` to `password,md5,scram-sha-256,none`
//!   and `gssencmode` to `disable` everywhere but `Conninfo::kerberos`, which writes
//!   `require_auth='gss'` and the `gssencmode` its declaration names. No settings key selects
//!   Kerberos yet.
//! - **`transport_anchors: system` is refused** ([`adbc::UnusableChannel::HostStore`]): libpq's
//!   `system` store is OpenSSL's compiled-in default, not the host store sutura reads.

pub mod adbc;
pub mod connection;
mod deadline;
/// The fixture tier's credential and its loader - a value that cannot exist unconfigured.
///
/// **Behind the default-off `fixtures` feature**, because every caller is a test and
/// `nix/shipped.nix` builds cargo's DEFAULT set: so no artefact a release publishes contains this
/// module, which deletes the *reachable from a consumer* half rather than hardening it.
/// `--all-features` compiles, lints and tests it on every run.
#[cfg(feature = "fixtures")]
pub mod fixture;
#[cfg(feature = "fixtures")]
mod importer;

use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture};
use sutura_domain::warehouse::MalformedRowSet;
use sutura_domain::warehouse::cardinality::CountsNotRead;
use sutura_sql::GenerateError;

/// Why this data system could not answer.
#[derive(Debug, thiserror::Error)]
pub enum PostgresError {
    /// The server refused a statement as `division by zero` (SQLSTATE `22012`) - how Postgres
    /// honours `zero_denominator: fails`, kept typed rather than folded into [`Self::Adbc`].
    #[error("the statement was refused by the server as a division by zero")]
    DivisionByZero {
        #[source]
        cause: adbc::AdbcError,
    },
    /// A column came back as a type this adapter does not map. An error, not a stringified value.
    #[error("column {column} came back as {postgres_type}, which this adapter does not map")]
    UnsupportedType { column: String, postgres_type: &'static str },
    /// A floating-point (or `NUMERIC`) column came back as a value that is not a number.
    #[error("column {column} came back as a value that is not a finite number")]
    NotFinite {
        column: String,
        #[source]
        cause: sutura_domain::warehouse::NotFinite,
    },
    /// A day came back that is not a date this build can represent.
    #[error("column {column} came back as a day that is not a date")]
    NotADate {
        column: String,
        #[source]
        cause: sutura_domain::calendar::InvalidDate,
    },
    #[error("the result set was not rectangular")]
    Shape {
        #[source]
        cause: MalformedRowSet,
    },
    /// A key probe's result was not the pair of counts its statement projects.
    ///
    /// A defect in the rendering or in this adapter's value mapping rather than anything about the
    /// data - two aggregates over no group produce one row of two integers - and it travels as an
    /// `Err` from the port, which the boot path reads as *this declaration went unchecked*.
    #[error("the key probe did not come back as two counts")]
    KeyCounts {
        #[source]
        cause: CountsNotRead,
    },
    #[error("the plan could not be rendered for Postgres")]
    Render {
        #[source]
        cause: GenerateError,
    },
    /// A fixture import failed at the server.
    #[cfg(feature = "fixtures")]
    #[error("could not load the fixture CSV {path} as table {table}")]
    Fixture {
        table: String,
        path: String,
        #[source]
        cause: adbc::AdbcError,
    },
    #[cfg(feature = "fixtures")]
    #[error("could not read the fixture CSV at {path}")]
    FixtureRead {
        path: String,
        #[source]
        cause: std::io::Error,
    },
    /// A CSV header named a column that is not a valid identifier. Refused, not interpolated.
    #[cfg(feature = "fixtures")]
    #[error("a CSV header named a column that is not a valid identifier")]
    InvalidColumnName {
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// The shared conformance fixture schema could not be inferred.
    #[cfg(feature = "fixtures")]
    #[error("the fixture CSV schema could not be inferred")]
    FixtureSchema {
        #[source]
        cause: sutura_domain::warehouse::csv::InferenceError,
    },
    /// The `fixtures` build's `statement_timeout` tuning value is not a `u32` millisecond count.
    ///
    /// The value becomes a `SET LOCAL statement_timeout = N` ceiling, so it is parsed at the
    /// boundary and refused if it is not a number or exceeds the `u32` ceiling - a value that
    /// cannot be a timeout must not reach the statement as uninterpreted text. The cause survives
    /// so the operator sees the number did not parse, not a plain refusal.
    #[error("SUTURA_DEV_STATEMENT_TIMEOUT_MS must be a whole number of milliseconds up to {ceiling}")]
    InvalidStatementTimeout {
        ceiling: u32,
        #[source]
        cause: core::num::ParseIntError,
    },
    /// The deadline was already spent before anything was sent - refused locally, no round trip.
    #[error("the deadline was already spent by the time the connection was open")]
    DeadlineSpent,
    /// The ADBC transport's own failure; its refusals before any SQL are the variants above.
    #[error("the ADBC transport could not answer")]
    Adbc {
        #[source]
        cause: adbc::AdbcError,
    },
    /// The credential broker handed this adapter subject material it has nowhere to put.
    #[error(
        "source `{at}` was handed {presented}, and this adapter has nowhere for a subject's own \
         credential to arrive: it is one connection under the deployment's declared identity. This is \
         a wiring defect between the credential broker and the source declaration"
    )]
    NoPlaceForASubject { at: String, presented: &'static str },
    #[error("the credential broker presented a leg that disagrees with how this source is declared")]
    PresentedDisagreesWithPosture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
}

/// The statement-timeout ceiling a build without the `fixtures` feature always uses.
const DEFAULT_STATEMENT_TIMEOUT_MS: u32 = 15_000;

/// The statement timeout ceiling: `SUTURA_DEV_STATEMENT_TIMEOUT_MS` in a `fixtures` build, and
/// [`DEFAULT_STATEMENT_TIMEOUT_MS`] in every other - the shipped binary links this crate without
/// `fixtures`, so a deployment's environment cannot move it.
///
/// Parsing is `u32` (not `u64` rounded down), so an oversized value is refused rather than becoming
/// a different number.
fn statement_timeout_ms() -> Result<u32, PostgresError> {
    statement_timeout_under(
        cfg!(feature = "fixtures"),
        std::env::var("SUTURA_DEV_STATEMENT_TIMEOUT_MS").ok().as_deref(),
    )
}

/// [`statement_timeout_ms`]'s decision with both inputs passed in: the environment value is read
/// only when `dev_build`.
fn statement_timeout_under(dev_build: bool, from_env: Option<&str>) -> Result<u32, PostgresError> {
    from_env
        .filter(|_| dev_build)
        .map_or(Ok(DEFAULT_STATEMENT_TIMEOUT_MS), parse_statement_timeout)
}

/// Parses a `statement_timeout` tuning value as a `u32` millisecond count.
fn parse_statement_timeout(raw: &str) -> Result<u32, PostgresError> {
    raw.parse::<u32>().map_err(|cause| PostgresError::InvalidStatementTimeout {
        ceiling: u32::MAX,
        cause,
    })
}

/// Refuses credential material this adapter has nowhere to put, then checks the presented leg
/// against how this source was DECLARED - the two questions `docs/adr/0008` part 4 names apart.
/// Called by every port method that takes a credential, so the pre-flight and the run cannot
/// disagree about what is accepted.
pub(crate) fn deliverable(
    source: &sutura_domain::model::SourceName,
    posture: &sutura_domain::source::SourcePosture,
    presented: &Presented,
) -> Result<(), PostgresError> {
    match *presented {
        Presented::SharedServiceUser { .. } => {}
        Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => {
            return Err(PostgresError::NoPlaceForASubject {
                at: String::from(source.as_str()),
                presented: presented.as_str(),
            });
        }
    }
    presented
        .agrees_with(posture, source)
        .map_err(|cause| PostgresError::PresentedDisagreesWithPosture { cause })
}

#[cfg(test)]
mod tests;

//! Where the PostgreSQL ADBC driver comes from, the one connection opened over it, and the typed
//! error of every ADBC call that follows.

use std::path::PathBuf;

use adbc_core::error::Error as CoreError;
use adbc_core::options::{AdbcVersion, OptionDatabase, OptionValue};
use adbc_core::{Database as _, Driver as _};
use adbc_driver_manager::{ManagedConnection, ManagedDriver};
use sutura_adbc::DriverLocation;
pub use sutura_adbc::UnusableDriverPath;
use sutura_domain::warehouse::{UnannouncedBatch, UnreadableCell};

use crate::conninfo::Conninfo;

/// Why a PostgreSQL ADBC call could not answer.
#[derive(Debug, thiserror::Error)]
pub enum AdbcError {
    #[error("could not load the PostgreSQL ADBC driver")]
    Load(#[source] CoreError),
    #[error("an ADBC call to the PostgreSQL driver failed")]
    Adbc(#[source] CoreError),
    #[error("could not read a result batch")]
    Batch(#[source] arrow_schema::ArrowError),
    /// The stream failed once the statement's timeout had run out - the server cancelling it,
    /// read by the clock where the stream carries no SQLSTATE.
    #[error("the source stopped the statement at its timeout")]
    TimedOut(#[source] arrow_schema::ArrowError),
    #[error("the ADBC result stream did not match its announced schema")]
    Unannounced(#[source] UnannouncedBatch),
    #[error("the plan's values could not be assembled for binding")]
    Parameters(#[source] arrow_schema::ArrowError),
    #[error("a result cell could not be read")]
    Unreadable(#[source] UnreadableCell),
    /// Spent once the connection was open - refused locally, before anything is sent.
    #[error("the deadline was already spent by the time the connection was open")]
    DeadlineSpent,
}

/// Where the PostgreSQL driver comes from: this artefact's own link, or a mounted `.so`.
///
/// Not `sutura_adbc::DriverLocation`, whose linked route is the `BigQuery` archive; a mounted path is
/// parsed by it, so an empty or relative one is refused exactly as for every ADBC adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresDriver(Route);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    Linked,
    Mounted(PathBuf),
}

/// The variable a host that links no driver names a mounted one with.
///
/// **Not a settings key**: which driver file a host carries is a property of the host rather than of
/// the semantic deployment, and a release artefact that links one never reads it - the order
/// `bigquery_driver` in `sutura-cli` gives for the other ADBC adapter, for its reason: a mounted
/// path must not be able to displace the driver a published artefact carries.
pub const MOUNTED_DRIVER: &str = "SUTURA_POSTGRES_ADBC_DRIVER";

/// Why this process has no PostgreSQL driver to open.
#[derive(Debug, thiserror::Error)]
pub enum NoDriver {
    #[error(
        "this build links no PostgreSQL ADBC driver and `{mounted}` is not set. A published \
         musl artefact carries its own; any other build points that variable at an absolute \
         libadbc_driver_postgresql shared library",
        mounted = MOUNTED_DRIVER
    )]
    Unset,
    #[error("`{mounted}` does not name a driver this process can open", mounted = MOUNTED_DRIVER)]
    Unusable(#[source] UnusableDriverPath),
}

impl PostgresDriver {
    /// The driver this process opens: the one this artefact links, else the one
    /// [`MOUNTED_DRIVER`] names.
    ///
    /// # Errors
    ///
    /// [`NoDriver`] where neither is there, or the named path is not one.
    pub fn from_host() -> Result<Self, NoDriver> {
        if let Some(linked) = Self::linked_in() {
            return Ok(linked);
        }
        let named = std::env::var(MOUNTED_DRIVER).map_err(|_absent| NoDriver::Unset)?;
        Self::parse(&named).map_err(NoDriver::Unusable)
    }

    /// The driver this artefact links, or `None` where it links none.
    #[must_use]
    pub fn linked_in() -> Option<Self> {
        sutura_adbc::LINKS_POSTGRES_DRIVER.then_some(Self(Route::Linked))
    }

    /// Parses a mounted driver's path.
    ///
    /// # Errors
    ///
    /// [`UnusableDriverPath`] for an empty or relative path. Whether a driver is there is the
    /// load's question, asked by the first call.
    pub fn parse(named: &str) -> Result<Self, UnusableDriverPath> {
        DriverLocation::parse(named).map(|_| Self(Route::Mounted(PathBuf::from(named))))
    }

    /// Loads and initialises the driver, opening no database - what `sutura doctor` asks.
    ///
    /// # Errors
    ///
    /// [`AdbcError::Load`], from either route.
    pub fn probe(&self) -> Result<(), AdbcError> {
        self.load().map(drop)
    }

    /// Loads the driver and opens one connection over `conninfo`. The connection keeps its
    /// database and driver alive itself.
    ///
    /// # Errors
    ///
    /// [`AdbcError::Load`] where the driver does not load, [`AdbcError::Adbc`] where the open fails.
    pub fn connect(&self, conninfo: &Conninfo) -> Result<ManagedConnection, AdbcError> {
        let mut driver = self.load()?;
        #[expect(
            clippy::disallowed_methods,
            reason = "the libpq connection string is the driver's credential, and handing it to the driver is its purpose"
        )]
        let uri = OptionValue::String(conninfo.secret().expose_secret().to_owned());
        let database = driver
            .new_database_with_opts([(OptionDatabase::Uri, uri)])
            .map_err(AdbcError::Adbc)?;
        database.new_connection().map_err(AdbcError::Adbc)
    }

    fn load(&self) -> Result<ManagedDriver, AdbcError> {
        match self.0 {
            Route::Linked => sutura_adbc::linked_postgres_driver(),
            Route::Mounted(ref path) => ManagedDriver::load_dynamic_from_filename(path, None, AdbcVersion::default()),
        }
        .map_err(AdbcError::Load)
    }
}

impl core::fmt::Display for PostgresDriver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Route::Linked => f.write_str("linked into this binary"),
            Route::Mounted(ref path) => write!(f, "mounted at {}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PostgresDriver, Route};

    #[test]
    fn a_relative_driver_path_is_refused_before_anything_loads_it() {
        let refused = PostgresDriver::parse("lib/libadbc_driver_postgresql.so").expect_err("a relative path is refused");
        assert!(matches!(refused, super::UnusableDriverPath::Relative { .. }), "{refused:?}");
    }

    #[test]
    fn the_linked_route_is_offered_exactly_where_the_linked_driver_initialises() {
        // A source build offers none and has none; a linked build offers the one it can open.
        let offered = PostgresDriver::linked_in();
        assert_eq!(
            offered.is_some(),
            sutura_adbc::linked_postgres_driver().is_ok(),
            "{offered:?}"
        );
        if let Some(linked) = offered {
            linked.probe().expect("the driver this build links initialises");
        }
    }

    #[test]
    fn each_route_reads_as_the_sentence_doctor_prints_and_the_driver_check_matches() {
        // `nix/bigquery-driver-check.sh` passes a musl artefact only on `linked into this binary`.
        assert_eq!(PostgresDriver(Route::Linked).to_string(), "linked into this binary");
        let mounted = PostgresDriver::parse("/opt/lib/libadbc_driver_postgresql.so").expect("an absolute path parses");
        assert_eq!(mounted.to_string(), "mounted at /opt/lib/libadbc_driver_postgresql.so");
    }
}

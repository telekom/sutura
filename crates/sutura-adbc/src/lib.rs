//! The linked-driver FFI and the shared helpers every ADBC adapter needs.
//!
//! This crate exists so the workspace's one `unsafe` declaration has one home
//! rather than one per adapter. `linked` declares the per-driver C ABI init function
//! each statically linked driver exports, and `ManagedDriver::load_static`
//! opens it through that pointer - the only route a STATIC musl artefact has, because
//! it has no dynamic loader. The `#[expect(unsafe_code)]` on that declaration is the
//! one lowering `cargo xtask check-unsafe` excepts, and this crate's root is the one
//! root that omits `#![forbid(unsafe_code)]`.
//!
//! **Why a shared crate and not a per-adapter `linked.rs`** (`telekom/sutura#913` PR 2):
//! a second ADBC adapter that links its own driver needs the same FFI, and a list of
//! excepted roots widens the invariant from "one site" to "N sites". One crate keeps
//! the single exception and removes the duplication - the same shape `sutura-tls` has
//! for the TLS-bundle read two adapters share.
//!
//! **The prefix is the role, not an adapter**: `sutura-adbc` joins no `-exec-` class
//! (it opens no data system and renders no dialect), so `xtask/src/boundaries/adapters.rs`'s
//! `data systems` prefix rule does not match it. An adapter depends on it behind a
//! default-off feature, the way `sutura-exec-postgres` depends on `sutura-tls`.
//!
//! # What lives here and what does not
//!
//! - `linked` (the `unsafe`, cfg-gated on `adbc_driver_linked`) and `location` (where a
//!   driver is, parsed once) are fully generic: any ADBC adapter that links or mounts a
//!   driver uses both.
//! - `bind` builds the one-row Arrow batch a positional-parameter driver binds from.
//!   It returns the Arrow error directly; each adapter wraps it into its own error
//!   type, because the error vocabulary is the adapter's and not the FFI's.
//! - The driver-option constants, the transport, the identity decision and the result
//!   drain stay in each adapter. This crate names no data system.

mod bind;
#[cfg(any(adbc_driver_linked, adbc_postgres_driver_linked, adbc_duckdb_driver_linked))]
mod linked;
mod location;

pub use bind::parameter_batch;
pub use location::{DriverLocation, UnusableDriverPath};

use adbc_core::error::Error as CoreError;
use adbc_core::options::AdbcVersion;
use adbc_driver_manager::ManagedDriver;

/// The `BigQuery` driver this artefact's own link carries, or an error where it carries none.
///
/// **The one route a STATIC musl binary has**, because it has no dynamic loader at all:
/// `linked`'s header carries what makes the declaration sound. A build that linked no
/// archive gets an `Err` here, which a caller renders rather than panicking on - the two
/// `cfg` halves have one signature, so an adapter's `load` needs no branch of its own.
///
/// # Errors
///
/// [`CoreError`] where a linked-in driver's own initialisation refused, and where this
/// build linked no archive at all - the same error type `ManagedDriver::load_dynamic_from_filename`
/// returns, so a caller cannot tell the two routes apart and does not have to.
pub fn linked_driver() -> Result<ManagedDriver, CoreError> {
    #[cfg(adbc_driver_linked)]
    {
        linked::driver()
    }
    #[cfg(not(adbc_driver_linked))]
    {
        Err(CoreError::with_message_and_status(
            "this build linked no ADBC driver",
            adbc_core::error::Status::NotFound,
        ))
    }
}

/// Whether this artefact's own link carries the PostgreSQL driver, asked without initialising it.
pub const LINKS_POSTGRES_DRIVER: bool = cfg!(adbc_postgres_driver_linked);

/// The PostgreSQL driver this artefact's own link carries - [`linked_driver`]'s contract, for the
/// archive `nix/postgres-adbc.nix` builds.
///
/// A build that linked only the `BigQuery` archive gets an `Err` here and never that driver: the two
/// are separate symbols, so no build can hand one out under the other's name.
///
/// # Errors
///
/// As [`linked_driver`].
pub fn linked_postgres_driver() -> Result<ManagedDriver, CoreError> {
    #[cfg(adbc_postgres_driver_linked)]
    {
        linked::postgres_driver()
    }
    #[cfg(not(adbc_postgres_driver_linked))]
    {
        Err(CoreError::with_message_and_status(
            "this build linked no PostgreSQL ADBC driver",
            adbc_core::error::Status::NotFound,
        ))
    }
}

/// Whether this artefact's own link carries the `DuckDB` driver, asked without initialising it.
pub const LINKS_DUCKDB_DRIVER: bool = cfg!(adbc_duckdb_driver_linked);

/// The `DuckDB` driver this artefact's own link carries - [`linked_driver`]'s contract, for the
/// archive `nix/duckdb-adbc.nix` builds.
///
/// # Errors
///
/// As [`linked_driver`].
pub fn linked_duckdb_driver() -> Result<ManagedDriver, CoreError> {
    #[cfg(adbc_duckdb_driver_linked)]
    {
        linked::duckdb_driver()
    }
    #[cfg(not(adbc_duckdb_driver_linked))]
    {
        Err(CoreError::with_message_and_status(
            "this build linked no DuckDB ADBC driver",
            adbc_core::error::Status::NotFound,
        ))
    }
}

/// The `DuckDB` library a deployment mounted at `named`, opened as an ADBC driver.
///
/// **The path is parsed, never handed to the loader on trust.** [`DriverLocation::parse`] refuses an
/// empty or relative path before `dlopen` sees it - `telekom/sutura#929`'s sixth finding - so a
/// relative path is a refusal, not a library that depends on this process's working directory.
///
/// **The entrypoint is passed, never derived.** Given none, the driver manager derives
/// `AdbcDuckdbInit` from `libduckdb.so` and falls back to `AdbcDriverInit`, and `DuckDB` defines
/// neither: `duckdb_adbc_init` is its one C-linkage ADBC name, the symbol the linked route declares
/// too.
///
/// # Errors
///
/// [`CoreError`] where the named path is unusable (empty or relative), where the library does not
/// load, or where its initialisation refused.
pub fn mounted_duckdb_driver(named: &str) -> Result<ManagedDriver, CoreError> {
    let at = DriverLocation::parse(named)
        .map_err(|cause| CoreError::with_message_and_status(cause.to_string(), adbc_core::error::Status::InvalidArguments))?;
    let Some(path) = at.mounted() else {
        return Err(CoreError::with_message_and_status(
            "a parsed driver path is mounted or linked; the linked route is linked_duckdb_driver",
            adbc_core::error::Status::Internal,
        ));
    };
    ManagedDriver::load_dynamic_from_filename(path, Some(b"duckdb_adbc_init"), AdbcVersion::default())
}

#![forbid(unsafe_code)]
//! The PostgreSQL ADBC connector: the libpq connection string and its TLS posture ([`Conninfo`],
//! [`Channel`]), the driver a process loads ([`PostgresDriver`]), and the one connection opened
//! over them ([`PostgresDriver::connect`]).
//!
//! It is shared rather than adapter code so that `sutura-exec-postgres` and a metadata reader dial
//! a source through one refusal set. It names no SQL dialect - a catalog adapter may not reach
//! `sutura-sql` (`xtask/src/boundaries/edges.rs`), and neither may this crate - and it carries no
//! adapter prefix, so its normal tree is held off adapters, the application, settings and the
//! transports by its own row in `xtask/src/boundaries/shared_client.rs`.

mod conninfo;
mod driver;
mod target;

pub use conninfo::{Channel, Conninfo, GssEncryption, InvalidKerberosService, Kerberos, KerberosService, UnusableChannel};
pub use driver::{AdbcError, MOUNTED_DRIVER, NoDriver, PostgresDriver, UnusableDriverPath};
pub use target::ConnectionTarget;

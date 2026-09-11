//! The driver configuration for one declared PostgreSQL connection.
//!
//! A composition root owns mapping its source declaration into these values. This module owns the
//! driver-specific half: how TCP and unix-socket targets are represented to `tokio-postgres`, and
//! reading the password file once at boot. Keeping that here means both shipped composition roots
//! reach the same driver behaviour without depending on each other.

use std::path::Path;

/// The address a PostgreSQL source is dialled through.
#[derive(Clone, Copy)]
pub enum ConnectionTarget<'a> {
    /// A TCP host name or address.
    Host(&'a str),
    /// A unix socket directory.
    UnixSocket(&'a Path),
}

/// The declared password file could not be read while the connection was built.
#[derive(Debug, thiserror::Error)]
#[error("could not read the PostgreSQL password file at {path}")]
pub struct PasswordFileUnreadable {
    path: String,
    #[source]
    cause: std::io::Error,
}

/// Builds the driver configuration for one declared PostgreSQL connection.
///
/// The password is trimmed exactly once after reading, so a trailing newline from a mounted secret
/// is not part of the credential. The returned config does not select TLS; [`crate::PostgresWarehouse::connect_secured`]
/// makes a supplied TLS client mandatory before it dials.
///
/// # Errors
///
/// Returns [`PasswordFileUnreadable`] when `password_file` cannot be read.
pub fn config(
    target: ConnectionTarget<'_>,
    port: u16,
    database: &str,
    user: &str,
    password_file: &Path,
) -> Result<tokio_postgres::Config, PasswordFileUnreadable> {
    let mut config = tokio_postgres::Config::new();
    match target {
        ConnectionTarget::Host(host) => {
            config.host(String::from(host));
        }
        ConnectionTarget::UnixSocket(socket) => {
            // A `/`-prefixed host is the driver's cross-platform representation of a unix socket
            // directory; on Unix it is converted into `Host::Unix` inside `Config::host`.
            config.host(socket.display().to_string());
        }
    }
    config.port(port).dbname(String::from(database)).user(String::from(user));
    let password = std::fs::read_to_string(password_file).map_err(|cause| PasswordFileUnreadable {
        path: password_file.display().to_string(),
        cause,
    })?;
    config.password(password.trim());
    Ok(config)
}

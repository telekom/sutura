//! What a composition root reads out of one PostgreSQL declaration before a `Conninfo` is built:
//! the address it dials and the password, read once at boot.

use std::path::Path;

use sutura_domain::identity::Secret;

pub use sutura_adbc_postgres::ConnectionTarget;

/// The declared password file could not be read.
#[derive(Debug, thiserror::Error)]
#[error("could not read the PostgreSQL password file at {path}")]
pub struct PasswordFileUnreadable {
    path: String,
    #[source]
    cause: std::io::Error,
}

/// Reads the declared password file into a [`Secret`].
///
/// The password is trimmed exactly once after reading, so a trailing newline from a mounted secret
/// is not part of the credential. The read `String` is shadowed by that `Secret`, not dropped - it
/// is not zeroised, and it lives unzeroised until this function returns (`docs/adr/0020`'s "not
/// claimed" list).
///
/// # Errors
///
/// [`PasswordFileUnreadable`] when `password_file` cannot be read.
pub fn read_password(password_file: &Path) -> Result<Secret, PasswordFileUnreadable> {
    let password = std::fs::read_to_string(password_file).map_err(|cause| PasswordFileUnreadable {
        path: password_file.display().to_string(),
        cause,
    })?;
    Ok(Secret::new(password.trim()))
}

//! One private schema user per opened cell, created on the tier by its administrator.
//!
//! **The administrator is `SYSTEM`, under the fixture password**: `compose.services.yaml`'s `oracle`
//! service sets `ORACLE_PASSWORD` from the same anchor it sets the application user's password
//! from, so no second secret is read. A cell user takes that same password - it exists in a
//! throwaway tier only - and owns nothing but the tables its own open loads.
//!
//! **No teardown.** A tier is ephemeral in CI and by hand a developer's own; a user left by a
//! finished run is replaced the next time a process with the same id opens the same ordinal, and
//! is otherwise harmless. Nothing here drops a user a live session holds.

use super::{FixtureCredential, FixtureError, SERVICE_NAME};
use crate::{Channel, Dial, DriverError, TokenSessions, connect_err};

#[cfg(test)]
mod tests;

/// The administrator every `gvenzl/oracle-free` tier sets the password of.
const ADMINISTRATOR: &str = "SYSTEM";

/// The `ORA-01918` an absent user answers `DROP USER` with.
const NO_SUCH_USER: usize = 1918;

/// The user one opened cell owns: `CELL_<process>_<ordinal>`.
///
/// Built from two integers rather than parsed from text, so the name reaches `CREATE USER` as an
/// identifier no caller's text could have shaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellName(String);

impl CellName {
    /// The `ordinal`-th cell of process `process`.
    #[must_use]
    pub fn of(process: u32, ordinal: usize) -> Self {
        Self(format!("CELL_{process}_{ordinal}"))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for CellName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

fn drop_user(cell: &CellName) -> String {
    format!("DROP USER \"{cell}\" CASCADE")
}

fn create_user(cell: &CellName, password: &str) -> Result<String, FixtureError> {
    if password.contains('"') {
        return Err(FixtureError::PasswordNotQuotable);
    }
    Ok(format!("CREATE USER \"{cell}\" IDENTIFIED BY \"{password}\""))
}

fn grant(cell: &CellName) -> String {
    format!("GRANT CREATE SESSION, CREATE TABLE, UNLIMITED TABLESPACE TO \"{cell}\"")
}

/// Creates `cell`'s user through the tier's administrator: see `FixtureCredential::provision_cell`.
pub(super) fn provision(
    tier: &FixtureCredential,
    host: &str,
    port: u16,
    cell: &CellName,
) -> Result<FixtureCredential, FixtureError> {
    #[expect(
        clippy::disallowed_methods,
        reason = "the password is written into the one `CREATE USER` it is the payload of, and handed to \
                  the administrator's login"
    )]
    let password = tier.password.expose_secret();
    let create = create_user(cell, password)?;
    let dialled = TokenSessions::new(Dial::new(
        host,
        port,
        SERVICE_NAME,
        Channel::Verified {
            anchors_pem: &tier.anchors_pem,
        },
    ))
    .map_err(FixtureError::Admin)?;
    #[expect(clippy::disallowed_methods, reason = "`TokenSessions::new` asked the packet-trace refusal")]
    let admin = oracledb::connect(dialled.0.set_credentials(ADMINISTRATOR, password))
        .map_err(|cause| FixtureError::Admin(connect_err(cause)))?;
    let refused = |cause: DriverError| FixtureError::Statement {
        subject: format!("user {cell}"),
        cause,
    };
    if let Err(cause) = admin.execute(&drop_user(cell), &[]) {
        let cause = DriverError::from(cause);
        if !cause.has_ora_code(NO_SUCH_USER) {
            return Err(refused(cause));
        }
    }
    for statement in [create, grant(cell)] {
        admin.execute(&statement, &[]).map_err(|cause| refused(cause.into()))?;
    }
    Ok(FixtureCredential {
        user: String::from(cell.as_str()),
        password: tier.password.clone(),
        anchors_pem: tier.anchors_pem.clone(),
    })
}

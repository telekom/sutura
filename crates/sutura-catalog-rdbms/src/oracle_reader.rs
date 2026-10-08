//! The live Oracle documentation-schema reader, behind the default-off `live` feature.
//!
//! [`crate::postgres_reader`]'s twin over an Oracle connection: the same documented `columns` view
//! (that module's header carries the column table), the same constructor checks, inline caps and
//! assembly - all held once in `crate::documentation` - so the conversion in [`crate::RdbmsCatalog`]
//! cannot tell the two apart. What differs is the SQL dialect and the driver.
//!
//! # The contract's Oracle spelling
//!
//! `is_primary_key` and `is_deleted` are `NUMBER(1)`, `1` for true and `0` for false; any other
//! value of `is_primary_key` is refused as missing evidence rather than read as either. The
//! schema, the view and the predicate column are validated identifiers, **folded to upper case and
//! then quoted** - Oracle folds an unquoted name to upper case when the object is created, so
//! `sutura_dictionary.columns` is found as written. A schema created under a quoted lower-case
//! name is not. The environment and an `equals` value are bound as `:1`/`:2`, never interpolated.
//!
//! # The limits, next to the claims
//!
//! - **One live Oracle run is cited, and no `just validate` venue reaches a server.**
//!   `compose.services.yaml`'s `oracle` row says why. The `oracle-tier` CI job ran both cells in
//!   `tests/oracle_provisioned.rs` green on 2026-10-02 at `53055e9a4`, the head of the pull request
//!   that landed them as #1232 (run 36997951066, job 110814411138, beside the adapter's acceptance
//!   cell: `3 tests run: 3 passed`): a read binds models to the declared source alias, and an
//!   absent documentation schema is refused by the server. Outside that job the unit cells prove
//!   the constructor refusals, the rendered statement, the flag decode, and a golden of the
//!   dictionary assembled from positional values handed to the decoder - never a read, because the
//!   driver cannot build a row outside a session.
//! - **Read-only by statement, not by driver flag.** The pinned driver has no read-only option;
//!   the reader issues `SET TRANSACTION READ ONLY` before its one `SELECT` and rolls back after it.
//!   That it makes the transaction read-only is unobserved against a server.
//! - **A read racing fresh DDL refuses.** For a short window after the documentation table is
//!   recreated, Oracle refuses the read-only snapshot with `ORA-01466` and the read fails as
//!   [`RdbmsError::Read`]; the reader does not retry. Measured on the compose tier after a
//!   `DROP`/`CREATE`, red at about 0.2 s and green at 5 s; the window is not narrowed further.
//! - **The byte cap is an estimate.** The driver exposes no row's wire size, so a row spends the
//!   UTF-8 length of its decoded text plus one byte - bounding the decoded payload, not the bytes
//!   on the wire. Neither cap limits elapsed read time, and neither bounds a fetch: the pinned
//!   driver prefetches 2 rows on execute and fetches 100 per round trip by default - read off its
//!   source, not observed against a server - so up to one batch is in memory before a cap refuses.
//! - **The connection is plaintext and stays on its declared loopback host.** `sutura-config`'s
//!   `OracleCatalogConnection` refuses TLS and a non-loopback host, and a listener's redirect is
//!   refused before authentication as [`RdbmsError::RedirectRefused`].
//! - **`SharedServiceUser`, not leg 2.** A catalog read has no caller to run as; it is one login
//!   under the user the deployment declared.

use std::num::NonZeroU64;

use sutura_domain::identity::Secret;

use crate::documentation::{Assembly, DOCUMENTATION_VIEW, Row as DocumentationRow, checked_bounds};
use crate::postgres_reader::{InvalidReaderConfig, RowPredicate};
use crate::{Dictionary, DictionaryBounds, DictionaryReader, RdbmsError};

/// Where and as whom the reader logs in: an EZCONNECT `host:port/service_name` dial.
#[derive(Debug, Clone)]
pub struct OracleLogin {
    host: String,
    port: u16,
    service_name: String,
    user: String,
    password: Secret,
}

impl OracleLogin {
    /// The EZCONNECT `host:port/service_name`, with an IPv6 literal bracketed.
    fn address(&self) -> String {
        if self.host.parse::<std::net::Ipv6Addr>().is_ok() {
            format!("[{}]:{}/{}", self.host, self.port, self.service_name)
        } else {
            format!("{}:{}/{}", self.host, self.port, self.service_name)
        }
    }

    #[must_use]
    pub const fn new(host: String, port: u16, service_name: String, user: String, password: Secret) -> Self {
        Self {
            host,
            port,
            service_name,
            user,
            password,
        }
    }
}

/// A live [`DictionaryReader`] over an Oracle documentation schema.
#[derive(Debug, Clone)]
pub struct OracleReader {
    login: OracleLogin,
    statement: String,
    environment: String,
    predicate: RowPredicate,
    bounds: DictionaryBounds,
}

impl OracleReader {
    /// Builds the reader without dialling. An absent `row_cap`/`byte_cap` selects the documented
    /// defaults [`crate::postgres_reader::PostgresReader`] uses.
    ///
    /// # Errors
    ///
    /// The documentation schema or the predicate column is not an identifier.
    pub fn new(
        login: OracleLogin,
        documentation_schema: &str,
        environment: String,
        predicate: RowPredicate,
        row_cap: Option<NonZeroU64>,
        byte_cap: Option<NonZeroU64>,
    ) -> Result<Self, InvalidReaderConfig> {
        let bounds = checked_bounds(documentation_schema, &predicate, row_cap, byte_cap)?;
        Ok(Self {
            login,
            statement: statement(documentation_schema, &predicate),
            environment,
            predicate,
            bounds,
        })
    }

    #[must_use]
    pub const fn bounds(&self) -> DictionaryBounds {
        self.bounds
    }

    /// The values bound to the statement's `:1` and `:2`, in order.
    fn binds(&self) -> Vec<&str> {
        let mut binds = vec![self.environment.as_str()];
        if let RowPredicate::Equals { value, .. } = &self.predicate {
            binds.push(value);
        }
        binds
    }

    fn connect(&self) -> Result<oracledb::Connection, RdbmsError> {
        let login = &self.login;
        #[expect(
            clippy::disallowed_methods,
            reason = "the password's destination is the connection handshake, which is the one place the \
                      value itself is the payload"
        )]
        let password = login.password.expose_secret();
        let config = oracledb::Config::default()
            .set_connect_string(&login.address())
            .map_err(read_err)?
            .set_credentials(&login.user, password)
            .set_follow_redirects(false);
        oracledb::connect(config).map_err(read_err)
    }
}

impl DictionaryReader for OracleReader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        let connection = self.connect()?;
        connection.execute("SET TRANSACTION READ ONLY", &[]).map_err(read_err)?;
        let binds = self.binds();
        let params: Vec<&dyn oracledb::ToDbValue> = binds.iter().map(|value| value as &dyn oracledb::ToDbValue).collect();
        let rows = connection.query(&self.statement, &params).map_err(read_err)?;
        let dictionary = assemble(
            &self.environment,
            self.bounds,
            rows.map(|row| decode_row(&row.map_err(read_err)?)),
        )?;
        connection.rollback().map_err(read_err)?;
        Ok(dictionary)
    }
}

/// Gathers decoded rows under `bounds`, refusing at the first row that fails to decode or crosses
/// a cap - so the read abandons its cursor there rather than after it.
fn assemble(
    environment: &str,
    bounds: DictionaryBounds,
    rows: impl Iterator<Item = Result<DocumentationRow, RdbmsError>>,
) -> Result<Dictionary, RdbmsError> {
    let mut assembly = Assembly::new(environment, bounds);
    for row in rows {
        let row = row?;
        assembly.admit(row.decoded_len())?;
        assembly.push(row)?;
    }
    Ok(assembly.finish())
}

fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.to_ascii_uppercase())
}

fn statement(documentation_schema: &str, predicate: &RowPredicate) -> String {
    let predicate_sql = match predicate {
        RowPredicate::None => String::new(),
        RowPredicate::IsNull(column) => format!(" AND {} IS NULL", quoted(column)),
        RowPredicate::IsNotNull(column) => format!(" AND {} IS NOT NULL", quoted(column)),
        RowPredicate::Equals { column, .. } => format!(" AND {} = :2", quoted(column)),
    };
    format!(
        "SELECT environment, catalog_name, schema_name, table_name, model_name, table_description, \
                column_name, column_type, column_description, is_primary_key \
         FROM {}.{} \
         WHERE is_deleted = 0 AND environment = :1{predicate_sql} \
         ORDER BY catalog_name, schema_name, table_name, column_ordinal",
        quoted(documentation_schema),
        quoted(DOCUMENTATION_VIEW),
    )
}

/// `NUMBER(1)` as the contract spells a flag: `1` or `0`, and nothing else is evidence.
const fn flag(value: Option<i64>) -> Option<bool> {
    match value {
        Some(1) => Some(true),
        Some(0) => Some(false),
        _ => None,
    }
}

/// Reads one row's columns by position, in [`statement`]'s `SELECT` order.
fn decode_row(row: &oracledb::Row) -> Result<DocumentationRow, RdbmsError> {
    decode(|index| row.get(index), |index| row.get(index))
}

/// [`decode_row`] over any positional source of the driver's values: the driver cannot build a
/// row outside a session, so this is the seam a cell reaches without a server.
fn decode(
    text_at: impl Fn(usize) -> Result<Option<String>, oracledb::Error>,
    number_at: impl Fn(usize) -> Result<Option<i64>, oracledb::Error>,
) -> Result<DocumentationRow, RdbmsError> {
    let text = |index| text_at(index).map_err(read_err);
    Ok(DocumentationRow {
        environment: text(0)?,
        catalog_name: text(1)?,
        schema_name: text(2)?,
        table_name: text(3)?,
        model_name: text(4)?,
        table_description: text(5)?,
        column_name: text(6)?,
        column_type: text(7)?,
        column_description: text(8)?,
        is_primary_key: flag(number_at(9).map_err(read_err)?),
    })
}

fn read_err(cause: oracledb::Error) -> RdbmsError {
    if matches!(cause.kind(), oracledb::ErrorKind::RedirectNotAllowed) {
        return RdbmsError::RedirectRefused;
    }
    RdbmsError::Read(Box::new(DriverError(cause)))
}

/// [`oracledb::Error`] as a `std::error::Error`, which the driver's own type does not implement.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct DriverError(oracledb::Error);

#[cfg(test)]
mod tests;

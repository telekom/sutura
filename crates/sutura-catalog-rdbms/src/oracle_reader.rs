//! The live Oracle documentation-schema reader, behind the default-off `live-oracle` feature.
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
//! - **A declared catalog is read over verified TLS only, inside this process.** `sutura-config`'s
//!   `OracleCatalogConnection` holds no plaintext channel, and [`OracleLogin::verified`]'s PEM
//!   anchors replace the driver's public authorities, and [`OracleReader::new`] refuses a bundle
//!   with no usable certificate; no client certificate is presented, and the
//!   handshake after the TCP connect is not bounded (`sutura_oracle_connect`'s own limits). The
//!   bytes still cross the network, encrypted. A listener's redirect is refused before
//!   authentication as [`RdbmsError::RedirectRefused`].
//! - **`SharedServiceUser`, not leg 2.** A catalog read has no caller to run as; it is one login
//!   under the user the deployment declared.

use std::num::NonZeroU64;

use sutura_domain::identity::Secret;
use sutura_oracle_connect::{Channel, Dial, PrepareError, PreparedDial};

use crate::documentation::{Assembly, DOCUMENTATION_VIEW, Row as DocumentationRow, checked_bounds};
use crate::postgres_reader::{InvalidReaderConfig, RowPredicate};
use crate::{Dictionary, DictionaryBounds, DictionaryReader, RdbmsError};

/// Where and as whom the reader logs in: an EZCONNECT `host:port/service_name` dial. No reader takes
/// it until [`OracleLogin::verified`] names the anchors the listener is verified against.
#[derive(Debug, Clone)]
pub struct OracleLogin {
    host: String,
    port: u16,
    service_name: String,
    user: String,
    password: Secret,
}

/// An [`OracleLogin`] over TLS, verified against `anchors_pem` and no other certificate, with the
/// login's host as the name the handshake verifies - the only login an [`OracleReader`] takes.
#[derive(Debug, Clone)]
pub struct VerifiedOracleLogin {
    login: OracleLogin,
    anchors_pem: String,
}

impl OracleLogin {
    /// The EZCONNECT `host:port/service_name` the dial formats.
    #[cfg(test)]
    fn address(&self) -> String {
        sutura_oracle_connect::ezconnect(&self.host, self.port, &self.service_name)
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

    /// The same login over TLS, verified against `anchors_pem` and no other certificate.
    #[must_use]
    pub const fn verified(self, anchors_pem: String) -> VerifiedOracleLogin {
        VerifiedOracleLogin {
            login: self,
            anchors_pem,
        }
    }
}

/// A live [`DictionaryReader`] over an Oracle documentation schema.
#[derive(Debug, Clone)]
pub struct OracleReader {
    login: OracleLogin,
    dial: PreparedDial,
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
    /// The documentation schema or the predicate column is not an identifier, the login's anchors are
    /// not certificates the driver can verify a server against, or the driver cannot parse its address.
    pub fn new(
        login: VerifiedOracleLogin,
        documentation_schema: &str,
        environment: String,
        predicate: RowPredicate,
        row_cap: Option<NonZeroU64>,
        byte_cap: Option<NonZeroU64>,
    ) -> Result<Self, InvalidReaderConfig> {
        let bounds = checked_bounds(documentation_schema, &predicate, row_cap, byte_cap)?;
        let VerifiedOracleLogin { login, anchors_pem } = login;
        let dial = Dial::new(
            &login.host,
            login.port,
            &login.service_name,
            Channel::Verified {
                anchors_pem: &anchors_pem,
            },
        )
        .prepare()
        .map_err(|refused| match refused {
            PrepareError::TrustAnchors(cause) => InvalidReaderConfig::TrustAnchors(cause.to_string()),
            PrepareError::Address(cause) => InvalidReaderConfig::Address(cause.to_string()),
        })?;
        Ok(Self {
            login,
            dial,
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

    /// The driver's configuration for the read's dial, with no credential, refused while the
    /// driver's packet trace is on.
    fn driver_config(&self) -> Result<oracledb::Config, RdbmsError> {
        self.dial.config().map_err(|on| RdbmsError::Read(Box::new(on)))
    }

    /// Opens the read's session, refused before it dials while the driver's packet trace is on.
    fn connect(&self) -> Result<oracledb::Connection, RdbmsError> {
        let config = self.driver_config()?;
        let login = &self.login;
        #[expect(
            clippy::disallowed_methods,
            reason = "the password's destination is the connection handshake, which is the one place the \
                      value itself is the payload"
        )]
        let password = login.password.expose_secret();
        #[expect(
            clippy::disallowed_methods,
            reason = "`PreparedDial::config` asked `sutura_oracle_connect::oracle_trace::refuse` first"
        )]
        let connection = oracledb::connect(config.set_credentials(&login.user, password));
        connection.map_err(read_err)
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

#[cfg(test)]
mod dial_bound {
    use sutura_domain::identity::Secret;

    use super::{OracleLogin, OracleReader};
    use crate::postgres_reader::{InvalidReaderConfig, RowPredicate};

    /// A self-signed certificate's PEM: anchors the driver accepts.
    fn anchor() -> String {
        let key = rcgen::KeyPair::generate().expect("a key pair generates");
        let params = rcgen::CertificateParams::new([String::from("127.0.0.1")]).expect("an IP name parameterizes");
        params.self_signed(&key).expect("a self-signed leaf signs").pem()
    }

    /// **The reader's dial is bounded by [`sutura_oracle_connect::DIAL_DEADLINE`].** Without the
    /// bound, a connect nobody answers waits on the operating system's own retry schedule.
    #[test]
    fn the_oracle_reader_dials_within_the_dial_deadline() {
        let login = OracleLogin::new(
            String::from("127.0.0.1"),
            1521,
            String::from("FREEPDB1"),
            String::from("reader"),
            Secret::new("a-test-password"),
        )
        .verified(anchor());
        let reader = OracleReader::new(login, "dictionary", String::from("prod"), RowPredicate::None, None, None)
            .expect("a valid reader config");
        let config = reader.driver_config().expect("a verified loopback dial configures");
        assert_eq!(
            config.transport_connect_timeout(),
            Some(sutura_oracle_connect::DIAL_DEADLINE),
            "the reader's dial is not bounded by the dial deadline"
        );
    }

    /// **An address the driver cannot parse refuses the reader at build, before any dial.**
    #[test]
    fn the_oracle_reader_refuses_an_address_the_driver_cannot_parse() {
        let login = OracleLogin::new(
            String::from("not a host"),
            1521,
            String::from("FREEPDB1"),
            String::from("reader"),
            Secret::new("a-test-password"),
        )
        .verified(anchor());
        let refusal = OracleReader::new(login, "dictionary", String::from("prod"), RowPredicate::None, None, None);
        assert!(matches!(refusal, Err(InvalidReaderConfig::Address(_))), "{refusal:?}");
    }
}

#[cfg(test)]
mod anchors {
    use sutura_domain::identity::Secret;

    use super::{OracleLogin, OracleReader};
    use crate::postgres_reader::{InvalidReaderConfig, RowPredicate};

    /// **Anchors that hold no certificate refuse the reader at build, before any dial.**
    #[test]
    fn the_oracle_reader_refuses_anchors_that_hold_no_certificate() {
        let login = OracleLogin::new(
            String::from("127.0.0.1"),
            1521,
            String::from("FREEPDB1"),
            String::from("reader"),
            Secret::new("a-test-password"),
        )
        .verified(String::from("not a certificate"));
        let refusal = OracleReader::new(login, "dictionary", String::from("prod"), RowPredicate::None, None, None);
        assert!(
            matches!(refusal, Err(InvalidReaderConfig::TrustAnchors(ref cause)) if cause.contains("no certificates found")),
            "{refusal:?}"
        );
    }
}

#[cfg(test)]
mod redirect {
    use std::net::TcpStream;
    use std::sync::Arc;

    use sutura_domain::identity::Secret;

    use super::{OracleLogin, OracleReader};
    use crate::postgres_reader::RowPredicate;
    use crate::{DictionaryReader as _, RdbmsError};

    /// A leaf for `127.0.0.1`, self-signed, so the PEM of the leaf is the anchor that admits it.
    fn issue() -> (rcgen::Certificate, rcgen::KeyPair) {
        let params = rcgen::CertificateParams::new([String::from("127.0.0.1")]).expect("an IP name parameterizes");
        let key = rcgen::KeyPair::generate().expect("a key pair generates");
        let certificate = params.self_signed(&key).expect("a self-signed leaf signs");
        (certificate, key)
    }

    /// The TLS session the fake listener speaks TNS inside.
    type Tls = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;

    /// Opens TLS presenting `certificate` on a connection the fake listener accepted, so it speaks TNS
    /// inside it.
    fn tls(certificate: &rcgen::Certificate, key: &rcgen::KeyPair) -> impl FnOnce(TcpStream) -> Option<Tls> + Send + 'static {
        let key = rustls::pki_types::PrivateKeyDer::try_from(key.serialize_der()).expect("a generated key is a private key");
        let config = Arc::new(
            rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("the default protocol versions are safe")
                .with_no_client_auth()
                .with_single_cert(vec![certificate.der().clone()], key)
                .expect("a generated chain and its own key are a pair"),
        );
        move |mut stream| {
            let mut server = rustls::ServerConnection::new(config).ok()?;
            while server.is_handshaking() {
                let _io = server.complete_io(&mut stream).ok()?;
            }
            Some(rustls::StreamOwned::new(server, stream))
        }
    }

    /// A listener that answers the login with a redirect is refused, typed, and the address it named is
    /// never dialled - so no login reaches it.
    #[test]
    fn the_oracle_reader_refuses_a_listener_redirect_before_authentication() {
        let (certificate, key) = issue();
        let listener =
            sutura_dev::tns_listener::RedirectingListener::start_over(tls(&certificate, &key)).expect("the fake listener binds");
        let login = OracleLogin::new(
            String::from("127.0.0.1"),
            listener.port(),
            String::from("FREEPDB1"),
            String::from("reader"),
            Secret::new("a-test-password"),
        )
        .verified(certificate.pem());
        let reader = OracleReader::new(login, "dictionary", String::from("prod"), RowPredicate::None, None, None)
            .expect("a valid reader config");
        let error = reader.read_dictionary().expect_err("a redirecting listener is refused");
        assert!(matches!(error, RdbmsError::RedirectRefused), "{error:?}");
        assert!(
            !listener.target_was_dialled(std::time::Duration::from_secs(2)),
            "the address the redirect named was dialled"
        );
    }
}

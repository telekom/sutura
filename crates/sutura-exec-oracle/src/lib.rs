//! A [`Warehouse`] adapter over Oracle Database - one connection under the deployment's declared
//! identity (`SharedServiceUser`). `github.com/telekom/sutura#127` PR 2, over PR 1's
//! `Dialect::Oracle` rendering.
//!
//! **Synchronous, unlike `sutura_exec_postgres::PostgresWarehouse`.** `oracledb::Connection`'s own
//! methods (`execute`, `query`, `set_call_timeout`) are plain blocking `fn`s over a
//! `std::net::TcpStream` - measured by reading `oracle/rust-oracledb`'s own source, not assumed -
//! so this adapter owns no `tokio` runtime and calls the driver directly. `oracledb` is
//! `#![forbid(unsafe_code)]` with no `build.rs` and no `links` key: nothing here can `dlopen` an
//! Instant Client the way an ODPI-C wrapper would, which was `#127`'s original blocker.
//!
//! SQL renders through `sutura-sql` (`Dialect::Oracle`); nothing here is compiled or translated.
//!
//! ## The one caveat this crate exists to hold, per the spike behind `#127`
//!
//! `Connection::set_call_timeout` is a PER-READ idle timeout (`TcpStream::set_read_timeout`), not a
//! total-call budget, and there is no cancellation API - a slow statement that keeps the socket
//! busy is not stopped by it. Worse, [`sutura_domain::warehouse::deadline::Deadline::remaining_at`]
//! returns `None` when the deadline is EXPIRED, and `set_call_timeout(None)` means *wait forever* -
//! so naively forwarding `deadline.remaining_at(now)` into `set_call_timeout` turns an expired
//! deadline into an unbounded wait, the opposite of a refusal. [`refuse_if_spent`] is the guard:
//! every call site asks it FIRST and never calls the driver, let alone `set_call_timeout`, once it
//! answers `None`. This module's own `#[cfg(test)]` cell,
//! `an_expired_deadline_refuses_before_reaching_the_connection`, is the refusal;
//! `a_live_deadline_answers_the_remaining_duration` is its predicate half.
//!
//! ## Limits
//!
//! - **No transport of its own, and less than `sutura_exec_postgres` carries.** ADR 0010's
//!   declared-trust-store rule (a bundle path, or the host store, resolved once by a composition
//!   root and handed to the adapter as a `rustls::ClientConfig`) has NOWHERE to attach here:
//!   `oracledb::Connection` builds its OWN `rustls::ClientConfig` internally, from a wallet
//!   directory's `ewallet.pem` when one is configured and from the bundled `webpki-roots` set when
//!   one is not (measured by reading `oracle/rust-oracledb/src/transport.rs`) - there is no
//!   constructor that takes an external root store or a caller-built `ClientConfig` at all. So
//!   [`OracleWarehouse::connect_secured`] takes a wallet directory rather than the
//!   `sutura_tls::Rotating<rustls::ClientConfig>` handle Postgres's own `connect_secured` takes, and
//!   a `transport_anchors: system` declaration has nothing on this adapter to reach: there is no
//!   "read the host trust store" option in the driver at all. This is a real fork in ADR 0010, not
//!   an oversight - and it is why `sutura-config` refuses any `transport_mode` but `plaintext` on
//!   a `kind: oracle` source, and its shared rule confines `plaintext` to a loopback host.
//!   [`OracleWarehouse::connect_secured`] therefore has no composition-root caller.
//! - **Wired behind a default-off feature, and in no release.** `sutura-cli`'s `oracle` feature
//!   links this crate into both composition roots through [`OracleWarehouse::connect`];
//!   `nix/shipped.nix` does not carry that feature - see its entry in `sutura-cli`'s manifest.
//! - **Every mapping below is reasoned from the driver's documented wire types, not measured
//!   against a live Oracle** - no docker socket was available while this adapter was written. The
//!   golden matrix's `oracle` cells (`crates/sutura-app/tests/adapters/adapters.rs`) skip rather
//!   than run wherever that is still true - that file's own `DataSystemUnderTest::available`
//!   decides which venues those are.
//! - **No venue that runs `just validate` can reach a live Oracle.** `compose.services.yaml`'s
//!   `oracle` service is a docker-compose tier brought up by hand (`just dev-up-oracle`); the nix
//!   sandbox has no docker socket and no `oracle-tier.nix` exists, so a gate leg cannot
//!   provision one - and Oracle Database is proprietary, so no nix-native tier could take
//!   `nix/postgres-tier.nix`'s shape even in principle. The render goldens this suite pins for
//!   Oracle therefore assert what `sutura-sql` emitted and nothing a data system said back; that
//!   is what `crates/sutura-app/tests/golden/dialects.rs`'s `Venue::ByHandOnly` arm declares. The
//!   check there holds this path, never this prose - a header that stops arguing this stays green.
//! - **One [`parking_lot::Mutex`] serializes every call**, the same shape
//!   `PostgresWarehouse::execution_lock` holds and for a matching reason: `Connection`'s own methods
//!   take `&self`, so the port's shared reference alone does not prove the driver tolerates two
//!   overlapping calls - and nothing here measured that it does.

use parking_lot::Mutex;
use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture};
use sutura_domain::plan::Executable;
use sutura_domain::warehouse::cardinality::{CountsNotRead, DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, MalformedRowSet, ParamValue, Real, RowSet, Value, Warehouse};
use sutura_sql::generate::{generate, generate_key_probe, generate_leg};
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};

/// The fixture tier's credential - a value that cannot exist unconfigured.
///
/// Behind the default-off `fixtures` feature - see that module's own header for why.
#[cfg(feature = "fixtures")]
pub mod fixture;

/// Why this data system could not answer.
#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    #[error("could not connect to Oracle")]
    Connect {
        #[source]
        cause: DriverError,
    },
    #[error("the statement failed while running")]
    Execute {
        #[source]
        cause: DriverError,
    },
    /// The server refused a statement as `ORA-01476: divisor is equal to zero`.
    #[error("the statement was refused by the server as a division by zero")]
    DivisionByZero {
        #[source]
        cause: DriverError,
    },
    /// A column came back as a type this adapter does not map. An error, not a stringified value.
    #[error("column {column} came back as {oracle_type}, which this adapter does not map")]
    UnsupportedType { column: String, oracle_type: &'static str },
    /// A value this adapter asked the driver to decode did not decode.
    #[error("column {column} could not be read as the type this adapter expected for it")]
    ValueDecode {
        column: String,
        #[source]
        cause: DriverError,
    },
    /// A `BINARY_DOUBLE` column came back as a value that is not a number.
    #[error("column {column} came back as a value that is not a finite number")]
    NotFinite {
        column: String,
        #[source]
        cause: sutura_domain::warehouse::NotFinite,
    },
    #[error("the result set was not rectangular")]
    Shape {
        #[source]
        cause: MalformedRowSet,
    },
    /// A key probe's result was not the pair of counts its statement projects.
    #[error("the key probe did not come back as two counts")]
    KeyCounts {
        #[source]
        cause: CountsNotRead,
    },
    #[error("the plan could not be rendered for Oracle")]
    Render {
        #[source]
        cause: GenerateError,
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
    /// The deadline was already spent before this call ever reached the driver - see
    /// [`refuse_if_spent`] for why this is checked rather than forwarded.
    #[error("the deadline was already spent before the call reached the connection")]
    DeadlineSpent,
    /// [`Connection::set_call_timeout`](oracledb::Connection::set_call_timeout) itself refused the value.
    #[error("the call timeout could not be set on the connection")]
    CallTimeout {
        #[source]
        cause: DriverError,
    },
}

/// An Oracle connection, behind the [`Warehouse`] port.
pub struct OracleWarehouse {
    source: sutura_domain::model::SourceName,
    posture: sutura_domain::source::SourcePosture,
    connection: oracledb::Connection,
    /// Serializes every call - see the module header for why.
    execution_lock: Mutex<()>,
}

impl core::fmt::Debug for OracleWarehouse {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OracleWarehouse")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl OracleWarehouse {
    /// Opens one connection over a plain TCP EZCONNECT string (`host:port/service_name`), with no
    /// transport security at all.
    pub fn connect(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        host: &str,
        port: u16,
        service_name: &str,
        user: &str,
        password: &str,
    ) -> Result<Self, OracleError> {
        Self::connect_string(source, posture, &format!("{host}:{port}/{service_name}"), user, password)
    }

    /// Opens one connection over `tcps://host:port/service_name`, with the driver's own wallet-based
    /// TLS - see the module header's limit on how far this reaches ADR 0010's declared-trust-store
    /// rule. `wallet` is a directory containing an `ewallet.pem`; `None` verifies against the
    /// driver's bundled `webpki-roots` set rather than against a declared anchor.
    pub fn connect_secured(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        host: &str,
        port: u16,
        service_name: &str,
        user: &str,
        password: &str,
        wallet: Option<&OracleWallet>,
    ) -> Result<Self, OracleError> {
        let connect_string = format!("tcps://{host}:{port}/{service_name}");
        let mut config = oracledb::Config::default()
            .set_connect_string(&connect_string)
            .map_err(|cause| OracleError::Connect { cause: cause.into() })?
            .set_credentials(user, password);
        if let Some(wallet) = wallet {
            #[expect(
                clippy::disallowed_methods,
                reason = "the wallet password's destination is a connection handshake, which is the one \
                          place the value itself is the payload"
            )]
            let password = wallet.password.expose_secret();
            config = config
                .set_wallet_location(wallet.location.clone())
                .set_wallet_password(password);
        }
        Self::open(source, posture, config)
    }

    /// The shared entry point both constructors above reduce to.
    fn connect_string(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        connect_string: &str,
        user: &str,
        password: &str,
    ) -> Result<Self, OracleError> {
        let config = oracledb::Config::default()
            .set_connect_string(connect_string)
            .map_err(|cause| OracleError::Connect { cause: cause.into() })?
            .set_credentials(user, password);
        Self::open(source, posture, config)
    }

    fn open(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        config: oracledb::Config,
    ) -> Result<Self, OracleError> {
        let connection = oracledb::connect(config).map_err(|cause| OracleError::Connect { cause: cause.into() })?;
        Ok(Self {
            source,
            posture,
            connection,
            execution_lock: Mutex::new(()),
        })
    }

    /// A connection config's host/port/credential for the fixture tier - the counterpart of
    /// `sutura_exec_postgres::PostgresWarehouse::local_config`. `service_name` is fixed at
    /// `FREEPDB1`, the community image's own pluggable database, which is not a secret the tier
    /// publishes - it is the image's name for itself.
    #[cfg(feature = "fixtures")]
    #[expect(
        clippy::disallowed_methods,
        reason = "the credential's destination is a connection handshake, which is the one place the \
                  value itself is the payload"
    )]
    pub fn connect_fixture(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        host: &str,
        port: u16,
        credential: &fixture::FixtureCredential,
    ) -> Result<Self, OracleError> {
        Self::connect(
            source,
            posture,
            host,
            port,
            "FREEPDB1",
            credential.user(),
            credential.password().expose_secret(),
        )
    }

    /// Refuses credential material this adapter has nowhere to put, then checks the presented leg
    /// against how this source was DECLARED - `docs/adr/0008` part 4's two questions, the same split
    /// `PostgresWarehouse::deliverable` draws.
    fn deliverable(&self, presented: &Presented) -> Result<(), OracleError> {
        match *presented {
            Presented::SharedServiceUser { .. } => {}
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => {
                return Err(OracleError::NoPlaceForASubject {
                    at: String::from(self.source.as_str()),
                    presented: presented.as_str(),
                });
            }
        }
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| OracleError::PresentedDisagreesWithPosture { cause })
    }

    fn render(executable: Executable<'_>) -> Result<GeneratedQuery, OracleError> {
        match executable {
            Executable::Query(plan) => generate(plan, Dialect::Oracle).map_err(|cause| OracleError::Render { cause }),
            Executable::Leg(leg) => generate_leg(leg, Dialect::Oracle).map_err(|cause| OracleError::Render { cause }),
        }
    }

    /// The parameters, as the driver wants them - `PostgresWarehouse::bind`'s counterpart.
    fn bind(params: &[ParamValue]) -> Vec<OracleParam> {
        params
            .iter()
            .map(|param| match *param {
                ParamValue::Text(ref v) => OracleParam::Text(v.clone()),
                ParamValue::Date(d) => OracleParam::Date(oracle_date(d)),
            })
            .collect()
    }

    /// One round trip: set the connection's call timeout to what `deadline` has left (never
    /// forwarding an expired one - see [`refuse_if_spent`]), run the statement, and read every row
    /// through [`Self::cell`].
    fn run_with_deadline(&self, query: &GeneratedQuery, deadline: Deadline) -> Result<RowSet, OracleError> {
        let _guard = self.execution_lock.lock();
        let remaining = refuse_if_spent(deadline)?;
        self.connection
            .set_call_timeout(Some(remaining))
            .map_err(|cause| OracleError::CallTimeout { cause: cause.into() })?;
        let bound = Self::bind(query.params());
        let refs: Vec<&dyn oracledb::ToDbValue> = bound.iter().map(OracleParam::as_dyn).collect();
        let cursor = self.connection.query(query.sql(), &refs).map_err(execute_err_mapped)?;
        rows_from_cursor(cursor)
    }

    /// The boot path's own runner - no deadline, called only from [`Warehouse::verify_anchor`] and
    /// [`Warehouse::declared_key`], which have no caller's budget to spend (`docs/adr/0008` part 1).
    fn run(&self, query: &GeneratedQuery) -> Result<RowSet, OracleError> {
        let _guard = self.execution_lock.lock();
        self.connection
            .set_call_timeout(None)
            .map_err(|cause| OracleError::CallTimeout { cause: cause.into() })?;
        let bound = Self::bind(query.params());
        let refs: Vec<&dyn oracledb::ToDbValue> = bound.iter().map(OracleParam::as_dyn).collect();
        let cursor = self.connection.query(query.sql(), &refs).map_err(execute_err_mapped)?;
        rows_from_cursor(cursor)
    }

    /// One cell, as a domain value, dispatched on the column's declared type.
    ///
    /// **Reasoned from the driver's documented types, not measured against a live Oracle** - the
    /// module header's own limit.
    fn cell(label: &str, column: &oracledb::Metadata, row: &oracledb::Row, index: usize) -> Result<Value, OracleError> {
        let decode_err = |cause: oracledb::Error| OracleError::ValueDecode {
            column: String::from(label),
            cause: cause.into(),
        };
        let db_type = column.db_type();
        if *db_type == oracledb::DB_TYPE_BOOLEAN {
            Ok(row
                .get::<Option<bool>>(index)
                .map_err(decode_err)?
                .map_or(Value::Null, |v| Value::Integer(i64::from(v))))
        } else if *db_type == oracledb::DB_TYPE_NUMBER {
            Ok(row
                .get::<Option<oracledb::OracleNumber>>(index)
                .map_err(decode_err)?
                .map_or(Value::Null, |value| numeric_cell(&value)))
        } else if db_type.is_string_type() {
            Ok(row
                .get::<Option<String>>(index)
                .map_err(decode_err)?
                .map_or(Value::Null, Value::Text))
        } else if db_type.is_date_type() {
            row.get::<Option<oracledb::OracleTimestamp>>(index)
                .map_err(decode_err)?
                .map_or(Ok(Value::Null), |ts| date_cell(&ts))
        } else if *db_type == oracledb::DB_TYPE_BINARY_DOUBLE {
            row.get::<Option<f64>>(index).map_err(decode_err)?.map_or_else(
                || Ok(Value::Null),
                |v| {
                    Real::parse(v).map(Value::Real).map_err(|cause| OracleError::NotFinite {
                        column: String::from(label),
                        cause,
                    })
                },
            )
        } else if *db_type == oracledb::DB_TYPE_BINARY_FLOAT {
            Err(OracleError::UnsupportedType {
                column: String::from(label),
                oracle_type: "BINARY_FLOAT; a 32-bit float has no exact 64-bit rendering",
            })
        } else {
            Err(OracleError::UnsupportedType {
                column: String::from(label),
                oracle_type: db_type.name(),
            })
        }
    }
}

/// Wraps [`oracledb::Error`] so it can be a `thiserror` `#[source]`.
///
/// **Measured, not assumed:** `oracledb::Error` implements `Debug` and `Display` but not
/// `std::error::Error` - `oracledb::error::ErrorKind` and `DbError` are its own typed detail, and
/// nothing upstream ties the type into `core::error::Error`'s chain. `#[source]` needs that trait,
/// and there is no orphan-rule obstacle to implementing it for a LOCAL wrapper around a foreign
/// type, so this is the newtype rather than a second, string-only error shape.
#[derive(Debug)]
pub struct DriverError(oracledb::Error);

impl From<oracledb::Error> for DriverError {
    fn from(cause: oracledb::Error) -> Self {
        Self(cause)
    }
}

impl core::fmt::Display for DriverError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for DriverError {}

impl DriverError {
    /// Whether the server's own error message names `code` (`"ORA-01476"`, say).
    ///
    /// **Measured, not the `main` branch's shape:** at the pinned `26.0.0-beta.3`,
    /// `oracledb::ErrorKind::DbError` carries the server's message as a bare `String`, not a
    /// structured type with its own error-number accessor - so this is a substring match on
    /// Oracle's own `ORA-NNNNN:` prefix rather than a typed comparison. A future driver release
    /// that structures this is a strictly easier match to write, not a compatibility break.
    fn names_ora_code(&self, code: &str) -> bool {
        matches!(self.0.kind(), oracledb::ErrorKind::DbError(message) if message.contains(code))
    }

    /// Whether this is the driver's own `CallTimeoutExceeded` - the per-read idle timeout firing,
    /// which the module header's caveat distinguishes from a total budget.
    fn is_call_timeout(&self) -> bool {
        matches!(self.0.kind(), oracledb::ErrorKind::CallTimeoutExceeded)
    }
}

/// A wallet directory for [`OracleWarehouse::connect_secured`] - a path to a directory containing
/// `ewallet.pem`, and the password protecting the private key inside it (if any).
pub struct OracleWallet {
    location: String,
    password: sutura_domain::identity::Secret,
}

impl OracleWallet {
    #[must_use]
    pub fn at(location: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            password: sutura_domain::identity::Secret::new(password.into()),
        }
    }
}

/// A bound parameter, kept owned so the borrows it turns into can outlive the query call.
///
/// `oracledb::ToDbValue`'s supertrait `ToBuf` lives in a private module of that crate, so no type
/// outside it may IMPLEMENT `ToDbValue` - measured by trying it and reading `E0277`. This enum
/// borrows the driver's OWN implementations (`String`, `OracleTimestamp`) instead of adding a third.
enum OracleParam {
    Text(String),
    Date(oracledb::OracleTimestamp),
}

impl OracleParam {
    fn as_dyn(&self) -> &dyn oracledb::ToDbValue {
        match *self {
            Self::Text(ref v) => v,
            Self::Date(ref d) => d,
        }
    }
}

/// A domain [`sutura_domain::calendar::Date`], as the driver's `OracleTimestamp::new_date` wants
/// it - via the date's own ISO text rather than a day-offset arithmetic, because
/// [`sutura_domain::calendar::Date`] exposes no field accessors and its ISO form is fixed-width.
fn oracle_date(date: sutura_domain::calendar::Date) -> oracledb::OracleTimestamp {
    let iso = date.to_iso();
    let mut parts = iso.split('-');
    let year: i16 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let month: u8 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let day: u8 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    oracledb::OracleTimestamp::new_date(year, month, day)
}

/// The reverse of [`oracle_date`]: a `DATE`/`TIMESTAMP` column, back to a domain
/// [`Value::Text`] carrying its ISO day - `PostgresWarehouse::cell`'s own `DATE` arm renders the
/// same way.
fn date_cell(ts: &oracledb::OracleTimestamp) -> Result<Value, OracleError> {
    sutura_domain::calendar::Date::new(ts.year(), ts.month(), ts.day())
        .map(|date| Value::Text(date.to_iso()))
        .map_err(|_cause| OracleError::UnsupportedType {
            column: String::from("<date>"),
            oracle_type: "a date this build cannot represent",
        })
}

/// Maps a decoded `NUMBER` exactly: a scale-zero value that fits `i64` maps to [`Value::Integer`],
/// every other value maps to exact [`Value::Text`] - `PostgresWarehouse::numeric_cell`'s own split,
/// over `OracleNumber`'s `Display` rather than a hand-rolled decoder, since the driver already
/// carries an exact base-10 rendering.
fn numeric_cell(value: &oracledb::OracleNumber) -> Value {
    let text = value.to_string();
    text.parse::<i64>().map_or_else(|_| Value::Text(text), Value::Integer)
}

fn rows_from_cursor(cursor: oracledb::Cursor) -> Result<RowSet, OracleError> {
    let columns: Vec<oracledb::Metadata> = cursor.columns().clone();
    let labels: Vec<String> = columns.iter().map(|c| c.name().to_owned()).collect();
    let mut out: Vec<Vec<Value>> = Vec::new();
    for row in cursor {
        let row = row.map_err(execute_err_mapped)?;
        let mut cells = Vec::with_capacity(columns.len());
        for (index, (label, column)) in labels.iter().zip(columns.iter()).enumerate() {
            cells.push(OracleWarehouse::cell(label, column, &row, index)?);
        }
        out.push(cells);
    }
    RowSet::new(labels, out).map_err(|cause| OracleError::Shape { cause })
}

/// The error from the RUN of a statement, with the one server refusal this adapter refuses to
/// re-render as a generic `Execute`: `ORA-01476: divisor is equal to zero` is how Oracle honors
/// `zero_denominator: fails`, the same split `PostgresWarehouse`'s own `execute_err_mapped` draws for
/// `22012`.
///
/// **Not measured against a live Oracle** - the module header's own limit; `1476` is Oracle's
/// documented error number for this condition.
fn execute_err_mapped(cause: oracledb::Error) -> OracleError {
    let cause = DriverError::from(cause);
    if cause.names_ora_code("ORA-01476") {
        OracleError::DivisionByZero { cause }
    } else {
        OracleError::Execute { cause }
    }
}

/// Refuses locally, no round trip, if `deadline` is already spent; otherwise answers what is left.
///
/// **This is the whole mechanism the module header's caveat is about.**
/// `deadline.remaining_at(now)` answers `None` exactly when the deadline is expired, and
/// `Connection::set_call_timeout(None)` means *wait forever* - so every call site asks this FIRST
/// and never reaches the driver once it answers `Err`. See this module's own
/// `an_expired_deadline_refuses_before_reaching_the_connection` test for the cell that fails if
/// this check is removed while [`Deadline::remaining_at`] is still read.
pub(crate) fn refuse_if_spent(deadline: Deadline) -> Result<std::time::Duration, OracleError> {
    deadline
        .remaining_at(std::time::Instant::now())
        .ok_or(OracleError::DeadlineSpent)
}

impl Warehouse for OracleWarehouse {
    type Error = OracleError;

    /// **One connection under the deployment's declared identity.** Oracle support is decided to
    /// declare `SharedServiceUser` for this issue - `#923` is where a per-subject path would live -
    /// so there is nowhere for a subject's own credential to arrive.
    const IMPERSONATION: sutura_domain::source::ImpersonationCapability =
        sutura_domain::source::ImpersonationCapability::NoPlaceForASubject;

    /// A [`LegPlan`](sutura_domain::plan::LegPlan) renders through `generate_leg` at
    /// [`Dialect::Oracle`] and is handed to [`Self::execute`] as any other statement.
    ///
    /// **Rendered and gate-checked, never executed, and that asymmetry with
    /// `sutura-exec-postgres` is the whole of what this constant is worth here.** No venue any
    /// gate reaches can provision an Oracle (this module's header and
    /// `xtask/src/conformance/reconcile.rs`'s `UNBOUND` entry say why), so this crate has no
    /// conformance binding and the golden matrix's `oracle` cells answer
    /// `DataSystemUnderTest::available() == false` unconditionally. What holds the wiring is
    /// this module's own `a_leg_renders_through_the_oracle_dialect` cell plus
    /// `crates/sutura-app/tests/golden/legs.rs`'s Oracle statements - both renderings. **Nothing
    /// establishes that an Oracle server accepts a leg**, which is exactly the shape
    /// `crates/sutura-app/tests/golden/dialects.rs` declares as `Evidence::RenderOnly` for this
    /// dialect and that declaration is unchanged by this constant.
    ///
    /// Identity is untouched: [`Self::IMPERSONATION`] stays `NoPlaceForASubject`. No composition
    /// root links this adapter, so no Oracle leg reaches `ExecutedAs::uniform` from any binary; if
    /// one ever does, `shared-service-user` is what it will present.
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &sutura_domain::model::SourceName {
        &self.source
    }

    fn posture(&self) -> &sutura_domain::source::SourcePosture {
        &self.posture
    }

    /// **Takes the trait's own default** (`Ok(PreFlight::NotAsked)`) rather than an override.
    ///
    /// **Measured, not assumed:** the pinned `oracledb` `26.0.0-beta.3` has no parse-only round
    /// trip at all - `Statement`'s only ways to reach the server are `execute`/`query`, which RUN
    /// the statement. The `main` branch this crate first read gained
    /// `Statement::ensure_fully_parsed` after that version was published; pinning to a released
    /// version rather than a `git` dependency means this adapter does not have it yet. Overriding
    /// `dry_run` with a real `execute`/`query` call would violate the port's own contract - "an
    /// adapter that overrides it must not read data" - so the honest thing this adapter can do
    /// today is nothing, exactly the trait's own documented escape hatch for "checking is not
    /// cheaper than running here".
    fn execute(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<RowSet, Self::Error> {
        self.deliverable(presented)?;
        let query = Self::render(executable)?;
        self.run_with_deadline(&query, deadline)
    }

    fn verify_anchor(&self, plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        let query = generate(plan.plan(), Dialect::Oracle).map_err(|cause| OracleError::Render { cause })?;
        self.run(&query).map(AnchorRows::of)
    }

    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        let query = generate_key_probe(&key, Dialect::Oracle).map_err(|cause| OracleError::Render { cause })?;
        let rows = self.run(&query)?;
        KeyUniqueness::read(&rows).map_err(|cause| OracleError::KeyCounts { cause })
    }

    /// `ORA-01476` via [`execute_err_mapped`], or a local [`OracleError::DeadlineSpent`] - the
    /// caveat this crate exists to hold.
    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(*error, OracleError::DeadlineSpent)
            || matches!(*error, OracleError::Execute { ref cause } if cause.is_call_timeout())
    }

    /// `ORA-01031: insufficient privileges` - the server refusing an identity at the permission
    /// level, the same class `PostgresWarehouse::source_refused` reads off `42501`.
    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(*error, OracleError::Execute { ref cause } if cause.names_ora_code("ORA-01031"))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use sutura_domain::plan::Executable;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};

    use super::{OracleWarehouse, refuse_if_spent};

    /// **The predicate half**: a deadline with time left answers the remaining duration rather than
    /// refusing.
    #[test]
    fn a_live_deadline_answers_the_remaining_duration() {
        let deadline = Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(30)).expect("a budget"));
        let remaining = refuse_if_spent(deadline).expect("a live deadline has time left");
        assert!(remaining <= Duration::from_secs(30));
        assert!(remaining > Duration::ZERO);
    }

    /// **The refusal half, and the one the module header's caveat is about.** An EXPIRED deadline
    /// must refuse HERE, before any call reaches the connection - never fall through to a
    /// `set_call_timeout(None)`, which the driver reads as *wait forever*.
    ///
    /// Neutralising the refusal while keeping [`Deadline::remaining_at`] itself read - replacing
    /// `.ok_or(OracleError::DeadlineSpent)` with, say, `.unwrap_or(Duration::ZERO)` - turns this cell
    /// from `Err` to `Ok(Duration::ZERO)` and fails it: `dead_code` cannot catch that mutation
    /// because the field is still read, only the refusal is gone.
    #[test]
    fn an_expired_deadline_refuses_before_reaching_the_connection() {
        let opened_long_ago = Instant::now()
            .checked_sub(Duration::from_secs(3600))
            .expect("an hour before now does not underflow an Instant");
        let deadline = Deadline::opened_at(opened_long_ago, Budget::parse(Duration::from_millis(1)).expect("a budget"));
        let refusal = refuse_if_spent(deadline);
        assert!(
            matches!(refusal, Err(super::OracleError::DeadlineSpent)),
            "an expired deadline must refuse locally rather than answer a duration to forward: {refusal:?}"
        );
    }

    /// **One leg of a federated question renders here rather than being refused**, and it renders
    /// at THIS adapter's dialect.
    ///
    /// The plan is `sutura_conformance::corpus`'s own leg case rather than a hand-built one, so no
    /// expectation below is computed from `generate_leg`: the placeholder form is read off
    /// `Dialect::Oracle`'s declared [`sutura_sql::PlaceholderStyle::Colon`] - `:1`, which Postgres
    /// renders as `$1` and `DuckDB` as `?` - so rendering this leg at any other dialect fails the
    /// cell rather than passing it. That is the mutation `dead_code` cannot see and the reason the
    /// assertion is not simply `is_ok`.
    ///
    /// **What it does NOT establish: that any Oracle server accepts the statement.** No venue a
    /// gate reaches provisions one - see this module's header - so this crate has no conformance
    /// binding and there is no executed counterpart to
    /// `conformance::postgres::a_leg_is_executed_because_the_adapter_declares_it_executes_legs`.
    #[test]
    fn a_leg_renders_through_the_oracle_dialect() {
        let case = sutura_conformance::corpus::leg_case();
        let query = OracleWarehouse::render(Executable::Leg(case.leg())).expect("a leg renders for Oracle");
        assert!(
            query.sql().contains(sutura_conformance::corpus::table().as_str()),
            "the leg must read the corpus table: {}",
            query.sql()
        );
        assert!(
            query.sql().contains(":1"),
            "a leg rendered for Oracle binds with a colon placeholder, not `$1` or `?`: {}",
            query.sql()
        );
        assert_eq!(
            query.params().len(),
            2,
            "the leg's range is two bound values: {:?}",
            query.params()
        );
    }
}

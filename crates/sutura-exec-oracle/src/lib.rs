#![forbid(unsafe_code)]
//! A [`Warehouse`] adapter over Oracle Database. `github.com/telekom/sutura#127` PR 2, over PR 1's
//! `Dialect::Oracle` rendering.
//!
//! **Two postures.** A `shared-service-user` source answers every question on one connection under
//! the deployment's declared user. An `impersonation-at-source` source opens a session of its own
//! for each question, with the asker's own verified token ([`TokenSessions`]), so the database
//! authenticates the asker and runs the statement as the user it maps that token to. The boot
//! connection under the declared user stays, for the boot path's own probes only
//! ([`Warehouse::verify_anchor`], [`Warehouse::declared_key`]); a question never runs on it.
//!
//! **Synchronous.** `oracledb::Connection`'s own
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
//! - **TLS verifies against the declared anchors only.** [`Channel::Verified`] hands the driver
//!   PEM certificates that REPLACE its bundled public certificate authorities, so a server is
//!   admitted only under a certificate those anchors issue. The driver still builds its own
//!   `rustls::ClientConfig`, so ADR 0010's host store (`transport_anchors: system`) and a client
//!   certificate (`mutual`) have nothing here to reach, and `sutura-config` refuses both on a
//!   `kind: oracle` source.
//! - **A listener's redirect is refused before authentication**, as
//!   [`OracleError::RedirectRefused`]: the driver is told not to follow one, so the connection
//!   stays on the address the source declared. A clustered listener that redirects every client is
//!   therefore refused too; declare the address that answers.
//! - **One [`parking_lot::Mutex`] serializes every call**, and for good reason:
//!   `Connection`'s own methods
//!   take `&self`, so the port's shared reference alone does not prove the driver tolerates two
//!   overlapping calls - and nothing here measured that it does.
//! - **The session lifecycle, and the bound on sessions.** A question at an impersonating source
//!   opens its session under that lock, runs on it, and closes it before the lock is released, so a
//!   source holds at most one asker's session at a time beside its boot connection, and no session
//!   outlives the question or serves a second asker. The dial bound covers each session's TCP
//!   connect; the handshake and the close after it are not bounded, so a database that stops
//!   answering there holds the source.
//! - **The token is copied once, into the driver's configuration**, as an ordinary `String` the
//!   driver keeps masked; nothing here zeroes that copy.

use parking_lot::Mutex;
use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture, Secret};
use sutura_domain::plan::Executable;
use sutura_domain::warehouse::arrow::of_row_set;
use sutura_domain::warehouse::cardinality::{CountsNotRead, DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, MalformedRowSet, ParamValue, Real, ResultBatches, RowSet, Value, Warehouse};
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
    /// The declared trust anchors are not PEM certificates the driver can verify a server against.
    #[error("the declared trust anchors are not usable certificates")]
    TrustAnchors {
        #[source]
        cause: DriverError,
    },
    #[error("could not connect to Oracle")]
    Connect {
        #[source]
        cause: DriverError,
    },
    /// The listener answered with a redirect. Every redirect is refused before authentication: the
    /// address it names is one no source declared, so no declared transport governs it.
    #[error("the listener redirected the connection to an address this source does not declare")]
    RedirectRefused {
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
    /// The collected result would cost more than this adapter's materialisation budget to hold.
    ///
    /// The sibling of [`Self::Shape`] for the byte budget the port's
    /// [`result_did_not_fit`](sutura_domain::warehouse::Warehouse::result_did_not_fit) reads: a
    /// result refused for crossing it is *the result did not fit*, never a data-system failure, so
    /// a caller is refused rather than told to retry.
    #[error("a result would cost more than the {most_bytes}-byte materialisation budget to hold")]
    OverBudget { most_bytes: usize },
    /// A key probe's result was not the pair of counts its statement projects.
    ///
    /// A defect in the rendering or in this adapter's value mapping, never anything about the data:
    /// the probe projects two aggregates over no group, so one row of two integers is the only
    /// shape it can have. It travels as an `Err` from the port, which the boot path reads as *this
    /// declaration went unchecked* rather than as a violated one.
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
    /// The credential broker handed this adapter a leg it cannot open a session for.
    #[error(
        "source `{at}` was handed {presented}, and this adapter opens a session only with the \
         asker's own token at an impersonating source, and names no principal to become. This is a \
         wiring defect between the credential broker and the source declaration"
    )]
    Undeliverable { at: String, presented: &'static str },
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
    /// The driver's packet trace is switched on - see [`sutura_runtime::oracle_trace::refuse`].
    #[error(transparent)]
    PacketTraceOn(#[from] sutura_runtime::oracle_trace::PacketTraceOn),
}

/// An Oracle connection, behind the [`Warehouse`] port.
pub struct OracleWarehouse {
    routing: Routing,
    /// The materialisation budget this adapter bounds every collected result with, derived by the
    /// composition root from the same working-set ceiling that sizes the in-process engine. There
    /// is no unset state: every constructor requires one, so a call site with no budget does not
    /// compile.
    result_budget: sutura_domain::warehouse::ResultBudget,
    /// The boot connection, under the declared user. A question runs on it only at a shared source.
    connection: oracledb::Connection,
    /// Serializes every call - see the module header for why.
    execution_lock: Mutex<()>,
}

/// The source, how it was declared, and where each question's own session is opened - for an
/// `impersonation-at-source` source alone.
struct Routing {
    source: sutura_domain::model::SourceName,
    posture: sutura_domain::source::SourcePosture,
    per_caller: Option<TokenSessions>,
}

impl Routing {
    /// Picks the session a leg can be answered on, then checks the presented leg against how this
    /// source was DECLARED - `docs/adr/0008` part 4's two questions, the same split
    /// `sutura_exec_postgres::deliverable` draws.
    fn deliverable<'leg>(&'leg self, presented: &'leg Presented) -> Result<Session<'leg>, OracleError> {
        let session = session_for(&self.source, self.per_caller.as_ref(), presented)?;
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| OracleError::PresentedDisagreesWithPosture { cause })?;
        Ok(session)
    }
}

impl core::fmt::Debug for OracleWarehouse {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OracleWarehouse")
            .field("source", &self.routing.source)
            .finish_non_exhaustive()
    }
}

/// How long the TCP connect to the listener may take before the dial is refused.
///
/// The bound covers the connect only: a listener that accepts and then never answers is not bounded
/// by it, because the driver reads its handshake with no timeout.
pub const DIAL_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

/// How the connection to the listener is secured.
#[derive(Debug, Clone, Copy)]
pub enum Channel<'pem> {
    /// No transport security. A composition root reaches this only for a loopback host.
    Plaintext,
    /// TLS, verified against these PEM certificates and no others: they replace the driver's bundled
    /// public certificate authorities rather than adding to them.
    Verified { anchors_pem: &'pem str },
}

/// Where one connection goes and how it is secured: an EZCONNECT `host:port/service_name`.
#[derive(Debug, Clone, Copy)]
pub struct Dial<'dial> {
    host: &'dial str,
    port: u16,
    service_name: &'dial str,
    channel: Channel<'dial>,
    deadline: std::time::Duration,
}

impl<'dial> Dial<'dial> {
    /// A dial bounded by [`DIAL_DEADLINE`].
    #[must_use]
    pub const fn new(host: &'dial str, port: u16, service_name: &'dial str, channel: Channel<'dial>) -> Self {
        Self {
            host,
            port,
            service_name,
            channel,
            deadline: DIAL_DEADLINE,
        }
    }

    /// The same dial with its TCP connect bounded by `deadline` instead.
    #[must_use]
    pub const fn within(self, deadline: std::time::Duration) -> Self {
        Self { deadline, ..self }
    }
}

/// The declared dial with no user or password: where each question's own session is opened, with
/// the asker's own token, at an `impersonation-at-source` source.
///
/// The driver refuses a token over anything but TLS, so a [`Channel::Plaintext`] dial opens no
/// session; the settings parse refuses that declaration before a dial exists.
#[derive(Clone)]
pub struct TokenSessions(oracledb::Config);

impl core::fmt::Debug for TokenSessions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TokenSessions").finish_non_exhaustive()
    }
}

impl TokenSessions {
    /// The driver's configuration for `dial`: its address, its channel and anchors, the dial bound, and a refused redirect.
    ///
    /// # Errors
    ///
    /// [`OracleError::PacketTraceOn`] while the driver's packet trace is switched on,
    /// [`OracleError::TrustAnchors`] for anchors that are not usable certificates, and
    /// [`OracleError::Connect`] for an address the driver cannot parse.
    pub fn new(dial: Dial<'_>) -> Result<Self, OracleError> {
        sutura_runtime::oracle_trace::refuse()?;
        let address = ezconnect(dial.host, dial.port, dial.service_name);
        let connect_string = match dial.channel {
            Channel::Plaintext => address,
            Channel::Verified { .. } => format!("tcps://{address}"),
        };
        let mut config = oracledb::Config::default()
            .set_connect_string(&connect_string)
            .map_err(connect_err)?
            .set_follow_redirects(false)
            .set_transport_connect_timeout(Some(dial.deadline));
        if let Channel::Verified { anchors_pem } = dial.channel {
            config = config
                .set_trust_anchors_pem(anchors_pem)
                .map_err(|cause| OracleError::TrustAnchors { cause: cause.into() })?;
        }
        Ok(Self(config))
    }

    /// Opens one session that authenticates with `token`. Dropping it closes the session.
    ///
    /// # Errors
    ///
    /// [`OracleError::RedirectRefused`] for a listener that redirects, and [`OracleError::Connect`]
    /// for every other refused dial or login. Neither carries the token.
    pub fn open(&self, token: &Secret) -> Result<oracledb::Connection, OracleError> {
        #[expect(
            clippy::disallowed_methods,
            reason = "the asker's token is what this session authenticates with, so it is handed to the \
                      driver once, here; nothing on this path logs or formats it"
        )]
        let token = String::from(token.expose_secret());
        #[expect(clippy::disallowed_methods, reason = "`TokenSessions::new` asked the packet-trace refusal")]
        let session = oracledb::connect(self.0.clone().set_external_auth(oracledb::ExternalAuth::AccessToken(token)));
        session.map_err(connect_err)
    }
}

/// Which session a question runs on.
#[derive(Debug, Clone, Copy)]
enum Session<'leg> {
    /// The boot connection, under the declared user.
    Boot,
    /// A session of its own, opened with the asker's token.
    Caller {
        sessions: &'leg TokenSessions,
        token: &'leg Secret,
    },
}

/// Which session `presented` is answered on: the boot connection for a shared leg, or a session
/// of its own for the asker's own token - and only where the source opens one.
///
/// Whether the leg agrees with the source's declared posture is the next question, and
/// [`Presented::agrees_with`]'s: a shared leg at an impersonating source passes here and is refused
/// there.
fn session_for<'leg>(
    at: &sutura_domain::model::SourceName,
    per_caller: Option<&'leg TokenSessions>,
    presented: &'leg Presented,
) -> Result<Session<'leg>, OracleError> {
    match (presented, per_caller) {
        (&Presented::SharedServiceUser { .. }, _) => Ok(Session::Boot),
        (Presented::SubjectToken { material }, Some(sessions)) => Ok(Session::Caller {
            sessions,
            token: material,
        }),
        _ => Err(OracleError::Undeliverable {
            at: String::from(at.as_str()),
            presented: presented.as_str(),
        }),
    }
}

impl OracleWarehouse {
    /// Opens one connection to the listener `dial` names, under the declared user.
    ///
    /// A redirect from the listener is refused before authentication as
    /// [`OracleError::RedirectRefused`], so the connection stays on the declared address. For an
    /// `impersonation-at-source` `posture` the same dial, with no user or password, is kept to open
    /// each question's own session.
    pub fn connect(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        dial: Dial<'_>,
        user: &str,
        password: &str,
        result_budget: sutura_domain::warehouse::ResultBudget,
    ) -> Result<Self, OracleError> {
        let dialled = TokenSessions::new(dial)?;
        #[expect(clippy::disallowed_methods, reason = "`TokenSessions::new` asked the packet-trace refusal")]
        let connection = oracledb::connect(dialled.0.clone().set_credentials(user, password)).map_err(connect_err)?;
        let per_caller = matches!(posture, sutura_domain::source::SourcePosture::ImpersonationAtSource).then_some(dialled);
        Ok(Self {
            routing: Routing {
                source,
                posture,
                per_caller,
            },
            result_budget,
            connection,
            execution_lock: Mutex::new(()),
        })
    }

    /// A connection config's host/port/credential for the fixture tier - the counterpart of
    /// `sutura_exec_postgres::adbc::Conninfo`. `service_name` is fixed at
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
        result_budget: sutura_domain::warehouse::ResultBudget,
    ) -> Result<Self, OracleError> {
        Self::connect(
            source,
            posture,
            Dial::new(host, port, "FREEPDB1", Channel::Plaintext),
            credential.user(),
            credential.password().expose_secret(),
            result_budget,
        )
    }

    fn render(executable: Executable<'_>) -> Result<GeneratedQuery, OracleError> {
        match executable {
            Executable::Query(plan) => generate(plan, Dialect::Oracle).map_err(|cause| OracleError::Render { cause }),
            Executable::Leg(leg) => generate_leg(leg, Dialect::Oracle).map_err(|cause| OracleError::Render { cause }),
        }
    }

    /// The parameters, as the driver wants them.
    fn bind(params: &[ParamValue]) -> Vec<OracleParam> {
        params
            .iter()
            .map(|param| match *param {
                ParamValue::Text(ref v) => OracleParam::Text(v.clone()),
                ParamValue::Date(d) => OracleParam::Date(oracle_date(d)),
            })
            .collect()
    }

    /// One round trip on `session`: set the connection's call timeout to what `deadline` has left
    /// (never forwarding an expired one - see [`refuse_if_spent`]), run the statement, and read
    /// every row through [`Self::cell`]. An asker's session is opened first and closed on return,
    /// both under the lock.
    fn run_with_deadline(
        &self,
        session: Session<'_>,
        query: &GeneratedQuery,
        deadline: Deadline,
        most_rows: Option<usize>,
    ) -> Result<RowSet, OracleError> {
        let _guard = self.execution_lock.lock();
        refuse_if_spent(deadline)?;
        let opened;
        let connection = match session {
            Session::Boot => &self.connection,
            Session::Caller { sessions, token } => {
                opened = sessions.open(token)?;
                &opened
            }
        };
        let remaining = refuse_if_spent(deadline)?;
        connection
            .set_call_timeout(Some(remaining))
            .map_err(|cause| OracleError::CallTimeout { cause: cause.into() })?;
        let bound = Self::bind(query.params());
        let refs: Vec<&dyn oracledb::ToDbValue> = bound.iter().map(OracleParam::as_dyn).collect();
        let cursor = connection.query(query.sql(), &refs).map_err(execute_err_mapped)?;
        rows_from_cursor(cursor, self.result_budget, most_rows)
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
        rows_from_cursor(cursor, self.result_budget, None)
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
        if db_type == oracledb::DB_TYPE_BOOLEAN {
            Ok(row
                .get::<Option<bool>>(index)
                .map_err(decode_err)?
                .map_or(Value::Null, |v| Value::Integer(i64::from(v))))
        } else if db_type == oracledb::DB_TYPE_NUMBER {
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
        } else if db_type == oracledb::DB_TYPE_BINARY_DOUBLE {
            row.get::<Option<f64>>(index).map_err(decode_err)?.map_or_else(
                || Ok(Value::Null),
                |v| {
                    Real::parse(v).map(Value::Real).map_err(|cause| OracleError::NotFinite {
                        column: String::from(label),
                        cause,
                    })
                },
            )
        } else if db_type == oracledb::DB_TYPE_BINARY_FLOAT {
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

/// An EZCONNECT `host:port/service_name`, with an IPv6 literal in brackets.
///
/// The driver's EZCONNECT parser reads a host as a bracketed literal or a run of name characters,
/// so an unbracketed `::1:1521/FREEPDB1` is not an EZCONNECT string to it at all: it falls through
/// to a `tnsnames.ora` alias lookup and fails - or, where a configuration directory is set in the
/// environment, resolves whatever that file names. Bracketing is decided on a parsed address, not
/// on the presence of a `:`, so a name is never wrapped.
fn ezconnect(host: &str, port: u16, service_name: &str) -> String {
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{host}]:{port}/{service_name}")
    } else {
        format!("{host}:{port}/{service_name}")
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
    /// Whether the server refused with Oracle error number `code` (`1476` for `ORA-01476`, say).
    ///
    /// The driver's `DbError` carries the number the server sent as its own field, so this is a
    /// typed comparison rather than a substring match on the message. Limit: it is the TOP error
    /// of a stack - an `ORA-01476` the server nests under another code no longer matches, where
    /// the old substring match did. Not measured against a server.
    fn has_ora_code(&self, code: usize) -> bool {
        matches!(self.0.kind(), oracledb::ErrorKind::DbError(db_error) if db_error.code() == code)
    }

    /// Whether this is the driver's own `CallTimeoutExceeded` - the per-read idle timeout firing,
    /// which the module header's caveat distinguishes from a total budget.
    fn is_call_timeout(&self) -> bool {
        matches!(self.0.kind(), oracledb::ErrorKind::CallTimeoutExceeded)
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
/// [`Value::Text`] carrying its ISO day - the domain's Arrow reader renders a Postgres `DATE` the
/// same way.
fn date_cell(ts: &oracledb::OracleTimestamp) -> Result<Value, OracleError> {
    sutura_domain::calendar::Date::new(ts.year(), ts.month(), ts.day())
        .map(|date| Value::Text(date.to_iso()))
        .map_err(|_cause| OracleError::UnsupportedType {
            column: String::from("<date>"),
            oracle_type: "a date this build cannot represent",
        })
}

/// Maps a decoded `NUMBER` exactly: a value whose driver-rendered base-10 text parses as an `i64`
/// maps to [`Value::Integer`], every other value maps to exact [`Value::Text`] -
/// the Postgres adapter's own `NUMERIC` split, over `OracleNumber`'s `Display` rather than a
/// hand-rolled decoder, since the driver already carries an exact base-10 rendering.
fn numeric_cell(value: &oracledb::OracleNumber) -> Value {
    let text = value.to_string();
    text.parse::<i64>().map_or_else(|_| Value::Text(text), Value::Integer)
}

fn rows_from_cursor(
    cursor: oracledb::Cursor,
    budget: sutura_domain::warehouse::ResultBudget,
    most_rows: Option<usize>,
) -> Result<RowSet, OracleError> {
    let columns: Vec<oracledb::Metadata> = cursor.columns().to_vec();
    let labels: Vec<String> = columns.iter().map(|c| c.name().to_owned()).collect();
    let values = cursor.map(|row| {
        let row = row.map_err(execute_err_mapped)?;
        columns
            .iter()
            .enumerate()
            .map(|(index, column)| OracleWarehouse::cell(column.name(), column, &row, index))
            .collect::<Result<Vec<_>, _>>()
    });
    collect_rows(labels, values, budget, most_rows)
}

fn collect_rows(
    labels: Vec<String>,
    rows: impl Iterator<Item = Result<Vec<Value>, OracleError>>,
    budget: sutura_domain::warehouse::ResultBudget,
    most_rows: Option<usize>,
) -> Result<RowSet, OracleError> {
    let mut collected = sutura_domain::warehouse::Budgeted::collecting(budget);
    for cells in rows {
        let cells = cells?;
        collected.push(cells).map_err(|cause| OracleError::OverBudget {
            most_bytes: cause.most_bytes(),
        })?;
        if most_rows.is_some_and(|most| collected.delivered() >= most) {
            break;
        }
    }
    collected.finish(labels).map_err(|cause| OracleError::Shape { cause })
}

/// The error from opening the connection, with a refused redirect as its own variant.
fn connect_err(cause: oracledb::Error) -> OracleError {
    let cause = DriverError::from(cause);
    if matches!(cause.0.kind(), oracledb::ErrorKind::RedirectNotAllowed) {
        OracleError::RedirectRefused { cause }
    } else {
        OracleError::Connect { cause }
    }
}

/// The error from the RUN of a statement, with the one server refusal this adapter refuses to
/// re-render as a generic `Execute`: `ORA-01476: divisor is equal to zero` is how Oracle honors
/// `zero_denominator: fails`, the same split the Postgres adapter's `DivisionByZero` draws for
/// `22012`.
///
/// **Not measured against a live Oracle** - the module header's own limit; `1476` is Oracle's
/// documented error number for this condition.
fn execute_err_mapped(cause: oracledb::Error) -> OracleError {
    let cause = DriverError::from(cause);
    if cause.has_ora_code(1476) {
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

    /// **A subject's own token arrives here**, at an `impersonation-at-source` source: each
    /// question opens its own session with it (`#923`).
    const IMPERSONATION: sutura_domain::source::ImpersonationCapability =
        sutura_domain::source::ImpersonationCapability::PerSubjectCredential;

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
    /// A leg is answered on the session its presented credential selects, as any other statement.
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &sutura_domain::model::SourceName {
        &self.routing.source
    }

    fn posture(&self) -> &sutura_domain::source::SourcePosture {
        &self.routing.posture
    }

    /// **Takes the trait's own default** (`Ok(PreFlight::NotAsked)`) rather than an override.
    ///
    /// The pinned `oracledb` does have a parse-only round trip, `Statement::ensure_fully_parsed`,
    /// and this adapter does not call it: adopting it is a separate change, not made here. Its own
    /// doc says a DDL statement is executed by it, and whether it reads data for every statement
    /// this adapter sends is unmeasured. Overriding `dry_run` with `execute`/`query`, which RUN the
    /// statement, would break the port's contract - "an adapter that overrides it must not read
    /// data" - so until that is measured the trait's own escape hatch for "checking is not cheaper
    /// than running here" is the honest answer.
    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        let session = self.routing.deliverable(presented)?;
        let query = Self::render(executable)?;
        let rows = self.run_with_deadline(session, &query, deadline, executable.row_limit())?;
        // The Arrow port's conversion, in the adapter that owns the row-speaking driver - see
        // `sutura_exec_postgres`'s own `execute` and `sutura_domain::warehouse::arrow`.
        of_row_set(&rows).map_err(|cause| OracleError::Shape { cause })
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

    /// A local [`OracleError::DeadlineSpent`] - the caveat this crate exists to hold - or a driver
    /// `CallTimeoutExceeded` re-rendered as [`OracleError::Execute`].
    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(*error, OracleError::DeadlineSpent)
            || matches!(*error, OracleError::Execute { ref cause } if cause.is_call_timeout())
    }

    /// `ORA-01031: insufficient privileges` - the server refusing an identity at the permission
    /// level, the same class `sutura_exec_postgres::adbc::AdbcPostgres::source_refused` reads off `42501`.
    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(*error, OracleError::Execute { ref cause } if cause.has_ora_code(1031))
    }

    /// Answers for the MATERIALISATION BUDGET alone: `rows_from_cursor` collects against this
    /// adapter's own budget, so a result refused for crossing it is exactly *the result did not
    /// fit* - a governance outcome the caller cannot retry past, reached as a refusal rather than
    /// the `503` a data-system failure would mean. Every other failure shape stays `false`.
    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(*error, OracleError::OverBudget { .. })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    use sutura_domain::plan::Executable;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_domain::warehouse::{ResultBudget, Value};

    use super::{
        Channel, Dial, OracleError, OracleWarehouse, Session, TokenSessions, collect_rows, ezconnect, refuse_if_spent,
        session_for,
    };

    mod routing;

    fn result_budget(bytes: usize) -> ResultBudget {
        ResultBudget::of_bytes(core::num::NonZeroUsize::new(bytes).expect("a test budget is positive"))
    }

    #[test]
    fn an_oracle_row_stream_stops_at_the_answer_witness() {
        let read = Cell::new(0_usize);
        let rows = std::iter::from_fn(|| {
            read.set(read.get().saturating_add(1));
            Some(Ok(vec![Value::Integer(1)]))
        });
        let result =
            collect_rows(vec![String::from("value")], rows, result_budget(1024), Some(2)).expect("two rows fit the byte budget");
        assert_eq!(result.rows().len(), 2);
        assert_eq!(read.get(), 2, "the third row was never decoded");
    }

    #[test]
    fn an_oracle_row_stream_refuses_when_the_second_row_crosses_the_byte_budget() {
        let read = Cell::new(0_usize);
        let rows = std::iter::from_fn(|| {
            read.set(read.get().saturating_add(1));
            Some(Ok(vec![Value::Text("x".repeat(200))]))
        });
        let error = collect_rows(vec![String::from("value")], rows, result_budget(1000), None)
            .expect_err("the second row exceeds the conversion budget");
        assert!(matches!(error, OracleError::OverBudget { most_bytes: 1000 }), "{error:?}");
        assert_eq!(read.get(), 2, "the third row was never decoded");
    }

    /// **An IPv6 loopback literal is dialled, not looked up.** `sutura-config` accepts `::1` as a
    /// loopback host; unbracketed, the driver reads the whole string as a `tnsnames.ora` alias and
    /// `set_connect_string` fails. The driver's own parse of the result is the assertion, so no
    /// socket is opened.
    #[test]
    fn an_ipv6_literal_host_reaches_the_driver_as_that_address() {
        let config = oracledb::Config::default()
            .set_connect_string(&ezconnect("::1", 1521, "FREEPDB1"))
            .unwrap_or_else(|e| panic!("the driver did not parse the connect string: {e:?}"));
        let descriptor = config.get_connect_descriptor();
        // The driver re-brackets an address it parsed as IPv6 when it renders the descriptor, so
        // this is `::1` read as an address - a tnsnames alias would never reach here at all.
        assert!(descriptor.contains("(HOST=[::1])"), "{descriptor}");
        assert!(descriptor.contains("(PORT=1521)"), "{descriptor}");
        assert_eq!(ezconnect("127.0.0.1", 1521, "FREEPDB1"), "127.0.0.1:1521/FREEPDB1");
    }

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

    /// The shipped connect bound, written out rather than read off `DIAL_DEADLINE`, so a change to
    /// the constant fails here.
    #[test]
    fn a_dial_is_bounded_by_ten_seconds() {
        let dial = Dial::new("127.0.0.1", 1521, "FREEPDB1", Channel::Plaintext);
        assert_eq!(dial.deadline, Duration::from_secs(10));
    }

    /// **An asker's token selects a session of its own, and nothing else does.** Only a source that
    /// opens per-caller sessions takes the token, and it is the asker's own token the session gets;
    /// a shared leg runs on the boot connection, and a principal to become is refused.
    #[test]
    fn an_askers_token_selects_a_session_of_its_own_and_nothing_else_does() {
        use sutura_domain::identity::{Presented, PrincipalName, Secret};

        let at = sutura_conformance::corpus::source();
        let sessions =
            TokenSessions::new(Dial::new("127.0.0.1", 2484, "FREEPDB1", Channel::Plaintext)).expect("a dial is usable unopened");
        let token = Presented::SubjectToken {
            material: Secret::new("the-askers-token"),
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "a cell asserting WHOSE token a session gets needs its text"
        )]
        let selected = match session_for(&at, Some(&sessions), &token) {
            Ok(Session::Caller { token, .. }) => String::from(token.expose_secret()),
            other => panic!("the asker's token must select a session of its own: {other:?}"),
        };
        assert_eq!(selected, "the-askers-token");
        let name = PrincipalName::parse("analyst_a").expect("a test principal is a principal");
        for (per_caller, presented) in [(None, &token), (Some(&sessions), &Presented::SubjectPrincipal { name })] {
            let refused = session_for(&at, per_caller, presented);
            assert!(matches!(refused, Err(OracleError::Undeliverable { .. })), "{refused:?}");
        }
        let shared = sutura_conformance::corpus::presented();
        assert!(matches!(shared, Presented::SharedServiceUser { .. }), "{shared:?}");
        assert!(matches!(session_for(&at, Some(&sessions), &shared), Ok(Session::Boot)));
    }
}

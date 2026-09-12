//! A [`Warehouse`] adapter over PostgreSQL - one connection under the deployment's declared
//! identity (`SharedServiceUser`). The static half of Postgres: no OAuth, no impersonation.
//!
//! The client is async and the [`Warehouse`] port is not, so this adapter owns a `tokio` runtime
//! and `block_on`s each call. `tokio-postgres` is pure Rust and links nothing native. SQL renders
//! through `sutura-sql` (`Dialect::Postgres`); nothing here is compiled or translated.
//!
//! ## Limits
//!
//! - `NoTls`, unconditional: a `hostssl`-only server refuses this connection.
//! - A `statement_timeout` is set at connect, so a slow server statement cannot hold a
//!   blocking-pool thread past the caller's request deadline.

/// The fixture tier's credential - a value that cannot exist unconfigured.
///
/// **Behind the default-off `fixtures` feature**, because both callers are tests
/// (`crates/sutura-exec-postgres/tests/conformance.rs` and
/// `crates/sutura-app/tests/adapters/mod.rs`) and `nix/shipped.nix` builds cargo's DEFAULT set: so
/// no artefact a release publishes contains this module or the connection config over it, which
/// deletes the *reachable from a consumer* half rather than hardening it. `--all-features` compiles,
/// lints and tests it on every run.
#[cfg(feature = "fixtures")]
pub mod fixture;
mod importer;

use std::path::Path;

use bytes::Bytes;
use bytes::{BufMut as _, BytesMut};
use futures_util::SinkExt as _;
use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture};
use sutura_domain::model::TableName;
use sutura_domain::plan::Executable;
use sutura_domain::warehouse::cardinality::{CountsNotRead, DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::{AnchorRows, MalformedRowSet, ParamValue, PreFlight, Real, RowSet, Value, Warehouse};
use sutura_sql::generate::{generate, generate_key_probe};
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};
use tokio_postgres::Row;
use tokio_postgres::types::{FromSql, IsNull, ToSql, Type};

/// Why this data system could not answer.
#[derive(Debug, thiserror::Error)]
pub enum PostgresError {
    #[error("a tokio runtime could not be built")]
    Runtime {
        #[source]
        cause: std::io::Error,
    },
    #[error("could not connect to PostgreSQL")]
    Connect {
        #[source]
        cause: tokio_postgres::Error,
    },
    #[error("the statement was not accepted")]
    Prepare {
        #[source]
        cause: tokio_postgres::Error,
    },
    #[error("the statement failed while running")]
    Execute {
        #[source]
        cause: tokio_postgres::Error,
    },
    /// The server refused a statement as `division by zero` (SQLSTATE `22012`).
    #[error("the statement was refused by the server as a division by zero")]
    DivisionByZero {
        #[source]
        cause: tokio_postgres::Error,
    },
    /// A column came back as a type this adapter does not map. An error, not a stringified value.
    #[error("column {column} came back as {postgres_type}, which this adapter does not map")]
    UnsupportedType { column: String, postgres_type: &'static str },
    /// A floating-point (or `NUMERIC`) column came back as a value that is not a number.
    #[error("column {column} came back as a value that is not a finite number")]
    NotFinite {
        column: String,
        #[source]
        cause: sutura_domain::warehouse::NotFinite,
    },
    /// A day came back that is not a date this build can represent.
    #[error("column {column} came back as a day that is not a date")]
    NotADate {
        column: String,
        #[source]
        cause: sutura_domain::calendar::InvalidDate,
    },
    #[error("the result set was not rectangular")]
    Shape {
        #[source]
        cause: MalformedRowSet,
    },
    /// A key probe's result was not the pair of counts its statement projects.
    ///
    /// A defect in the rendering or in this adapter's value mapping rather than anything about the
    /// data - two aggregates over no group produce one row of two integers - and it travels as an
    /// `Err` from the port, which the boot path reads as *this declaration went unchecked*.
    #[error("the key probe did not come back as two counts")]
    KeyCounts {
        #[source]
        cause: CountsNotRead,
    },
    #[error("the plan could not be rendered for Postgres")]
    Render {
        #[source]
        cause: GenerateError,
    },
    /// A fixture import failed.
    #[error("could not load the fixture CSV {path} as table {table}")]
    Fixture {
        table: String,
        path: String,
        #[source]
        cause: tokio_postgres::Error,
    },
    #[error("could not read the fixture CSV at {path}")]
    FixtureRead {
        path: String,
        #[source]
        cause: std::io::Error,
    },
    /// A CSV header named a column that is not a valid identifier. Refused, not interpolated.
    #[error("a CSV header named a column that is not a valid identifier")]
    InvalidColumnName {
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// The shared conformance fixture schema could not be inferred.
    #[cfg(feature = "fixtures")]
    #[error("the fixture CSV schema could not be inferred")]
    FixtureSchema {
        #[source]
        cause: sutura_domain::warehouse::csv::InferenceError,
    },
    /// A schema name this adapter was asked to open that is not a word. Refused, not interpolated.
    #[error("the schema name {schema} is not a single word character")]
    InvalidSchemaName { schema: String },
    /// The dev-only `statement_timeout` tuning value is not a `u32` millisecond count.
    ///
    /// The value becomes a `SET statement_timeout = N` line verbatim, so it is parsed at the
    /// boundary and refused if it is not a number or exceeds the `u32` ceiling - a value that
    /// cannot be a timeout must not reach the statement as uninterpreted text. The cause
    /// survives so the operator sees the number did not parse, not a plain refusal.
    #[error("SUTURA_DEV_STATEMENT_TIMEOUT_MS must be a whole number of milliseconds up to {ceiling}")]
    InvalidStatementTimeout {
        ceiling: u32,
        #[source]
        cause: core::num::ParseIntError,
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
    /// One leg of a federated answer, which nothing here can assemble above.
    #[error("this adapter answers a whole plan, and the leg against {table} needs a combiner above it")]
    LegWithoutCombiner { table: String },
}

/// A `PostgreSQL` connection, behind the [`Warehouse`] port.
pub struct PostgresWarehouse {
    source: sutura_domain::model::SourceName,
    posture: sutura_domain::source::SourcePosture,
    runtime: tokio::runtime::Runtime,
    client: tokio_postgres::Client,
}

impl core::fmt::Debug for PostgresWarehouse {
    /// Hand-written because a driver's own `Debug` is the sort of thing that prints a connection
    /// handle or a host:port into a log for no benefit.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PostgresWarehouse")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl PostgresWarehouse {
    /// Opens one connection under the supplied [`tokio_postgres::Config`] and keeps it for this
    /// adapter's life.
    pub fn connect(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        config: &tokio_postgres::Config,
    ) -> Result<Self, PostgresError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|cause| PostgresError::Runtime { cause })?;
        let (client, connection) = runtime
            .block_on(config.connect(tokio_postgres::NoTls))
            .map_err(|cause| PostgresError::Connect { cause })?;
        // The connection's driver task is owned by this runtime, so it is polled exactly while this
        // adapter is inside a `block_on`. `Client` is `Send + Sync`, so the multi-thread runtime
        // serializes calls onto its workers. The driver task's ultimate error has no caller to
        // report to; the next `block_on` fails on its own.
        #[expect(
            clippy::let_underscore_must_use,
            clippy::let_underscore_untyped,
            reason = "the connection driver task's own error has no caller to route to, and the next \
                      block_on fails on the connection's state"
        )]
        runtime.spawn(async move {
            let _ = connection.await;
        });
        // The transport that calls this adapter holds a request timeout, but the server-side work a
        // `block_on` here is polling is NOT cancelled by it - a slow statement would hold this
        // blocking-pool thread past the caller's deadline. `statement_timeout` is the cheap guard:
        // the server aborts the statement itself. The value is generous (a development tier, not a
        // query budget) and overridable - a BUDGET, which is the one thing left here that a default
        // is the right answer for. The connection's credential is not: see `fixture`.
        let timeout_ms = statement_timeout_ms()?;
        runtime
            .block_on(async { client.batch_execute(&format!("SET statement_timeout = {timeout_ms}")).await })
            .map_err(|cause| PostgresError::Execute { cause })?;
        Ok(Self {
            source,
            posture,
            runtime,
            client,
        })
    }

    /// Like `connect`, but every unqualified table name resolves to a fresh,
    /// private schema - so several warehouses can share one Postgres without clobbering each other.
    /// The caller-supplied schema name is validated to a word before it reaches `CREATE SCHEMA`.
    pub fn connect_in_schema(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        config: &tokio_postgres::Config,
        schema: &str,
    ) -> Result<Self, PostgresError> {
        if !schema.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(PostgresError::InvalidSchemaName {
                schema: String::from(schema),
            });
        }
        let warehouse = Self::connect(source, posture, config)?;
        warehouse.runtime.block_on(async {
            warehouse
                .client
                .batch_execute(&format!(
                    "CREATE SCHEMA IF NOT EXISTS \"{schema}\"; SET search_path TO \"{schema}\""
                ))
                .await
                .map_err(|cause| PostgresError::Fixture {
                    table: schema.to_owned(),
                    path: String::from("<schema>"),
                    cause,
                })
        })?;
        Ok(warehouse)
    }

    /// A connection config for the fixture tier, over a credential that has already been parsed.
    ///
    /// **It TAKES the credential and reads no environment of its own**, which is the whole change:
    /// `host` and `port` are parameters, so this function cannot know it is talking to an ephemeral
    /// local server, and the shape it replaced offered `sutura`/`sutura`/`sutura` to whatever host
    /// it was handed whenever nothing was set. There is no unconfigured state to substitute for now
    /// - [`fixture::FixtureCredential`] cannot hold one - so this stays infallible.
    #[must_use]
    #[cfg(feature = "fixtures")]
    #[expect(
        clippy::disallowed_methods,
        reason = "the credential's destination is a connection handshake, which is the one place the \
                  value itself is the payload"
    )]
    pub fn local_config(host: &str, port: u16, credential: &fixture::FixtureCredential) -> tokio_postgres::Config {
        let mut config = tokio_postgres::Config::new();
        config
            .host(host)
            .port(port)
            .user(credential.user())
            .password(credential.password().expose_secret())
            .dbname(credential.database());
        config
    }

    /// Exposes a fixture CSV as a table: infers column types, recreates the table, then pushes the
    /// rows through `COPY ... FROM STDIN`. Re-inferring from the committed CSV each run cannot
    /// drift from it, and recreating makes a run idempotent.
    pub fn load_csv(&self, table: &TableName, path: &Path) -> Result<(), PostgresError> {
        let text = std::fs::read_to_string(path).map_err(|cause| PostgresError::FixtureRead {
            path: path.display().to_string(),
            cause,
        })?;
        let schema = importer::infer_schema(&text).map_err(|cause| PostgresError::InvalidColumnName { cause })?;
        self.load_schema(table, path, &schema)
    }

    /// Exposes a conformance fixture with the same exact types as the other adapter bindings.
    ///
    /// Available only with the default-off `fixtures` feature.
    #[cfg(feature = "fixtures")]
    pub fn load_fixture_csv(&self, table: &TableName, path: &Path) -> Result<(), PostgresError> {
        let text = std::fs::read_to_string(path).map_err(|cause| PostgresError::FixtureRead {
            path: path.display().to_string(),
            cause,
        })?;
        let schema = importer::infer_fixture_schema(&text).map_err(|cause| PostgresError::FixtureSchema { cause })?;
        self.load_schema(table, path, &schema)
    }

    fn load_schema(&self, table: &TableName, path: &Path, schema: &importer::Schema) -> Result<(), PostgresError> {
        let create = schema.create_statement(table);
        let copy_statement = schema.copy_statement(table);
        let body = schema.body().to_owned();
        self.runtime.block_on(async {
            self.client
                .batch_execute(&create)
                .await
                .map_err(|cause| PostgresError::Fixture {
                    table: table.to_string(),
                    path: path.display().to_string(),
                    cause,
                })?;
            let mut sink = Box::pin(
                self.client
                    .copy_in(&copy_statement)
                    .await
                    .map_err(|cause| PostgresError::Fixture {
                        table: table.to_string(),
                        path: path.display().to_string(),
                        cause,
                    })?,
            );
            sink.send(Bytes::from(body.into_bytes()))
                .await
                .map_err(|cause| PostgresError::Fixture {
                    table: table.to_string(),
                    path: path.display().to_string(),
                    cause,
                })?;
            sink.as_mut().finish().await.map_err(|cause| PostgresError::Fixture {
                table: table.to_string(),
                path: path.display().to_string(),
                cause,
            })?;
            Ok(())
        })
    }

    /// Refuses credential material this adapter has nowhere to put, then checks the presented leg
    /// against how this source was DECLARED - the two questions `docs/adr/0008` part 4 names apart.
    /// Called by both port methods that take a credential, so the pre-flight cannot disagree with
    /// the run about what this adapter accepts.
    fn deliverable(&self, presented: &Presented) -> Result<(), PostgresError> {
        match *presented {
            Presented::SharedServiceUser { .. } => {}
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => {
                return Err(PostgresError::NoPlaceForASubject {
                    at: String::from(self.source.as_str()),
                    presented: presented.as_str(),
                });
            }
        }
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| PostgresError::PresentedDisagreesWithPosture { cause })
    }

    fn render(executable: Executable<'_>) -> Result<GeneratedQuery, PostgresError> {
        match executable {
            Executable::Query(plan) => generate(plan, Dialect::Postgres).map_err(|cause| PostgresError::Render { cause }),
            Executable::Leg(leg) => Err(PostgresError::LegWithoutCombiner {
                table: leg.table().to_string(),
            }),
        }
    }

    /// The parameters, as the driver wants them.
    ///
    /// A date is bound as a `DATE`, not as ISO text (`DuckDB` casts text; Postgres does not, and
    /// `date_col >= $1` infers the placeholder's type from the column - text gives an
    /// operator-not-exist error). The conversion is the driver's day number (days since
    /// 2000-01-01) minus the domain's era (1970-01-01), a 10 957-day offset.
    fn bind(params: &[ParamValue]) -> Vec<PgParam> {
        params
            .iter()
            .map(|param| match *param {
                ParamValue::Text(ref v) => PgParam::Text(v.clone()),
                ParamValue::Date(d) => PgParam::Date(PgDate::from_domain(d.days_since_epoch())),
            })
            .collect()
    }

    /// One cell, as a domain value, dispatched on the column's declared type.
    ///
    /// The `DuckDB` half maps the same logical figure its own way; `tests/differential.rs` holds the
    /// two to an arm-for-arm agreement.
    ///
    /// `NUMERIC` is decoded exactly (never through an `f64`): a scale-zero value that fits `i64`
    /// maps to [`Value::Integer`], while a fractional or wider one maps to exact [`Value::Text`].
    fn cell(label: &str, column_type: &Type, row: &Row, index: usize) -> Result<Value, PostgresError> {
        let unsupported = |postgres_type: &'static str| PostgresError::UnsupportedType {
            column: String::from(label),
            postgres_type,
        };
        match *column_type {
            Type::BOOL => Ok(row
                .try_get::<_, Option<bool>>(index)
                .map_err(execute_err)?
                .map_or(Value::Null, |v| Value::Integer(i64::from(v)))),
            Type::INT2 => Ok(row
                .try_get::<_, Option<i16>>(index)
                .map_err(execute_err)?
                .map_or(Value::Null, |v| Value::Integer(i64::from(v)))),
            Type::INT4 => Ok(row
                .try_get::<_, Option<i32>>(index)
                .map_err(execute_err)?
                .map_or(Value::Null, |v| Value::Integer(i64::from(v)))),
            Type::INT8 => Ok(row
                .try_get::<_, Option<i64>>(index)
                .map_err(execute_err)?
                .map_or(Value::Null, Value::Integer)),
            Type::FLOAT4 => Err(unsupported("REAL; a 32-bit float has no exact 64-bit rendering")),
            Type::FLOAT8 => row.try_get::<_, Option<f64>>(index).map_err(execute_err)?.map_or_else(
                || Ok(Value::Null),
                |v| {
                    Real::parse(v).map(Value::Real).map_err(|cause| PostgresError::NotFinite {
                        column: String::from(label),
                        cause,
                    })
                },
            ),
            Type::NUMERIC => row
                .try_get::<_, Option<PgNumeric>>(index)
                .map_err(execute_err)?
                .map_or(Ok(Value::Null), |value| numeric_cell(&value, label)),
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => Ok(row
                .try_get::<_, Option<String>>(index)
                .map_err(execute_err)?
                .map_or(Value::Null, Value::Text)),
            Type::DATE => row.try_get::<_, Option<PgDate>>(index).map_err(execute_err)?.map_or_else(
                || Ok(Value::Null),
                |day| {
                    let unix = day.to_domain_days();
                    sutura_domain::calendar::Date::from_days_since_epoch(unix)
                        .map(|date| Value::Text(date.to_iso()))
                        .map_err(|cause| PostgresError::NotADate {
                            column: String::from(label),
                            cause,
                        })
                },
            ),
            _ => Err(unsupported("a type this adapter does not map")),
        }
    }

    /// Runs a statement and collects its rows.
    ///
    /// The column names and types are read from the PREPARED statement, so an answer with no rows
    /// still carries its projection - the same reason `sutura-exec-duckdb` reads labels from the
    /// executed statement rather than guessing.
    fn run(&self, query: &GeneratedQuery) -> Result<RowSet, PostgresError> {
        let bound = Self::bind(query.params());
        let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = bound.iter().map(PgParam::as_ref).collect();
        let (columns, rows) = self.runtime.block_on(async {
            let statement = self
                .client
                .prepare(query.sql())
                .await
                .map_err(|cause| PostgresError::Prepare { cause })?;
            let columns: Vec<(String, Type)> = statement
                .columns()
                .iter()
                .map(|column| (column.name().to_owned(), column.type_().clone()))
                .collect();
            let rows = self
                .client
                .query(&statement, refs.as_slice())
                .await
                .map_err(execute_err_mapped)?;
            Ok::<_, PostgresError>((columns, rows))
        })?;
        let labels: Vec<String> = columns.iter().map(|(name, _)| name.to_owned()).collect();
        let width = labels.len();
        let mut out: Vec<Vec<Value>> = Vec::with_capacity(rows.len());
        for row in rows {
            let mut cells = Vec::with_capacity(width);
            for (index, (label, column_type)) in columns.iter().enumerate() {
                cells.push(Self::cell(label, column_type, &row, index)?);
            }
            out.push(cells);
        }
        RowSet::new(labels, out).map_err(|cause| PostgresError::Shape { cause })
    }
}

/// A bound parameter, kept owned so the borrows it turns into can outlive the `block_on` that uses
/// them.
enum PgParam {
    Text(String),
    Date(PgDate),
}

impl PgParam {
    fn as_ref(&self) -> &(dyn tokio_postgres::types::ToSql + Sync) {
        match *self {
            Self::Text(ref v) => v,
            Self::Date(ref d) => d,
        }
    }
}

/// A `DATE`, carried as the driver's internal day number (days since 2000-01-01).
///
/// This is the `T` in `tokio_postgres::types::Date<T>` that the `with-chrono` / `with-time` features
/// would otherwise supply; implementing it by hand keeps a date library out of an adapter that only
/// needs to box and unbox an `i32`. `DATE` is four big-endian bytes of days-since-the-Postgres-epoch
/// on the wire, which is exactly what this type reads and writes.
#[derive(Debug, Clone, Copy)]
struct PgDate {
    days: i32,
}

impl PgDate {
    const EPOCH_OFFSET: i32 = 10_957; // days between 1970-01-01 (the domain era) and 2000-01-01 (the driver's).

    /// From a domain [`sutura_domain::calendar::Date`], via its days-since-epoch (`i32`).
    const fn from_domain(days_since_epoch: i32) -> Self {
        Self {
            days: days_since_epoch - Self::EPOCH_OFFSET,
        }
    }

    /// Back to the domain's days-since-epoch.
    const fn to_domain_days(self) -> i32 {
        self.days + Self::EPOCH_OFFSET
    }
}

impl<'a> FromSql<'a> for PgDate {
    #[expect(
        clippy::big_endian_bytes,
        reason = "the Postgres DATE wire format is documented as a big-endian i32 of days-since-epoch"
    )]
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        let bytes: [u8; 4] = raw
            .try_into()
            .map_err(|_err| "a DATE came back with a width other than four bytes")?;
        Ok(Self {
            days: i32::from_be_bytes(bytes),
        })
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::DATE
    }
}

impl ToSql for PgDate {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        out.put_i32(self.days);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::DATE
    }

    #[expect(
        clippy::use_self,
        reason = "Self::accepts would be ambiguous between the FromSql and ToSql impls, so the \
                  fully-qualified path is the one that compiles"
    )]
    fn to_sql_checked(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        if !<PgDate as ToSql>::accepts(ty) {
            return Err(format!("cannot convert a date to {ty}").into());
        }
        self.to_sql(ty, out)
    }
}

const fn execute_err(cause: tokio_postgres::Error) -> PostgresError {
    PostgresError::Execute { cause }
}

/// The error from the RUN of a statement, with the one server refusal this adapter refuses to
/// re-render as a generic `Execute`: `division by zero` (SQLSTATE `22012`) is how Postgres honors
/// `zero_denominator: fails`, and the conformance cells match on the TYPED variant rather than on a
/// message substring.
fn execute_err_mapped(cause: tokio_postgres::Error) -> PostgresError {
    if cause.code() == Some(&tokio_postgres::error::SqlState::DIVISION_BY_ZERO) {
        PostgresError::DivisionByZero { cause }
    } else {
        PostgresError::Execute { cause }
    }
}

/// A `NUMERIC`, decoded from the wire as its exact components.
///
/// `tokio-postgres` 0.7 ships NO `FromSql` for `NUMERIC` (the type OID exists, a Rust type does
/// not), and `sum(int8)` / `AVG` over an integer column return exactly `NUMERIC`. So this is a
/// hand-rolled decoder of the documented binary format - the same decision as [`PgDate`]: the raw
/// bytes are all the driver gives. It is kept EXACT (there is no `f64` on the value), then mapped
/// through the same shared decimal boundary as the other adapters.
///
/// A non-finite value (`NaN`, `±Infinity`) is carried by its sign word alone
/// ([`PgNumeric::is_not_finite`]) so the caller refuses it at the same place every other non-finite
/// cell is refused, rather than as a driver error.
#[derive(Debug, Clone)]
struct PgNumeric {
    /// The base-10000 digits, most significant first.
    digits: Vec<u16>,
    /// The exponent of `10000` for the first digit.
    weight: i16,
    /// The sign word.
    sign: u16,
    /// The display scale: how many decimal digits the server declares to the right of the point.
    dscale: u16,
}

impl PgNumeric {
    const NEGATIVE: u16 = 0x4000;
    const NAN: u16 = 0xC000;
    const POSITIVE_INFINITY: u16 = 0xD000;
    const NEGATIVE_INFINITY: u16 = 0xF000;

    /// A `NaN` or `±Infinity` numeric, which has no finite value to carry.
    const fn is_not_finite(&self) -> bool {
        matches!(self.sign, Self::NAN | Self::POSITIVE_INFINITY | Self::NEGATIVE_INFINITY)
    }
}

impl<'a> FromSql<'a> for PgNumeric {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        decode_numeric(raw)
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::NUMERIC
    }
}

/// Maps a decoded `NUMERIC` exactly: a fitting integer is numeric; every other finite value is text.
fn numeric_cell(value: &PgNumeric, label: &str) -> Result<Value, PostgresError> {
    if value.is_not_finite() {
        return Err(PostgresError::NotFinite {
            column: String::from(label),
            cause: sutura_domain::warehouse::NotFinite::NotANumber,
        });
    }
    let text = render_numeric(value);
    if value.dscale == 0 {
        Ok(text.parse::<i64>().map_or_else(|_| Value::Text(text), Value::Integer))
    } else {
        Ok(Value::Text(text))
    }
}

/// The exact decimal text of a finite `NUMERIC`.
///
/// Rendered straight from the base-10000 digits and `weight`, so it introduces no `f64` and no
/// rounding anywhere: a `Numeric` is `Σ digit[i] · 10000^(weight−i)`, and expanding each group to
/// its four decimal places with the point after `weight+1` groups of the integer part is exact. The
/// declared `dscale` decides how many digits sit after the point.
fn render_numeric(value: &PgNumeric) -> String {
    let mut text = String::new();
    if value.sign == PgNumeric::NEGATIVE {
        text.push('-');
    }
    text.push_str(&integer_part(&value.digits, value.weight));
    if value.dscale > 0 {
        text.push('.');
        text.push_str(&fraction_part(&value.digits, value.weight, value.dscale));
    }
    text
}

/// The integer part of a `NUMERIC`: `weight+1` base-10000 groups, the leading one un-padded and the
/// rest to four digits, with missing trailing groups read as zero.
fn integer_part(digits: &[u16], weight: i16) -> String {
    if weight < 0 {
        return String::from("0");
    }
    let mut out = String::new();
    match digits.first() {
        Some(leading) => out.push_str(&leading.to_string()),
        None => out.push('0'),
    }
    let groups = usize::from(u16::try_from(weight).unwrap_or(0));
    for index in 1..=groups {
        match digits.get(index) {
            Some(digit) => {
                for c in format!("{digit:04}").chars() {
                    out.push(c);
                }
            }
            None => out.push_str("0000"),
        }
    }
    out
}

/// The fractional part of a `NUMERIC`, to exactly `dscale` decimal digits.
fn fraction_part(digits: &[u16], weight: i16, dscale: u16) -> String {
    // Fractional group zero is 10^-4. Its source digit is `weight+1`; a negative index is an
    // omitted zero group before the first stored digit, not permission to start at digit zero.
    let base = i32::from(weight) + 1;
    let mut out = String::new();
    let mut gathered: u16 = 0;
    let mut group_index: i32 = 0;
    while gathered < dscale {
        let source_index = usize::try_from(base + group_index).ok();
        let group: Vec<char> = source_index
            .and_then(|index| digits.get(index))
            .map_or_else(|| vec!['0', '0', '0', '0'], |digit| format!("{digit:04}").chars().collect());
        let need = usize::from(dscale - gathered);
        for c in group.iter().take(need) {
            out.push(*c);
        }
        gathered = gathered.saturating_add(u16::try_from(need.min(4)).unwrap_or(0));
        group_index += 1;
    }
    out
}

/// One big-endian `u16` at `offset`, if the bytes are there.
#[expect(
    clippy::big_endian_bytes,
    reason = "the Postgres NUMERIC wire format is documented big-endian, so reading a u16 is a \
              direct big-endian decode rather than an accident"
)]
fn u16_at(raw: &[u8], offset: usize) -> Option<u16> {
    let slice: [u8; 2] = raw.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(slice))
}

/// The error a wire decoder returns: a message-only error, because the driver's raw bytes carry no
/// typed context to preserve.
type WireError = Box<dyn std::error::Error + Sync + Send>;

/// The Postgres `NUMERIC` binary format: two bytes of digit count, two of weight, two of sign, two
/// of display scale, then `ndigits` base-10000 digits. Values combine as
/// `Σ digit[i] · 10000^(weight − i)`.
#[expect(
    clippy::cast_possible_wrap,
    reason = "the wire weight is a signed i16 carried as two bytes, so reinterpreting the unsigned \
              read as i16 is the documented decode, not an arithmetic wrap"
)]
fn decode_numeric(raw: &[u8]) -> Result<PgNumeric, WireError> {
    if raw.len() < 8 {
        return Err("a NUMERIC came back shorter than its header".into());
    }
    let ndigits = u16_at(raw, 0).ok_or("a NUMERIC header was truncated")?;
    let weight_bits = u16_at(raw, 2).ok_or("a NUMERIC header was truncated")?;
    let sign = u16_at(raw, 4).ok_or("a NUMERIC header was truncated")?;
    let dscale = u16_at(raw, 6).ok_or("a NUMERIC header was truncated")?;
    let mut digits = Vec::with_capacity(usize::from(ndigits));
    for index in 0..ndigits {
        digits.push(u16_at(raw, 8 + 2 * usize::from(index)).ok_or("a NUMERIC value was truncated")?);
    }
    Ok(PgNumeric {
        digits,
        weight: weight_bits as i16,
        sign,
        dscale,
    })
}

/// A tuning value, or this build's own. **Not for a credential** - it was, and the credential half
/// is `fixture::FixtureCredential` now: a fallback is the right shape for a timeout a host may want
/// to widen and the wrong shape for a secret nobody chose.
fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| String::from(fallback))
}

/// The dev-only statement timeout, parsed to a `u32`.
///
/// `env_or` hands back text and this line becomes `SET statement_timeout = N`, so the value is a
/// typed ceiling at the boundary: something that is not a number, or is larger than `u32`, cannot
/// reach the statement as raw text. Parsing is `u32` (not `u64` rounded down), so an oversized
/// value is refused rather than becoming a different number.
fn statement_timeout_ms() -> Result<u32, PostgresError> {
    parse_statement_timeout(&env_or("SUTURA_DEV_STATEMENT_TIMEOUT_MS", "15000"))
}

/// Parses a `statement_timeout` tuning value as a `u32` millisecond count.
fn parse_statement_timeout(raw: &str) -> Result<u32, PostgresError> {
    raw.parse::<u32>().map_err(|cause| PostgresError::InvalidStatementTimeout {
        ceiling: u32::MAX,
        cause,
    })
}

impl Warehouse for PostgresWarehouse {
    type Error = PostgresError;

    /// **One connection under the deployment's declared identity**, so there is nowhere for a
    /// subject's own credential to arrive - the static-credential half. A source configured
    /// `impersonation-at-source` on this adapter does not start.
    const IMPERSONATION: sutura_domain::source::ImpersonationCapability =
        sutura_domain::source::ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &sutura_domain::model::SourceName {
        &self.source
    }

    fn posture(&self) -> &sutura_domain::source::SourcePosture {
        &self.posture
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented) -> Result<PreFlight, Self::Error> {
        self.deliverable(presented)?;
        let query = Self::render(executable)?;
        drop(
            self.runtime
                .block_on(self.client.prepare(query.sql()))
                .map_err(|cause| PostgresError::Prepare { cause })?,
        );
        Ok(PreFlight::Accepted)
    }

    fn execute(&self, executable: Executable<'_>, presented: &Presented) -> Result<RowSet, Self::Error> {
        self.deliverable(presented)?;
        let query = Self::render(executable)?;
        self.run(&query)
    }

    fn verify_anchor(&self, plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        let query = generate(plan.plan(), Dialect::Postgres).map_err(|cause| PostgresError::Render { cause })?;
        self.run(&query).map(AnchorRows::of)
    }

    /// Counts a declared join key's values and its distinct values, in one statement.
    ///
    /// Overridden rather than defaulted because this adapter can ask: one aggregate scan over the
    /// dimension table, no group, no parameter. It takes no credential, for
    /// [`Warehouse::verify_anchor`]'s reason - there is no caller at boot - so what it establishes is
    /// what the identity this connection was opened with can see.
    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        let query = generate_key_probe(&key, Dialect::Postgres).map_err(|cause| PostgresError::Render { cause })?;
        let rows = self.run(&query)?;
        KeyUniqueness::read(&rows).map_err(|cause| PostgresError::KeyCounts { cause })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sutura_domain::warehouse::Value;

    /// The wire bytes for a `NUMERIC`: two bytes each of digit count, weight, sign and display
    /// scale, then the base-10000 digits. Built big-endian exactly as the documented format.
    #[expect(
        clippy::big_endian_bytes,
        reason = "the NUMERIC wire format is documented big-endian, which is exactly what the test \
                  helper writes"
    )]
    fn numeric_bytes(digits: &[u16], weight: i16, sign: u16, dscale: u16) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + 2 * digits.len());
        out.extend_from_slice(&u16::try_from(digits.len()).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(&weight.to_be_bytes());
        out.extend_from_slice(&sign.to_be_bytes());
        out.extend_from_slice(&dscale.to_be_bytes());
        for &digit in digits {
            out.extend_from_slice(&digit.to_be_bytes());
        }
        out
    }

    fn decode(digits: &[u16], weight: i16, sign: u16, dscale: u16) -> PgNumeric {
        decode_numeric(&numeric_bytes(digits, weight, sign, dscale)).expect("a valid NUMERIC decodes")
    }

    #[test]
    fn an_integral_numeric_that_fits_is_an_integer_cell() {
        // 300 as NUMERIC(,0): one base-10000 digit, weight 0.
        assert_eq!(
            numeric_cell(&decode(&[300], 0, 0x0000, 0), "total").unwrap(),
            Value::Integer(300)
        );
        // 10000 = 1·10000^1, weight 1.
        assert_eq!(
            numeric_cell(&decode(&[1], 1, 0x0000, 0), "total").unwrap(),
            Value::Integer(10_000)
        );
    }

    #[test]
    fn a_fractional_numeric_is_exact_text() {
        // 100.5 = 100·10000^0 + 5000·10000^-1, declared scale 1.
        assert_eq!(
            numeric_cell(&decode(&[100, 5000], 0, 0x0000, 1), "mean").unwrap(),
            Value::Text(String::from("100.5"))
        );
        // The declared scale renders 5000·10000^-1 as 0.5, not 0.5000.
        assert_eq!(
            numeric_cell(&decode(&[5000], -1, 0x0000, 1), "mean").unwrap(),
            Value::Text(String::from("0.5"))
        );
        // The wire's declared scale is preserved exactly.
        assert_eq!(
            numeric_cell(&decode(&[100], 0, 0x0000, 2), "mean").unwrap(),
            Value::Text(String::from("100.00"))
        );
        // The absent 10^-4 group implied by weight -2 is still part of the value.
        assert_eq!(
            numeric_cell(&decode(&[1000], -2, 0x0000, 5), "mean").unwrap(),
            Value::Text(String::from("0.00001"))
        );
    }

    #[test]
    fn a_negative_numeric_keeps_its_sign_exactly() {
        assert_eq!(
            numeric_cell(&decode(&[300], 0, 0x4000, 0), "total").unwrap(),
            Value::Integer(-300)
        );
        assert_eq!(
            numeric_cell(&decode(&[100, 5000], 0, 0x4000, 1), "mean").unwrap(),
            Value::Text(String::from("-100.5"))
        );
    }

    #[test]
    fn a_non_finite_numeric_is_refused_as_a_non_finite_cell() {
        assert!(matches!(
            numeric_cell(&decode(&[0], 0, 0xC000, 0), "mean"),
            Err(PostgresError::NotFinite { .. })
        ));
        assert!(matches!(
            numeric_cell(&decode(&[0], 0, 0xD000, 2), "mean"),
            Err(PostgresError::NotFinite { .. })
        ));
    }

    #[test]
    fn a_numeric_wider_than_i64_stays_exact_text() {
        // 10^20 is beyond i64 and must not be rounded or refused.
        assert_eq!(
            numeric_cell(&decode(&[1], 5, 0x0000, 0), "total").unwrap(),
            Value::Text(String::from("100000000000000000000"))
        );
    }

    #[test]
    fn a_truncated_numeric_header_is_a_decoder_error() {
        let short = decode_numeric(&[0, 1, 0]).expect_err("fewer than the eight header bytes");
        assert!(short.to_string().contains("shorter"), "{short}");
        // Eight header bytes but claims a digit it does not carry.
        let missing_digit = decode_numeric(&[0, 1, 0, 0, 0, 0, 0, 0]).expect_err("claims a digit that is not there");
        assert!(missing_digit.to_string().contains("value was truncated"), "{missing_digit}");
    }

    #[test]
    fn pg_date_round_trips_through_the_epoch_offset() {
        // The driver's epoch (2000-01-01) is day 0 in its own numbering.
        assert_eq!(PgDate { days: 0 }.to_domain_days(), 10_957);
        // The domain epoch (1970-01-01) is the driver's -10957.
        assert_eq!(PgDate::from_domain(0).days, -10_957);
        assert_eq!(PgDate::from_domain(0).to_domain_days(), 0);
    }

    #[test]
    fn a_statement_timeout_is_a_u32_ceiling_or_it_is_refused() {
        // The tuning value becomes a `SET statement_timeout = N` line verbatim, so it is a typed
        // ceiling at the boundary: a number that fits parses...
        assert_eq!(parse_statement_timeout("15000").expect("a number parses"), 15_000);
        assert_eq!(parse_statement_timeout("0").expect("zero is a valid timeout"), 0);
        assert_eq!(
            parse_statement_timeout(&u32::MAX.to_string()).expect("the ceiling parses"),
            u32::MAX
        );
        // ...and anything that cannot be a `u32` is refused rather than reaching the statement.
        // `u32::MAX + 1` is the ceiling's far side, and decimals are refused rather than truncated.
        assert!(matches!(
            parse_statement_timeout("not-a-number"),
            Err(PostgresError::InvalidStatementTimeout { .. })
        ));
        assert!(matches!(
            parse_statement_timeout("4294967296"),
            Err(PostgresError::InvalidStatementTimeout { .. })
        ));
        assert!(matches!(
            parse_statement_timeout("15000.5"),
            Err(PostgresError::InvalidStatementTimeout { .. })
        ));
    }
}

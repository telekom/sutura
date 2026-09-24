#![forbid(unsafe_code)]
//! A [`Warehouse`] adapter over `ClickHouse`, over its HTTP interface.
//!
//! One connection under the deployment's declared identity (`SharedServiceUser`) - the static
//! half, like `sutura_exec_postgres`: no OAuth, no impersonation.
//!
//! # Why `ureq` and not an async driver
//!
//! **The port carries no `async fn`, and `ureq` is natively blocking - so this adapter needs no
//! `tokio` runtime and no `block_on` at all.** That is not a simplification made for this crate
//! alone: `sutura-exec-bigquery`'s own `Cargo.toml` states the reasoning for its ADBC driver
//! at length, and it transfers unchanged - `Warehouse::execute` reaches this adapter through
//! `sutura_runtime::spawn_carrying_span`, on a blocking-pool thread, and starting a runtime
//! *there* (which an async driver wrapped the way `sutura_exec_postgres` wraps `tokio-postgres`
//! would need) is exactly the `reqwest::blocking` shape that entry warns against. `ClickHouse`'s
//! own HTTP interface asks for nothing more than one request/response per statement, which is
//! `ureq`'s whole job. **A pure-Rust driver either way**: `ureq` with the `rustls` feature links
//! no C TLS library, and this crate's own `tls` module (over `sutura-tls`) is what most of
//! `sutura_exec_postgres::tls`'s reasoning transfers to.
//!
//! # Why a driver-shaped seam, and not just a hand-rolled client
//!
//! [`transport::ClickHouseTransport`] is the port this adapter's own port methods call through -
//! [`transport::Http`] for a real connection, and a canned implementor for the
//! crate's own unit tests. The split exists for the reason `sutura-exec-bigquery`'s `JobTransport` does:
//! the port's decisions - rendering, the refusals, the decode - are held by cells that need no
//! server, so they run on every build. What a SERVER answers is held elsewhere: the golden matrix in
//! `sutura-app`'s tests loads the example corpus into the server `nix/clickhouse-tier.nix` starts
//! (through `fixture`, behind the default-off `fixtures` feature) and pins the rows, refusals,
//! error and anchor report it answers - in `checks.nextest` and under `just test`.
//!
//! `Warehouse::Error` for this adapter is [`ClickHouseError`], generic over `T::Error` - the same
//! shape `sutura_exec_bigquery::BigQueryError<E>`
//! takes, for the same reason: [`transport::ClickHouseTransport::source_refused`]/
//! [`transport::ClickHouseTransport::deadline_exceeded`] let the TRANSPORT answer the port's own
//! two predicates about a failure only it can read the wire-level shape of.
//!
//! # Parameters travel as `ClickHouse`'s own named parameters, never as text
//!
//! [`transport::Http`] rewrites each rendered `?` into `{pN:Type}` and sends the value as a
//! separate `param_pN` field, which the SERVER binds by the declared type - see
//! `transport::rewrite_placeholders`'s own doc for why counting `?` occurrences against
//! `params.len()` is safe rather than a guess.
//!
//! # `Deadline`, and what was checked about it - see [`deadline`]'s own header
//!
//! Every `execute`/`declared_key`/`verify_anchor` round trip sends `max_execution_time`, computed
//! from what the port's [`sutura_domain::warehouse::deadline::Deadline`] has left. `deadline`'s
//! module header states what was measured about that setting's semantics and its limit, mirroring
//! `sutura_exec_postgres::deadline`'s own record for `SET LOCAL statement_timeout`.
//!
//! # Every request pins the server settings that decide what the rows say
//!
//! `join_use_nulls=1`: under `ClickHouse`'s default `0`, the unmatched side of an outer join
//! answers the column type's default - `''` for a `String` - where the SQL standard answers `NULL`.
//! Measured by hand against the pinned compose server over the example corpus: 6 of the 23
//! questions with a committed `@duckdb` row golden disagreed without it, 0 with it, under a
//! non-`Nullable` schema; under `Nullable` columns the setting changes nothing, so a fixture
//! importer's type choice decides whether the defect shows - `fixture` declares no column
//! `Nullable` for that reason. `output_format_json_quote_denormals=1`: under the default `0` an
//! infinite float answers the JSON `null`. `timeout_overflow_mode=throw`: `break` answers a spent
//! `max_execution_time` with HTTP 200 and the rows read so far. A unit cell holds what is sent; the
//! executed goldens hold what the first two settings make the server answer, and nothing executed
//! holds the third.
//!
//! # What is NOT here
//!
//! **No release links this crate.** `sutura-cli` opens a `kind: clickhouse` source behind its
//! default-off `clickhouse` feature, which `nix/shipped.nix` does not enable - see that feature's
//! own manifest entry for why.
//!
//! **No raw-SQL tool support** (`Warehouse::ACCEPTS_RAW_STATEMENTS` stays at its `false` default)
//! and **no leg execution** (`Warehouse::EXECUTES_LEGS` stays at its `false` default, so
//! [`Executable::Leg`] answers [`ClickHouseError::LegWithoutCombiner`] exactly as
//! `sutura_exec_bigquery` does).
//!
//! **No `dry_run` override.** `ClickHouse`'s HTTP interface has no cheap "prepare, do not run"
//! step this adapter could ask for without paying most of the cost of running the statement, so
//! this stays at the port's own default (`Ok(PreFlight::NotAsked)`) - the same honest absence
//! `Warehouse::dry_run`'s own doc names for an adapter where checking is not cheaper than running.

mod deadline;
#[cfg(feature = "fixtures")]
pub mod fixture;
pub mod tls;
pub mod transport;

use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture};
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::arrow::of_row_set;
use sutura_domain::warehouse::cardinality::{CountsNotRead, DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::{AnchorRows, MalformedRowSet, NotFinite, Real, ResultBatches, RowSet, Value, Warehouse};
use sutura_sql::generate::{generate, generate_key_probe};
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};
use transport::ClickHouseTransport;

/// Why this data system could not answer.
///
/// Generic in the transport's own error type - the shape `sutura_exec_bigquery::BigQueryError<E>`
/// takes, for the same reason: `E` is `T::Error`, the one place a failure's wire-level shape
/// lives, and this adapter's own `source_refused`/`deadline_exceeded` delegate to it through
/// [`ClickHouseError::Endpoint`].
#[derive(Debug, thiserror::Error)]
pub enum ClickHouseError<E>
where
    E: core::error::Error + 'static,
{
    /// The endpoint (or the canned pack) did not answer.
    #[error("the data system did not answer")]
    Endpoint {
        #[source]
        cause: E,
    },
    #[error("the plan could not be rendered for ClickHouse")]
    Render {
        #[source]
        cause: GenerateError,
    },
    /// A leg without a combiner - `Warehouse::EXECUTES_LEGS` stays at its default; see the crate
    /// header.
    #[error("this adapter answers a whole plan, and the leg against {table} needs a combiner above it")]
    LegWithoutCombiner { table: String },
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
    /// A column came back as a `ClickHouse` type this adapter does not map.
    #[error("column {column} came back as {clickhouse_type}, which this adapter does not map")]
    UnsupportedType { column: String, clickhouse_type: String },
    /// A floating-point column came back as a value that is not a number.
    #[error("column {column} came back as a value that is not a finite number")]
    NotFinite {
        column: String,
        #[source]
        cause: NotFinite,
    },
    /// The response body was not the JSON shape this adapter reads.
    #[error("the response body was not the {expected} this adapter reads")]
    MalformedResponse { expected: &'static str },
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
}

/// [`ClickHouseError`] over one transport's own error type - named so a signature reads as one
/// type rather than as the two-level generic clippy's `type_complexity` lint asks not to repeat.
type ChResult<T, E> = core::result::Result<T, ClickHouseError<E>>;

/// A `ClickHouse` connection, behind the [`Warehouse`] port.
///
/// Generic in [`ClickHouseTransport`] so this crate's own conformance pack can bind the port to a
/// canned implementor with no live endpoint; see this crate's own header.
pub struct ClickHouseWarehouse<T> {
    source: SourceName,
    posture: SourcePosture,
    transport: T,
}

impl<T> core::fmt::Debug for ClickHouseWarehouse<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ClickHouseWarehouse")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl<T> ClickHouseWarehouse<T>
where
    T: ClickHouseTransport,
{
    /// Opens an adapter over an already-constructed transport - `T = transport::Http` for a real
    /// connection, and a fake for the conformance pack.
    #[must_use]
    pub const fn of(source: SourceName, posture: SourcePosture, transport: T) -> Self {
        Self {
            source,
            posture,
            transport,
        }
    }

    /// Refuses credential material this adapter has nowhere to put, then checks the presented leg
    /// against how this source was DECLARED - `sutura_exec_postgres::PostgresWarehouse::
    /// deliverable`'s exact shape, for the exact reason (`docs/adr/0008` part 4).
    fn deliverable(&self, presented: &Presented) -> ChResult<(), T::Error> {
        match *presented {
            Presented::SharedServiceUser { .. } => {}
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => {
                return Err(ClickHouseError::NoPlaceForASubject {
                    at: String::from(self.source.as_str()),
                    presented: presented.as_str(),
                });
            }
        }
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| ClickHouseError::PresentedDisagreesWithPosture { cause })
    }

    fn render(executable: Executable<'_>) -> ChResult<GeneratedQuery, T::Error> {
        match executable {
            Executable::Query(plan) => generate(plan, Dialect::ClickHouse).map_err(|cause| ClickHouseError::Render { cause }),
            Executable::Leg(leg) => Err(ClickHouseError::LegWithoutCombiner {
                table: leg.table().to_string(),
            }),
        }
    }

    fn run(&self, query: &GeneratedQuery, deadline: Deadline) -> ChResult<RowSet, T::Error> {
        let body = self
            .transport
            .run(query.sql(), query.params(), deadline)
            .map_err(|cause| ClickHouseError::Endpoint { cause })?;
        rows_from_json(&body)
    }
}

impl<T> Warehouse for ClickHouseWarehouse<T>
where
    T: ClickHouseTransport,
{
    type Error = ClickHouseError<T::Error>;

    /// **One connection under the deployment's declared identity**, so there is nowhere for a
    /// subject's own credential to arrive - the static-credential half, exactly
    /// `sutura_exec_postgres`'s own declaration and for the same reason.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.deliverable(presented)?;
        let query = Self::render(executable)?;
        let rows = self.run(&query, deadline)?;
        // The Arrow port's conversion, in the adapter that owns the row-speaking driver - see
        // `sutura_domain::warehouse::arrow`.
        of_row_set(&rows).map_err(|cause| ClickHouseError::Shape { cause })
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        let query = generate(plan.plan(), Dialect::ClickHouse).map_err(|cause| ClickHouseError::Render { cause })?;
        self.run(&query, boot_deadline()).map(AnchorRows::of)
    }

    /// Overridden for the reason `sutura_exec_postgres::PostgresWarehouse::declared_key` gives: one
    /// aggregate scan, no group, no parameter, so this adapter can ask cheaply. Takes no
    /// credential, for [`Warehouse::verify_anchor`]'s reason - there is no caller at boot.
    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        let query = generate_key_probe(&key, Dialect::ClickHouse).map_err(|cause| ClickHouseError::Render { cause })?;
        let rows = self.run(&query, boot_deadline())?;
        KeyUniqueness::read(&rows).map_err(|cause| ClickHouseError::KeyCounts { cause })
    }

    /// Delegates to the TRANSPORT, for the reason `sutura_exec_bigquery::BigQueryWarehouse::
    /// source_refused` gives: `Self::Error` is `ClickHouseError::Endpoint` wrapping the
    /// transport's own type, and only the transport can read the wire-level shape.
    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(*error, ClickHouseError::Endpoint { ref cause } if self.transport.source_refused(cause))
    }

    /// Delegates to the TRANSPORT, for [`Self::source_refused`]'s reason.
    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(*error, ClickHouseError::Endpoint { ref cause } if self.transport.deadline_exceeded(cause))
    }
}

/// A wide placeholder budget for the two boot-only port methods, which have no caller-facing
/// deadline to spend against - `verify_anchor` and `declared_key` run before a listener is bound,
/// with nobody asking, so there is nothing here for a one-hour ceiling to compete against; it
/// exists only so `max_execution_time` is sent as *generous*, never *unbounded*, per this
/// crate's own "zero reads as unbounded" rule (`deadline`'s module header).
#[expect(
    clippy::expect_used,
    reason = "3600 seconds is a literal, non-zero constant - Budget::parse can only fail on zero, so this cannot \
              actually panic; a Result-returning fallback would invent a second, never-taken code path"
)]
fn boot_deadline() -> Deadline {
    let budget = Budget::parse(core::time::Duration::from_secs(3600)).expect("3600 seconds is a non-zero duration");
    Deadline::opened_at(std::time::Instant::now(), budget)
}

/// Decodes a `JSONCompactEachRowWithNamesAndTypes` response body into a [`RowSet`]: a names row, a
/// types row, then one row per result row - each a JSON array on its own line.
fn rows_from_json<E>(body: &[u8]) -> ChResult<RowSet, E>
where
    E: core::error::Error + 'static,
{
    let mut values = serde_json::Deserializer::from_slice(body).into_iter::<serde_json::Value>();
    let names = read_string_array(values.next())?;
    let types = read_string_array(values.next())?;
    if names.len() != types.len() {
        return Err(ClickHouseError::MalformedResponse {
            expected: "a names row and a types row of equal width",
        });
    }
    let mut out: Vec<Vec<Value>> = Vec::new();
    for row in values {
        let row = row.map_err(|_cause| ClickHouseError::MalformedResponse {
            expected: "one JSON array per row",
        })?;
        let serde_json::Value::Array(cells) = row else {
            return Err(ClickHouseError::MalformedResponse {
                expected: "one JSON array per row",
            });
        };
        if cells.len() != types.len() {
            return Err(ClickHouseError::MalformedResponse {
                expected: "a row as wide as the names and types rows",
            });
        }
        let mut mapped = Vec::with_capacity(cells.len());
        for (label, (kind, cell)) in names.iter().zip(types.iter().zip(cells)) {
            mapped.push(cell_of(label, kind, cell)?);
        }
        out.push(mapped);
    }
    RowSet::new(names, out).map_err(|cause| ClickHouseError::Shape { cause })
}

fn read_string_array<E>(item: Option<serde_json::Result<serde_json::Value>>) -> ChResult<Vec<String>, E>
where
    E: core::error::Error + 'static,
{
    let missing = || ClickHouseError::MalformedResponse {
        expected: "a names row and a types row before any data row",
    };
    let value = item.ok_or_else(missing)?.map_err(|_cause| missing())?;
    let serde_json::Value::Array(items) = value else {
        return Err(missing());
    };
    items
        .into_iter()
        .map(|item| match item {
            serde_json::Value::String(text) => Ok(text),
            _ => Err(missing()),
        })
        .collect()
}

/// One cell, as a domain [`Value`], dispatched on the column's declared `ClickHouse` type.
///
/// **A small, closed set of column types - the ones this crate's own fixture importer ever
/// declares - for `sutura_domain::warehouse::ParamValue`'s own reason: an unreachable arm holding a
/// semantic choice nobody reviewed is worse than not having the arm.** `Nullable(T)` is unwrapped
/// first, so every arm below sees the same closed set whether the column is nullable or not.
fn cell_of<E>(label: &str, kind: &str, value: serde_json::Value) -> ChResult<Value, E>
where
    E: core::error::Error + 'static,
{
    let (kind, value) = match kind.strip_prefix("Nullable(").and_then(|rest| rest.strip_suffix(')')) {
        Some(inner) => match value {
            serde_json::Value::Null => return Ok(Value::Null),
            other => (inner, other),
        },
        None => (kind, value),
    };
    let unsupported = || ClickHouseError::UnsupportedType {
        column: String::from(label),
        clickhouse_type: String::from(kind),
    };
    match kind {
        "String" | "Date" | "Date32" => match value {
            serde_json::Value::String(text) => Ok(Value::Text(text)),
            serde_json::Value::Null => Ok(Value::Null),
            _ => Err(unsupported()),
        },
        "UInt8" | "UInt16" | "UInt32" | "UInt64" | "Int8" | "Int16" | "Int32" | "Int64" => {
            integer_value(&value).ok_or_else(unsupported)
        }
        "Float32" | "Float64" => {
            let raw = float_text(&value).ok_or_else(unsupported)?;
            Real::parse(raw).map(Value::Real).map_err(|cause| ClickHouseError::NotFinite {
                column: String::from(label),
                cause,
            })
        }
        _ if kind.starts_with("Decimal") => decimal_value(&value).ok_or_else(unsupported),
        _ => Err(unsupported()),
    }
}

/// An integer cell: [`Value::Integer`] where it fits an `i64`, and its exact digits as
/// [`Value::Text`] where it is a `UInt64` past `i64::MAX` - the split `sutura_exec_duckdb` makes for
/// `UBIGINT`, so the same wide sum reads the same on both. Measured by running
/// `sutura-conformance`'s `wide-total-by-day` case against the tier, which this adapter refused as
/// an unmapped `UInt64` before; `xtask/src/conformance/reconcile.rs`'s `clickhouse` entry says why
/// that binding is not committed.
///
/// A narrower integer arrives as a JSON number; a 64-bit one as a string, because every request
/// pins `output_format_json_quote_64bit_integers` (`transport`'s `JSON_QUOTE_64BIT_INTEGERS`) - so
/// a value past `i64::MAX` has exactly one form here, and a bare number past it is refused as
/// unmapped rather than read by a branch no venue produces.
fn integer_value(value: &serde_json::Value) -> Option<Value> {
    let wide = |unsigned: u64| Value::Text(unsigned.to_string());
    match *value {
        serde_json::Value::Number(ref number) => number.as_i64().map(Value::Integer),
        serde_json::Value::String(ref text) => text.parse().map(Value::Integer).ok().or_else(|| text.parse().ok().map(wide)),
        _ => None,
    }
}

fn float_text(value: &serde_json::Value) -> Option<f64> {
    match *value {
        serde_json::Value::Number(ref number) => number.as_f64(),
        serde_json::Value::String(ref text) => Some(match text.as_str() {
            "nan" => f64::NAN,
            "inf" => f64::INFINITY,
            "-inf" => f64::NEG_INFINITY,
            other => other.parse().ok()?,
        }),
        _ => None,
    }
}

/// A `Decimal` cell, mapped exactly.
///
/// `ClickHouse`'s JSON formats already render it as human-readable decimal text (a JSON number or
/// a quoted string, either way carrying its own digits rather than this adapter's own base-10000
/// decode the way `sutura_exec_postgres::PgNumeric` needs for Postgres's binary wire) - an
/// integral value with no fractional digits maps to [`Value::Integer`], and every other finite
/// value keeps its exact text as [`Value::Text`], the same split `sutura_exec_postgres::
/// numeric_cell` makes.
fn decimal_value(value: &serde_json::Value) -> Option<Value> {
    let text = match *value {
        serde_json::Value::Number(ref number) => number.to_string(),
        serde_json::Value::String(ref text) => text.clone(),
        _ => return None,
    };
    if text.contains('.') {
        Some(Value::Text(text))
    } else {
        text.parse::<i64>().map(Value::Integer).ok().or(Some(Value::Text(text)))
    }
}

#[cfg(test)]
mod tests;

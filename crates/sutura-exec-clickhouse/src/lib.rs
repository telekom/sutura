#![forbid(unsafe_code)]
//! A [`Warehouse`](sutura_domain::warehouse::Warehouse) adapter over `ClickHouse`, through its
//! vendored ADBC driver: [`adbc::AdbcClickHouseWarehouse`].
//!
//! A `shared-service-user` source runs every statement as the deployment's declared user. An
//! `impersonation-at-source` source runs each one as the `ClickHouse` user named by the asking
//! caller's verified subject: through `EXECUTE AS` on the deployment's sign-in (see
//! [`execute_as`]), or - when the source declares a `delegation` - signed in as that user with the
//! token the caller's verified assertion was exchanged for. `ClickHouse` hands that token to the
//! operator's checker.
//!
//! # One transport, and what holds it
//!
//! [`adbc`] is the only transport: the one `sutura serve` composes, the one the golden matrix opens,
//! and the one the fixture loader ingests through. What a SERVER answers is held by the executed
//! cells - the golden matrix in `sutura-app`'s tests and this crate's own `tests/` load the example
//! corpus into the server `nix/clickhouse-tier.nix` starts (through [`fixture`], behind the
//! default-off `fixtures` feature) and pin the rows, refusals, error and anchor report it answers -
//! in `checks.nextest` and under `just test`.
//!
//! # Parameters travel as `ClickHouse`'s own named parameters, never as text
//!
//! The adapter rewrites each rendered `?` into `{pN:Type}` and binds the value through the driver,
//! which sends it as a separate parameter the SERVER binds by the declared type - see
//! `adbc::rewrite_placeholders` for why counting `?` occurrences against the bound values is safe
//! rather than a guess.
//!
//! # `Deadline`, and what was checked about it - see [`deadline`]'s own header
//!
//! Every statement sends `max_execution_time`, computed from what the port's
//! [`sutura_domain::warehouse::deadline::Deadline`] has left. `deadline`'s module header states what
//! was measured about that setting's semantics and its limit, mirroring
//! `sutura_exec_postgres::deadline`'s own record for `SET LOCAL statement_timeout`.
//!
//! # Every connection and every answering statement pin the server settings sutura needs
//!
//! Each is a driver option on the connection and a setting in the statement's own `SETTINGS`
//! clause; under `EXECUTE AS` only the clause holds - see [`settings`], which also states what the
//! pin does not cover: the fixture loader's statements, and every setting sutura does not pin.
//!
//! `join_use_nulls=1`: under `ClickHouse`'s default `0`, the unmatched side of an outer join
//! answers the column type's default - `''` for a `String` - where the SQL standard answers `NULL`.
//! Measured by hand against the pinned compose server over the example corpus: 6 of the 23
//! questions with a committed `@duckdb` row golden disagreed without it, 0 with it, under a
//! non-`Nullable` schema; under `Nullable` columns the setting changes nothing, so a fixture
//! importer's type choice decides whether the defect shows - `fixture` declares no matrix column
//! `Nullable` for that reason. `output_format_json_quote_denormals=1`: under the default `0` an
//! infinite float answers the JSON `null`. `timeout_overflow_mode=throw`: `throw` answers a spent
//! `max_execution_time` with Code 159; `break` answers HTTP 200 with the rows read so far. The
//! executed goldens hold what the first two settings make the server answer, and nothing executed
//! holds the third. Offline, a loopback cell in `tests/adbc_execute_as.rs` holds that the statement
//! carries the clause; what the server does with a dropped setting needs the tier.
//!
//! # What is NOT here
//!
//! **Shipped in every release binary since `github.com/telekom/sutura#1237`.** `sutura-cli` opens a
//! `kind: clickhouse` source behind its default-off `clickhouse` feature, which `nix/shipped.nix`
//! now enables, once the `github.com/telekom/sutura#955` join-nulls fix landed.
//!
//! **No raw-SQL tool support** (`Warehouse::ACCEPTS_RAW_STATEMENTS` stays at its `false` default).
//!
//! **No `dry_run` override.** The ADBC driver has no cheap "prepare, do not run" step this adapter
//! could ask for without paying most of the cost of running the statement, so this stays at the
//! port's own default (`Ok(PreFlight::NotAsked)`) - the same honest absence
//! `Warehouse::dry_run`'s own doc names for an adapter where checking is not cheaper than running.

pub mod adbc;
mod deadline;
pub mod execute_as;
#[cfg(feature = "fixtures")]
pub mod fixture;
mod settings;
mod sum;
pub mod tls;

/// Environment variable naming a mounted `ClickHouse` ADBC driver.
pub const MOUNTED_DRIVER: &str = "SUTURA_CLICKHOUSE_ADBC_DRIVER";

#![forbid(unsafe_code)]
//! The raw SQL tool's execution path, against a real Postgres through the real driver -
//! `docs/adr/0013`'s amendment, measured rather than asserted from the driver's source.
//!
//! Same tier, same absence handling as `tests/conformance.rs`: every venue that runs the suite
//! provisions the tier, so these cells RUN; a developer machine with none writes `NOT RUN` and the
//! reason to stderr. See that file's header for the whole argument - it is not restated here.
//!
//! **What these cells hold that no unit test can:** the SERVER's own refusal. `AdbcPostgres` sends
//! `SET TRANSACTION READ ONLY` inside the transaction the driver opens and reaches the statement
//! through `PQprepare`; both properties are about what Postgres does with bytes on the wire.
//! Each call opens its own connection, so no two callers share a transaction to interleave in.

#[cfg(test)]
mod raw {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::model::TableName;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::{Value, Warehouse as _};
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::adbc::{AdbcError, AdbcPostgres};
    use sutura_exec_postgres::fixture::FixtureCredential;

    const SERVICE: &str = "postgres";

    /// A fresh schema per test, the same isolation `tests/conformance.rs` uses - so cells running in
    /// parallel against one server do not read or write one another's tables.
    fn open(case: &str) -> Option<AdbcPostgres> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("raw::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let schema = format!("raw_{case}_{}", std::process::id());
        let conninfo = credential
            .conninfo_in(&corpus::source(), endpoint.host(), endpoint.port(), &schema)
            .unwrap_or_else(|e| panic!("no connection string for the tier at {endpoint}: {e}"));
        let warehouse = AdbcPostgres::new(
            corpus::source(),
            corpus::posture(),
            sutura_exec_postgres::adbc::PostgresDriver::from_host().expect("the tier is up, so a driver is named"),
            conninfo,
        )
        .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}"));
        warehouse
            .create_schema(&schema)
            .unwrap_or_else(|e| panic!("postgres could not create {schema}: {e}"));
        Some(warehouse)
    }

    fn statement(sql: &str) -> RawStatement {
        RawStatement::parse(sql).expect("a test statement is a statement")
    }

    /// The SQLSTATE the server answered with, read off the driver's error.
    fn sqlstate(error: &PostgresError) -> Option<String> {
        let PostgresError::Adbc {
            cause: AdbcError::Adbc(ref cause),
        } = *error
        else {
            return None;
        };
        Some(
            cause
                .sqlstate
                .iter()
                .map(|&c| char::from(u8::try_from(c).unwrap_or(b'?')))
                .collect(),
        )
    }

    /// The property `docs/adr/0013`'s amendment states as the transaction boundary's whole job: a
    /// write is refused by the SERVER, `25006 read_only_sql_transaction`, independent of what the
    /// connecting role's grants would otherwise allow. `CREATE TABLE` rather than an `INSERT`, so
    /// this cell needs no fixture: a `READ ONLY` transaction refuses DDL exactly as it refuses a row.
    #[test]
    fn a_write_inside_the_call_is_refused_by_the_server_as_read_only() {
        let Some(warehouse) = open("write") else { return };
        let error = warehouse
            .execute_raw(
                &statement("create table raw_write_attempt (x int)"),
                &corpus::presented(),
                corpus::deadline(),
            )
            .expect("this adapter accepts a raw statement")
            .expect_err("a write inside a read-only transaction does not succeed");
        assert!(
            warehouse.source_refused(&error),
            "a read-only violation must classify as `source_refused`, so the raw path reports \
             `RawRefusalReason::SourceRefused` rather than a generic failure: {error}"
        );
        assert_eq!(sqlstate(&error).as_deref(), Some("25006"), "{error:?}");
    }

    /// Postgres refuses more than one command in one `Parse` message, so `SELECT 1; SELECT 2` is
    /// refused before either half runs - the server behaviour `execute` going through `PQprepare`
    /// depends on, measured here rather than read off the driver.
    #[test]
    fn a_multi_statement_string_is_refused_before_either_half_runs() {
        let Some(warehouse) = open("multi") else { return };
        let error = warehouse
            .execute_raw(&statement("select 1; select 2"), &corpus::presented(), corpus::deadline())
            .expect("this adapter accepts a raw statement")
            .expect_err("a multi-statement string is refused");
        // `42601 syntax_error`: "cannot insert multiple commands into a prepared statement".
        assert_eq!(sqlstate(&error).as_deref(), Some("42601"), "{error:?}");
        assert!(!warehouse.source_refused(&error), "{error}");
    }

    /// The ordinary path still works: one statement, inside the transaction every call is wrapped
    /// in - and the wrapping is invisible to a statement that never tried to write.
    #[test]
    fn an_ordinary_select_runs_and_returns_its_rows() {
        let Some(warehouse) = open("select") else { return };
        let rows = warehouse
            .execute_raw(
                &statement("select 1 as n, 'hi' as label"),
                &corpus::presented(),
                corpus::deadline(),
            )
            .expect("this adapter accepts a raw statement")
            .expect("an ordinary select succeeds");
        assert_eq!(rows.columns(), ["n", "label"]);
        assert_eq!(rows.rows(), [vec![Value::Integer(1), Value::Text(String::from("hi"))]]);
    }

    /// A refused write leaves nothing behind: the row a fixture committed is the only one there
    /// after a raw `INSERT` is refused, read back through the raw path itself.
    #[test]
    fn a_refused_write_persists_nothing() {
        let Some(warehouse) = open("persist") else { return };
        let dir = std::env::temp_dir().join(format!("sutura-raw-persist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir is creatable");
        let csv = dir.join("race_t.csv");
        std::fs::write(&csv, "x\n1\n").expect("the fixture is writable");
        let table = TableName::parse("race_t").expect("a test table is a table");
        warehouse.load_csv(&table, &csv).expect("the fixture loads");
        let refused = warehouse
            .execute_raw(
                &statement("insert into race_t values (2)"),
                &corpus::presented(),
                corpus::deadline(),
            )
            .expect("this adapter accepts a raw statement");
        assert!(refused.is_err(), "{refused:?}");
        let rows = warehouse
            .execute_raw(
                &statement("select count(*) from race_t"),
                &corpus::presented(),
                corpus::deadline(),
            )
            .expect("this adapter accepts a raw statement")
            .expect("the count reads");
        assert_eq!(rows.rows(), [vec![Value::Integer(1)]]);
    }

    /// The stream stops one row past `MAX_ROWS` - a statement that would return more hands back
    /// exactly `MAX_ROWS + 1`.
    ///
    /// **What this does NOT prove**: the stop bounds this process's own heap, not the server's work.
    /// The server still computes the statement until the stream is dropped and the transaction
    /// rolled back; `docs/adr/0013`'s `statement_timeout` is what bounds that, not this stop.
    #[test]
    fn a_statement_over_the_row_cap_returns_exactly_the_capped_count() {
        let Some(warehouse) = open("rowcap") else { return };
        let over_cap = sutura_domain::plan::MAX_ROWS + 2;
        let rows = warehouse
            .execute_raw(
                &statement(&format!("select generate_series(1, {over_cap})")),
                &corpus::presented(),
                corpus::deadline(),
            )
            .expect("this adapter accepts a raw statement")
            .expect("a statement over the cap is not itself a server-side error");
        assert_eq!(
            rows.rows().len(),
            usize::try_from(sutura_domain::plan::MAX_ROWS + 1).expect("the cap plus one fits a usize"),
            "the adapter must stop one row past the cap, neither at it nor at the statement's own count"
        );
    }
}

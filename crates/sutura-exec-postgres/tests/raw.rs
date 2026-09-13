//! The raw SQL tool's execution path, against a real Postgres - `docs/adr/0013`'s amendment,
//! measured rather than asserted from the driver's documentation.
//!
//! Same tier, same absence handling as `tests/conformance.rs`: every venue that runs the suite
//! provisions the tier, so these cells RUN; a developer machine with none writes `NOT RUN` and the
//! reason to stderr. See that file's header for the whole argument - it is not restated here.
//!
//! **What these cells hold that no unit test can:** the SERVER's own refusal. `PostgresWarehouse`
//! wraps every raw call in `BEGIN READ ONLY` / `ROLLBACK` and reaches the statement through the
//! extended query protocol; both properties are about what Postgres does with bytes on the wire, not
//! about anything this adapter's own Rust can assert on itself.

#[cfg(test)]
mod raw {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_exec_postgres::fixture::FixtureCredential;
    use sutura_exec_postgres::{PostgresError, PostgresWarehouse};

    const SERVICE: &str = "postgres";

    /// A fresh schema per test, the same isolation `tests/conformance.rs` uses - so cells running in
    /// parallel against one server do not read or write one another's tables.
    fn open(case: &str) -> Option<PostgresWarehouse> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("raw::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let config = PostgresWarehouse::local_config(endpoint.host(), endpoint.port(), &credential);
        let schema = format!("raw_{case}_{}", std::process::id());
        Some(
            PostgresWarehouse::connect_in_schema(corpus::source(), corpus::posture(), &config, &schema)
                .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}")),
        )
    }

    fn statement(sql: &str) -> RawStatement {
        RawStatement::parse(sql).expect("a test statement is a statement")
    }

    /// The property `docs/adr/0013`'s amendment states as the transaction boundary's whole job: a
    /// write is refused by the SERVER, `25006 read_only_sql_transaction`, independent of what the
    /// connecting role's grants would otherwise allow - measured, not asserted from documentation.
    /// `CREATE TABLE` rather than an `INSERT` into a fixture table, so this cell needs no corpus
    /// schema: a `READ ONLY` transaction refuses DDL exactly as it refuses a row write.
    #[test]
    fn a_write_inside_the_call_is_refused_by_the_server_as_read_only() {
        let Some(warehouse) = open("write") else { return };
        let outcome = warehouse.execute_raw(&statement("create table raw_write_attempt (x int)"), &corpus::presented());
        let error = outcome
            .expect("this adapter accepts a raw statement")
            .expect_err("a write inside a read-only transaction does not succeed");
        assert!(
            warehouse.source_refused(&error),
            "a read-only violation must classify as `source_refused`, so the raw path reports \
             `RawRefusalReason::SourceRefused` rather than a generic failure: {error}"
        );
        let (PostgresError::Prepare { ref cause } | PostgresError::Execute { ref cause }) = error else {
            panic!("expected a prepare or execute failure, got {error}");
        };
        assert_eq!(
            cause.code(),
            Some(&tokio_postgres::error::SqlState::READ_ONLY_SQL_TRANSACTION),
            "expected 25006 read_only_sql_transaction, got {error}"
        );
    }

    /// The property the extended protocol gives for free: Postgres refuses more than one command in
    /// one `Parse` message, so `SELECT 1; SELECT 2` is refused before either half runs - never a
    /// generic-driver-error guess, the actual server behaviour this adapter depends on.
    #[test]
    fn a_multi_statement_string_is_refused_before_either_half_runs() {
        let Some(warehouse) = open("multi") else { return };
        let outcome = warehouse.execute_raw(&statement("select 1; select 2"), &corpus::presented());
        let error = outcome
            .expect("this adapter accepts a raw statement")
            .expect_err("a multi-statement string is refused");
        assert!(
            matches!(error, PostgresError::Prepare { .. }),
            "a multi-statement string is refused while PREPARING, not while running: {error}"
        );
        // Not a read-only violation: this is a syntax-shaped refusal, and the two must stay
        // distinguishable so a caller is told the right thing.
        assert!(!warehouse.source_refused(&error), "{error}");
    }

    /// The ordinary path still works: one statement, over the extended protocol, inside the
    /// transaction this adapter wraps every call in - and the wrapping is invisible to a statement
    /// that never tried to write.
    #[test]
    fn an_ordinary_select_runs_and_returns_its_rows() {
        let Some(warehouse) = open("select") else { return };
        let rows = warehouse
            .execute_raw(&statement("select 1 as n, 'hi' as label"), &corpus::presented())
            .expect("this adapter accepts a raw statement")
            .expect("an ordinary select succeeds");
        assert_eq!(rows.columns(), ["n", "label"]);
        assert_eq!(rows.rows().len(), 1);
    }

    /// And the wrapper's own rollback is not a special case a second call has to work around: the
    /// same connection answers a second raw call normally, which is what "always rolled back, never
    /// left half-open" has to mean in practice.
    #[test]
    fn the_same_connection_answers_a_second_raw_call_after_a_refused_one() {
        let Some(warehouse) = open("sequence") else { return };
        let refused = warehouse.execute_raw(&statement("create table raw_sequence_attempt (x int)"), &corpus::presented());
        drop(refused.expect("accepts raw").unwrap_err());
        let rows = warehouse
            .execute_raw(&statement("select 2 as n"), &corpus::presented())
            .expect("this adapter accepts a raw statement")
            .expect("the connection still answers after a refused call");
        assert_eq!(rows.rows().len(), 1);
    }
}

#![forbid(unsafe_code)]
//! What each PostgreSQL type the adapter maps reads as, through the real driver against the tier.
//!
//! **The golden for the type surface.** The driver's own Arrow mapping, and this adapter's
//! `NUMERIC` re-read over it, are what an answer is made of; these cells pin the value each type
//! becomes - or the refusal it is - so a driver bump that changes a mapping fails here by name.
//!
//! Same tier, same absence handling as `tests/conformance.rs`: every venue that runs the suite
//! provisions the tier, so these cells RUN; a developer machine with none writes `NOT RUN`.

#[cfg(test)]
mod types {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::raw::RawRows;
    use sutura_domain::warehouse::{Real, Value, Warehouse as _};
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::adbc::{AdbcPostgres, Channel};
    use sutura_exec_postgres::fixture::FixtureCredential;

    fn open() -> Option<AdbcPostgres> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "postgres") {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("types: NOT RUN - {absent}");
                return None;
            }
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let conninfo = credential
            .conninfo(&corpus::source(), endpoint.host(), endpoint.port(), Channel::Plaintext)
            .unwrap_or_else(|e| panic!("no connection string for the tier at {endpoint}: {e}"));
        Some(
            AdbcPostgres::new(
                corpus::source(),
                corpus::posture(),
                sutura_exec_postgres::adbc::PostgresDriver::from_host().expect("the tier is up, so a driver is named"),
                conninfo,
            )
            .expect("the default ceiling parses"),
        )
    }

    /// What `sql` answered, or its refusal.
    fn answered(warehouse: &AdbcPostgres, sql: &str) -> Option<Result<RawRows, PostgresError>> {
        let statement = RawStatement::parse(sql).expect("a test statement is a statement");
        warehouse.execute_raw(&statement, &corpus::presented(), corpus::deadline())
    }

    /// The one cell `sql` answers.
    fn read(warehouse: &AdbcPostgres, sql: &str) -> Value {
        let rows = answered(warehouse, sql)
            .expect("this adapter accepts raw statements")
            .unwrap_or_else(|refused| panic!("{sql}: {refused:?}"));
        let [row] = rows.rows() else {
            panic!("{sql}: one row, got {:?}", rows.rows());
        };
        let [cell] = row.as_slice() else {
            panic!("{sql}: one column, got {row:?}");
        };
        cell.clone()
    }

    /// The refusal `sql` answers with.
    fn refusal(warehouse: &AdbcPostgres, sql: &str) -> PostgresError {
        match answered(warehouse, sql).expect("this adapter accepts raw statements") {
            Ok(rows) => panic!("{sql}: refused, not {:?}", rows.rows()),
            Err(refused) => refused,
        }
    }

    fn text(value: &str) -> Value {
        Value::Text(String::from(value))
    }

    #[test]
    fn each_mapped_type_reads_as_its_domain_value() {
        let Some(warehouse) = open() else { return };
        let finite = |value| Value::Real(Real::parse(value).expect("a finite test value"));
        for (sql, expected) in [
            ("SELECT 7::int2 AS v", Value::Integer(7)),
            ("SELECT 7::int4 AS v", Value::Integer(7)),
            (
                "SELECT 1234567890123456789::int8 AS v",
                Value::Integer(1_234_567_890_123_456_789),
            ),
            ("SELECT NULL::int4 AS v", Value::Null),
            ("SELECT true AS v", Value::Integer(1)),
            ("SELECT 2.5::float8 AS v", finite(2.5)),
            ("SELECT 'north'::text AS v", text("north")),
            ("SELECT 'north'::varchar AS v", text("north")),
            ("SELECT 'north'::bpchar AS v", text("north")),
            ("SELECT 'north'::name AS v", text("north")),
            ("SELECT DATE '2026-06-01' AS v", text("2026-06-01")),
            // NUMERIC: scale zero that fits an i64 is the integer; anything else stays exact text.
            ("SELECT 1::numeric AS v", Value::Integer(1)),
            ("SELECT -42::numeric AS v", Value::Integer(-42)),
            (
                "SELECT sum(x) AS v FROM (VALUES (1::int8), (2::int8)) AS t(x)",
                Value::Integer(3),
            ),
            ("SELECT 1.50::numeric AS v", text("1.50")),
            (
                "SELECT 123456789012345678901234567890::numeric AS v",
                text("123456789012345678901234567890"),
            ),
        ] {
            assert_eq!(read(&warehouse, sql), expected, "{sql}");
        }
    }

    #[test]
    fn a_value_with_no_exact_domain_reading_is_refused_by_its_variant() {
        let Some(warehouse) = open() else { return };
        for sql in [
            "SELECT 'NaN'::numeric AS v",
            "SELECT 'Infinity'::numeric AS v",
            "SELECT '-Infinity'::numeric AS v",
            "SELECT 'NaN'::float8 AS v",
        ] {
            let refused = refusal(&warehouse, sql);
            assert!(
                matches!(refused, PostgresError::NotFinite { ref column, .. } if column == "v"),
                "{sql}: {refused:?}"
            );
        }
        let refused = refusal(&warehouse, "SELECT 1.5::float4 AS v");
        assert!(
            matches!(refused, PostgresError::UnsupportedType { ref column, .. } if column == "v"),
            "{refused:?}"
        );
    }
}

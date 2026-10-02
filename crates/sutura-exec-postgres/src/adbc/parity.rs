//! The two transports' VALUES for one statement each, held equal: the `tokio-postgres` path runs it
//! against the tier, and the ADBC path reads the Arrow column its pinned driver returns for that
//! type - built here from `postgres_type.h`'s `SetSchema` and `copy/reader.h`'s numeric writer,
//! because no cell runs the driver yet. Stage 2's real-driver run is what replaces that half.
//!
//! Same tier, same absence handling as `tests/raw.rs`: every venue that runs the suite provisions
//! it, and a developer machine with none writes `NOT RUN` and the reason.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use arrow_array::{
    ArrayRef, BooleanArray, Date32Array, Float32Array, Float64Array, Int16Array, Int32Array, Int64Array, RecordBatch, StringArray,
};
use arrow_schema::{Field, Schema};
use sutura_conformance::corpus;
use sutura_dev::provisioned::{self, Provisioned};
use sutura_domain::calendar::Date;
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::Warehouse as _;
use sutura_domain::warehouse::raw::RawRows;

use super::tests::{CEILING_MS, FakeConnection, open_for};
use crate::PostgresError;
use crate::PostgresWarehouse;
use crate::fixture::FixtureCredential;

/// One statement, and the column the driver hands back for it.
struct Case {
    sql: &'static str,
    column: ArrayRef,
    /// Whether the driver tags the column `numeric`, which it does for `NUMERIC` and nothing else.
    numeric: bool,
}

fn cases() -> Vec<Case> {
    let day = Date::parse("2026-06-01").expect("a test date is a date").days_since_epoch();
    let case = |sql, column: ArrayRef, numeric| Case { sql, column, numeric };
    vec![
        case("SELECT 7::int2 AS v", Arc::new(Int16Array::from(vec![7_i16])), false),
        case("SELECT 7::int4 AS v", Arc::new(Int32Array::from(vec![7_i32])), false),
        case(
            "SELECT 1234567890123456789::int8 AS v",
            Arc::new(Int64Array::from(vec![1_234_567_890_123_456_789_i64])),
            false,
        ),
        case("SELECT NULL::int4 AS v", Arc::new(Int32Array::from(vec![None::<i32>])), false),
        case("SELECT true AS v", Arc::new(BooleanArray::from(vec![true])), false),
        case("SELECT 2.5::float8 AS v", Arc::new(Float64Array::from(vec![2.5_f64])), false),
        case("SELECT 'north'::text AS v", Arc::new(StringArray::from(vec!["north"])), false),
        case(
            "SELECT 'north'::varchar AS v",
            Arc::new(StringArray::from(vec!["north"])),
            false,
        ),
        case(
            "SELECT 'north'::bpchar AS v",
            Arc::new(StringArray::from(vec!["north"])),
            false,
        ),
        case("SELECT 'north'::name AS v", Arc::new(StringArray::from(vec!["north"])), false),
        case("SELECT DATE '2026-06-01' AS v", Arc::new(Date32Array::from(vec![day])), false),
        case("SELECT 1::numeric AS v", Arc::new(StringArray::from(vec!["1"])), true),
        case("SELECT -42::numeric AS v", Arc::new(StringArray::from(vec!["-42"])), true),
        case("SELECT 1.50::numeric AS v", Arc::new(StringArray::from(vec!["1.50"])), true),
        case(
            "SELECT 123456789012345678901234567890::numeric AS v",
            Arc::new(StringArray::from(vec!["123456789012345678901234567890"])),
            true,
        ),
        // Refused by both, under the same variant: a non-finite NUMERIC or float8, and a 32-bit
        // float neither maps.
        case("SELECT 'NaN'::numeric AS v", Arc::new(StringArray::from(vec!["nan"])), true),
        case(
            "SELECT 'Infinity'::numeric AS v",
            Arc::new(StringArray::from(vec!["inf"])),
            true,
        ),
        case(
            "SELECT '-Infinity'::numeric AS v",
            Arc::new(StringArray::from(vec!["-inf"])),
            true,
        ),
        case(
            "SELECT 'NaN'::float8 AS v",
            Arc::new(Float64Array::from(vec![f64::NAN])),
            false,
        ),
        case("SELECT 1.5::float4 AS v", Arc::new(Float32Array::from(vec![1.5_f32])), false),
    ]
}

/// The ADBC path's answer: the driver's column through the raw path, as `Warehouse::execute_raw`
/// reads it.
fn adbc(case: &Case) -> Result<RawRows, PostgresError> {
    let metadata = if case.numeric {
        HashMap::from([(String::from("ADBC:postgresql:typname"), String::from("numeric"))])
    } else {
        HashMap::new()
    };
    let field = Field::new("v", case.column.data_type().clone(), true).with_metadata(metadata);
    let batch = RecordBatch::try_new(Arc::new(Schema::new(vec![field])), vec![Arc::clone(&case.column)])
        .map_err(crate::adbc::AdbcError::Batch)?;
    let mut connection = FakeConnection::replying(batch);
    let batches = super::session::raw(
        &mut connection,
        case.sql,
        CEILING_MS,
        open_for(std::time::Duration::from_secs(60)),
    )
    .map_err(PostgresError::from)?;
    super::raw_rows(&batches)
}

fn tier() -> Option<PostgresWarehouse> {
    let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "postgres") {
        Provisioned::At(endpoint) => endpoint,
        Provisioned::Skipped(absent) => {
            eprintln!("adbc::parity: NOT RUN - {absent}");
            return None;
        }
    };
    let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
    let config = PostgresWarehouse::local_config(endpoint.host(), endpoint.port(), &credential);
    Some(
        PostgresWarehouse::connect(corpus::source(), corpus::posture(), &config)
            .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}")),
    )
}

#[test]
fn each_mapped_type_reads_the_same_through_both_transports() {
    let Some(tokio) = tier() else { return };
    for case in cases() {
        let statement = RawStatement::parse(case.sql).expect("a test statement is a statement");
        let over_tokio = tokio
            .execute_raw(&statement, &corpus::presented(), corpus::deadline())
            .expect("this adapter accepts raw statements");
        match (over_tokio, adbc(&case)) {
            (Ok(tokio), Ok(adbc)) => {
                assert_eq!(adbc.columns(), tokio.columns(), "{}", case.sql);
                assert_eq!(adbc.rows(), tokio.rows(), "{}", case.sql);
            }
            (Err(tokio), Err(adbc)) => assert_eq!(
                core::mem::discriminant(&adbc),
                core::mem::discriminant(&tokio),
                "{}: tokio-postgres {tokio:?}, ADBC {adbc:?}",
                case.sql
            ),
            (tokio, adbc) => panic!("{}: tokio-postgres {tokio:?}, ADBC {adbc:?}", case.sql),
        }
    }
}

#[test]
fn a_non_finite_numeric_is_the_same_refusal_on_both_transports() {
    // The tier cell above holds the variants equal where a tier runs; this one holds the ADBC half
    // to the `tokio-postgres` path's variant everywhere.
    let nan = cases()
        .into_iter()
        .find(|case| case.sql.contains("NaN"))
        .expect("the NaN case");
    let refused = adbc(&nan).expect_err("a NaN NUMERIC is refused");
    assert!(
        matches!(refused, PostgresError::NotFinite { ref column, .. } if column == "v"),
        "{refused:?}"
    );
}

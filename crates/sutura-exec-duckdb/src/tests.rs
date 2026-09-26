//! The `DuckDB` adapter's unit tests, split from `lib.rs` for `cargo xtask max-lines`.

#[cfg(feature = "fixtures")]
use super::duck_types;
use super::{DuckDbError, DuckDbWarehouse, Presented, Real};
use duckdb::types::{Decimal, TimeUnit, Value as DuckValue};
use sutura_domain::calendar::Date;
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::{RowSet, Value};
use sutura_sql::GeneratedQuery;

fn real(value: f64) -> Real {
    Real::parse(value).expect("a test literal is finite")
}

/// One row of the shared table: what the value is called, what the driver hands over, and the
/// domain value both adapters have to produce for it. Named because the tuple is over the
/// `type_complexity` threshold this workspace tightened, and a `Vec<(..)>` of three is where it
/// starts to be unreadable anyway.
type Case = (&'static str, DuckValue, Value);

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

/// The posture a test opens this adapter with.
///
/// Shared, and it is the honest declaration rather than a convenience: one process holds one
/// connection under one operating-system identity, which is what the capability constant above
/// says out loud.
fn shared_posture() -> sutura_domain::source::SourcePosture {
    sutura_domain::source::SourcePosture::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(
            sutura_domain::source::AcknowledgementReason::parse("one process, one connection, one operating-system identity")
                .expect("a fixture reason is a reason"),
        ),
    }
}

/// What this adapter can execute a leg as: the deployment's own identity for the source, carrying
/// the same acknowledgement [`shared_posture`] declares.
fn shared_leg() -> Presented {
    match shared_posture() {
        sutura_domain::source::SourcePosture::SharedServiceUser { declared } => Presented::SharedServiceUser { declared },
        sutura_domain::source::SourcePosture::ImpersonationAtSource => {
            panic!("the fixture posture is shared, one function above")
        }
    }
}

#[test]
#[cfg(feature = "fixtures")]
fn fixture_schema_forces_the_shared_boolean_wide_and_decimal_types() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let conformance = duck_types(&root.join("crates/sutura-conformance/corpus/conformance_events.csv"))
        .expect("the conformance fixture has shared types");
    assert!(conformance.contains("'wide_amount': 'UBIGINT'"), "{conformance}");
    assert!(conformance.contains("'decimal_amount': 'DECIMAL(38,2)'"), "{conformance}");

    let example = duck_types(&root.join("examples/single-player/data/fct_subscription_monthly.csv"))
        .expect("the example fixture has shared types");
    assert!(example.contains("'churned_in_month': 'BOOLEAN'"), "{example}");
}

#[test]
fn every_type_this_adapter_maps_answers_what_the_engine_answers() {
    // The twin of `every_type_the_interior_maps_answers_what_the_data_source_answers` in
    // `crates/sutura-domain/src/warehouse/arrow/tests.rs`. Same logical values, same
    // expected column, one row per width - because a Parquet `INT32` column under a `min` or a
    // `max` used to answer here and error there.
    // Boundary values rather than round ones, written in hex where the decimal form is a bit
    // pattern nobody reads: an arm that reached for the wrong width would come back truncated
    // or sign-flipped, and 42 would survive that.
    let cases: Vec<Case> = vec![
        ("NULL", DuckValue::Null, Value::Null),
        ("BOOLEAN true", DuckValue::Boolean(true), Value::Integer(1)),
        ("BOOLEAN false", DuckValue::Boolean(false), Value::Integer(0)),
        ("TINYINT", DuckValue::TinyInt(i8::MIN), Value::Integer(-128)),
        ("SMALLINT", DuckValue::SmallInt(i16::MIN), Value::Integer(-0x8000)),
        ("INTEGER", DuckValue::Int(i32::MAX), Value::Integer(0x7FFF_FFFF)),
        ("BIGINT", DuckValue::BigInt(i64::MIN), Value::Integer(i64::MIN)),
        ("UTINYINT", DuckValue::UTinyInt(u8::MAX), Value::Integer(255)),
        ("USMALLINT", DuckValue::USmallInt(u16::MAX), Value::Integer(0xFFFF)),
        ("UINTEGER", DuckValue::UInt(u32::MAX), Value::Integer(0xFFFF_FFFF)),
        ("UBIGINT that fits an i64", DuckValue::UBigInt(42), Value::Integer(42)),
        (
            "UBIGINT that does not",
            DuckValue::UBigInt(u64::MAX),
            Value::Text(String::from("18446744073709551615")),
        ),
        (
            "HUGEINT wider than the shared decimal",
            DuckValue::HugeInt(i128::MAX),
            Value::Text(i128::MAX.to_string()),
        ),
        ("DOUBLE", DuckValue::Double(0.1), Value::Real(real(0.1))),
        // Zero is finite, and it is here because the check that refuses `inf` is a check about a
        // division BY zero: a metric that legitimately answers zero must still answer.
        ("DOUBLE zero", DuckValue::Double(0.0), Value::Real(real(0.0))),
        (
            "DECIMAL stays text so it stays exact",
            DuckValue::Decimal(Decimal::new(9, 2, 12_345).expect("a test decimal is a decimal")),
            Value::Text(String::from("123.45")),
        ),
        (
            "whole DECIMAL fitting i64",
            DuckValue::Decimal(Decimal::new(2, 0, 42).expect("a test whole decimal is a decimal")),
            Value::Integer(42),
        ),
        (
            "VARCHAR",
            DuckValue::Text(String::from("north")),
            Value::Text(String::from("north")),
        ),
        (
            "DATE as ISO text",
            DuckValue::Date32(day("2026-06-01").days_since_epoch()),
            Value::Text(String::from("2026-06-01")),
        ),
    ];
    for (name, raw, expected) in cases {
        assert_eq!(DuckDbWarehouse::cell(name, raw).expect(name), expected, "{name}");
    }
}

#[test]
fn a_32_bit_float_is_refused_here_because_it_is_refused_there() {
    // The finding this arm exists for. It was `Value::Real(f64::from(v))`, and the engine's
    // `cell` refused `Float32` in the same release - so one plan over a `REAL` column answered
    // 0.10000000149011612 through the data source and errored through the engine. Refusing is
    // the half of the disagreement that can be fixed without inventing a rendering: there is no
    // `f64` that is `0.1_f32`, and picking one silently is how a number nobody got wrong stops
    // matching itself.
    let error = DuckDbWarehouse::cell("amount", DuckValue::Float(0.1)).expect_err("a 32-bit float is not mapped");
    assert!(
        matches!(error, DuckDbError::UnsupportedType { ref column, .. } if column == "amount"),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("REAL"), "{message}");
    assert!(message.contains("column amount"), "{message}");
    // And the type that DOES answer, so this is not a test that would pass with every float
    // refused.
    assert_eq!(
        DuckDbWarehouse::cell("amount", DuckValue::Double(0.1)).expect("a 64-bit float is mapped"),
        Value::Real(real(0.1))
    );
}

#[test]
fn a_non_finite_double_is_refused_here_because_it_is_refused_there() {
    // THE FINDING THIS ARM EXISTS FOR, and the twin of
    // `a_non_finite_double_is_refused_on_both_sides_of_the_port` in
    // `crates/sutura-domain/src/warehouse/arrow/tests.rs`. This arm was `Value::Real(v)`
    // on a raw `f64`, and the value that reached it was real: a ratio measure declaring
    // `zero_denominator: fails` renders as an unguarded division with the numerator cast to
    // `DOUBLE`, and `CAST(3 AS DOUBLE) / 0` in this data system is `inf`, not an error. So the
    // metric answered the string "inf" under its own certified name, and the engine answered the
    // same string, so the differential test agreed and passed.
    //
    // All three of the class, not just the one a zero denominator produces first: a guard on the
    // division would have left `-inf` and `NaN` on the way in.
    for (name, raw) in [
        ("positive infinity", f64::INFINITY),
        ("negative infinity", f64::NEG_INFINITY),
        ("not a number", f64::NAN),
    ] {
        let error = DuckDbWarehouse::cell("revenue_per_refunded_order", DuckValue::Double(raw)).expect_err(name);
        assert!(
            matches!(error, DuckDbError::NotFinite { ref column, .. } if column == "revenue_per_refunded_order"),
            "{name}: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "column revenue_per_refunded_order came back as a value that is not a finite number",
            "{name}"
        );
    }
    // And the values that DO answer, so this is not a test that would pass with every double
    // refused - zero included, because the check is about dividing BY zero and not about it.
    assert_eq!(
        DuckDbWarehouse::cell("revenue", DuckValue::Double(0.0)).expect("zero is a finite number"),
        Value::Real(real(0.0))
    );
    assert_eq!(
        DuckDbWarehouse::cell("average_order", DuckValue::Double(63_335.777_777_777_78)).expect("an average is a finite number"),
        Value::Real(real(63_335.777_777_777_78))
    );
}

#[test]
fn a_type_neither_adapter_maps_names_itself_rather_than_being_rendered() {
    // A `Debug` fallback here would flow into an answer looking like data, and an anchor
    // comparison against it would pass or fail for a reason nobody could read.
    for raw in [
        DuckValue::Blob(vec![0_u8, 1_u8]),
        DuckValue::Timestamp(TimeUnit::Microsecond, 0),
        DuckValue::List(vec![DuckValue::Int(1)]),
    ] {
        let error = DuckDbWarehouse::cell("payload", raw).expect_err("an unmapped type is an error");
        assert!(
            matches!(error, DuckDbError::UnsupportedType { ref column, .. } if column == "payload"),
            "{error:?}"
        );
    }
}

#[test]
fn the_column_labels_come_from_the_statement_that_answered() {
    // The half of the schema read that is reachable, and the reason the labels are taken from
    // the executed statement at all: a projection is what an answer is read by. `run` is
    // exercised directly with a literal statement, because the labels have to be right before
    // any plan is involved.
    let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture()).expect("an in-memory database opens");
    let query = GeneratedQuery::literal(source(), String::from("SELECT 1 AS period, 'north' AS region"));
    let rows = warehouse.run(&query).expect("a literal select answers");
    assert_eq!(rows.columns(), ["period", "region"]);
    assert_eq!(rows.rows().len(), 1);
}

#[test]
fn credential_material_this_adapter_cannot_use_is_refused_before_anything_is_prepared() {
    // The wiring defect between a credential broker and a source declaration, at the adapter that
    // has to answer for it. This one holds a connection under one operating-system identity -
    // which is what `IMPERSONATION` declares - so a subject's own token has nowhere to go, and
    // accepting it would report a leg as impersonated that ran as this process.
    //
    // Both port methods that take a credential are asserted, because the pre-flight is the one
    // where a missing check would be least visible: a statement PREPARED as the wrong identity
    // resolves against tables the asker may not be able to see.
    let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture()).expect("an in-memory database opens");
    for handed in [
        Presented::SubjectToken {
            material: sutura_domain::identity::Secret::new("an-exchanged-token"),
            impersonate: None,
        },
        Presented::SubjectPrincipal {
            name: sutura_domain::identity::PrincipalName::parse("analyst_role").expect("a test name is a name"),
        },
    ] {
        let expected = handed.as_str();
        let error = warehouse
            .deliverable(&handed)
            .expect_err("this adapter cannot carry a subject");
        let DuckDbError::NoPlaceForASubject { ref at, presented } = error else {
            panic!("the adapter names what it was handed: {error:?}");
        };
        assert_eq!(at, "local");
        assert_eq!(presented, expected);
    }
    // And the shape it CAN execute with is accepted, so the assertion above is not passing against
    // an adapter that refuses everything.
    warehouse
        .deliverable(&shared_leg())
        .expect("the deployment's own identity for this source is what this adapter can execute with");
}

#[test]
fn a_shared_leg_carrying_another_acknowledgement_is_refused_rather_than_prepared() {
    // THE CHECK THE SHAPE MATCH DOES NOT MAKE. The match above compares what arrived against what
    // this CODE can carry - `IMPERSONATION` - and reads `posture` not at all, so a leg whose
    // variant is right and whose operator acknowledgement is another source's got past it and
    // executed. Provenance is read off `posture`, so the answer would then have recorded this
    // adapter's declaration rather than what the broker presented: a record of a leg that did not
    // happen. A review found this, and the two values compared here are genuinely independent -
    // the broker reads the settings tree and the adapter holds what the composition root handed it.
    let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture()).expect("an in-memory database opens");
    let fabricated = Presented::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(
            sutura_domain::source::AcknowledgementReason::parse("a witness no operator wrote for this source")
                .expect("a test reason is a reason"),
        ),
    };
    // It matches the variant this adapter accepts, which is why the shape check cannot see it.
    assert_eq!(fabricated.as_str(), shared_leg().as_str());

    let error = warehouse
        .deliverable(&fabricated)
        .expect_err("a witness that is not this source's is not this source's");
    let DuckDbError::PresentedDisagreesWithPosture { ref cause } = error else {
        panic!("the adapter names the disagreement rather than preparing anything: {error:?}");
    };
    assert_eq!(
        *cause,
        sutura_domain::identity::PresentedDisagreesWithPosture::WitnessIsNotThisSources { at: source() }
    );

    // And this source's OWN witness is still accepted, so the assertion above is not passing
    // against an adapter that refuses every shared leg.
    warehouse
        .deliverable(&shared_leg())
        .expect("this source's own acknowledgement is what this adapter executes with");
}

#[test]
fn a_result_set_with_no_statement_behind_it_is_refused_rather_than_answered_with_no_columns() {
    // THE DEGRADATION THIS VARIANT REPLACED. The column labels were read as
    // `rows.as_ref().map(Statement::column_names).unwrap_or_default()`, so an absent handle
    // produced an empty projection - and this is why nothing downstream would have caught it: a
    // `RowSet` with no columns and N rows is REJECTED BY NOTHING, because every row has no cells
    // either and the result is rectangular.
    let degraded = RowSet::new(Vec::new(), vec![Vec::new(), Vec::new()])
        .expect("no columns and no cells per row is rectangular, which is what made the default silent");
    assert!(degraded.columns().is_empty(), "the exact defect being refused");
    assert_eq!(degraded.rows().len(), 2, "two rows of nothing, and a valid result set");
    // So the shape has to be refused where it arises. Constructed rather than provoked: the
    // handle is present for every statement this adapter runs - the test above is that path -
    // and a driver that stopped handing one over is exactly the change that must not turn into
    // an answer. The message names what is missing, because "no columns" on its own reads as a
    // fact about the metric rather than about the driver.
    let error = DuckDbError::NoSchema;
    assert_eq!(
        error.to_string(),
        "the result set came back without the statement that produced it, so it has no columns"
    );
    assert!(
        core::error::Error::source(&error).is_none(),
        "an absent handle carries no cause, and inventing one would be worse than saying so"
    );
}

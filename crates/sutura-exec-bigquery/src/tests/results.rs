//! Result mapping and refusal behavior under the fake transport.

use super::{
    BigQueryError, BigQueryWarehouse, Broken, Case, Cell, Executable, Field, FieldType, JobRows, Paged, Recording, Value,
    Warehouse as _, leg_of, one_cell, open, plan, shared_posture,
};

#[test]
fn every_type_this_adapter_maps_answers_what_the_other_sql_adapter_answers() {
    // **The value mapping as a table, and the duplication with `sutura-exec-duckdb` is deliberate.**
    // The two `cell` functions live in crates that may not depend on each other, so the agreement is
    // asserted as the same expected column written out in both places. Three arms below are the ones
    // where disagreeing would produce a wrong number rather than an error, and each says so.
    let warehouse = open(Recording::empty(), shared_posture());
    let cases: Vec<Case> = vec![
        (FieldType::Int64, Cell::Text(String::from("250")), Value::Integer(250)),
        (
            FieldType::String,
            Cell::Text(String::from("north")),
            Value::Text(String::from("north")),
        ),
        // A boolean becomes an integer, because the domain has no boolean and the other adapter maps
        // `BOOLEAN` to `Integer(i64::from(v))`. The example catalog counts a churn flag, so the two
        // would otherwise disagree about a metric.
        (FieldType::Bool, Cell::Text(String::from("true")), Value::Integer(1)),
        (FieldType::Bool, Cell::Text(String::from("false")), Value::Integer(0)),
        // An exact decimal stays TEXT. Turning it into a double is how a total that was correct in the
        // data system stops being correct in an answer.
        (
            FieldType::Numeric,
            Cell::Text(String::from("12345.67")),
            Value::Text(String::from("12345.67")),
        ),
        // A date is re-rendered from a parse, so a malformed one is an error rather than text that
        // looks like a date downstream.
        (
            FieldType::Date,
            Cell::Text(String::from("2026-06-01")),
            Value::Text(String::from("2026-06-01")),
        ),
        (FieldType::Int64, Cell::Null, Value::Null),
    ];
    for (kind, cell, expected) in cases {
        let rows = BigQueryWarehouse::<Recording>::rows(&one_cell(kind.clone(), cell.clone()))
            .unwrap_or_else(|e| panic!("{kind:?} with {cell:?} should map: {e}"));
        assert_eq!(rows.rows().first().and_then(|r| r.first()), Some(&expected), "{kind:?}");
    }
    // A double maps too, and is asserted separately because `Real` has no `Eq`.
    let rows = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Float64, Cell::Text(String::from("1.5"))))
        .expect("a finite double maps");
    match rows.rows().first().and_then(|r| r.first()) {
        Some(&Value::Real(real)) => assert!((real.get() - 1.5).abs() < f64::EPSILON),
        other => panic!("expected a real, got {other:?}"),
    }
    drop(warehouse);
}

#[test]
fn a_non_finite_double_is_refused_rather_than_answered() {
    // **The arm that keeps a stored non-finite value from answering under a certified number.** A
    // `FLOAT64` column holding a non-finite value is refused here - in GoogleSQL the unguarded `/`
    // raises on a zero divisor, so this arm is not the ratio case that `zero_denominator: fails`
    // carries on DuckDB; it is a STORED `Infinity`. The fixture spells the values the way the endpoint
    // does - `Infinity`/`-Infinity`/`NaN` - rather than the standard library's `inf`, so the test
    // keeps measuring the wire's shape.
    for hostile in ["Infinity", "-Infinity", "NaN"] {
        let error = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Float64, Cell::Text(String::from(hostile))))
            .expect_err("a non-finite double is refused");
        assert!(matches!(error, BigQueryError::NotFinite { .. }), "{hostile}: {error:?}");
    }
}

#[test]
fn a_type_this_adapter_does_not_map_names_itself_rather_than_answering_null() {
    // The reason `FieldType::Unmapped` carries the endpoint's own spelling: a null here would be a
    // wrong number, and a message that said "an unsupported type" would not say which column to fix.
    let error = BigQueryWarehouse::<Recording>::rows(&one_cell(
        FieldType::Unmapped(String::from("GEOGRAPHY")),
        Cell::Text(String::from("POINT(0 0)")),
    ))
    .expect_err("an unmapped type is refused");
    match error {
        BigQueryError::UnmappedType { ref column, ref named } => {
            assert_eq!(column, "value");
            assert_eq!(named, "GEOGRAPHY");
        }
        other => panic!("expected an unmapped-type refusal, got {other:?}"),
    }
}

#[test]
fn a_declared_number_that_did_not_come_back_as_one_is_refused() {
    // The endpoint sends every value as text, so "declared INT64" and "parses as an integer" are two
    // different facts and this is where they are reconciled.
    // Two variants rather than one, because the CAUSE differs and each keeps its own on the chain.
    let integer = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Int64, Cell::Text(String::from("not a number"))))
        .expect_err("a non-numeric value in an INT64 column is refused");
    assert!(matches!(integer, BigQueryError::NotAnInteger { .. }), "{integer:?}");
    let double = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Float64, Cell::Text(String::from("not a number"))))
        .expect_err("a non-numeric value in a FLOAT64 column is refused");
    assert!(matches!(double, BigQueryError::NotADouble { .. }), "{double:?}");
    // The standard-library cause survives, because `#[source]` is not wired for you.
    assert!(core::error::Error::source(&integer).is_some());
    assert!(core::error::Error::source(&double).is_some());
    // And a boolean that is neither spelling, which carries no cause because there is no parse
    // behind it.
    let boolean = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Bool, Cell::Text(String::from("yes"))))
        .expect_err("a BOOL column holding `yes` is refused");
    assert!(matches!(boolean, BigQueryError::NotABool { .. }), "{boolean:?}");
    // And a malformed date, which is the same argument on a different type.
    let error = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Date, Cell::Text(String::from("2026-13-45"))))
        .expect_err("a malformed date is refused");
    assert!(matches!(error, BigQueryError::NotADate { .. }), "{error:?}");
}

#[test]
fn a_row_at_the_wrong_width_is_refused_and_names_which_row() {
    // The endpoint disagreeing with its own schema. `RowSet::new` would catch it too; this catches it
    // first so the message can say which row, which is the difference between a usable failure and a
    // count.
    let rows = JobRows::of(
        vec![
            Field::of(String::from("a"), FieldType::Int64),
            Field::of(String::from("b"), FieldType::Int64),
        ],
        vec![
            vec![Cell::Text(String::from("1")), Cell::Text(String::from("2"))],
            vec![Cell::Text(String::from("3"))],
        ],
        2,
    );
    let error = BigQueryWarehouse::<Recording>::rows(&rows).expect_err("a ragged result is refused");
    match error {
        BigQueryError::RowWidth { row, cells, columns } => {
            assert_eq!((row, cells, columns), (1, 1, 2));
        }
        other => panic!("expected a row-width refusal, got {other:?}"),
    }
}

#[test]
fn a_result_shorter_than_what_the_endpoint_reported_is_refused() {
    // `jobs.query` answers ONE page; completeness is the endpoint's `totalRows`, never the rows alone.
    // A first page, or an incomplete job's empty `rows`, would otherwise read to `answer()` as *under
    // the cap, not truncated* - a wrong number under a certified name, through the exact row the
    // row-cap invariant exists to hold. This is the seam refusing it.
    let answered = JobRows::of(
        vec![
            Field::of(String::from("a"), FieldType::Int64),
            Field::of(String::from("b"), FieldType::Int64),
        ],
        vec![
            vec![Cell::Text(String::from("1")), Cell::Text(String::from("2"))],
            vec![Cell::Text(String::from("3")), Cell::Text(String::from("4"))],
        ],
        3,
    );
    let error = BigQueryWarehouse::<Recording>::rows(&answered).expect_err("a partial result is refused");
    match error {
        BigQueryError::Incomplete { delivered, total } => assert_eq!((delivered, total), (2, 3)),
        other => panic!("expected an incomplete-result refusal, got {other:?}"),
    }
}

#[test]
fn a_federated_leg_is_refused_because_there_is_nothing_above_it_to_combine_legs() {
    // A leg executed with nothing above it returns rows at a finer grouping than the question asked
    // for, which is a wrong number under a certified name. Both SQL adapters answer this the same way.
    let warehouse = open(Recording::empty(), shared_posture());
    let leg = crate::tests::a_leg();
    let error = warehouse
        .execute(Executable::Leg(&leg), &leg_of(&shared_posture()))
        .expect_err("a leg has no combiner above it");
    match error {
        BigQueryError::LegWithoutCombiner { ref table } => assert_eq!(table, "fct_subscription_monthly"),
        other => panic!("expected a leg refusal, got {other:?}"),
    }
    assert!(warehouse.transport.seen.borrow().is_empty());
}

#[test]
fn a_result_the_endpoint_would_not_return_at_once_is_a_size_bound_and_not_an_outage() {
    // THE defect the second bound exists for, at the adapter. `jobs.query` answers one page - as many
    // rows as fit the maximum permitted reply size - so a result UNDER the row cap can still be over
    // that, and both shapes that say so used to leave here as `BigQueryError` and reach a caller as
    // `503`: the status a dead endpoint produces, inviting a retry that returns the same page.
    //
    // Two shapes, and each is asked of the thing that knows. The page token is a fact about the wire
    // document, so the transport is asked - which is why the two failing transports below are
    // indistinguishable to `BigQueryError::Endpoint` and answer differently.
    let paged = open(Paged, shared_posture());
    let error = paged
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()))
        .expect_err("a paged result is not a result");
    assert!(
        paged.result_did_not_fit(&error),
        "a page of a larger result is a size bound: {error:?}"
    );

    // The control, and the reason this is not a test that says yes to everything: the same variant,
    // an error the adapter cannot tell from the one above, and a transport that does not claim the
    // bound. It must stay a failure.
    let broken = open(Broken, shared_posture());
    let refused = broken
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()))
        .expect_err("the endpoint said no");
    assert!(
        !broken.result_did_not_fit(&refused),
        "an endpoint that refused is not a result too large: {refused:?}"
    );
}

#[test]
fn a_page_shorter_than_the_reported_total_is_a_size_bound_and_a_longer_one_is_not() {
    // The second shape, and this one the ADAPTER decides: a delivered count BELOW the reported total
    // is the same bound reached without a page token, so it is a governance refusal rather than a
    // `503`. `a_result_shorter_than_what_the_endpoint_reported_is_refused` above asserts that it is
    // refused at all; this asserts what a caller is then told it was.
    let warehouse = open(Recording::empty(), shared_posture());
    assert!(
        warehouse.result_did_not_fit(&BigQueryError::Incomplete { delivered: 2, total: 3 }),
        "a partial page is a result too large"
    );

    // And the OTHER side of the same variant is deliberately NOT this bound. More rows delivered than
    // the endpoint says exist is the endpoint contradicting itself - a defect, which a retry may well
    // not repeat - so calling it a governance refusal would tell a caller not to retry the one shape
    // here where retrying could work.
    assert!(
        !warehouse.result_did_not_fit(&BigQueryError::Incomplete { delivered: 3, total: 2 }),
        "an endpoint contradicting itself is not a result too large"
    );
}

#[test]
fn a_schema_this_adapter_cannot_map_is_refused_whatever_the_data_happened_to_be() {
    // **The hole review found, and it was in the comment as well as in the code.** `cell` answers a
    // null BEFORE it reads the column's type, which is right for a null and wrong for the schema: a
    // result with NO rows never reaches `cell` at all, and a result whose unmapped column happens to
    // be entirely null reaches it and is answered. So a `TIMESTAMP` column came back as a successful
    // empty `RowSet`, and whether this adapter maps a type depended on what the data happened to be.
    //
    // Both shapes, because they were reachable for two different reasons.
    let empty = JobRows::of(
        vec![Field::of(String::from("at"), FieldType::Unmapped(String::from("TIMESTAMP")))],
        Vec::new(),
        0,
    );
    match BigQueryWarehouse::<Recording>::rows(&empty).expect_err("a zero-row unmapped schema is refused") {
        BigQueryError::UnmappedType { ref column, ref named } => {
            assert_eq!(column, "at");
            assert_eq!(named, "TIMESTAMP");
        }
        other => panic!("a zero-row unmapped schema was mapped to {other:?}"),
    }

    let all_null = JobRows::of(
        vec![Field::of(String::from("at"), FieldType::Unmapped(String::from("BYTES")))],
        vec![vec![Cell::Null], vec![Cell::Null]],
        2,
    );
    match BigQueryWarehouse::<Recording>::rows(&all_null).expect_err("an all-null unmapped column is refused") {
        BigQueryError::UnmappedType { ref named, .. } => assert_eq!(named, "BYTES"),
        other => panic!("an all-null unmapped column was mapped to {other:?}"),
    }

    // A malformed type name is the same case rather than a third one: an empty `type` decodes to
    // `Unmapped("")`, so it is named as what it is rather than read as a column that answers.
    let malformed = JobRows::of(vec![Field::of(String::from("at"), FieldType::parse(""))], Vec::new(), 0);
    assert!(
        matches!(
            BigQueryWarehouse::<Recording>::rows(&malformed),
            Err(BigQueryError::UnmappedType { .. })
        ),
        "an empty type name was accepted"
    );
}

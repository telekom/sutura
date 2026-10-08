#[cfg(feature = "fixtures")]
use super::duck_types;
use super::{DuckDbError, DuckDbWarehouse, Presented};
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::{Real, RowSet, UnreadableCell, Value};
use sutura_sql::GeneratedQuery;

fn real(value: f64) -> Real {
    Real::parse(value).expect("a test literal is finite")
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

/// A materialisation budget roomy enough for any fixture this module's tests produce. Sized
/// deliberately away from the budget the budgeted-collection cells whip up, so those cells own
/// the bound and this helper only needs to get out of the way.
fn budget() -> sutura_domain::warehouse::ResultBudget {
    sutura_domain::warehouse::ResultBudget::of_bytes(core::num::NonZeroUsize::new(1 << 20).expect("a test budget is positive"))
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

/// `SELECT <expression> AS v` through the driver, read as the boot path reads it.
fn read(expression: &str) -> Result<RowSet, DuckDbError> {
    DuckDbWarehouse::in_memory(source(), shared_posture(), budget())?
        .run(&GeneratedQuery::literal(source(), format!("SELECT {expression} AS v")))
}

/// The one cell `expression` answered.
fn cell_of(expression: &str) -> Value {
    let rows = read(expression).expect(expression);
    let [row] = rows.rows() else {
        panic!("{expression}: one row, got {}", rows.rows().len());
    };
    row.first().cloned().expect("one column")
}

/// The cell the domain's decode refused for `expression`.
fn refused(expression: &str) -> UnreadableCell {
    match read(expression) {
        Err(DuckDbError::Unreadable { cause }) => cause,
        other => panic!("{expression} must be refused by the domain's decode: {other:?}"),
    }
}

#[test]
fn every_type_this_adapter_maps_answers_what_the_engine_answers() {
    // The twin of `every_type_the_interior_maps_answers_what_the_data_source_answers` in
    // `crates/sutura-domain/src/warehouse/arrow/tests.rs`, now through the Arrow type the pinned
    // driver gives each `DuckDB` type rather than a value this adapter mapped itself. Boundary values,
    // so an arm that reached for the wrong width comes back truncated or sign-flipped.
    let cases = [
        ("NULL::INTEGER", Value::Null),
        ("true", Value::Integer(1)),
        ("false", Value::Integer(0)),
        ("(-128)::TINYINT", Value::Integer(-128)),
        ("(-32768)::SMALLINT", Value::Integer(-0x8000)),
        ("2147483647::INTEGER", Value::Integer(0x7FFF_FFFF)),
        ("(-9223372036854775808)::BIGINT", Value::Integer(i64::MIN)),
        ("255::UTINYINT", Value::Integer(255)),
        ("65535::USMALLINT", Value::Integer(0xFFFF)),
        ("4294967295::UINTEGER", Value::Integer(0xFFFF_FFFF)),
        ("42::UBIGINT", Value::Integer(42)),
        (
            "18446744073709551615::UBIGINT",
            Value::Text(String::from("18446744073709551615")),
        ),
        // What a `SUM` over integers comes back as: whole, so an integer where it fits an `i64` and
        // its exact text where it does not, never wrapped.
        ("42::HUGEINT", Value::Integer(42)),
        (
            "99999999999999999999999999999999999999::HUGEINT",
            Value::Text(String::from("99999999999999999999999999999999999999")),
        ),
        ("0.1::DOUBLE", Value::Real(real(0.1))),
        // Zero is finite: the check that refuses `inf` is about a division BY zero.
        ("0.0::DOUBLE", Value::Real(real(0.0))),
        ("123.45::DECIMAL(9,2)", Value::Text(String::from("123.45"))),
        ("42::DECIMAL(2,0)", Value::Integer(42)),
        ("'north'", Value::Text(String::from("north"))),
        ("DATE '2026-06-01'", Value::Text(String::from("2026-06-01"))),
    ];
    for (expression, expected) in cases {
        assert_eq!(cell_of(expression), expected, "{expression}");
    }
}

#[test]
fn a_32_bit_float_is_refused_here_because_it_is_refused_there() {
    // `0.1_f32` as an `f64` renders as `0.10000000149011612`, so a widened `REAL` is a number nobody
    // got wrong failing to match itself. The driver hands `REAL` over as `Float32`, not widened, and
    // the domain's decode refuses it by its column - and a `DOUBLE` still answers, so this is not a
    // cell that would pass with every float refused.
    let refusal = refused("0.1::REAL");
    assert!(
        matches!(refusal, UnreadableCell::UnsupportedType { ref column, ref arrow_type } if column == "v" && arrow_type == "Float32"),
        "{refusal:?}"
    );
    assert_eq!(cell_of("0.1::DOUBLE"), Value::Real(real(0.1)));
}

#[test]
fn a_non_finite_double_is_refused_here_because_it_is_refused_there() {
    // What `zero_denominator: fails` produces here: the numerator is cast to `DOUBLE`, so the
    // division is IEEE and answers `inf` rather than failing. All three of the class.
    for special in ["'inf'::DOUBLE", "'-inf'::DOUBLE", "'nan'::DOUBLE", "CAST(3 AS DOUBLE) / 0"] {
        let refusal = refused(special);
        assert!(
            matches!(refusal, UnreadableCell::NotFinite { ref column, .. } if column == "v"),
            "{special}: {refusal:?}"
        );
    }
}

#[test]
fn a_type_neither_adapter_maps_names_itself_rather_than_being_rendered() {
    // A `Debug` rendering would flow into an answer looking like data. Refused at the schema, so an
    // all-null or empty column of these types is refused too.
    for unmapped in [
        "'\\x00\\x01'::BLOB",
        "TIMESTAMP '2026-06-01 00:00:00'",
        "[1]",
        "NULL::TIMESTAMP",
    ] {
        let refusal = refused(unmapped);
        assert!(
            matches!(refusal, UnreadableCell::UnsupportedType { ref column, .. } if column == "v"),
            "{unmapped}: {refusal:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_database_path_that_is_not_utf8_is_refused_rather_than_converted() {
    // A lossy conversion would open a file the caller did not name.
    use std::os::unix::ffi::OsStrExt as _;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/tmp/not-utf8-\xff.duckdb"));
    let outcome = DuckDbWarehouse::open(source(), shared_posture(), path, budget());
    assert!(
        matches!(outcome, Err(DuckDbError::Open { ref cause, .. }) if cause.status == adbc_core::error::Status::InvalidArguments),
        "{outcome:?}"
    );
}

#[test]
fn the_column_labels_come_from_the_statement_that_answered() {
    // The half of the schema read that is reachable, and the reason the labels are taken from
    // the executed statement at all: a projection is what an answer is read by. `run` is
    // exercised directly with a literal statement, because the labels have to be right before
    // any plan is involved.
    let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture(), budget()).expect("an in-memory database opens");
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
    let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture(), budget()).expect("an in-memory database opens");
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
    let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture(), budget()).expect("an in-memory database opens");
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
fn a_result_that_would_not_fit_the_materialisation_budget_is_refused_at_the_row_that_crosses_it() {
    // A generous budget does not stop at the shape, and a budget designed to be crossed must cross
    // on a batch the adapter has actually read - so the test reads the result with a budget too
    // small to hold one batch, refuses, and checks that the refusal is the budget's own
    // (`OverBudget`) and not a shape or type failure. The consequence on the port is asserted
    // through `result_did_not_fit`, the predicate the router reads to refuse a caller.
    let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture(), budget()).expect("an in-memory database opens");
    let query = GeneratedQuery::literal(
        source(),
        String::from("SELECT 'a fairly long value' AS name UNION ALL SELECT 'another long one' AS name"),
    );
    let full = warehouse.run(&query).expect("a roomy budget reads every row");
    assert_eq!(full.rows().len(), 2, "the statement answers two rows under a generous budget");

    let thin = DuckDbWarehouse::in_memory(
        source(),
        shared_posture(),
        sutura_domain::warehouse::ResultBudget::of_bytes(core::num::NonZeroUsize::new(8).expect("a test budget is positive")),
    )
    .expect("an in-memory database opens");
    let error = thin
        .run(&query)
        .expect_err("the budget is far too small for even one decoded row");
    assert!(
        matches!(error, DuckDbError::OverBudget { .. }),
        "a crossed budget is refused as the materialisation-budget shape, not as anything else: {error:?}"
    );
    assert!(
        <DuckDbWarehouse as sutura_domain::warehouse::Warehouse>::result_did_not_fit(&thin, &error),
        "a result refused for crossing the budget IS the result that did not fit"
    );
}

#[test]
fn a_query_reads_only_its_row_ceiling_witness() {
    // The driver streams one `DuckDB` chunk per batch, so the stop lands on the batch that reaches
    // the ceiling: far fewer than the statement's rows, and never fewer than the ceiling.
    let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture(), budget()).expect("an in-memory database opens");
    let query = GeneratedQuery::literal(source(), String::from("SELECT range AS id FROM range(10000)"));
    assert_eq!(
        warehouse.run(&query).expect("the source produces every row").rows().len(),
        10_000
    );
    let bounded = warehouse
        .answered(&query, Some(2), None)
        .expect("the first chunk fits the byte budget");
    assert!(
        (2..10_000).contains(&bounded.rows()),
        "the adapter stops once the caller can refuse on rows: {} read",
        bounded.rows()
    );
}

#[test]
fn a_statement_still_running_at_its_deadline_is_stopped_and_refused_by_name() {
    use std::time::{Duration, Instant};
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};

    let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture(), budget()).expect("an in-memory database opens");
    let long = GeneratedQuery::literal(
        source(),
        String::from(
            "SELECT count(*) FROM range(100000) a, range(100000) b, range(1000) c WHERE (a.range * b.range + c.range) % 7 = 3",
        ),
    );
    let deadline = Deadline::opened_at(
        Instant::now(),
        Budget::parse(Duration::from_secs(1)).expect("a second is a budget"),
    );
    let started = Instant::now();
    let error = warehouse
        .answered(&long, None, Some(deadline))
        .expect_err("the statement outlives its deadline");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "stopped by the watchdog, not by finishing"
    );
    assert!(warehouse.deadline_exceeded(&error), "{error:?}");
}

/// The screen reads the CTEs of a node it can find all of, and only those: a `cte_map` of any other
/// shape than the one the pinned parser answers is refused unread, so no CTE in it goes unwalked.
#[test]
fn a_cte_map_of_another_shape_is_refused_unread() {
    for cte_map in [r#"{"map": [], "extra": []}"#, r#"{"other": []}"#, r#"{"map": {}}"#, "[]"] {
        let tree = format!(r#"{{"error": false, "statements": [{{"node": {{"type": "SELECT_NODE", "cte_map": {cte_map}}}}}]}}"#);
        let refused = super::screen::screen(&tree);
        assert!(
            matches!(refused, Err(super::NotARead::Unreadable { cause: None })),
            "{cte_map}: {refused:?}"
        );
    }
    let read =
        super::screen::screen(r#"{"error": false, "statements": [{"node": {"type": "SELECT_NODE", "cte_map": {"map": []}}}]}"#);
    assert!(read.is_ok(), "{read:?}");
}

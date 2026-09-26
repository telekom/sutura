use std::time::{Duration, Instant};

use parking_lot::Mutex;
use sutura_domain::identity::{Presented, Secret};
use sutura_domain::plan::Executable;
use sutura_domain::warehouse::ParamValue;
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use super::*;

/// What the next [`ClickHouseTransport::run`] answers, for [`Scripted`].
type ScriptedAnswer = Option<Result<Vec<u8>, ScriptedError>>;

/// A scripted [`ClickHouseTransport`], so this crate's own port behaviour can be driven with no
/// live endpoint. The conformance binding and golden matrix also run against the provisioned tier.
///
/// Calls [`crate::deadline::refuse_if_spent`] before answering, so the deadline guard is held at
/// the transport seam rather than only in [`crate::transport::Http`] - and captures the rendered
/// statement, so a test can assert the dialect the plan was rendered through.
///
/// `parking_lot::Mutex`, per `clippy.toml`'s ban on `std::sync::Mutex` - test code is not exempt
/// from that entry, and `.lock()` here never crosses an `.await` to deadlock across anyway.
struct Scripted {
    /// Taken once, so a test that calls `execute` twice notices if it did.
    next: Mutex<ScriptedAnswer>,
    /// The rendered statement the last `run` was handed, captured so a test can assert the dialect.
    statement: Mutex<Option<String>>,
}

#[derive(Debug, thiserror::Error)]
enum ScriptedError {
    #[error("the script had nothing queued for this call")]
    NothingQueued,
    #[error("the deadline was already spent before a request could be sent")]
    DeadlineSpent,
}

impl Scripted {
    fn answering(body: &str) -> Self {
        Self {
            next: Mutex::new(Some(Ok(body.as_bytes().to_vec()))),
            statement: Mutex::new(None),
        }
    }

    /// The rendered statement the last `run` was handed, or `None` if `run` was never called.
    fn last_statement(&self) -> Option<String> {
        self.statement.lock().clone()
    }
}

impl ClickHouseTransport for Scripted {
    type Error = ScriptedError;

    fn run(&self, statement: &str, _params: &[ParamValue], deadline: Deadline) -> Result<Vec<u8>, Self::Error> {
        crate::deadline::refuse_if_spent(deadline).map_err(|_spent| ScriptedError::DeadlineSpent)?;
        *self.statement.lock() = Some(String::from(statement));
        self.next.lock().take().unwrap_or(Err(ScriptedError::NothingQueued))
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(*error, ScriptedError::DeadlineSpent)
    }
}

fn warehouse(transport: Scripted) -> ClickHouseWarehouse<Scripted> {
    warehouse_with(transport, budget())
}

fn warehouse_with(transport: Scripted, result_budget: sutura_domain::warehouse::ResultBudget) -> ClickHouseWarehouse<Scripted> {
    ClickHouseWarehouse::of(
        sutura_conformance::corpus::source(),
        sutura_conformance::corpus::posture(),
        transport,
        result_budget,
    )
}

/// A materialisation budget roomy enough for any fixture this module's tests produce, sized away
/// from the budget the budgeted-collection cells whip up so those cells own the bound.
fn budget() -> sutura_domain::warehouse::ResultBudget {
    sutura_domain::warehouse::ResultBudget::of_bytes(core::num::NonZeroUsize::new(1 << 20).expect("a test budget is positive"))
}

/// A deadline already spent before the call, for tests that need one refused.
fn spent_deadline() -> Deadline {
    let budget = Budget::parse(Duration::from_millis(1)).expect("a non-zero budget");
    let opened = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("one second ago is representable");
    Deadline::opened_at(opened, budget)
}

/// `execute` round-trips a real corpus plan through the shared JSON decode, over a scripted
/// response - the plumbing this crate exists to prove, with no endpoint.
#[test]
fn execute_decodes_a_names_types_and_rows_response() {
    let case = sutura_conformance::corpus::cases()
        .into_iter()
        .next()
        .expect("the corpus has a case");
    let body = "[\"region\",\"total\"]\n[\"String\",\"Int64\"]\n[\"north\",7]\n";
    let warehouse = warehouse(Scripted::answering(body));
    let rows = warehouse
        .execute(
            Executable::Query(case.plan()),
            &sutura_conformance::corpus::presented(),
            sutura_conformance::corpus::deadline(),
        )
        .expect("a well-formed response decodes");
    assert_eq!(rows.rows(), 1);
}

/// A spent deadline is refused before any round trip, held at the transport seam and not only in
/// the `deadline` helper. `Scripted::run` calls `refuse_if_spent` - the same guard
/// `crate::transport::Http::run` calls before any request - so a deadline with nothing left never
/// reaches a scripted answer. The error comes back through the port as `ClickHouseError::Endpoint`,
/// and `deadline_exceeded` reads it as the spent-deadline shape.
#[test]
fn a_spent_deadline_is_refused_before_any_round_trip() {
    let case = sutura_conformance::corpus::cases()
        .into_iter()
        .next()
        .expect("the corpus has a case");
    let warehouse = warehouse(Scripted::answering("[]\n[]\n"));
    let error = warehouse
        .execute(
            Executable::Query(case.plan()),
            &sutura_conformance::corpus::presented(),
            spent_deadline(),
        )
        .expect_err("a spent deadline is refused before any round trip");
    assert!(matches!(error, ClickHouseError::Endpoint { .. }), "{error}");
    assert!(
        warehouse.deadline_exceeded(&error),
        "the transport should classify a spent deadline"
    );
}

/// The clickhouse half of the byte budget the row-speaking adapters share. A result the budget
/// cannot hold must be refused by the DECODE at the row that crosses it - via `rows_from_json`,
/// not by the transport, so the refusal is a materialisation-budget shape the port's
/// `result_did_not_fit` reads - and the same body must decode fully under a roomy budget, or the
/// test would be passing against an adapter that refuses everything.
#[test]
fn a_result_that_would_not_fit_the_materialisation_budget_is_refused_at_the_row_that_crosses_it() {
    let body = "[\"region\",\"total\"]\n[\"String\",\"Int64\"]\n[\"north\",7]\n[\"south\",9]\n";
    let first_row_bytes = (core::mem::size_of::<Vec<Value>>() + 2 * core::mem::size_of::<Value>() + 6) * 3;
    let roomy = warehouse(Scripted::answering(body));
    let thin = warehouse_with(
        Scripted::answering(&format!("{body}not-json\n")),
        sutura_domain::warehouse::ResultBudget::of_bytes(
            core::num::NonZeroUsize::new(first_row_bytes.saturating_add(1)).expect("a test budget is positive"),
        ),
    );
    let case = sutura_conformance::corpus::cases()
        .into_iter()
        .next()
        .expect("the corpus has a case");
    let presented = sutura_conformance::corpus::presented();
    let deadline = sutura_conformance::corpus::deadline();

    roomy
        .execute(Executable::Query(case.plan()), &presented, deadline)
        .expect("a roomy budget decodes the whole body");

    let error = thin
        .execute(Executable::Query(case.plan()), &presented, deadline)
        .expect_err("the second row crosses the budget before the malformed third row is decoded");
    assert!(
        matches!(error, ClickHouseError::OverBudget { .. }),
        "a crossed budget is refused as the materialisation-budget shape, not as anything else: {error:?}"
    );
    assert!(
        thin.result_did_not_fit(&error),
        "a result refused for crossing the budget IS the result that did not fit"
    );
}

#[test]
fn a_query_decodes_only_the_rows_needed_to_witness_its_ceiling() {
    let body = b"[\"region\"]\n[\"String\"]\n[\"north\"]\n[\"south\"]\nnot-json\n";
    let rows = rows_from_json_with_limit::<ScriptedError>(body, budget(), Some(2))
        .expect("the third row is not decoded once two rows prove the ceiling was crossed");
    assert_eq!(rows.rows().len(), 2);
}

/// The rendered SQL is the `ClickHouse` spelling, not merely that the fake answered. `Scripted`
/// captures the statement `run` was handed; this cell asserts what the adapter rendered through
/// `Dialect::ClickHouse` rather than just that a scripted body came back.
///
/// The mutation this kills: swapping `Dialect::ClickHouse` to `Dialect::Postgres` at the three
/// render call sites in `lib.rs`. The two dialects render the corpus plan's filters with
/// different placeholder syntax (`?` vs `$1`), so the captured statement changes and this cell
/// fails.
/// What this discriminates is `PlaceholderStyle`, not the dialect: `DuckDb` and `BigQuery` also
/// render `?` (`PlaceholderStyle::Question`), so swapping to either leaves this cell green.
#[test]
fn execute_renders_the_plan_through_the_clickhouse_dialect() {
    let case = sutura_conformance::corpus::cases()
        .into_iter()
        .next()
        .expect("the corpus has a case");
    let transport = Scripted::answering("[\"region\",\"total\"]\n[\"String\",\"Int64\"]\n[\"north\",7]\n");
    let warehouse = warehouse(transport);
    warehouse
        .execute(
            Executable::Query(case.plan()),
            &sutura_conformance::corpus::presented(),
            sutura_conformance::corpus::deadline(),
        )
        .expect("a well-formed response decodes");
    // `tests` is a child module of `lib`, so it can read the private `transport` field.
    let rendered = warehouse
        .transport
        .last_statement()
        .expect("the transport was called and captured the statement");
    // ClickHouse renders `?` placeholders (PlaceholderStyle::Question); Postgres renders `$1`.
    // The corpus plan binds two date filters, so the rendered SQL carries `?`.
    assert!(
        rendered.contains('?'),
        "ClickHouse rendering uses `?` placeholders, and this SQL does not: {rendered}",
    );
    assert!(
        !rendered.contains("$1"),
        "a `$1` placeholder is the Postgres spelling, not ClickHouse: {rendered}",
    );
}

/// **The refusal, not just the predicate.** `deliverable` reads `presented`'s variant before
/// anything else runs; neutralising the read (`&& false`) would leave this branch unreachable and
/// the field unread, which is exactly the class of defect `AGENTS.md` asks a refusal test to catch
/// rather than a `dead_code` warning.
#[test]
fn a_subject_credential_is_refused_as_no_place_to_arrive() {
    let case = sutura_conformance::corpus::cases()
        .into_iter()
        .next()
        .expect("the corpus has a case");
    let warehouse = warehouse(Scripted::answering("[]\n[]\n"));
    let presented = Presented::SubjectToken {
        material: Secret::new(String::from("unused")),
        impersonate: None,
    };
    let error = warehouse
        .execute(
            Executable::Query(case.plan()),
            &presented,
            sutura_conformance::corpus::deadline(),
        )
        .expect_err("a subject credential has nowhere to arrive on this adapter");
    assert!(matches!(error, ClickHouseError::NoPlaceForASubject { .. }), "{error}");
}

/// `Warehouse::EXECUTES_LEGS` stays at its domain default, so a leg needs a combiner this adapter
/// does not have.
#[test]
fn a_leg_is_refused_without_a_combiner() {
    let leg_case = sutura_conformance::corpus::leg_case();
    let warehouse = warehouse(Scripted::answering("[]\n[]\n"));
    let error = warehouse
        .execute(
            Executable::Leg(leg_case.leg()),
            &sutura_conformance::corpus::presented(),
            sutura_conformance::corpus::deadline(),
        )
        .expect_err("a leg with no combiner above this adapter is refused");
    assert!(matches!(error, ClickHouseError::LegWithoutCombiner { .. }), "{error}");
}

#[test]
fn a_nullable_column_reads_a_null_cell_as_value_null() {
    let body = "[\"maybe\"]\n[\"Nullable(String)\"]\n[null]\n";
    let rows: RowSet = rows_from_json::<ScriptedError>(body.as_bytes(), budget()).expect("a null cell decodes");
    assert_eq!(rows.rows()[0][0], Value::Null);
}

#[test]
fn a_uint64_rendered_as_a_json_string_still_decodes_as_an_integer() {
    let body = "[\"n\"]\n[\"UInt64\"]\n[\"9223372036854775807\"]\n";
    let rows: RowSet = rows_from_json::<ScriptedError>(body.as_bytes(), budget()).expect("a quoted 64-bit integer decodes");
    assert_eq!(rows.rows()[0][0], Value::Integer(i64::MAX));
}

#[test]
fn a_uint64_past_i64_max_decodes_as_its_exact_digits() {
    let body = "[\"n\"]\n[\"UInt64\"]\n[\"10000000000000000006\"]\n";
    let rows: RowSet = rows_from_json::<ScriptedError>(body.as_bytes(), budget()).expect("a wide unsigned integer decodes");
    assert_eq!(rows.rows()[0][0], Value::Text(String::from("10000000000000000006")));
}

#[test]
fn a_uint128_above_i128_max_decodes_as_its_exact_digits() {
    let body = "[\"n\"]\n[\"UInt128\"]\n[\"340282366920938463463374607431768211455\"]\n";
    let rows: RowSet = rows_from_json::<ScriptedError>(body.as_bytes(), budget()).expect("a UInt128 decodes");
    assert_eq!(
        rows.rows()[0][0],
        Value::Text(String::from("340282366920938463463374607431768211455"))
    );
}

#[test]
fn a_clickhouse_sum_keeps_wide_integers_decimal_scale_and_float_type() {
    let body = "[\"total\"]\n[\"Tuple(String, Dynamic)\"]\n[[\"Int128\",\"9223372036854775808\"]]\n[[\"Int128\",\"18446744073709551616\"]]\n[[\"Decimal(38, 2)\",\"11.50\"]]\n[[\"Nullable(Decimal(38, 2))\",\"19.50\"]]\n[[\"Float64\",3]]\n[[\"Nullable(Float64)\",4]]\n";
    let rows: RowSet = rows_from_json::<ScriptedError>(body.as_bytes(), budget()).expect("typed sums decode");
    assert_eq!(rows.rows()[0][0], Value::Text(String::from("9223372036854775808")));
    assert_eq!(rows.rows()[1][0], Value::Text(String::from("18446744073709551616")));
    assert_eq!(rows.rows()[2][0], Value::Text(String::from("11.50")));
    assert_eq!(rows.rows()[3][0], Value::Text(String::from("19.50")));
    assert_eq!(rows.rows()[4][0], Value::Real(Real::parse(3.0).expect("finite")));
    assert_eq!(rows.rows()[5][0], Value::Real(Real::parse(4.0).expect("finite")));
}

#[test]
fn a_float_column_that_is_not_finite_is_refused_rather_than_rendered() {
    let body = "[\"ratio\"]\n[\"Float64\"]\n[\"nan\"]\n";
    let error = rows_from_json::<ScriptedError>(body.as_bytes(), budget()).expect_err("NaN is not a finite cell");
    assert!(matches!(error, ClickHouseError::NotFinite { .. }), "{error}");
}

#[test]
fn an_integral_decimal_maps_to_an_integer_cell() {
    assert_eq!(decimal_value(&serde_json::json!("300")), Some(Value::Integer(300)));
    assert_eq!(decimal_value(&serde_json::json!(300)), Some(Value::Integer(300)));
}

#[test]
fn a_fractional_decimal_keeps_its_exact_text() {
    assert_eq!(
        decimal_value(&serde_json::json!("12.4500")),
        Some(Value::Text(String::from("12.4500")))
    );
}

#[test]
fn an_unmapped_type_is_an_error_naming_it() {
    let body = "[\"weird\"]\n[\"Tuple(Int64, Int64)\"]\n[[1, 2]]\n";
    let error = rows_from_json::<ScriptedError>(body.as_bytes(), budget()).expect_err("an unmapped type is refused");
    assert!(matches!(error, ClickHouseError::UnsupportedType { .. }), "{error}");
}

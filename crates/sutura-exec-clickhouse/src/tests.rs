use parking_lot::Mutex;
use sutura_domain::identity::{Presented, Secret};
use sutura_domain::plan::Executable;
use sutura_domain::warehouse::ParamValue;

use super::*;

/// What the next [`ClickHouseTransport::run`] answers, for [`Scripted`].
type ScriptedAnswer = Option<Result<Vec<u8>, ScriptedError>>;

/// A scripted [`ClickHouseTransport`], so this crate's own port behaviour can be driven with no
/// live endpoint - the unit-level sibling of `tests/conformance.rs`'s canned pack binding.
///
/// `parking_lot::Mutex`, per `clippy.toml`'s ban on `std::sync::Mutex` - test code is not exempt
/// from that entry, and `.lock()` here never crosses an `.await` to deadlock across anyway.
struct Scripted {
    /// Taken once, so a test that calls `execute` twice notices if it did.
    next: Mutex<ScriptedAnswer>,
}

#[derive(Debug, thiserror::Error)]
#[error("the script had nothing queued for this call")]
struct ScriptedError;

impl Scripted {
    fn answering(body: &str) -> Self {
        Self {
            next: Mutex::new(Some(Ok(body.as_bytes().to_vec()))),
        }
    }
}

impl ClickHouseTransport for Scripted {
    type Error = ScriptedError;

    fn run(&self, _statement: &str, _params: &[ParamValue], _deadline: Deadline) -> Result<Vec<u8>, Self::Error> {
        self.next.lock().take().unwrap_or(Err(ScriptedError))
    }
}

fn warehouse(transport: Scripted) -> ClickHouseWarehouse<Scripted> {
    ClickHouseWarehouse::of(
        sutura_conformance::corpus::source(),
        sutura_conformance::corpus::posture(),
        transport,
    )
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
    assert_eq!(rows.rows().len(), 1);
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
    let rows: RowSet = rows_from_json::<ScriptedError>(body.as_bytes()).expect("a null cell decodes");
    assert_eq!(rows.rows()[0][0], Value::Null);
}

#[test]
fn a_uint64_rendered_as_a_json_string_still_decodes_as_an_integer() {
    let body = "[\"n\"]\n[\"UInt64\"]\n[\"9223372036854775807\"]\n";
    let rows: RowSet = rows_from_json::<ScriptedError>(body.as_bytes()).expect("a quoted 64-bit integer decodes");
    assert_eq!(rows.rows()[0][0], Value::Integer(i64::MAX));
}

#[test]
fn a_float_column_that_is_not_finite_is_refused_rather_than_rendered() {
    let body = "[\"ratio\"]\n[\"Float64\"]\n[\"nan\"]\n";
    let error = rows_from_json::<ScriptedError>(body.as_bytes()).expect_err("NaN is not a finite cell");
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
    let error = rows_from_json::<ScriptedError>(body.as_bytes()).expect_err("an unmapped type is refused");
    assert!(matches!(error, ClickHouseError::UnsupportedType { .. }), "{error}");
}

//! The cells of `agree`, written with the behaviour they hold.

use super::*;

fn answer(columns: &[&str], rows: &[&[&str]]) -> String {
    serde_json::json!({
        "outcome": "answer",
        "provenance": {},
        "executed_as": [],
        "columns": columns,
        "rows": rows,
    })
    .to_string()
}

fn rows_of(text: &str, real: &[&str]) -> RowSet {
    let real: Vec<String> = real.iter().map(|label| String::from(*label)).collect();
    answer_rows(Path::new("answer.json"), text, &real).expect("an answer body")
}

fn judge(left: &str, right: &str, real: &[&str], ordered: bool) -> Result<Agreed, AgreeError> {
    agree(&rows_of(left, real), &rows_of(right, real), ordered)
}

#[test]
fn identical_answers_agree_on_content_and_on_order() {
    let both = answer(&["region", "orders"], &[&["north", "10"], &["south", "20"]]);
    assert!(matches!(
        judge(&both, &both, &[], true),
        Ok(Agreed { rows: 2, ordered: true })
    ));
}

#[test]
fn a_cell_spelled_as_a_number_on_one_side_and_a_decimal_on_the_other_is_a_variant_difference() {
    let counted = answer(&["lines"], &[&["5"]]);
    let scaled = answer(&["lines"], &[&["5.00"]]);
    assert!(matches!(
        judge(&counted, &scaled, &[], false),
        Err(AgreeError::Content(ContentDisagreement::Multiplicity {
            left: 1,
            right: 0,
            ..
        }))
    ));
}

#[test]
fn a_row_answered_twice_on_one_side_is_a_multiplicity_disagreement() {
    let twice = answer(&["region"], &[&["north"], &["north"]]);
    let once = answer(&["region"], &[&["north"]]);
    assert!(matches!(
        judge(&twice, &once, &[], false),
        Err(AgreeError::Content(ContentDisagreement::Multiplicity {
            left: 2,
            right: 1,
            ..
        }))
    ));
}

#[test]
fn a_last_place_difference_in_a_declared_real_column_agrees() {
    let summed = 1.0_f64 / 3.0;
    let other_way = f64::from_bits(summed.to_bits().saturating_add(1));
    assert_ne!(summed.to_bits(), other_way.to_bits());
    let engine = answer(&["mean"], &[&[summed.to_string().as_str()]]);
    let source = answer(&["mean"], &[&[other_way.to_string().as_str()]]);
    assert!(matches!(judge(&engine, &source, &["mean"], false), Ok(Agreed { .. })));
}

#[test]
fn a_coarser_difference_in_a_declared_real_column_disagrees() {
    let engine = answer(&["mean"], &[&["0.3333333333333333"]]);
    let source = answer(&["mean"], &[&["0.333334"]]);
    assert!(matches!(
        judge(&engine, &source, &["mean"], false),
        Err(AgreeError::Content(ContentDisagreement::Multiplicity { .. }))
    ));
}

#[test]
fn a_last_place_difference_in_an_undeclared_column_is_compared_exactly() {
    let summed = 1.0_f64 / 3.0;
    let other_way = f64::from_bits(summed.to_bits().saturating_add(1));
    let engine = answer(&["mean"], &[&[summed.to_string().as_str()]]);
    let source = answer(&["mean"], &[&[other_way.to_string().as_str()]]);
    assert!(matches!(
        judge(&engine, &source, &[], false),
        Err(AgreeError::Content(ContentDisagreement::Multiplicity { .. }))
    ));
}

#[test]
fn a_whole_number_in_a_declared_real_column_is_a_real() {
    let whole = answer(&["mean"], &[&["12"]]);
    let fractional = answer(&["mean"], &[&["12.0000000000001"]]);
    assert!(matches!(judge(&whole, &fractional, &["mean"], false), Ok(Agreed { .. })));
    assert!(matches!(judge(&whole, &fractional, &[], false), Err(AgreeError::Content(_))));
}

#[test]
fn the_same_rows_in_another_order_agree_unless_an_order_was_asked_for() {
    let forward = answer(&["region"], &[&["north"], &["south"]]);
    let reversed = answer(&["region"], &[&["south"], &["north"]]);
    assert!(matches!(judge(&forward, &reversed, &[], false), Ok(Agreed { .. })));
    assert!(matches!(
        judge(&forward, &reversed, &[], true),
        Err(AgreeError::Order(OrderDisagreement::Cell { row: 0, .. }))
    ));
}

#[test]
fn a_wrong_number_is_reported_as_content_even_when_an_order_was_asked_for() {
    let engine = answer(&["region", "orders"], &[&["north", "10"]]);
    let source = answer(&["region", "orders"], &[&["north", "11"]]);
    assert!(matches!(judge(&engine, &source, &[], true), Err(AgreeError::Content(_))));
}

#[test]
fn two_answers_that_label_their_columns_differently_disagree() {
    let engine = answer(&["region"], &[&["north"]]);
    let source = answer(&["Region"], &[&["north"]]);
    assert!(matches!(
        judge(&engine, &source, &[], false),
        Err(AgreeError::Content(ContentDisagreement::Columns { .. }))
    ));
}

#[test]
fn a_null_cell_agrees_with_a_null_cell_and_a_declared_real_column_may_hold_one() {
    let engine = answer(&["mean"], &[&["null"], &["1.5"]]);
    assert!(matches!(judge(&engine, &engine, &["mean"], true), Ok(Agreed { .. })));
    assert!(matches!(judge(&engine, &engine, &[], true), Ok(Agreed { .. })));
}

#[test]
fn a_refusal_is_no_answer_and_names_its_code() {
    let refusal = serde_json::json!({
        "outcome": "refusal",
        "reason": {"code": "result_too_large", "detail": "x"},
    })
    .to_string();
    let error = answer_rows(Path::new("one.json"), &refusal, &[]).expect_err("a refusal");
    assert!(matches!(error, AgreeError::Refused { ref code, .. } if code == "result_too_large"));
}

#[test]
fn a_body_that_is_not_an_answer_is_refused_by_what_is_missing() {
    for (text, wanted) in [
        (r#"{"outcome":"answer","rows":[]}"#, "no `columns` array"),
        (r#"{"outcome":"answer","columns":[]}"#, "no `rows` array"),
        (r#"{"columns":[],"rows":[]}"#, "`outcome` is neither"),
        (r#"{"outcome":"answer","columns":["a"],"rows":[[1]]}"#, "a cell is not text"),
    ] {
        let error = answer_rows(Path::new("x.json"), text, &[]).expect_err(text);
        assert!(error.to_string().contains(wanted), "{error} should name {wanted}");
    }
    assert!(matches!(
        answer_rows(Path::new("x.json"), "not json", &[]),
        Err(AgreeError::NotJson { .. })
    ));
}

#[test]
fn a_ragged_answer_is_refused() {
    let ragged = answer(&["a", "b"], &[&["1"]]);
    assert!(matches!(
        answer_rows(Path::new("x.json"), &ragged, &[]),
        Err(AgreeError::Ragged { .. })
    ));
}

#[test]
fn a_real_declaration_the_answer_cannot_honour_is_refused_rather_than_skipped() {
    let body = answer(&["mean", "name"], &[&["1.5", "north"]]);
    assert!(matches!(
        answer_rows(Path::new("x.json"), &body, &[String::from("avg")]),
        Err(AgreeError::NoSuchRealColumn { ref label, .. }) if label == "avg"
    ));
    for text in ["north", "inf", "NaN"] {
        let body = answer(&["mean"], &[&[text]]);
        assert!(
            matches!(
                answer_rows(Path::new("x.json"), &body, &[String::from("mean")]),
                Err(AgreeError::NotAReal { row: 0, .. })
            ),
            "{text} is no real number"
        );
    }
}

#[test]
fn the_command_line_is_two_files_and_known_flags() {
    let args = |parts: &[&str]| -> Vec<String> { parts.iter().map(|part| String::from(*part)).collect() };
    assert_eq!(
        Arguments::parse(&args(&["a.json", "--ordered", "b.json", "--real", "mean"])).expect("valid"),
        Arguments {
            left: PathBuf::from("a.json"),
            right: PathBuf::from("b.json"),
            ordered: true,
            real: vec![String::from("mean")],
        }
    );
    for bad in [
        &["a.json"][..],
        &["a.json", "b.json", "c.json"],
        &["a.json", "b.json", "--real"],
        &["a.json", "b.json", "--unordered"],
    ] {
        let error = Arguments::parse(&args(bad)).expect_err("malformed");
        assert!(error.is_usage(), "{bad:?}");
    }
}

#[test]
fn run_reads_two_files_and_names_an_unreadable_one() {
    let dir = std::env::temp_dir().join(format!("sutura-agree-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let (one, other, missing) = (dir.join("one.json"), dir.join("other.json"), dir.join("absent.json"));
    std::fs::write(&one, answer(&["n"], &[&["1"]])).expect("write");
    std::fs::write(&other, answer(&["n"], &[&["2"]])).expect("write");
    let path = |p: &Path| p.to_string_lossy().into_owned();

    let same = run(&[path(&one), path(&one)]);
    let differ = run(&[path(&one), path(&other)]);
    let absent = run(&[path(&one), path(&missing)]);
    drop(std::fs::remove_dir_all(&dir));

    assert!(matches!(same, Ok(Agreed { rows: 1, ordered: false })));
    assert!(matches!(differ, Err(AgreeError::Content(_))));
    assert!(matches!(absent, Err(AgreeError::Unreadable { .. })));
}

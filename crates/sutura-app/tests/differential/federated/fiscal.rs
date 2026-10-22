//! **A crossing key truncated to a grain, executed**: a fact whose day truncates to a month a lookup
//! on a second data system holds (`telekom/sutura#1489`).
//!
//! The fact leg truncates the link at its own source, with the function a single-source join renders,
//! and the lookup leg projects its month as stored. The corpus is derived beside
//! [`super::corpus::derived`] under its own stem rather than as entries in `CATALOG_CASES`, so no
//! other cell reads these tables or documents.
//!
//! **What this does NOT establish.** Dates only: no cell here holds a timestamp or a session zone
//! that is not UTC, so a boundary row of those is held by the rendered statement alone. Every leg
//! runs under one operating-system identity, as in the rest of `federated.rs`.

use std::path::Path;

use sutura_domain::pinned::view::ScopedView;
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content};
use sutura_domain::warehouse::{Real, RowSet, Value};
use sutura_semantic::{Compiled, compile};

use super::corpus::{TWO_FACT_BY_QUARTER, with_fiscal_months};
use super::harness::{
    Attempted, Side, answered, bundle, validating_on_one_source, validating_on_two_engines, validating_on_two_sources,
};

const QUESTION: &str = "metrics: [voice_by_fiscal_quarter]\ngrain: day\nrange:\n  start: 2026-06-30\n  end: 2026-07-02\ndimensions: [fiscal_quarter]\n";

pub(super) fn side<W>((bundle, warehouses): Attempted<W>) -> Side<W> {
    Side {
        bundle: bundle.expect("the anchors and the declarations hold"),
        warehouses,
    }
}

/// The rows one side answers, or a panic naming the outcome it gave instead.
pub(super) fn rows<W>(side: &Side<W>, question: &str, name: &str) -> RowSet
where
    W: sutura_domain::warehouse::Warehouse + Sync,
    W::Error: Send,
{
    let query: Query = serde_norway::from_str(question).expect("a fixture question is a question");
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    match answered(side, &query, name, &combiner).unwrap_or_else(|e| panic!("{e}")) {
        ToolOutcome::Answer { rows, .. } => rows,
        other @ ToolOutcome::Refusal { .. } => panic!("{name}: the question is answered, not {other:?}"),
    }
}

/// The rows a metric over [`USAGE_ROWS`] answers: each quarter's one day, summed by hand.
fn expected_usage() -> RowSet {
    let real = |figure: f64| Value::Real(Real::parse(figure).expect("a hand-worked sum is finite"));
    answer_rows(
        "voice_by_fiscal_quarter",
        vec![
            vec![Value::Text("FY27-Q1".into()), Value::Text("2026-07-01".into()), real(4.0)],
            vec![Value::Text("FY26-Q4".into()), Value::Text("2026-06-30".into()), real(10.0)],
        ],
    )
}

/// Tickets over subscription-months by fiscal quarter, worked by hand: `fct_subscription_monthly`
/// holds 62 rows in each of May and June, and [`TICKET_ROWS`] 4 + 3 tickets in May and 2 + 3 in June.
pub(super) fn expected_tickets() -> RowSet {
    let real = |figure: f64| Value::Real(Real::parse(figure).expect("a hand-worked ratio is finite"));
    answer_rows(
        "tickets_per_subscription_by_fiscal_quarter",
        vec![
            vec![
                Value::Text("FY26-Q4".into()),
                Value::Text("2026-05-01".into()),
                real(0.112_903_225_806_451_6),
            ],
            vec![
                Value::Text("FY26-Q4".into()),
                Value::Text("2026-06-01".into()),
                real(0.080_645_161_290_322_58),
            ],
        ],
    )
}

fn answer_rows(metric: &str, rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(
        vec![
            String::from("fiscal_quarter"),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            String::from(metric),
        ],
        rows,
    )
    .expect("the hand-worked answer is rectangular")
}

fn compiled(catalog: &Path) -> Compiled {
    let query: Query = serde_norway::from_str(QUESTION).expect("the question is a question");
    compile(&query, &ScopedView::everything(&bundle(catalog)), RowCeiling::DEFAULT).expect("the derived catalog is consistent")
}

/// **A fact whose day truncates to a month answers what one source answers.**
///
/// The one-source side joins `daily_usage` to the lookup with the truncation in the `JOIN`; the
/// two-source side runs two legs, truncating at the fact's own source, and links on the month. A
/// link on the raw day matches no lookup row, so every row would come back with a null quarter. Both
/// are held to the quarters' totals worked from [`USAGE_ROWS`], which no code under test produced.
#[test]
fn a_crossing_key_truncated_to_a_month_answers_what_one_source_answers() {
    let corpus = with_fiscal_months("federated-differential-fiscal-months");
    assert!(
        matches!(compiled(&corpus.one_source), Compiled::Planned { .. }),
        "with the months beside the facts the question is one statement"
    );
    assert!(
        matches!(compiled(&corpus.two_source), Compiled::Federated { .. }),
        "with the months on a second data system the question is two legs"
    );
    let name = "a-truncated-crossing-key";
    let one = rows(
        &side(validating_on_one_source(&corpus.data, bundle(&corpus.one_source))),
        QUESTION,
        name,
    );
    let engines = rows(
        &side(validating_on_two_engines(&corpus.data, bundle(&corpus.two_source))),
        QUESTION,
        name,
    );
    let duckdbs = rows(
        &side(validating_on_two_sources(&corpus.data, bundle(&corpus.two_source))),
        QUESTION,
        name,
    );
    for (side, answer) in [("one source", &one), ("two engines", &engines), ("two DuckDB", &duckdbs)] {
        if let Err(d) = agree_on_content(&expected_usage(), answer, RealTolerance::DIFFERENTIAL) {
            panic!("{name}: {side} did not answer the hand-worked quarters - {d}\n{answer:?}");
        }
    }
}

/// **A truncated key against a lookup whose month is text is refused before either leg runs**, and
/// the same question over the unedited catalog federates.
///
/// The fact leg truncates to a date, so a text month can never match it. The planner reads that from
/// the declared types: no leg has run, and the data is the same dates.
#[test]
fn a_truncated_crossing_key_against_a_text_target_is_refused_before_either_leg_runs() {
    let corpus = with_fiscal_months("federated-differential-fiscal-months-text");
    assert!(
        matches!(compiled(&corpus.two_source), Compiled::Federated { .. }),
        "the control declares the month as a date and must federate"
    );
    let months = corpus.two_source.join("models/fiscal_months.md");
    let text = std::fs::read_to_string(&months).expect("the derived catalog has the lookup model");
    let declared = "  - name: month\n    type: DATE";
    assert_eq!(
        text.matches(declared).count(),
        1,
        "fiscal_months no longer declares its month as a DATE exactly once, so this case would derive nothing"
    );
    std::fs::write(&months, text.replace(declared, "  - name: month\n    type: VARCHAR"))
        .expect("the derived catalog is writable");
    let Compiled::Refused { reason } = compiled(&corpus.two_source) else {
        panic!("a date against a text month is refused, not planned");
    };
    assert_eq!(reason.code(), "federation_link_type_mismatch", "{reason:?}");
    assert!(format!("{reason:?}").contains("DATE"), "{reason:?}");
}

/// **A second fact whose link differs from the first fact's in grain, target column or key set is
/// refused, never answered.**
///
/// A date meets a date whatever the grain or the lookup column, so no type check can tell the links
/// apart: a fact linking on months beside one linking on raw days, on another date column of the
/// lookup, or on two keys would meet the lookup on different values and the ratio would divide
/// unrelated figures. The control is the corpus as derived, where both truncate to the month and the
/// ratio is planned.
#[test]
fn a_second_fact_linking_at_another_grain_than_the_first_is_refused_before_any_leg_runs() {
    let query: Query = serde_norway::from_str(TWO_FACT_BY_QUARTER).expect("the question is a question");
    let compiled = |catalog: &Path| {
        compile(&query, &ScopedView::everything(&bundle(catalog)), RowCeiling::DEFAULT)
            .expect("the derived catalog is consistent")
    };
    let control = with_fiscal_months("federated-differential-fiscal-grain-control");
    assert!(
        matches!(compiled(&control.two_source), Compiled::Federated { .. }),
        "the control truncates both links to the month and must federate"
    );
    let cases = [
        ("first-plain", "subscription_fiscal_month", "grain: month, ", ""),
        ("second-plain", "ticket_fiscal_month", "grain: month, ", ""),
        ("second-yearly", "ticket_fiscal_month", "grain: month", "grain: year"),
        (
            "second-other-target",
            "ticket_fiscal_month",
            "target: month",
            "target: fiscal_quarter",
        ),
        (
            "second-two-keys",
            "ticket_fiscal_month",
            "target: month }\n",
            "target: month }\n  - { origin: opened_on, target: fiscal_quarter }\n",
        ),
    ];
    for (stem, relationship, find, with) in cases {
        let corpus = with_fiscal_months(&format!("federated-differential-fiscal-grain-{stem}"));
        let document = corpus.two_source.join(format!("relationships/{relationship}.md"));
        let text = std::fs::read_to_string(&document).expect("the derived catalog has the relationship");
        assert_eq!(text.matches(find).count(), 1, "{relationship} no longer holds one {find:?}");
        std::fs::write(&document, text.replace(find, with)).expect("the derived catalog is writable");
        let Compiled::Refused { reason } = compiled(&corpus.two_source) else {
            panic!("{stem}: a second fact linking at another grain is refused, not planned");
        };
        assert_eq!(
            reason.code(),
            "cross_model_ratio_without_shared_dimension",
            "{stem}: {reason:?}"
        );
    }
}

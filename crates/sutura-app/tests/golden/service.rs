//! Off every axis: what the service and the compiler decide, asserted against a fake.
//!
//! What is here is decided ABOVE the port, so a matrix over adapters would be the same assertion
//! run twice. The instrument is a fake: the port is a Rust trait, so the honest stand-in is a type
//! that implements it, and a test asserting on the text of an HTTP request would prove something
//! about the test.
use sutura_domain::query::{MAX_RANGE_DAYS, Query, RefusalReason};
use sutura_semantic::compile;

use crate::shared::{PROVOKED, question, settings};

#[test]
fn a_plan_that_would_reach_a_second_data_system_is_refused() {
    // Not reachable from a question file: it needs a catalog whose models sit on two data systems,
    // which `SourceUnavailable` and this variant are the only defences against. Built in code
    // rather than as a fixture, because a fixture catalog with a second source would make every
    // other test in the suite span two - which is also why it is not a registry entry.
    //
    // The refusal exists because a second data system is a second identity to satisfy, and a plan
    // that runs partly as somebody else is the failure the whole design is arranged against.
    use sutura_domain::pinned::SemanticCatalog as _;

    let split = crate::support::two_source_catalog()
        .load()
        .expect("a two-source catalog can be built");
    let asked = Query::new(
        sutura_domain::model::MetricName::parse("revenue").expect("a name"),
        sutura_domain::model::Grain::Month,
        crate::support::june_range(),
        vec![sutura_domain::model::DimensionName::parse("region").expect("a name")],
        Vec::new(),
    );
    let compiled = compile(&asked, &split).expect("this is a refusal");
    assert!(
        matches!(compiled.refusal(), Some(&RefusalReason::PlanSpansTwoSources { sources: 2 })),
        "expected a two-source refusal, got {:?}",
        compiled.refusal()
    );
}

#[test]
fn a_plan_for_a_data_system_this_process_did_not_open_is_refused() {
    // The service checks the plan's source against the adapter it is about to call. Without it, a
    // question would be answered against whatever happened to be connected, under provenance that
    // named something else. A fake claiming to be somewhere else is the whole instrument: no
    // registered adapter can be asked to lie about its own name, and none should be able to.
    let validated = crate::support::validated_bundle(crate::adapters::load::<crate::adapters::ReferenceCatalog>());
    let elsewhere = crate::support::RecordingWarehouse::pretending_to_be("somewhere_else");
    let outcome =
        sutura_app::answer(&validated, &question("revenue-total-june.yaml"), &elsewhere).expect("a refusal is not an error");
    assert!(
        matches!(outcome.refusal(), Some(&RefusalReason::SourceUnavailable { .. })),
        "expected a source refusal, got {outcome:?}"
    );
    assert!(
        elsewhere.asked_about().is_empty(),
        "a refused question must not have reached the data system"
    );
}

#[test]
fn a_refused_question_never_reaches_the_data_system() {
    // The property that makes a refusal worth having: it is decided before anything runs, so a
    // question that may not be asked costs nothing and reads nothing. Asserted with the recording
    // fake rather than per data system, because "was it reached at all" is a question only a fake
    // can answer - and the answer is a property of `sutura_app::answer`, not of an adapter.
    let validated = crate::support::validated_bundle(crate::adapters::load::<crate::adapters::ReferenceCatalog>());
    // A SECOND warehouse, and a fresh one: the bundle's anchors were run against the certified
    // fake, and what this asserts is that nothing reaches THIS one.
    let fake = crate::support::RecordingWarehouse::new();
    for &(fixture, _) in PROVOKED {
        let outcome =
            sutura_app::answer(&validated, &question(&format!("{fixture}.yaml")), &fake).expect("a refusal is not an error");
        assert!(outcome.is_refusal(), "{fixture} was answered");
    }
    assert!(
        fake.asked_about().is_empty(),
        "refused questions reached the data system: {:?}",
        fake.asked_about()
    );
}

#[test]
fn a_corrupted_anchor_makes_the_bundle_unservable_rather_than_answering() {
    // The fourth milestone criterion, and the difference between a demo and a governed service. A
    // definition that has stopped computing its own number must fail readiness, not answer.
    //
    // The drift is applied to the DATA SYSTEM's answer, not to a report and not to a file on disk.
    // Not to a file, because a test that edited a fixture would leave the tree dirty when it
    // failed. Not to a report, which is what this used to do, because a fabricated report is no
    // longer an input to anything: the old version recorded a `Mismatch` by hand and handed it to
    // `Validated::new`. Here the expected half comes from the catalog and only the actual half is
    // this test's - which is also why the instrument is a fake rather than a registered adapter: no
    // real one can be asked to misreport a number.
    let pinned = crate::adapters::load::<crate::adapters::ReferenceCatalog>();
    let (first, _) = pinned
        .anchored_metrics()
        .next()
        .expect("the fixture catalog declares at least one anchor");
    let drifted = first.clone();
    let stopped_computing_it = crate::support::CertifiedNumbers::of(&pinned).misreporting(&drifted, "470022");
    let err = sutura_app::verify_and_validate(pinned, &stopped_computing_it)
        .expect_err("a bundle with a mismatched anchor must not be servable");
    settings("").bind(|| insta::assert_snapshot!("anchor_mismatch", err.to_string()));
}

#[test]
fn the_longest_permitted_range_is_answered_and_one_day_more_is_refused() {
    // Both sides of the availability boundary, one day apart. `refused-range-too-long.yaml`
    // provokes the variant from a question file - it asks for `[0001-01-01, 9999-12-31)`, the
    // three-and-a-half-million-day range that a "bounded" `TimeRange` accepts and that made the
    // word bounded mean nothing. What that fixture cannot show is WHERE the refusal starts, and a
    // cap nobody has stood on is a cap nobody knows the position of.
    //
    // 2020-01-01 to 2030-01-01 is ten calendar years at their longest: three leap days, so 3653
    // days, which is exactly what `MAX_RANGE_DAYS` admits. Asserted through `days()` rather than
    // by trusting the arithmetic in the dates, so a change to either shows up here.
    let pinned = crate::adapters::load::<crate::adapters::ReferenceCatalog>();
    let ask = |start: &str, end: &str| {
        let day = |raw: &str| sutura_domain::calendar::Date::parse(raw).expect("a test date is a date");
        Query::new(
            sutura_domain::model::MetricName::parse("revenue").expect("a name"),
            sutura_domain::model::Grain::Month,
            sutura_domain::calendar::TimeRange::new(day(start), day(end)).expect("a test range is a range"),
            Vec::new(),
            Vec::new(),
        )
    };

    let longest = ask("2020-01-01", "2030-01-01");
    assert_eq!(longest.range().days(), MAX_RANGE_DAYS);
    assert!(
        compile(&longest, &pinned)
            .expect("a refusal is not an error")
            .plan()
            .is_some(),
        "the longest permitted range must still be answerable, or the cap breaks real reporting"
    );

    let one_day_longer = ask("2020-01-01", "2030-01-02");
    assert_eq!(one_day_longer.range().days(), MAX_RANGE_DAYS + 1);
    assert_eq!(
        compile(&one_day_longer, &pinned)
            .expect("a refusal is not an error")
            .refusal(),
        Some(&RefusalReason::TimeRangeTooLong {
            days: MAX_RANGE_DAYS + 1,
            limit: MAX_RANGE_DAYS,
        }),
        "one day past the cap must be refused, and refused for being too long"
    );
}

#[test]
fn a_question_carrying_sql_is_an_error_and_not_a_dropped_field() {
    // Promised by `sutura_domain::query` and asserted here, because provoking it needs a real
    // format parser and the domain crate deliberately has none.
    //
    // Without `deny_unknown_fields`, `sql:` deserializes cleanly and is discarded, so a caller who
    // believes they sent SQL gets a confident answer to a different question.
    let with_sql = "metric: revenue\ngrain: month\nrange:\n  start: 2026-06-01\n  end: 2026-07-01\nsql: SELECT 1\n";
    let err = serde_norway::from_str::<Query>(with_sql).expect_err("sql is not a field of a question");
    assert!(err.to_string().contains("sql"), "{err}");
}

#[test]
fn a_range_with_no_end_is_not_a_range() {
    // The reason there is no `RefusalReason::TimeRangeUnbounded`: an unbounded range does not
    // deserialize, so the refusal would be unprovokable and a variant with no test that can reach
    // it looks like coverage.
    //
    // That is the whole of what the type does, and it is worth being exact about: it refuses an
    // ABSENT bound. A range with both bounds present and ten thousand years between them
    // deserializes cleanly, and what refuses that is `TimeRangeTooLong` - a refusal, not a parse
    // error, provoked by `refused-range-too-long.yaml` and pinned to the day above.
    let unbounded = "metric: revenue\ngrain: month\nrange:\n  start: 2026-06-01\n";
    drop(serde_norway::from_str::<Query>(unbounded).expect_err("a range without an end is not a range"));
}

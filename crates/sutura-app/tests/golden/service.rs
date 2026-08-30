//! Off every axis: what the service and the compiler decide, asserted against a fake.
//!
//! What is here is decided ABOVE the port, so a matrix over adapters would be the same assertion
//! run twice. The instrument is a fake: the port is a Rust trait, so the honest stand-in is a type
//! that implements it, and a test asserting on the text of an HTTP request would prove something
//! about the test.
use sutura_domain::plan::MAX_ROWS;
use sutura_domain::query::{MAX_RANGE_DAYS, Query, RefusalReason, ToolOutcome};
use sutura_semantic::compile;

use crate::shared::{PROVOKED, question, settings};

#[test]
fn a_question_that_would_reach_a_second_data_system_is_split_into_two_legs() {
    // Not reachable from a question file: it needs a catalog whose models sit on two data systems,
    // which is why it is not a registry entry. The two-source fake spans exactly one more source,
    // and a question whose dimension sits on it now compiles into a fact leg and a lookup leg rather
    // than into the refusal this test used to assert.
    use sutura_domain::pinned::SemanticCatalog as _;

    let split = crate::support::two_source_catalog()
        .load()
        .expect("a two-source catalog can be built");
    let asked = Query::new(
        sutura_domain::model::MetricName::parse("recurring_revenue").expect("a name"),
        sutura_domain::model::Grain::Month,
        crate::support::june_range(),
        vec![sutura_domain::model::DimensionName::parse("region").expect("a name")],
        Vec::new(),
    );
    let compiled = compile(&asked, &split).expect("this is a plan, not an error");
    match compiled {
        sutura_semantic::Compiled::Federated { ref plan } => {
            assert_eq!(plan.legs().len(), 2, "a fact leg and a lookup leg");
        }
        other => panic!("a two-source question should federate, got {other:?}"),
    }
}

#[test]
fn a_question_whose_join_would_read_two_tables_of_one_name_is_refused() {
    // **A reproduced wrong-answer report reaching a caller as a refusal.** A fact table at
    // `analytics_prod.sales.orders` joined to a dimension table at `reference_data.crm.orders` used to
    // render a `FROM` and a `LEFT JOIN` whose `ON` clause compared `orders.customer_id` with
    // `orders.customer_id` - one table with itself - because a column is qualified by the LAST part of
    // a path. A real DuckDB refuses that statement as `Ambiguous reference to table "orders"`; a target
    // that binds it to one side answers with a number under a certified metric name.
    //
    // Built in code rather than as a directory of documents, for `TwoSourceCatalog`'s reason: the
    // shipped corpus is deliberately unqualified, so qualifying it would move every existing golden.
    use sutura_domain::pinned::SemanticCatalog as _;

    let collides = crate::support::same_name_tables_catalog()
        .load()
        .expect("a catalog whose two tables share a name still LOADS - the question is what is refused");

    // The dimension that needs the colliding join.
    let through_the_join = Query::new(
        sutura_domain::model::MetricName::parse("revenue").expect("a name"),
        sutura_domain::model::Grain::Month,
        crate::support::june_range(),
        vec![sutura_domain::model::DimensionName::parse("region").expect("a name")],
        Vec::new(),
    );
    let compiled = compile(&through_the_join, &collides).expect("this is a refusal, not an error");
    let expected = RefusalReason::PlanTablesShareAnIdentifier {
        table: sutura_domain::model::TableName::parse("orders").expect("a name"),
    };
    assert_eq!(
        compiled.refusal(),
        Some(&expected),
        "expected an ambiguous-alias refusal, got {:?}",
        compiled.refusal()
    );

    // **And the half that makes the refusal narrow rather than a ban on the metric:** a dimension on
    // the fact table's own model needs no join, so the same metric over the same catalog still plans.
    // Without this the assertion above would pass on a check that refused the metric outright, which
    // is the load-time refusal this deliberately is not.
    let no_join = Query::new(
        sutura_domain::model::MetricName::parse("revenue").expect("a name"),
        sutura_domain::model::Grain::Month,
        crate::support::june_range(),
        vec![sutura_domain::model::DimensionName::parse("customer").expect("a name")],
        Vec::new(),
    );
    let planned = compile(&no_join, &collides).expect("a question that needs no join is not a refusal");
    assert_eq!(
        planned.refusal(),
        None,
        "a question that puts one table in the statement is still answered, got {:?}",
        planned.refusal()
    );
}

#[test]
fn a_plan_for_a_data_system_this_process_did_not_open_is_refused() {
    // The service checks the plan's source against the adapter it is about to call. Without it, a
    // question would be answered against whatever happened to be connected, under provenance that
    // named something else. A fake claiming to be somewhere else is the whole instrument: no
    // registered adapter can be asked to lie about its own name, and none should be able to.
    let validated = crate::support::validated_bundle(crate::adapters::load::<crate::adapters::ReferenceCatalog>());
    let elsewhere = sutura_app::Warehouses::of(crate::support::RecordingWarehouse::pretending_to_be("somewhere_else"));
    let outcome = sutura_app::answer(
        &validated,
        &question("recurring-revenue-june.yaml"),
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &elsewhere,
    )
    .expect("a refusal is not an error")
    .into_outcome();
    assert!(
        matches!(outcome.refusal(), Some(&RefusalReason::SourceUnavailable { .. })),
        "expected a source refusal, got {outcome:?}"
    );
    // Reached back out of the registry by the name it was registered under, which is the honest way
    // to ask a fake what it saw now that the service holds a lookup rather than one adapter.
    let claimed = sutura_domain::model::SourceName::parse("somewhere_else").expect("a test source is a source");
    assert!(
        elsewhere
            .get(&claimed)
            .expect("the fake is registered under the name it claims")
            .asked_about()
            .is_empty(),
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
    let fake = sutura_app::Warehouses::of(crate::support::RecordingWarehouse::new());
    for &(fixture, _) in PROVOKED {
        let outcome = sutura_app::answer(
            &validated,
            &question(&format!("{fixture}.yaml")),
            &crate::adapters::a_caller(),
            &crate::adapters::shared_credential(),
            &fake,
        )
        .expect("a refusal is not an error")
        .into_outcome();
        assert!(outcome.is_refusal(), "{fixture} was answered");
    }
    let recorded = fake
        .get(&crate::adapters::source())
        .expect("the fake is registered under the corpus source");
    assert!(
        recorded.asked_about().is_empty(),
        "refused questions reached the data system: {:?}",
        recorded.asked_about()
    );
}

#[test]
fn an_exhausted_working_set_is_a_refusal_and_not_a_transport_failure() {
    // THE defect this variant exists for, asserted above the port. Exhaustion used to leave here as
    // `ServiceError::Warehouse`, which the HTTP surface answers `503 unavailable` - the same status a
    // data system that is down produces - so a caller was told to retry against a configured bound
    // that fires again in exactly the same place.
    //
    // A fake rather than the engine, because what is decided here is the branch taken on the way out
    // and not whether an engine can be made to run out of memory. The real one is shown biting in
    // `sutura_exec_datafusion::pool::ceiling_tests`.
    let validated = crate::support::validated_bundle(crate::adapters::load::<crate::adapters::ReferenceCatalog>());
    let exhausted = sutura_app::Warehouses::of(crate::support::ExhaustedEngine::at(1024 * 1024 * 1024));
    let outcome = sutura_app::answer(
        &validated,
        &question("recurring-revenue-by-region.yaml"),
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &exhausted,
    )
    .expect("exhaustion is a refusal, not an error")
    .into_outcome();
    assert_eq!(
        outcome.refusal(),
        Some(&RefusalReason::ResourcesExhausted {
            ceiling_bytes: 1024 * 1024 * 1024
        }),
        "an exhausted working set must be refused, and refused for being exhausted"
    );

    // The other direction, which is the more dangerous mistake: a failure that is NOT the ceiling
    // must still be an error. A caller told "do not retry" about a data system that is briefly unwell
    // has been told the wrong thing, and the port's default answer of `None` is what keeps that true
    // for every adapter with no pool to bound.
    let broken = sutura_app::Warehouses::of(crate::support::BrokenEngine::new());
    let failure = sutura_app::answer(
        &validated,
        &question("recurring-revenue-by-region.yaml"),
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &broken,
    )
    .expect_err("a failure that is not the ceiling is not a refusal");
    assert!(matches!(failure, sutura_app::ServiceError::Warehouse { .. }), "{failure:?}");
}

#[test]
fn a_result_that_reached_the_row_cap_is_refused_rather_than_silently_truncated() {
    // THE BUG THIS EXISTS FOR, and it was a wrong number under a certified name. `plan::MAX_ROWS`
    // was the `LIMIT` on the statement and on the engine's plan, and NOTHING compared the rows that
    // came back against it. So a question at `day` grain over a year, grouped by up to
    // `MAX_DIMENSIONS` keys, answered with the first ten thousand groups by group key - with a
    // provenance digest attached and no indication whatsoever that it was partial. Summing those
    // rows gives a total that is wrong by omission, which is the one failure a caller cannot detect
    // and the one this design is arranged against.
    //
    // Not in `PROVOKED` and it cannot be: that table is refusals a QUESTION FILE provokes, decided
    // before anything runs, and `a_refused_question_never_reaches_the_data_system` asserts exactly
    // that about every entry. This refusal is decided after a data system has answered. The corpus
    // CSVs hold a few hundred subscription-months, so nothing authorable here reaches ten thousand
    // groups either - the honest instrument is a fake that decides its own row count.
    let validated = crate::support::validated_bundle(crate::adapters::load::<crate::adapters::ReferenceCatalog>());
    let cap = usize::try_from(MAX_ROWS).expect("the row cap fits a usize on every target this builds for");

    // One row past the cap. That row exists only because the plan asked for it: see below.
    let too_wide = sutura_app::Warehouses::of(crate::support::WideResult::of(cap.saturating_add(1)));
    let outcome = sutura_app::answer(
        &validated,
        &question("recurring-revenue-by-region.yaml"),
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &too_wide,
    )
    .expect("a refusal is not an error")
    .into_outcome();
    assert_eq!(
        outcome.refusal(),
        Some(&RefusalReason::ResultTooLarge { limit: MAX_ROWS }),
        "a result past the row cap must be refused, and refused for being too large"
    );

    // The other half of the mechanism, asserted rather than assumed: the adapter was asked for one
    // row MORE than the cap. Without the extra row a result of exactly `MAX_ROWS` is
    // indistinguishable from one the cap cut short, and the check above would have to refuse both -
    // which would make a legitimate ten-thousand-row answer unobtainable.
    assert_eq!(
        too_wide
            .get(&crate::adapters::source())
            .expect("the fake is registered under the corpus source")
            .asked_for(),
        vec![MAX_ROWS.saturating_add(1)],
        "the plan must ask for one row past the cap, or reaching the cap cannot be told from being cut off by it"
    );

    // And exactly the cap still answers, so this is not a test that would pass with every wide
    // question refused.
    let at_the_cap = sutura_app::Warehouses::of(crate::support::WideResult::of(cap));
    let outcome = sutura_app::answer(
        &validated,
        &question("recurring-revenue-by-region.yaml"),
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &at_the_cap,
    )
    .expect("a refusal is not an error")
    .into_outcome();
    let ToolOutcome::Answer { ref rows, .. } = outcome else {
        panic!("a result of exactly the cap is answerable, not {outcome:?}");
    };
    assert_eq!(rows.rows().len(), cap);
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
    // Whichever metric sorts first among the anchored ones, so this test does not name a metric and
    // does not go stale when the catalog gains one. The drift value has to be wrong for whatever
    // that turns out to be, which is why it is negative rather than a plausible-looking figure:
    // every anchored metric in this catalog is a count, a sum of minor units or a share of a month,
    // and none of the three can be less than zero. A number that merely differed would test the same
    // comparison; one that could not be that metric's answer says so to whoever reads the snapshot.
    let pinned = crate::adapters::load::<crate::adapters::ReferenceCatalog>();
    let (first, _) = pinned
        .anchored_metrics()
        .next()
        .expect("the example catalog declares at least one anchor");
    let drifted = first.clone();
    let stopped_computing_it =
        sutura_app::Warehouses::of(crate::support::CertifiedNumbers::of(&pinned).misreporting(&drifted, "-1"));
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
            sutura_domain::model::MetricName::parse("recurring_revenue").expect("a name"),
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
    let with_sql = "metric: recurring_revenue\ngrain: month\nrange:\n  start: 2026-06-01\n  end: 2026-07-01\nsql: SELECT 1\n";
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
    let unbounded = "metric: recurring_revenue\ngrain: month\nrange:\n  start: 2026-06-01\n";
    drop(serde_norway::from_str::<Query>(unbounded).expect_err("a range without an end is not a range"));
}

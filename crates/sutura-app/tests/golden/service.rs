//! Off every axis: what the service and the compiler decide, asserted against a fake.
//!
//! What is here is decided ABOVE the port, so a matrix over adapters would be the same assertion
//! run twice. The instrument is a fake: the port is a Rust trait, so the honest stand-in is a type
//! that implements it, and a test asserting on the text of an HTTP request would prove something
//! about the test.
use sutura_domain::plan::MAX_ROWS;
use sutura_domain::query::{MAX_RANGE_DAYS, Query, RefusalReason, ResultBound, ToolOutcome};
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
        1 << 30,
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
            1 << 30,
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
fn a_working_set_exhaustion_wins_over_a_result_too_large_when_an_adapter_reports_both() {
    // The precedence decision, pinned. `sutura_app::answer` asks `working_set_exhausted` before
    // `result_did_not_fit` on the same `execute` failure, so an adapter that maps ONE endpoint error
    // into both predicates must be reported as exhausted - the more fundamental bound - and never as
    // a too-large result a caller would try to narrow to nothing. Which refusal a caller sees must not
    // be the order the arms happened to be written in. `BothPredicatesEngine` answers both.
    let validated = crate::support::validated_bundle(crate::adapters::load::<crate::adapters::ReferenceCatalog>());
    let both = sutura_app::Warehouses::of(crate::support::BothPredicatesEngine::at(1024 * 1024 * 1024));
    let outcome = sutura_app::answer(
        &validated,
        &question("recurring-revenue-by-region.yaml"),
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &both,
        1 << 30,
    )
    .expect("a both-predicate failure is still a refusal, not an error")
    .into_outcome();
    assert_eq!(
        outcome.refusal(),
        Some(&RefusalReason::ResourcesExhausted {
            ceiling_bytes: 1024 * 1024 * 1024
        }),
        "exhaustion must win when an adapter reports both predicates"
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
        1 << 30,
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
        1 << 30,
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
        1 << 30,
    )
    .expect("a refusal is not an error")
    .into_outcome();
    assert_eq!(
        outcome.refusal(),
        Some(&RefusalReason::ResultTooLarge {
            bound: ResultBound::Rows { limit: MAX_ROWS }
        }),
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
        1 << 30,
    )
    .expect("a refusal is not an error")
    .into_outcome();
    let ToolOutcome::Answer { ref rows, .. } = outcome else {
        panic!("a result of exactly the cap is answerable, not {outcome:?}");
    };
    assert_eq!(rows.rows().len(), cap);
}

#[test]
fn a_result_the_data_system_would_not_return_at_once_is_refused_and_not_reported_as_an_outage() {
    // THE defect the second bound exists for, asserted above the port where the branch is taken. A
    // result INSIDE the row cap that a data system will not hand back in one piece used to leave here
    // as `ServiceError::Warehouse`, which the HTTP surface answers `503 unavailable` - the one refusal
    // where retrying is reasonable per `docs/adr/0005`. It is not an outage and the retry returns the
    // same reply, so the caller was told to retry against a bound that fires again in the same place.
    // The identical sentence is in `an_exhausted_working_set_is_a_refusal_and_not_a_transport_failure`
    // above, which is the point: this is that fix applied to the bound one step further out.
    //
    // A fake rather than the real adapter, for the reason the exhaustion test gives: what is decided
    // here is the branch on the way out. `sutura-exec-bigquery`'s own suite is where the real
    // predicate is shown answering `true` for a page token and for a delivered count under the total.
    let validated = crate::support::validated_bundle(crate::adapters::load::<crate::adapters::ReferenceCatalog>());
    let would_not_fit = sutura_app::Warehouses::of(crate::support::WideForTheWire::new());
    let outcome = sutura_app::answer(
        &validated,
        &question("recurring-revenue-by-region.yaml"),
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &would_not_fit,
        1 << 30,
    )
    .expect("a result the data system would not return is a refusal, not an error")
    .into_outcome();
    assert_eq!(
        outcome.refusal(),
        Some(&RefusalReason::ResultTooLarge {
            bound: ResultBound::Volume
        }),
        "a result the data system would not hand back must be refused as too much data, and say which bound"
    );

    // The other direction, which is the more dangerous mistake and the reason the port defaults to
    // `false`: a failure that is NOT that bound must still be an error. A caller told "do not retry"
    // about a data system that is briefly unwell has been told the wrong thing.
    let broken = sutura_app::Warehouses::of(crate::support::BrokenEngine::new());
    let failure = sutura_app::answer(
        &validated,
        &question("recurring-revenue-by-region.yaml"),
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &broken,
        1 << 30,
    )
    .expect_err("a failure that is not a size bound is not a refusal");
    assert!(matches!(failure, sutura_app::ServiceError::Warehouse { .. }), "{failure:?}");
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
fn a_key_inside_a_range_is_an_error_and_not_a_dropped_field() {
    // The same promise one level down, where it did not hold: `TimeRange` deserializes through a
    // private input shape that carried no `deny_unknown_fields`, so a key written inside `range:`
    // was the one key in a question that was discarded in silence. `ends:` rather than `sql:`
    // because a typo is what actually arrives - the question above still deserialized cleanly and
    // was answered over June, with the author's intended end date on the floor.
    //
    // Asserted over YAML as well as over the DataHub crate's JSON because they are different
    // deserializers: `deny_unknown_fields` is honoured by the Deserializer, so serde_norway's
    // behaviour is not implied by serde_json's. Not a second statement of one fact.
    let typo = "metric: recurring_revenue\ngrain: month\nrange:\n  start: 2026-06-01\n  end: 2026-07-01\n  ends: 2026-08-01\n";
    let err = serde_norway::from_str::<Query>(typo).expect_err("a key inside a range is not a field of a range");
    assert!(err.to_string().contains("ends"), "{err}");
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

#[test]
fn a_dimension_named_like_the_remote_join_column_still_answers() {
    // **`telekom/sutura#325`'s F2, from the compiler through the combiner.** The splitter labelled
    // the column the two legs join on with the physical remote join column's TEXT, and put it beside
    // the public dimension labels in the same leg result. `customer_key` is a legal dimension name,
    // so a metric declaring one - backed by a different column - projected two fact columns under one
    // label. The compiler and the combiner reported it as:
    //
    // ```text
    // fact key labels: ["customer_key", "customer_key"]
    // Err(DuplicateLabels { side: "fact", label: "customer_key" })
    // ```
    //
    // The constraint the fix has to respect is that the DIMENSION stays legal: an internal naming
    // rule must not become a restriction on what a question may ask for. So the internal labels moved
    // into a namespace no identifier can spell, and this test asks the question that used to collide.
    //
    // Through the real splitter and the real combiner, because that pair is the defect: the domain's
    // own suite hands `combine` hand-built leg results, so nothing there can see a label the SPLITTER
    // chose. Executing the legs is `#325`'s F5 and a separate slice; the rows here are the identity
    // fixture that shows the join found its column.
    use sutura_domain::pinned::SemanticCatalog as _;
    use sutura_domain::warehouse::{RowSet, Value};

    let split = crate::support::two_source_catalog()
        .load()
        .expect("a two-source catalog can be built");
    let asked = Query::new(
        sutura_domain::model::MetricName::parse("recurring_revenue").expect("a name"),
        sutura_domain::model::Grain::Month,
        crate::support::june_range(),
        vec![
            sutura_domain::model::DimensionName::parse("customer_key").expect("a name"),
            sutura_domain::model::DimensionName::parse("region").expect("a name"),
        ],
        Vec::new(),
    );
    let compiled = compile(&asked, &split).expect("this is a plan, not an error");
    let sutura_semantic::Compiled::Federated { plan } = compiled else {
        panic!("a two-source question should federate");
    };

    // One fact row and one lookup row that join, fed back under each leg's OWN labels - which is
    // what a data system would return them under. The measure is asserted, so a combine that lost
    // the link column could not pass by answering no rows.
    let fact = RowSet::new(
        plan.fact().result_labels(),
        vec![vec![
            Value::Text("s1".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]],
    )
    .expect("a fact result under the fact leg's own labels");
    let lookup = RowSet::new(
        plan.lookup().result_labels(),
        vec![vec![Value::Text("c1".into()), Value::Text("north".into())]],
    )
    .expect("a lookup result under the lookup leg's own labels");

    let answer = plan
        .combine(&fact, &lookup, 1 << 20)
        .expect("a question whose dimension is named like the remote join column still combines");
    assert_eq!(
        answer.columns(),
        &["customer_key", "region", "period", "recurring_revenue"],
        "the answer's columns are the question's, not the scheme's"
    );
    assert_eq!(
        answer.rows(),
        &[vec![
            Value::Text("s1".into()),
            Value::Text("north".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]]
    );

    // And the labels themselves, which is the report the combine's refusal is downstream of: two
    // columns under one label in either leg, and a public dimension that lost its own name.
    for (side, labels) in [("fact", fact.columns()), ("lookup", lookup.columns())] {
        let distinct: std::collections::BTreeSet<&String> = labels.iter().collect();
        assert_eq!(
            distinct.len(),
            labels.len(),
            "the {side} leg projects two columns under one label: {labels:?}"
        );
    }
    assert!(
        fact.columns().contains(&String::from("customer_key")),
        "the public dimension keeps its own name: {:?}",
        fact.columns()
    );
}

#[test]
fn a_federated_question_whose_fact_leg_would_read_two_tables_of_one_name_is_refused() {
    // **The same reproduced wrong-answer report as the test above, on the OTHER plan shape, and it
    // was reproduced here too rather than reasoned about from the first one.** A two-source question
    // is split into a fact leg and a lookup leg, and the fact leg keeps every SAME-SOURCE hop as a
    // `JOIN` of its own - so the whole ambiguity is available inside one leg's statement. The
    // splitter built that leg by struct literal and never asked `StatementTables::parse` about it,
    // and `sutura_sql::generate_leg` rendered this for `Dialect::BigQuery`, measured:
    //
    // ```text
    // SELECT `orders`.`segment` AS `segment`, ... FROM `analytics_prod`.`sales`.`orders`
    //   LEFT JOIN `reference_data`.`crm`.`orders`
    //   ON `orders`.`customer_id` = `orders`.`customer_id` ...
    // ```
    //
    // One table compared with itself, every projected column qualified by an identifier naming two
    // tables, and a `SUM` under a certified metric name over whichever side a target happened to
    // bind. Worse than the whole-answer case rather than equal to it, because a leg's rows are
    // combined above it: nothing downstream sees the statement.
    //
    // Asserted at the compiler and not on a rendered string, for the sibling test's reason: the fix
    // is that no such leg exists to be rendered.
    use sutura_domain::pinned::SemanticCatalog as _;

    let collides = crate::support::federated_same_name_tables_catalog()
        .load()
        .expect("a catalog whose two same-source tables share a name still LOADS");

    // `segment` reaches the colliding same-source table, `region` reaches the second data system -
    // so this question both federates AND puts two `orders` in the fact leg.
    let both = Query::new(
        sutura_domain::model::MetricName::parse("revenue").expect("a name"),
        sutura_domain::model::Grain::Month,
        crate::support::june_range(),
        vec![
            sutura_domain::model::DimensionName::parse("segment").expect("a name"),
            sutura_domain::model::DimensionName::parse("region").expect("a name"),
        ],
        Vec::new(),
    );
    let compiled = compile(&both, &collides).expect("this is a refusal, not an error");
    let expected = RefusalReason::PlanTablesShareAnIdentifier {
        table: sutura_domain::model::TableName::parse("orders").expect("a name"),
    };
    assert_eq!(
        compiled.refusal(),
        Some(&expected),
        "expected the fact leg's tables to be refused as indistinguishable, got {:?}",
        compiled.refusal()
    );

    // **And the half that keeps this a refusal about the QUESTION.** The same catalog, the same
    // second data system, and a local dimension that needs no colliding join: still split into two
    // legs. Without this the assertion above would pass on a splitter that refused every federated
    // question over this catalog, which is not the guard being claimed.
    let without_the_collision = Query::new(
        sutura_domain::model::MetricName::parse("revenue").expect("a name"),
        sutura_domain::model::Grain::Month,
        crate::support::june_range(),
        vec![
            sutura_domain::model::DimensionName::parse("customer").expect("a name"),
            sutura_domain::model::DimensionName::parse("region").expect("a name"),
        ],
        Vec::new(),
    );
    let split = compile(&without_the_collision, &collides).expect("this is a plan, not an error");
    match split {
        sutura_semantic::Compiled::Federated { ref plan } => {
            assert_eq!(plan.legs().len(), 2, "a fact leg and a lookup leg");
        }
        ref other => panic!("a two-source question with no colliding join should still federate, got {other:?}"),
    }
}

use crate::catalog::TIME_BUCKET_LABEL;
use crate::federation::Federation;
use crate::measure::Measure;
use crate::model::{Aggregate, ColumnName, DimensionName, InvalidIdentifier, MetricName, TableName};
use crate::plan::leg::LegPlan;
use crate::plan::{
    AnswerKey, FederatedFailure, FederatedPlan, FederatedPlanError, InternalLabel, PlanBindings, ResultLabel, StatementTables,
};
use crate::warehouse::{Real, RowSet, Value};

mod fixtures;
use fixtures::*;

#[test]
fn joins_two_legs_and_reaggregates_by_remote_key() {
    let plan = sum_plan(true);
    let fact = fact(vec![
        vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ],
        vec![
            Value::Text("A".into()),
            Value::Text("c2".into()),
            Value::Text("2026-06".into()),
            Value::Integer(200),
        ],
        vec![
            Value::Text("B".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(50),
        ],
    ]);
    let lookup = lookup(vec![
        vec![Value::Text("c1".into()), Value::Text("north".into())],
        vec![Value::Text("c2".into()), Value::Text("north".into())],
    ]);

    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("a two-leg question combines");
    assert_eq!(combined.columns(), &["product_family", "region", "period", "revenue"]);
    assert_eq!(
        combined.rows(),
        &[
            vec![
                Value::Text("A".into()),
                Value::Text("north".into()),
                Value::Text("2026-06".into()),
                Value::Integer(300)
            ],
            vec![
                Value::Text("B".into()),
                Value::Text("north".into()),
                Value::Text("2026-06".into()),
                Value::Integer(50)
            ],
        ]
    );
}

#[test]
fn an_inner_join_drops_an_unmatched_fact_row() {
    let plan = sum_plan(false);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c9".into()),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("combines");
    assert!(
        combined.rows().is_empty(),
        "an unmatched fact row is dropped by an inner join"
    );
}

#[test]
fn a_left_join_keeps_an_unmatched_fact_row_with_null_remote() {
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c9".into()),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("combines");
    assert_eq!(
        combined.rows(),
        &[vec![
            Value::Text("A".into()),
            Value::Null,
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]]
    );
}

#[test]
fn an_average_is_undivided_in_the_leg_and_divided_above() {
    let plan = avg_plan();
    let fact = avg_fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(300),
        Value::Integer(3),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);

    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("an average combines");
    assert_eq!(
        combined.columns(),
        &["product_family", "region", "period", "mean_subscription_mrr"]
    );
    match &combined.rows()[0][3] {
        Value::Real(r) => assert_eq!(r.get(), 100.0),
        other => panic!("an average answers a real number, got {other:?}"),
    }
}

#[test]
fn a_minimum_leaf_reaggregates_across_the_group() {
    let plan = plan_for("min_mrr", &Measure::Simple(term(Aggregate::Min, "mrr_cents")), true);
    let fact = RowSet::new(
        vec![
            String::from("product_family"),
            link(),
            String::from(TIME_BUCKET_LABEL),
            leaf(0),
        ],
        vec![
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(300),
            ],
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ],
        ],
    )
    .expect("a min fact result is well formed");
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);

    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("a minimum combines");
    assert_eq!(
        combined.rows(),
        &[vec![
            Value::Text("A".into()),
            Value::Text("north".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100)
        ]]
    );
}

#[test]
fn a_failing_ratio_guard_errors_on_a_zero_denominator() {
    let plan = failing_ratio_plan();
    let fact = avg_fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(300),
        Value::Integer(0),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::NonFinite { .. })
    ));
}

#[test]
fn a_decomposed_average_with_zero_over_zero_is_null() {
    let plan = avg_plan();
    let fact = avg_fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(0),
        Value::Integer(0),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("a null guard answers");
    assert!(matches!(combined.rows()[0][3], Value::Null));
}

#[test]
fn a_missing_leaf_label_is_an_error() {
    let plan = sum_plan(true);
    let fact = RowSet::new(
        vec![String::from("product_family"), link(), String::from(TIME_BUCKET_LABEL)],
        vec![vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
        ]],
    )
    .expect("a fact result missing the measure column");
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::MissingColumn { .. })
    ));
}

#[test]
fn a_non_numeric_leaf_is_refused_not_counted_as_zero() {
    // The DuckDB adapter returns a DECIMAL money column as Text to keep it exact; a sum that
    // meets it must refuse rather than certify a zero.
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Text("1234.56".into()),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::NonNumericLeaf {
            aggregate: Aggregate::Sum,
            ..
        })
    ));
}

#[test]
fn a_sum_over_a_column_mixing_integers_and_reals_is_refused() {
    // `RowSet::new` checks a row's width and nothing about its cells, so one leaf column holding an
    // `Integer` beside a `Real` is representable. Neither subtotal may be dropped and neither may be
    // widened onto the other, so the column is refused: this group answered `1.5` for a column
    // totalling `101.5`, under the metric's own certified name.
    let plan = sum_plan(true);
    let fact = fact(vec![
        fact_row(Value::Integer(100)),
        fact_row(Value::Real(Real::parse(1.5).expect("a finite real"))),
    ]);

    // Asserted on the refusal's own sentence rather than on the variant naming it: `MixedNumericLeaf`
    // is declared in the file `just causality` reverts, and naming it here costs the base tree its
    // build - which takes the base verdict for every other test in this file with it.
    let refusal = plan
        .combine(&fact, &one_lookup(), UNBOUNDED)
        .expect_err("a column mixing integers and reals has no exact total");
    assert_eq!(
        refusal.to_string(),
        "a `Sum` re-aggregation met a leaf column mixing integer and real cells"
    );
}

#[test]
fn a_minimum_over_a_column_mixing_integers_and_reals_is_refused() {
    // The refusal is a property of the column, not of the sum: a minimum over the same column
    // compares an `i64` against an `f64`, which above `2^53` reads two distinguishable cells as
    // equal. One type per column is what makes the comparison exact rather than checked.
    let plan = extreme_plan(Aggregate::Min, "min_mrr");
    let fact = fact(vec![
        fact_row(Value::Integer(100)),
        fact_row(Value::Real(Real::parse(1.5).expect("a finite real"))),
    ]);

    let refusal = plan
        .combine(&fact, &one_lookup(), UNBOUNDED)
        .expect_err("a column mixing integers and reals has no exact minimum");
    assert_eq!(
        refusal.to_string(),
        "a `Min` re-aggregation met a leaf column mixing integer and real cells"
    );
}

#[test]
fn a_maximum_over_wide_integers_answers_the_larger_cell() {
    // Two integers one apart, which a data system tells apart and an `f64` does not. Taken as `f64`
    // the comparison reads them as equal and keeps whichever arrived first - here the smaller - so
    // the maximum of the column was not the larger of its cells.
    let plan = extreme_plan(Aggregate::Max, "max_mrr");
    let fact = fact(vec![
        fact_row(Value::Integer(TWO_POW_53)),
        fact_row(Value::Integer(TWO_POW_53_PLUS_ONE)),
    ]);

    let combined = plan.combine(&fact, &one_lookup(), UNBOUNDED).expect("a maximum combines");
    assert_eq!(
        combined.rows(),
        &[vec![
            Value::Text("A".into()),
            Value::Text("north".into()),
            Value::Text("2026-06".into()),
            Value::Integer(TWO_POW_53_PLUS_ONE),
        ]]
    );
}

#[test]
fn a_lone_non_numeric_cell_is_refused_by_a_minimum() {
    // The cell the sum above refuses, in a group of one. A minimum accepted its first cell as its
    // own best without ever reading it as a number, so a `DECIMAL` money column - which the
    // `DuckDB` adapter returns as `Text` to keep it exact - came back as the metric's value.
    let plan = extreme_plan(Aggregate::Min, "min_mrr");
    let fact = fact(vec![fact_row(Value::Text("1234.56".into()))]);

    assert!(matches!(
        plan.combine(&fact, &one_lookup(), UNBOUNDED),
        Err(FederatedFailure::NonNumericLeaf {
            aggregate: Aggregate::Min,
            ..
        })
    ));
}

#[test]
fn a_non_numeric_cell_does_not_win_a_minimum_over_a_number() {
    // The two-cell shape of the same defect: a candidate that is a number could not be compared
    // against a best that is not, and an incomparable pair kept the incumbent - so the text won a
    // comparison it was never in.
    //
    // Both row orders, because only one is the defect: with the text second the old comparison
    // reached the `other` arm and refused anyway, so an order-dependent assertion would have been
    // red against base for the wrong reason.
    let plan = extreme_plan(Aggregate::Min, "min_mrr");
    for cells in [
        vec![fact_row(Value::Text("1234.56".into())), fact_row(Value::Integer(1))],
        vec![fact_row(Value::Integer(1)), fact_row(Value::Text("1234.56".into()))],
    ] {
        let fact = fact(cells);
        assert!(matches!(
            plan.combine(&fact, &one_lookup(), UNBOUNDED),
            Err(FederatedFailure::NonNumericLeaf {
                aggregate: Aggregate::Min,
                ..
            })
        ));
    }
}

#[test]
fn a_leaf_column_of_nulls_answers_null_and_never_names_a_refusal() {
    // Which reduction re-aggregates a leaf is settled by the plan, not the group's cells - two
    // halves of one property, of which only the second changed here. A group with no non-null cell
    // is an answer: nothing was contributed, which is a null and not a zero, and it is not the
    // column's job to decide whether the aggregate above it exists.
    let all_null = sum_plan(true)
        .combine(
            &fact(vec![fact_row(Value::Null), fact_row(Value::Null)]),
            &one_lookup(),
            UNBOUNDED,
        )
        .expect("a group of nulls is an answer");
    assert_eq!(
        all_null.rows(),
        &[vec![
            Value::Text("A".into()),
            Value::Text("north".into()),
            Value::Text("2026-06".into()),
            Value::Null,
        ]]
    );

    // And a leaf the combine has no re-aggregating function for is refused before a plan exists.
    // `Federation::of` is total, so `count_distinct` classifies as a leaf carrying grouping keys
    // whose combine is a `CountDistinct` nothing above the legs can apply. Deciding that while
    // reducing made the diagnosis depend on the data: the same plan refused a group holding a value
    // and answered `Null` for a group of nulls, under the metric's own certified name.
    let refused = try_plan_for(
        "distinct_customers",
        &Measure::Simple(term(Aggregate::CountDistinct, "customer_key")),
        true,
    )
    .expect_err("a leaf with no re-aggregating function is not a plan");
    assert_eq!(
        refused.to_string(),
        "a carried leaf re-aggregates with `count_distinct`, which the combine cannot apply"
    );
}

#[test]
fn a_float_link_key_is_refused() {
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Real(Real::parse(1001.0).expect("a finite real")),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::FloatLinkKey { .. })
    ));
}

#[test]
fn an_integer_link_and_a_text_link_do_not_false_match() {
    // Integer(1001) and Text("1001") are different cells; comparing them as rendered text would
    // join them, which is the false match the typed link key refuses. Under an inner join the
    // non-matching fact row is dropped.
    let plan = sum_plan(false);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Integer(1001),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    // The lookup holds the same digits as text.
    let lookup = lookup(vec![vec![Value::Text("1001".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("combines");
    assert!(
        combined.rows().is_empty(),
        "an integer link must not join to a text link with the same digits"
    );
}

#[test]
fn a_null_link_never_joins() {
    let plan = sum_plan(false);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Null,
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("combines");
    assert!(
        combined.rows().is_empty(),
        "a null link value never joins, not even to itself"
    );
}

#[test]
fn a_duplicate_leaf_label_is_refused() {
    // Two columns under one name would be traced to one of them arbitrarily, so the boundary
    // refuses the result rather than answer a wrong number.
    let plan = sum_plan(true);
    let fact = RowSet::new(
        vec![
            String::from("product_family"),
            link(),
            String::from(TIME_BUCKET_LABEL),
            leaf(0),
            leaf(0),
        ],
        vec![vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
            Value::Integer(200),
        ]],
    )
    .expect("a fact result with a duplicated label");
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::DuplicateLabels { .. })
    ));
}

#[test]
fn an_ambiguous_lookup_link_is_refused() {
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    // Two lookup rows for one link would double the measure.
    let lookup = RowSet::new(
        vec![link(), String::from("region")],
        vec![
            vec![Value::Text("c1".into()), Value::Text("north".into())],
            vec![Value::Text("c1".into()), Value::Text("south".into())],
        ],
    )
    .expect("a lookup result with two rows for one link");
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::AmbiguousLink { .. })
    ));
}

#[test]
fn an_answer_that_crosses_the_byte_budget_is_refused_not_truncated() {
    // #72's acceptance criterion and the ADR-0009 conversion-boundary bound: the answer is
    // refused as the budget is crossed, never returned part-way. A one-byte budget cannot hold
    // any answer; it provokes the refusal no matter how the accounting is split.
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, 1),
        Err(FederatedFailure::ResourcesExhausted { ceiling_bytes: 1 })
    ));
    // And the boundary holds the other way: the same question under a ceiling it fits answers.
    plan.combine(&fact, &lookup, UNBOUNDED)
        .expect("an unbounded budget fits the answer");
}

#[test]
fn a_plan_with_two_legs_on_one_source_does_not_construct() {
    // The constructor is the newtype convention: a plan that is not a fact leg beside a lookup
    // leg on a different data system is not a value of this type. A lookup passed in the fact
    // slot is the first guard to fire, before any source agreement is even compared.
    let plan = FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        lookup_leg(),
        lookup_leg(),
        true,
        Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents"))),
        vec![AnswerKey::fact(ResultLabel::dimension(&dimension("product_family")))],
    );
    assert!(matches!(plan, Err(FederatedPlanError::NotFact { .. })));
}

#[test]
fn an_internal_label_is_in_a_namespace_no_question_can_name() {
    // **The property first: nothing a catalog author or a caller can write reaches this namespace.**
    // Every rendering starts with a digit, which the identifier parser refuses as a FIRST character,
    // so it is one refusal for every spelling and length, not a list of reserved words to maintain.
    let widest = InternalLabel::Leaf(usize::MAX).label();
    for label in [InternalLabel::Link.label(), InternalLabel::Leaf(0).label(), widest.clone()] {
        assert!(
            matches!(DimensionName::parse(&label), Err(InvalidIdentifier::BadFirstCharacter { .. })),
            "{label} must not be a dimension name"
        );
        assert!(MetricName::parse(&label).is_err(), "{label} must not be a metric name");
        assert!(ColumnName::parse(&label).is_err(), "{label} must not be a column name");
        assert!(TableName::parse(&label).is_err(), "{label} must not be a table name");
    }

    // Then the SPELLING, which this test owns: every other test here takes its labels from the type,
    // so a test that derived this expectation too would assert nothing about what the splitter and
    // the combiner actually agree on.
    assert_eq!(InternalLabel::Link.label(), "0_link");
    assert_eq!(InternalLabel::Leaf(0).label(), "0_leaf_0");
    assert_eq!(InternalLabel::Leaf(7).label(), "0_leaf_7");

    // And the length, against the limit a data system TRUNCATES at rather than refusing - which
    // turns two distinct leaf columns into one. The scheme this replaced was `metric__{n}`, which
    // over a 63-character metric name was 66 characters; nothing here reads a metric's name, so the
    // widest label a `usize` can index is the bound.
    assert!(
        widest.len() <= 63,
        "an internal label must fit the tightest identifier limit, {widest} is {} characters",
        widest.len()
    );
}

#[test]
fn a_plan_whose_legs_do_not_project_the_link_does_not_construct() {
    // The link is not an answer key, so the answer-key loop never saw it and the combiner reported
    // a missing column when a leg had not projected it. A plan that cannot be joined is not a plan.
    let unlinked = LegPlan::Lookup {
        source: source(REMOTE_SOURCE),
        table: table(FACT).into(),
        keys: vec![key("region", FACT)],
        bindings: PlanBindings::none(),
    };
    let plan = FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        fact_leg(),
        unlinked,
        true,
        Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents"))),
        Vec::new(),
    );
    match plan {
        Err(FederatedPlanError::KeyNotOnLeg { side, ref label }) => {
            assert!(matches!(side, crate::plan::LegSide::Lookup));
            assert_eq!(*label, InternalLabel::Link.label());
        }
        ref other => panic!("a lookup leg that projects no link is not a plan, got {other:?}"),
    }
}

#[test]
fn a_fact_leg_that_does_not_project_the_link_does_not_construct_either() {
    // **The other arm of the same check, and the reachable one.** The test above builds an unlinked
    // LOOKUP leg, so `KeyNotOnLeg { side: Fact }` was the untested half - and the half that matters:
    // the fact leg is the one the splitter builds from the question's own keys, so a change there
    // that stopped pushing `InternalLabel::Link` is what this arm catches. The only production
    // caller no longer erases the cause (`telekom/sutura#338`): it leaves as
    // `sutura_semantic::CompileFailure::NotAssembled`, keeping the side and label this reads.
    let unlinked = LegPlan::Fact {
        source: source(FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys: vec![key("product_family", FACT)],
        terms: Vec::new(),
        bindings: PlanBindings::none(),
        range: range(),
    };
    let plan = FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        unlinked,
        lookup_leg(),
        true,
        Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents"))),
        Vec::new(),
    );
    match plan {
        Err(FederatedPlanError::KeyNotOnLeg { side, ref label }) => {
            assert!(matches!(side, crate::plan::LegSide::Fact));
            assert_eq!(*label, InternalLabel::Link.label());
        }
        ref other => panic!("a fact leg that projects no link is not a plan, got {other:?}"),
    }
}

#[test]
fn the_join_kind_decides_a_null_fact_key_the_way_it_decides_an_unmatched_one() {
    // **The cross-product the two null tests above missed between them.** One covered an unmatched
    // NON-null key under LEFT and one a null key under INNER, so the cell neither asked - a null key
    // under LEFT - dropped the fact row and its measure with it, under a join kind whose whole
    // promise is that an unmatched fact row survives.
    //
    // A null link matches nothing (`NULL = NULL` is not true in SQL), so a null-keyed fact row is
    // UNMATCHED and the join kind decides it: retained with null remote keys under LEFT, dropped
    // under INNER. The lookup carries a null-linked row of its own, so a fix that retained the fact
    // row by MATCHING it there would answer `nowhere` instead of null. Each answer row is
    // distinguished by its first key, so this is independent of where the comparator places a null.
    let facts = fact(vec![
        keyed_fact_row("matched", Value::Text("c1".into()), 100),
        keyed_fact_row("unmatched", Value::Text("c9".into()), 200),
        keyed_fact_row("null_key", Value::Null, 400),
    ]);
    let lookup = lookup(vec![
        vec![Value::Text("c1".into()), Value::Text("north".into())],
        vec![Value::Null, Value::Text("nowhere".into())],
    ]);

    let left = sum_plan(true)
        .combine(&facts, &lookup, UNBOUNDED)
        .expect("a left join combines");
    assert_eq!(
        left.rows(),
        &[
            vec![
                Value::Text("matched".into()),
                Value::Text("north".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ],
            vec![
                Value::Text("null_key".into()),
                Value::Null,
                Value::Text("2026-06".into()),
                Value::Integer(400),
            ],
            vec![
                Value::Text("unmatched".into()),
                Value::Null,
                Value::Text("2026-06".into()),
                Value::Integer(200),
            ],
        ],
        "a left join retains a null-keyed fact row with its measure, unmatched"
    );

    let inner = sum_plan(false)
        .combine(&facts, &lookup, UNBOUNDED)
        .expect("an inner join combines");
    assert_eq!(
        inner.rows(),
        &[vec![
            Value::Text("matched".into()),
            Value::Text("north".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]],
        "an inner join drops both unmatched shapes, the null-keyed one included"
    );
}

#[test]
fn the_answer_orders_ascending_with_nulls_last_like_the_mono_path() {
    // **The ordered-result contract is one contract, and federation was on the other side of it.**
    // A whole-answer plan emits `ORDER BY <key> ASC NULLS LAST` - `sutura_sql::generate`'s
    // `ordered_nulls_last`, which #92 decided and which makes all four dialects converge on the
    // engine's own placement. This comparator ranked a null FIRST, so one certified metric came back
    // in one order from one data system and in another from two.
    //
    // Both comparator halves are asserted here because they are one `ORDER BY`: a numeric key
    // orders NUMERICALLY (`9` before `10`, not `"10"` before `"9"`) and a null goes LAST.
    let facts = fact(vec![
        keyed_fact_row("A", Value::Text("c1".into()), 10),
        keyed_fact_row("A", Value::Text("c2".into()), 20),
        keyed_fact_row("A", Value::Text("c9".into()), 30),
    ]);
    let lookup = lookup(vec![
        vec![Value::Text("c1".into()), Value::Integer(10)],
        vec![Value::Text("c2".into()), Value::Integer(9)],
    ]);

    let combined = sum_plan(true)
        .combine(&facts, &lookup, UNBOUNDED)
        .expect("a left join combines");
    assert_eq!(
        combined.rows(),
        &[
            vec![
                Value::Text("A".into()),
                Value::Integer(9),
                Value::Text("2026-06".into()),
                Value::Integer(20),
            ],
            vec![
                Value::Text("A".into()),
                Value::Integer(10),
                Value::Text("2026-06".into()),
                Value::Integer(10),
            ],
            vec![
                Value::Text("A".into()),
                Value::Null,
                Value::Text("2026-06".into()),
                Value::Integer(30),
            ],
        ],
        "ascending by value, nulls last - the same order the mono path's ORDER BY asks for"
    );
}

#[test]
fn a_null_key_and_an_unmatched_key_re_aggregate_into_one_unmatched_group() {
    // The measure is what this asserts and a row count could not: both rows land in the one group
    // whose remote keys are null, so the answer is their SUM. Dropping the null-keyed row answered
    // 200 under the metric's own certified name - one row, right shape, wrong number.
    let facts = fact(vec![
        keyed_fact_row("A", Value::Text("c9".into()), 200),
        keyed_fact_row("A", Value::Null, 400),
    ]);
    let combined = sum_plan(true)
        .combine(&facts, &one_lookup(), UNBOUNDED)
        .expect("a left join combines");
    assert_eq!(
        combined.rows(),
        &[vec![
            Value::Text("A".into()),
            Value::Null,
            Value::Text("2026-06".into()),
            Value::Integer(600),
        ]]
    );
}

#[test]
fn leaves_measure_the_tree_they_were_built_from() {
    let federation = Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents")));
    let leaves: super::Leaves<'_> = super::Leaves::of(&federation, &[vec![Value::Integer(7)]], &metric("sum")).expect("leaves");
    assert_eq!(leaves.measure(&metric("sum")).expect("measure"), Value::Integer(7));
}

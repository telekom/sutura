//! What a combine declines to answer, and which side of the port's split each refusal falls on.
//!
//! Split out of `super` for that file's own `max-lines` reason, along the seam the module header
//! draws: what a combine answers, and what it refuses. `use super::*` reaches every fixture.

use super::*;
use std::sync::Arc;
use sutura_domain::plan::{FederatedAnswerRefusal, FederationCombiner as _};

use crate::combine::CombineError;
use datafusion::arrow::array::{ArrayRef, Int64Array, StringArray};
use datafusion::arrow::datatypes::DataType;

#[test]
fn a_float_link_key_is_refused() {
    // `docs/adr/0007`'s float-key rule: formatting a float into an equality lets distinct values
    // collide, so a floating-point link column is refused rather than joined. From the TYPE now,
    // which is why the lookup leg's own link column can be anything at all.
    let plan = sum_plan(true);
    let fact = fact(vec![fact_row("A", real(1.5), Value::Integer(100))]);
    let lookup = lookup(vec![vec![real(1.5), text("north")]]);
    assert_eq!(refusal(&plan, &fact, &lookup), FederatedAnswerRefusal::FloatLinkKey);
}

#[test]
fn an_integer_link_and_a_text_link_are_refused_not_silently_unmatched() {
    // `telekom/sutura#138`. Two legs whose link columns disagree in KIND miss on every row by
    // construction, and the combine used to return that silently: an empty inner answer, or a left
    // answer whose every fact row survived with a null remote side. Both are a wrong answer that
    // looks like a right one.
    let fact = fact(vec![fact_row("A", Value::Integer(1), Value::Integer(100))]);
    let lookup = lookup(vec![vec![text("1"), text("north")]]);
    assert_eq!(
        refusal(&sum_plan(true), &fact, &lookup),
        FederatedAnswerRefusal::LinkTypeMismatch,
        "a left join must not answer a null remote side for a mismatch"
    );
    assert_eq!(
        refusal(&sum_plan(false), &fact, &lookup),
        FederatedAnswerRefusal::LinkTypeMismatch,
        "an inner join must not answer no rows for a mismatch"
    );
}

#[test]
fn two_empty_legs_whose_link_kinds_disagree_are_refused() {
    // **The refusal the Arrow port made reachable, and it was not before.** The hand-written
    // combine decided a link column's kind from the first non-null CELL it found, so with no rows
    // on either side it decided nothing, joined anyway, and answered *no rows* - a right-looking
    // answer to a question that can never have one. An Arrow column carries a declared type whether
    // it has rows or not, so the same mismatch is refused from the schema.
    let plan = sum_plan(true);
    let fact = typed(
        vec![
            ("product_family", DataType::Utf8),
            (link().as_str(), DataType::Utf8),
            (sutura_domain::catalog::TIME_BUCKET_LABEL, DataType::Utf8),
            (leaf(0).as_str(), DataType::Int64),
        ],
        Vec::new(),
        0,
    );
    let lookup = typed(
        vec![(link().as_str(), DataType::Int64), ("region", DataType::Utf8)],
        Vec::new(),
        0,
    );
    assert_eq!(refusal(&plan, &fact, &lookup), FederatedAnswerRefusal::LinkTypeMismatch);
}

#[test]
fn two_empty_legs_whose_link_kinds_agree_answer_no_rows() {
    // The negative control for the cell above, and it is what makes that one a statement about the
    // KINDS rather than about emptiness: the same two empty legs, link columns agreeing, answer an
    // empty result instead of refusing.
    let plan = sum_plan(true);
    let fact = typed(
        vec![
            ("product_family", DataType::Utf8),
            (link().as_str(), DataType::Utf8),
            (sutura_domain::catalog::TIME_BUCKET_LABEL, DataType::Utf8),
            (leaf(0).as_str(), DataType::Int64),
        ],
        Vec::new(),
        0,
    );
    let lookup = typed(
        vec![(link().as_str(), DataType::Utf8), ("region", DataType::Utf8)],
        Vec::new(),
        0,
    );
    let combined = combined(&plan, &fact, &lookup, UNBOUNDED);
    assert!(combined.rows().is_empty(), "{:?}", combined.rows());
    assert_eq!(combined.columns(), &["product_family", "region", "period", "revenue"]);
}

#[test]
fn a_non_numeric_leaf_is_refused_not_counted_as_zero() {
    // A row-speaking adapter returns an exact `DECIMAL` money column as text to keep it exact, and
    // the row builder gives a column of such text `Utf8`. A sum over it cannot certify a number, so
    // it is refused rather than counted as zero.
    let plan = sum_plan(true);
    let fact = fact(vec![fact_row("A", text("c1"), text("11.50"))]);
    assert_eq!(refusal(&plan, &fact, &one_lookup()), FederatedAnswerRefusal::NonNumericLeaf);
}

#[test]
fn a_leaf_mixing_integers_with_exact_integral_text_is_answered_rather_than_refused() {
    // The negative control for the cell above, and it is the behaviour the interior's own row
    // builder buys: a column that mixes a fitting integer with an exact wide one arrives as a
    // zero-scale decimal, which IS numeric - so the total is exact where a text column is refused.
    // The hand-written combine refused this shape as `MixedNumericLeaf`.
    let plan = sum_plan(true);
    let wide = "10000000000000000006";
    let fact = fact(vec![
        fact_row("A", text("c1"), text(wide)),
        fact_row("A", text("c1"), Value::Integer(4)),
    ]);
    let combined = combined(&plan, &fact, &one_lookup(), UNBOUNDED);
    let exact = (wide.parse::<i128>().expect("the fixture is an integer") + 4).to_string();
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), text(&exact)]]
    );
}

#[test]
fn a_failing_ratio_guard_refuses_a_zero_denominator() {
    // A definition that declared `fails` asked for exactly this, and the refusal is the combine's
    // rather than the presentation edge's: `ResultBatches::to_rows` would refuse the non-finite
    // double too, but as a data-system failure - which invites a retry that refuses again.
    let plan = failing_ratio_plan();
    let fact = two_leaf_fact(vec![vec![
        text("A"),
        text("c1"),
        text("2026-06"),
        Value::Integer(100),
        Value::Integer(0),
    ]]);
    assert_eq!(refusal(&plan, &fact, &one_lookup()), FederatedAnswerRefusal::NonFinite);
}

#[test]
fn a_failing_ratio_over_a_nonzero_denominator_answers() {
    // The negative control: the same plan with a denominator answers, so the cell above is about
    // the ZERO and not about the guard refusing every ratio.
    let plan = failing_ratio_plan();
    let fact = two_leaf_fact(vec![vec![
        text("A"),
        text("c1"),
        text("2026-06"),
        Value::Integer(100),
        Value::Integer(4),
    ]]);
    let combined = combined(&plan, &fact, &one_lookup(), UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), real(25.0)]]
    );
}

#[test]
fn an_ambiguous_lookup_link_is_refused() {
    // Two lookup rows for one link value double every measure joined to it, so it is refused rather
    // than certified. The probe is its own `DataFusion` plan over the lookup leg and reads no cell -
    // what it learns is whether a group past one exists.
    let plan = sum_plan(true);
    let fact = fact(vec![fact_row("A", text("c1"), Value::Integer(100))]);
    let lookup = lookup(vec![vec![text("c1"), text("north")], vec![text("c1"), text("south")]]);
    assert_eq!(refusal(&plan, &fact, &lookup), FederatedAnswerRefusal::AmbiguousLink);
}

#[test]
fn two_null_linked_lookup_rows_are_not_ambiguous() {
    // The negative control the probe needs, and it is a real case rather than a symmetry: a null
    // link never joins, so two null-linked lookup rows duplicate nothing. The probe excludes nulls
    // before it counts, and without that this is refused.
    let plan = sum_plan(true);
    let fact = fact(vec![fact_row("A", text("c1"), Value::Integer(100))]);
    let lookup = lookup(vec![
        vec![text("c1"), text("north")],
        vec![Value::Null, text("nowhere")],
        vec![Value::Null, text("elsewhere")],
    ]);
    let combined = combined(&plan, &fact, &lookup, UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), Value::Integer(100)]]
    );
}

#[test]
fn a_duplicate_leaf_label_is_refused() {
    // Two columns under one label is the one shape a combine cannot disambiguate, and Arrow permits
    // it: `RecordBatch` validates positionally and never reads a field name against its siblings.
    let plan = sum_plan(true);
    let duplicated = typed(
        vec![
            ("product_family", DataType::Utf8),
            (link().as_str(), DataType::Utf8),
            (sutura_domain::catalog::TIME_BUCKET_LABEL, DataType::Utf8),
            (leaf(0).as_str(), DataType::Int64),
            (leaf(0).as_str(), DataType::Int64),
        ],
        vec![
            Arc::new(StringArray::from(vec!["A"])) as ArrayRef,
            Arc::new(StringArray::from(vec!["c1"])) as ArrayRef,
            Arc::new(StringArray::from(vec!["2026-06"])) as ArrayRef,
            Arc::new(Int64Array::from(vec![100_i64])) as ArrayRef,
            Arc::new(Int64Array::from(vec![200_i64])) as ArrayRef,
        ],
        1,
    );
    let error = try_combine(&plan, &duplicated, &one_lookup(), UNBOUNDED).expect_err("a duplicate label is refused");
    assert!(
        matches!(error, CombineError::DuplicateLabels { side: "fact", .. }),
        "{error:?}"
    );
    assert_eq!(
        combiner().answer_not_well_formed(&error),
        None,
        "a duplicate label is this workspace's own wiring, never a refusal about the question"
    );
}

#[test]
fn a_missing_leaf_label_is_a_wiring_defect_and_not_a_refusal() {
    // The splitter and the combiner read the label scheme from ONE function, so a fact result with
    // no leaf column is a defect between the two halves rather than anything a caller asked for.
    let plan = sum_plan(true);
    let without_the_leaf = batches_of(
        vec![
            String::from("product_family"),
            link(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
        ],
        vec![vec![text("A"), text("c1"), text("2026-06")]],
    );
    let error = try_combine(&plan, &without_the_leaf, &one_lookup(), UNBOUNDED).expect_err("a missing leaf is refused");
    assert!(matches!(error, CombineError::MissingColumn { side: "fact", .. }), "{error:?}");
    assert_eq!(combiner().answer_not_well_formed(&error), None);
    assert_eq!(combiner().working_set_exhausted(&error), None);
}

#[test]
fn an_answer_that_crosses_the_working_set_ceiling_is_refused_not_truncated() {
    // **The bound, biting on a real operator.** The ceiling sizes this call's own memory pool, so
    // the join build side and the aggregate state reserve against it - and a reservation over it
    // fails immediately with nowhere to spill. A refusal is honest in the way a truncated answer is
    // not: a caller sees that the question is too wide rather than a result that stopped early.
    let plan = sum_plan(true);
    let rows: Vec<Vec<Value>> = (0..2_000_i64)
        .map(|n| fact_row("A", Value::Text(format!("c{n}")), Value::Integer(n)))
        .collect();
    let lookup_rows: Vec<Vec<Value>> = (0..2_000_i64)
        .map(|n| vec![Value::Text(format!("c{n}")), Value::Text(format!("r{n}"))])
        .collect();
    let error =
        try_combine(&plan, &fact(rows), &lookup(lookup_rows), 1).expect_err("a one-byte ceiling refuses the first reservation");
    assert_eq!(
        combiner().working_set_exhausted(&error),
        Some(1),
        "the refusal carries the ceiling that fired: {error:?}"
    );
    assert_eq!(
        combiner().answer_not_well_formed(&error),
        None,
        "the ceiling has its own refusal and must not be classified as one about the data"
    );
}

#[test]
fn the_same_question_under_a_generous_ceiling_answers() {
    // The negative control for the ceiling, and it is not decoration: without it that cell passes
    // just as well against a combine that cannot answer anything at all.
    let plan = sum_plan(true);
    let rows: Vec<Vec<Value>> = (0..2_000_i64)
        .map(|n| fact_row("A", Value::Text(format!("c{n}")), Value::Integer(n)))
        .collect();
    let lookup_rows: Vec<Vec<Value>> = (0..2_000_i64)
        .map(|n| vec![Value::Text(format!("c{n}")), Value::Text(format!("r{n}"))])
        .collect();
    let combined = combined(&plan, &fact(rows), &lookup(lookup_rows), UNBOUNDED);
    assert_eq!(combined.rows().len(), 2_000, "one answer row per remote key");
}

#[test]
fn a_non_finite_leaf_total_is_refused() {
    // The other half of the non-finite check: a float total that leaves the finite range is refused
    // under the metric's own name rather than rendered as `inf` into an answer.
    let plan = sum_plan(true);
    let huge = real(f64::MAX);
    let fact = fact(vec![fact_row("A", text("c1"), huge.clone()), fact_row("A", text("c1"), huge)]);
    assert_eq!(refusal(&plan, &fact, &one_lookup()), FederatedAnswerRefusal::NonFinite);
}

#[test]
fn every_caller_facing_refusal_is_reachable_and_every_wiring_defect_is_not() {
    // **The census over the port's own split**, so a variant that changed sides is a failing cell
    // rather than a caller told to retry something a retry cannot answer. It reads the classifier
    // directly, because the arms reachable through a combine are asserted one by one above and
    // three of these are not reachable through one at all.
    let combiner = combiner();
    for (error, expected) in [
        (CombineError::NonFinite, Some(FederatedAnswerRefusal::NonFinite)),
        (
            CombineError::FloatLinkKey {
                side: "fact",
                arrow_type: String::from("Float64"),
            },
            Some(FederatedAnswerRefusal::FloatLinkKey),
        ),
        (CombineError::AmbiguousLink, Some(FederatedAnswerRefusal::AmbiguousLink)),
        (
            CombineError::LinkTypeMismatch {
                fact: "an exact integer",
                lookup: "text",
            },
            Some(FederatedAnswerRefusal::LinkTypeMismatch),
        ),
        (
            CombineError::NonNumericLeaf {
                label: String::from("0_leaf_0"),
                arrow_type: String::from("Utf8"),
            },
            Some(FederatedAnswerRefusal::NonNumericLeaf),
        ),
        (
            CombineError::MissingColumn {
                side: "fact",
                label: String::from("0_leaf_0"),
            },
            None,
        ),
        (
            CombineError::DuplicateLabels {
                side: "lookup",
                label: String::from("region"),
            },
            None,
        ),
        (
            CombineError::UnsupportedAggregate {
                aggregate: Aggregate::CountDistinct,
            },
            None,
        ),
        (CombineError::Exhausted { ceiling_bytes: 7 }, None),
        (
            CombineError::LinkTypeNotMapped {
                side: "fact",
                label: String::from("0_link"),
                arrow_type: String::from("Binary"),
            },
            None,
        ),
    ] {
        assert_eq!(
            combiner.answer_not_well_formed(&error),
            expected,
            "classification changed for {error:?}"
        );
    }
    assert_eq!(
        combiner.working_set_exhausted(&CombineError::Exhausted { ceiling_bytes: 7 }),
        Some(7),
        "the ceiling is read off the error, because a per-question bound is nowhere else"
    );
}

#[test]
fn a_combiner_keyed_to_a_subject_discriminates_and_discloses_nothing() {
    // `docs/adr/0039` step 5's wiring, and the property is the NEGATIVE one: two subjects on one
    // source must not produce one compute context, because a federation provider's equality is
    // `name() == name() && compute_context() == compute_context()` and equal contexts are what its
    // optimizer fuses into one federated node executed through one of them.
    //
    // **The limit is asserted too**: what the combiner publishes is a digest, never a subject, and a
    // combiner built without one publishes nothing at all.
    use sutura_domain::identity::{ComputeContext, Subject};
    use sutura_domain::model::SourceName;

    let source = SourceName::parse("warehouse").expect("a test source is a source");
    let raw = "user@example.com";
    let one = combiner().for_subject(ComputeContext::of(
        &source,
        &Subject::verified(raw).expect("a test subject is a subject"),
    ));
    let other = combiner().for_subject(ComputeContext::of(
        &source,
        &Subject::verified("other@example.com").expect("a test subject is a subject"),
    ));
    assert_ne!(
        one.compute_context(),
        other.compute_context(),
        "two subjects on one source must not share a compute context"
    );
    let published = one.compute_context().expect("a keyed combiner publishes its digest");
    assert!(!published.contains(raw), "{published}");
    assert!(!published.contains('@'), "{published}");
    assert!(
        !format!("{one:?}").contains(raw),
        "a combiner's own Debug must not carry a subject"
    );
    assert_eq!(
        combiner().compute_context(),
        None,
        "a combiner a root built with no subject publishes nothing"
    );
}

//! A relationship with more than one key: the combine joins on every link part, not on the first.

use datafusion::arrow::datatypes::DataType;
use sutura_domain::catalog::TIME_BUCKET_LABEL;
use sutura_domain::plan::{FederatedAnswerRefusal, FederationCombiner as _, InternalLabel, LinkKind};
use sutura_domain::warehouse::{Real, ResultBatches, Value};

use crate::combine::CombineError;

use super::fixtures::{UNBOUNDED, batches_of, combiner, compound_plan, compound_two_fact_plan, leaf, try_combine_every};

fn text(value: &str) -> Value {
    Value::Text(value.into())
}

fn part(part: usize) -> String {
    InternalLabel::Link { leg: 0, part }.label()
}

fn period() -> String {
    String::from(TIME_BUCKET_LABEL)
}

/// One fact row: `(customer, month key, revenue)`.
type Revenue<'a> = (&'a str, &'a str, i64);

/// One lookup row: `(customer, month key, region)`.
type Region<'a> = (&'a str, &'a str, &'a str);

/// The fact leg's rows, all in June.
fn fact(rows: &[Revenue<'_>]) -> ResultBatches {
    batches_of(
        vec![part(0), part(1), period(), leaf(0)],
        rows.iter()
            .map(|&(customer, month, cents)| vec![text(customer), text(month), text("2026-06"), Value::Integer(cents)])
            .collect(),
    )
}

/// The lookup leg's rows: `(customer, month key, region)`.
fn regions(rows: &[Region<'_>]) -> ResultBatches {
    batches_of(
        vec![part(0), part(1), String::from("region")],
        rows.iter()
            .map(|&(customer, month, region)| vec![text(customer), text(month), text(region)])
            .collect(),
    )
}

fn answered(legs: &[ResultBatches], include_unmatched: bool) -> Vec<Vec<Value>> {
    try_combine_every(&compound_plan(include_unmatched), legs, UNBOUNDED)
        .expect("a compound link answers")
        .to_rows()
        .expect("a combined answer decodes")
        .rows()
        .to_vec()
}

/// One customer on two lookup rows that differ on the second part only: not a repeated tuple, and
/// each fact row meets the lookup row of its own tuple. A join on the first part alone fans both
/// rows out across both regions. A third row shares the second part with the first and differs on
/// the first part, so a probe on the last part alone refuses it.
#[test]
fn a_lookup_tuple_repeated_on_one_part_only_is_answered() {
    let legs = [
        fact(&[("c1", "m1", 10), ("c1", "m2", 20), ("c2", "m1", 30)]),
        regions(&[("c1", "m1", "north"), ("c1", "m2", "south"), ("c2", "m1", "west")]),
    ];
    assert_eq!(
        answered(&legs, true),
        vec![
            vec![text("north"), text("2026-06"), Value::Integer(10)],
            vec![text("south"), text("2026-06"), Value::Integer(20)],
            vec![text("west"), text("2026-06"), Value::Integer(30)],
        ]
    );
}

/// A declared comparison reaches the first link part only: a text second part against an integer one
/// stays exact and is refused, not cast.
#[test]
fn a_declared_comparison_does_not_reach_the_second_link_part() {
    let legs = [
        batches_of(
            vec![part(0), part(1), period(), leaf(0)],
            vec![vec![text("1"), text("5"), text("2026-06"), Value::Integer(10)]],
        ),
        batches_of(
            vec![part(0), part(1), String::from("region")],
            vec![vec![Value::Integer(1), Value::Integer(5), text("north")]],
        ),
    ];
    let plan = compound_plan(true).with_link_comparison(Some(LinkKind::ExactInteger));
    let refused = try_combine_every(&plan, &legs, UNBOUNDED).expect_err("the second part compares exactly");
    assert_eq!(
        combiner().answer_not_well_formed(&refused),
        Some(FederatedAnswerRefusal::LinkTypeMismatch)
    );
}

/// A null part matches no lookup row, as `NULL = x` does on one source: LEFT keeps the fact row with
/// no region, INNER drops it. The second fact row gives the part column its text kind; a join on the
/// first part alone would pair the null-part row with the lookup row too and answer `north` 30.
#[test]
fn a_null_link_part_matches_no_lookup_row() {
    let legs = [
        batches_of(
            vec![part(0), part(1), period(), leaf(0)],
            vec![
                vec![text("c1"), Value::Null, text("2026-06"), Value::Integer(10)],
                vec![text("c1"), text("m1"), text("2026-06"), Value::Integer(20)],
            ],
        ),
        regions(&[("c1", "m1", "north")]),
    ];
    assert_eq!(
        answered(&legs, true),
        vec![
            vec![text("north"), text("2026-06"), Value::Integer(20)],
            vec![Value::Null, text("2026-06"), Value::Integer(10)],
        ]
    );
    assert_eq!(
        answered(&legs, false),
        vec![vec![text("north"), text("2026-06"), Value::Integer(20)]]
    );
}

/// Each part is typed on its own: an integer second part against a text one is refused.
#[test]
fn a_link_part_whose_kind_disagrees_is_refused() {
    let legs = [
        fact(&[("c1", "m1", 10)]),
        batches_of(
            vec![part(0), part(1), String::from("region")],
            vec![vec![text("c1"), Value::Integer(1), text("north")]],
        ),
    ];
    let refused = try_combine_every(&compound_plan(true), &legs, UNBOUNDED).expect_err("a kind mismatch refuses");
    assert_eq!(
        combiner().answer_not_well_formed(&refused),
        Some(FederatedAnswerRefusal::LinkTypeMismatch)
    );
}

/// The second fact's part is typed against the first fact's, part by part: an integer second part
/// beside a text one is refused.
#[test]
fn a_second_fact_link_part_whose_kind_disagrees_is_refused() {
    let second = batches_of(
        vec![part(0), part(1), period(), leaf(1)],
        vec![vec![text("c1"), Value::Integer(1), text("2026-06"), Value::Integer(1)]],
    );
    let legs = [fact(&[("c1", "m1", 150)]), second, regions(&[("c1", "m1", "north")])];
    let refused = try_combine_every(&compound_two_fact_plan(), &legs, UNBOUNDED).expect_err("a kind mismatch refuses");
    assert_eq!(
        combiner().answer_not_well_formed(&refused),
        Some(FederatedAnswerRefusal::LinkTypeMismatch)
    );
}

/// Both facts join on every part and the bucket: `150 / 1` north and `100 / 2` south. The visit
/// counts differ so a fan-out cannot cancel: a join on the first part and the bucket alone pairs each
/// revenue with both visit rows and answers `(150 + 150) / (1 + 2)` and `(100 + 100) / (1 + 2)`.
#[test]
fn a_second_fact_joins_on_every_link_part_and_the_bucket() {
    let second = batches_of(
        vec![part(0), part(1), period(), leaf(1)],
        vec![
            vec![text("c1"), text("m1"), text("2026-06"), Value::Integer(1)],
            vec![text("c1"), text("m2"), text("2026-06"), Value::Integer(2)],
        ],
    );
    let legs = [
        fact(&[("c1", "m1", 150), ("c1", "m2", 100)]),
        second,
        regions(&[("c1", "m1", "north"), ("c1", "m2", "south")]),
    ];
    let answered = try_combine_every(&compound_two_fact_plan(), &legs, UNBOUNDED)
        .expect("a compound cross-model ratio answers")
        .to_rows()
        .expect("a combined answer decodes");
    let real = |value: f64| Value::Real(Real::parse(value).expect("a test real is finite"));
    assert_eq!(
        answered.rows(),
        &[
            vec![text("north"), text("2026-06"), real(150.0)],
            vec![text("south"), text("2026-06"), real(50.0)],
        ]
    );
}

/// The two facts' buckets are one kind of period. A second fact whose calendar came back as another
/// Arrow type than the first's (a timestamp beside a date, here an integer beside text) is refused
/// before any join is built, and not left to the join's own typing.
#[test]
fn a_second_fact_bucketed_as_another_kind_is_refused() {
    let second = batches_of(
        vec![part(0), part(1), period(), leaf(1)],
        vec![vec![text("c1"), text("m1"), Value::Integer(202_606), Value::Integer(1)]],
    );
    let legs = [fact(&[("c1", "m1", 150)]), second, regions(&[("c1", "m1", "north")])];
    let refused = try_combine_every(&compound_two_fact_plan(), &legs, UNBOUNDED).expect_err("two kinds of period refuse");
    assert!(
        matches!(&refused, CombineError::BucketKindMismatch { first, other }
            if *first == format!("{:?}", DataType::Utf8) && *other == format!("{:?}", DataType::Int64)),
        "{refused:?}"
    );
    assert_eq!(
        combiner().answer_not_well_formed(&refused),
        Some(FederatedAnswerRefusal::BucketKindMismatch),
        "the refusal is about the data the legs returned, and a caller can read it"
    );
}

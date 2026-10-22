//! Fan-in: three lookup legs, one joined onto another, each on its own link.

use sutura_domain::catalog::TIME_BUCKET_LABEL;
use sutura_domain::plan::{FederatedAnswerRefusal, InternalLabel, LinkKind};
use sutura_domain::warehouse::Value;

use super::fixtures::{UNBOUNDED, batches_of, fan_in_plan, leaf, try_combine_every};
use crate::combine::DataFusionCombiner;

fn text(value: &str) -> Value {
    Value::Text(value.into())
}

fn link(leg: usize) -> String {
    InternalLabel::link(leg).label()
}

fn named(name: &str) -> String {
    String::from(name)
}

/// The fact leg and the three lookup legs, `crm` given `crm_rows`.
fn legs(crm_rows: Vec<Vec<Value>>) -> Vec<sutura_domain::warehouse::ResultBatches> {
    let fact = batches_of(
        vec![link(0), link(2), named(TIME_BUCKET_LABEL), leaf(0)],
        ["c1", "c2", "c3"]
            .into_iter()
            .zip([10, 20, 40])
            .map(|(customer, cents)| vec![text(customer), text(customer), text("2026-06"), Value::Integer(cents)])
            .collect(),
    );
    let geo = batches_of(
        vec![link(0), link(1), named("region")],
        vec![
            vec![text("c1"), text("r1"), text("north")],
            vec![text("c2"), text("r2"), text("south")],
            vec![text("c3"), text("r9"), text("west")],
        ],
    );
    let crm = batches_of(vec![link(1), named("area")], crm_rows);
    let ops = batches_of(vec![link(2), named("tier")], vec![vec![text("c1"), text("gold")]]);
    vec![fact, geo, crm, ops]
}

/// `crm` joins onto `geo` on `region_code`, INNER, so `c3`, whose region code `crm` lacks, is
/// dropped; `ops` joins onto the fact, LEFT, so `c2`, which `ops` lacks, stays with no tier.
#[test]
fn three_lookup_legs_and_a_chain_combine_on_their_own_links() {
    let crm = vec![vec![text("r1"), text("emea")], vec![text("r2"), text("apac")]];
    let answered = try_combine_every(&fan_in_plan(), &legs(crm), UNBOUNDED)
        .expect("a fan-in combine answers")
        .to_rows()
        .expect("a combined answer decodes");
    assert_eq!(answered.columns(), &["region", "area", "tier", "period", "revenue"]);
    assert_eq!(
        answered.rows(),
        &[
            vec![text("north"), text("emea"), text("gold"), text("2026-06"), Value::Integer(10)],
            vec![text("south"), text("apac"), Value::Null, text("2026-06"), Value::Integer(20)],
        ]
    );
}

/// A leg that joins onto an earlier lookup leg is checked on its own link: two `crm` rows for one
/// region code would double every measure joined through it.
#[test]
fn a_second_lookup_leg_with_two_rows_for_one_link_value_is_ambiguous() {
    let crm = vec![vec![text("r1"), text("emea")], vec![text("r1"), text("apac")]];
    let refused = try_combine_every(&fan_in_plan(), &legs(crm), UNBOUNDED).expect_err("an ambiguous link refuses");
    assert_eq!(
        sutura_domain::plan::FederationCombiner::answer_not_well_formed(
            &DataFusionCombiner::new().expect("a combiner builds"),
            &refused
        ),
        Some(FederatedAnswerRefusal::AmbiguousLink)
    );
}

/// A leg that joins onto an earlier lookup leg is typed on its own link: an integer `crm` link
/// against `geo`'s text link is refused, not joined under coercion.
#[test]
fn a_second_lookup_leg_whose_link_kind_disagrees_with_its_onto_leg_is_refused() {
    let crm = vec![vec![Value::Integer(1), text("emea")]];
    let refused = try_combine_every(&fan_in_plan(), &legs(crm), UNBOUNDED).expect_err("a link kind mismatch refuses");
    assert_eq!(
        sutura_domain::plan::FederationCombiner::answer_not_well_formed(
            &DataFusionCombiner::new().expect("a combiner builds"),
            &refused
        ),
        Some(FederatedAnswerRefusal::LinkTypeMismatch)
    );
}

/// A declaration names the first link only. Here the first link is an integer fact key against
/// `geo`'s text key, which the declaration casts; `crm`'s link against `geo`'s is two text keys,
/// which a declaration that reached it would refuse for want of an anchor, and which compare
/// exactly as they always did.
#[test]
fn a_later_lookup_leg_is_compared_exactly_under_a_declaration() {
    let mut every = legs(vec![vec![text("r1"), text("emea")], vec![text("r2"), text("apac")]]);
    every[0] = batches_of(
        vec![link(0), link(2), named(TIME_BUCKET_LABEL), leaf(0)],
        ["c1", "c2", "c3"]
            .into_iter()
            .zip([1, 2, 3])
            .zip([10, 20, 40])
            .map(|((customer, key), cents)| vec![Value::Integer(key), text(customer), text("2026-06"), Value::Integer(cents)])
            .collect(),
    );
    every[1] = batches_of(
        vec![link(0), link(1), named("region")],
        vec![
            vec![text("1"), text("r1"), text("north")],
            vec![text("2"), text("r2"), text("south")],
            vec![text("3"), text("r9"), text("west")],
        ],
    );
    let plan = fan_in_plan().with_link_comparison(Some(LinkKind::ExactInteger));
    let answered = try_combine_every(&plan, &every, UNBOUNDED)
        .expect("only the first link is declared")
        .to_rows()
        .expect("a combined answer decodes");
    assert_eq!(
        answered.rows(),
        &[
            vec![text("north"), text("emea"), text("gold"), text("2026-06"), Value::Integer(10)],
            vec![text("south"), text("apac"), Value::Null, text("2026-06"), Value::Integer(20)],
        ]
    );
}

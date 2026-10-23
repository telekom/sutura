//! A relationship with more than one key: one link column per key part, and what a plan refuses when
//! the legs do not project the same parts.

use super::fixtures::{
    FACT, FACT_SOURCE, SECOND_FACT_SOURCE, bucket, column, dimension, key, metric, range, source, table, term, terms_for,
};
use crate::federation::Federation;
use crate::measure::Measure;
use crate::model::Aggregate;
use crate::plan::leg::LegPlan;
use crate::plan::{
    AnswerKey, FederatedPlan, FederatedPlanError, InternalLabel, LegAt, LookupLeg, LookupLegs, PlanBindings, PlanColumn, PlanKey,
    ResultLabel, StatementTables,
};

fn part(leg: usize, part: usize) -> PlanKey {
    PlanKey::new(
        ResultLabel::internal(InternalLabel::Link { leg, part }),
        PlanColumn::new(table(FACT), column("customer_key")),
    )
}

fn federation() -> Federation {
    Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents")))
}

fn fact_keyed(keys: Vec<PlanKey>) -> LegPlan {
    LegPlan::Fact {
        source: source(FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys,
        terms: terms_for(&federation()),
        bindings: PlanBindings::none(),
        range: range(),
    }
}

fn second_fact_keyed(keys: Vec<PlanKey>) -> LegPlan {
    LegPlan::Fact {
        source: source(SECOND_FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table("dim_orders")),
        bucket: bucket(),
        keys,
        terms: Vec::new(),
        bindings: PlanBindings::none(),
        range: range(),
    }
}

fn lookup_keyed(keys: Vec<PlanKey>) -> LegPlan {
    LegPlan::Lookup {
        source: source("geo"),
        metric: crate::model::MetricName::parse("revenue").expect("a test metric is a metric"),
        table: table(FACT).into(),
        keys,
        bindings: PlanBindings::none(),
    }
}

fn plan(fact: LegPlan, second: Option<LegPlan>, lookup: Vec<PlanKey>) -> Result<FederatedPlan, FederatedPlanError> {
    FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        fact,
        second,
        LookupLegs::one(lookup_keyed(lookup), true),
        federation(),
        vec![AnswerKey::lookup(0, ResultLabel::dimension(&dimension("region")))],
    )
}

fn region() -> PlanKey {
    key("region", FACT)
}

#[test]
fn a_fact_leg_with_a_link_part_the_lookup_lacks_cannot_be_built() {
    let refused = plan(fact_keyed(vec![part(0, 0), part(0, 1)]), None, vec![part(0, 0), region()]);
    assert_eq!(
        refused,
        Err(FederatedPlanError::LinkPartNotOnLookup {
            side: LegAt::Fact,
            label: InternalLabel::Link { leg: 0, part: 1 }.label(),
        })
    );
}

#[test]
fn a_lookup_leg_with_a_link_part_the_fact_lacks_cannot_be_built() {
    let refused = plan(fact_keyed(vec![part(0, 0)]), None, vec![part(0, 0), part(0, 1), region()]);
    assert_eq!(
        refused,
        Err(FederatedPlanError::KeyNotOnLeg {
            side: LegAt::Fact,
            label: InternalLabel::Link { leg: 0, part: 1 }.label(),
        })
    );
}

#[test]
fn a_lookup_leg_whose_link_parts_skip_one_cannot_be_built() {
    let refused = plan(
        fact_keyed(vec![part(0, 0), part(0, 2)]),
        None,
        vec![part(0, 0), part(0, 2), region()],
    );
    assert_eq!(
        refused,
        Err(FederatedPlanError::LinkPartNotOnLookup {
            side: LegAt::Lookup(0),
            label: InternalLabel::Link { leg: 0, part: 2 }.label(),
        })
    );
}

#[test]
fn a_second_fact_must_project_every_link_part_and_nothing_beside_them() {
    let lookup = vec![part(0, 0), part(0, 1), region()];
    let both = || vec![part(0, 0), part(0, 1)];
    let ok = plan(fact_keyed(both()), None, lookup.clone());
    assert!(ok.is_ok(), "{ok:?}");

    let linked = plan(fact_keyed(both()), Some(second_fact_keyed(both())), lookup.clone());
    assert!(linked.is_ok(), "two facts linking on both parts are a plan: {linked:?}");

    let missing = plan(fact_keyed(both()), Some(second_fact_keyed(vec![part(0, 0)])), lookup.clone());
    assert_eq!(
        missing,
        Err(FederatedPlanError::KeyNotOnLeg {
            side: LegAt::Fact,
            label: InternalLabel::Link { leg: 0, part: 1 }.label(),
        })
    );

    let stray_part = plan(
        fact_keyed(both()),
        Some(second_fact_keyed(vec![part(0, 0), part(0, 1), part(0, 2)])),
        lookup.clone(),
    );
    assert_eq!(
        stray_part,
        Err(FederatedPlanError::FactKeyNotShared {
            label: InternalLabel::Link { leg: 0, part: 2 }.label()
        }),
        "a second fact's part beside the run is the shared-key refusal, not LinkPartNotOnLookup"
    );

    let extra = plan(
        fact_keyed(both()),
        Some(second_fact_keyed(vec![
            part(0, 0),
            part(0, 1),
            key("unrelated", "dim_orders"),
        ])),
        lookup,
    );
    assert_eq!(
        extra,
        Err(FederatedPlanError::FactKeyNotShared {
            label: String::from("unrelated")
        })
    );
}

/// Every part of the link of EACH lookup leg that joins onto the fact is a fact link, not only the
/// first leg's: `geo` has one part and `crm` two, and a second fact lacking `crm`'s second part is
/// refused by that part.
#[test]
fn a_second_fact_must_project_every_link_part_of_every_lookup_leg_onto_the_fact() {
    let lookups = || {
        LookupLegs::parse(vec![
            LookupLeg::onto_fact(lookup_keyed(vec![part(0, 0), region()]), true),
            LookupLeg::onto_fact(lookup_keyed(vec![part(1, 0), part(1, 1), key("segment", FACT)]), true),
        ])
        .expect("two legs onto the fact are a tree")
    };
    let ratio = |second: Vec<PlanKey>| {
        FederatedPlan::new(
            metric("revenue"),
            ResultLabel::measure(&metric("revenue")),
            bucket(),
            fact_keyed(vec![part(0, 0), part(1, 0), part(1, 1)]),
            Some(second_fact_keyed(second)),
            lookups(),
            federation(),
            Vec::new(),
        )
    };
    let built = ratio(vec![part(0, 0), part(1, 0), part(1, 1)]);
    assert!(
        built.is_ok(),
        "both facts carrying every part of both legs are a plan: {built:?}"
    );
    assert_eq!(
        ratio(vec![part(0, 0), part(1, 0)]),
        Err(FederatedPlanError::KeyNotOnLeg {
            side: LegAt::Fact,
            label: InternalLabel::Link { leg: 1, part: 1 }.label(),
        })
    );
}

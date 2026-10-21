//! Fan-in: a plan with more than one lookup leg, a lookup leg that joins onto another, and the
//! results that must stand for every one of them.

use super::fixtures::{FACT, FACT_SOURCE, bucket, column, dimension, key, metric, range, source, table, term};
use crate::federation::Federation;
use crate::measure::Measure;
use crate::model::{Aggregate, SourceName};
use crate::plan::leg::LegPlan;
use crate::plan::{
    AnswerKey, ExecuteError, FederatedPlan, FederatedPlanError, FederationCombiner as _, InternalLabel, LegResult, LegRunner,
    LegSide, Legs, LegsMiswired, LookupLeg, LookupLegs, PlanBindings, PlanColumn, PlanKey, RefusingCombiner, ResultLabel,
    StatementTables,
};
use crate::warehouse::ResultBatches;
use crate::warehouse::arrow::of_rows;

fn link(leg: usize) -> PlanKey {
    PlanKey::new(
        ResultLabel::internal(InternalLabel::link(leg)),
        PlanColumn::new(table(FACT), column("customer_key")),
    )
}

/// A fact leg that projects the link of each lookup leg in `links`.
fn fact_linking(links: &[usize]) -> LegPlan {
    let federation = federation();
    LegPlan::Fact {
        source: source(FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys: links.iter().map(|&leg| link(leg)).collect(),
        terms: super::fixtures::terms_for(&federation),
        bindings: PlanBindings::none(),
        range: range(),
    }
}

/// A lookup leg on `on` that projects its own link, the links of the legs joining onto it, and
/// `keys`.
fn lookup_on(on: &str, links: &[usize], keys: &[&str]) -> LegPlan {
    LegPlan::Lookup {
        source: source(on),
        table: table(FACT).into(),
        keys: links
            .iter()
            .map(|&leg| link(leg))
            .chain(keys.iter().map(|name| key(name, FACT)))
            .collect(),
        bindings: PlanBindings::none(),
    }
}

fn federation() -> Federation {
    Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents")))
}

fn tree(lookups: Vec<LookupLeg>) -> LookupLegs {
    LookupLegs::parse(lookups).expect("the fixture's legs form a tree")
}

fn plan(fact: LegPlan, lookups: Vec<LookupLeg>, keys: Vec<AnswerKey>) -> Result<FederatedPlan, FederatedPlanError> {
    FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        fact,
        None,
        tree(lookups),
        federation(),
        keys,
    )
}

/// A chain that crosses to `geo`, then to `crm`, beside a third leg on `ops` onto the fact.
fn chain_and_fan() -> FederatedPlan {
    plan(
        fact_linking(&[0, 2]),
        vec![
            LookupLeg::onto_fact(lookup_on("geo", &[0, 1], &["region"]), true),
            LookupLeg::onto_lookup(lookup_on("crm", &[1], &["segment"]), 0, false),
            LookupLeg::onto_fact(lookup_on("ops", &[2], &[]), true),
        ],
        vec![
            AnswerKey::lookup(0, ResultLabel::dimension(&dimension("region"))),
            AnswerKey::lookup(1, ResultLabel::dimension(&dimension("segment"))),
        ],
    )
    .expect("three lookup legs, one onto another, are a plan")
}

/// A leg and the leg it joins onto on one data system are one statement, and refused. A chain
/// that crosses away and comes back reads that system in two legs that are not neighbours, and is
/// a plan.
#[test]
fn a_lookup_leg_on_the_source_of_the_leg_it_joins_onto_is_refused_and_a_return_is_not() {
    let onto_geo = plan(
        fact_linking(&[0]),
        vec![
            LookupLeg::onto_fact(lookup_on("geo", &[0, 1], &[]), true),
            LookupLeg::onto_lookup(lookup_on("geo", &[1], &[]), 0, true),
        ],
        Vec::new(),
    );
    assert_eq!(
        onto_geo,
        Err(FederatedPlanError::SameSource {
            source_name: source("geo")
        })
    );
    let back = plan(
        fact_linking(&[0]),
        vec![
            LookupLeg::onto_fact(lookup_on("geo", &[0, 1], &[]), true),
            LookupLeg::onto_lookup(lookup_on(FACT_SOURCE, &[1], &[]), 0, true),
        ],
        Vec::new(),
    )
    .expect("a chain that comes back to the fact's system is two legs there, not one statement");
    let sources: Vec<&str> = back.sources().map(SourceName::as_str).collect();
    assert_eq!(sources, [FACT_SOURCE, "geo", FACT_SOURCE]);
}

/// A leg that joins onto another projects that leg's link, and the leg it joins onto does too.
#[test]
fn a_lookup_leg_whose_parent_does_not_project_its_link_is_refused() {
    let refused = plan(
        fact_linking(&[0]),
        vec![
            LookupLeg::onto_fact(lookup_on("geo", &[0], &[]), true),
            LookupLeg::onto_lookup(lookup_on("crm", &[1], &[]), 0, true),
        ],
        Vec::new(),
    );
    assert_eq!(
        refused,
        Err(FederatedPlanError::KeyNotOnLeg {
            side: crate::plan::LegAt::Lookup(0),
            label: InternalLabel::link(1).label(),
        })
    );
}

fn batches(label: &str) -> ResultBatches {
    of_rows(&[String::from(label)], &[]).expect("an empty result of one column is well formed")
}

/// One result per leg in plan order builds; one missing, or one of the wrong side in a place, is
/// miswired rather than combined.
#[test]
fn a_count_of_results_that_differs_from_the_plans_legs_is_miswired() {
    let plan = chain_and_fan();
    let results: Vec<LegResult> = plan.legs().into_iter().map(|leg| LegResult::of(leg, batches("x"))).collect();
    let legs = Legs::assign(&plan, &results).expect("one result per leg is every leg");
    assert_eq!(legs.lookups().len(), 3);
    let onto: Vec<_> = legs.lookups().iter().map(|(leg, _)| leg.onto()).collect();
    assert_eq!(onto, plan.lookups().legs().iter().map(LookupLeg::onto).collect::<Vec<_>>());
    let short = results.get(..3).expect("four results");
    assert_eq!(
        Legs::assign(&plan, short).map(|_| ()),
        Err(LegsMiswired::Count { legs: 4, results: 3 })
    );
    let long: Vec<LegResult> = results.iter().cloned().chain(results.first().cloned()).collect();
    assert_eq!(
        Legs::assign(&plan, &long).map(|_| ()),
        Err(LegsMiswired::Count { legs: 4, results: 5 })
    );
    let mut swapped = results.clone();
    swapped.swap(0, 1);
    assert_eq!(
        Legs::assign(&plan, &swapped).map(|_| ()),
        Err(LegsMiswired::WrongSide {
            expected: LegSide::Fact,
            found: LegSide::Lookup
        })
    );
    let mut twice = results.clone();
    *twice.get_mut(1).expect("a second result") = results.first().cloned().expect("a fact result");
    assert_eq!(
        Legs::assign(&plan, &twice).map(|_| ()),
        Err(LegsMiswired::WrongSide {
            expected: LegSide::Lookup,
            found: LegSide::Fact
        })
    );
}

/// Answers every leg but `failing`, which answers with its own source's name.
struct FailingOne<'p> {
    failing: &'p LegPlan,
}

impl LegRunner for FailingOne<'_> {
    type Error = String;

    fn run(&self, leg: &LegPlan) -> Result<ResultBatches, Self::Error> {
        if core::ptr::eq(leg, self.failing) {
            return Err(String::from(leg.source().as_str()));
        }
        Ok(batches("x"))
    }
}

/// Each lookup leg in turn does not answer while the others do, the second of the chain among
/// them, and the answer ends with that leg's error: the combine is never reached, so a leg that
/// did not answer cannot read as a hop with no match.
#[test]
fn each_lookup_leg_that_does_not_answer_ends_the_answer_with_no_rows() {
    let plan = chain_and_fan();
    for lookup in plan.lookups().legs() {
        let runner = FailingOne { failing: lookup.leg() };
        let combiner = RefusingCombiner;
        let outcome = combiner.execute(combiner.plan(&plan), &runner, 1 << 20);
        match outcome {
            Err(ExecuteError::Leg(source)) => assert_eq!(source, lookup.leg().source().as_str()),
            other => panic!("a lookup leg on {} did not answer, got {other:?}", lookup.leg().source()),
        }
    }
}

//! The cross-model ratio drill-across cell whose fixture carries the second calendar.

use sutura_domain::plan::{Arm, DrillAcrossPlan};

use super::*;
use crate::resolve::{CrossModel, Resolved};

fn plan_resolved(resolved: &Resolved<'_>) -> Result<Plan, PlanError> {
    crate::plan::plan_resolved(resolved, PlanCeilings::DEFAULT)
}

fn simple(name: &str) -> Metric {
    TwoSources::metric(
        name,
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
    )
}

fn across(answer: Result<Plan, PlanError>) -> DrillAcrossPlan {
    match answer {
        Ok(Plan::DrillAcross(planned)) => *planned,
        other => panic!("expected an arm per metric, got {:?}", other.err()),
    }
}

/// `revenue` beside a cross-model ratio over `visits`, both by `region` on `customers`: the ratio is
/// an arm with a second fact leg beside the plain metric's two. The same pair with no dimension is
/// refused by name, not planned as one statement.
#[test]
fn a_cross_model_ratio_beside_another_metric_is_an_arm_with_its_second_fact() {
    let corpus = TwoSources::new();
    let visits = model("visits", "local", &["visit_id", "customer_key", "day_key"]);
    let calendar = model("calendar", "local", &["day_key", "day"]);
    let facts_calendar = relationship("facts_calendar", ("facts", "day_key"), ("calendar", "day_key"));
    let visits_calendar = relationship("visits_calendar", ("visits", "day_key"), ("calendar", "day_key"));
    let visits_customer = relationship("visits_customer", ("visits", "customer_key"), ("customers", "customer_key"));
    let revenue = simple("revenue");
    let ratio = TwoSources::metric(
        "revenue_per_visit",
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::Count,
                column("visit_id"),
                ModelName::parse("visits").expect("a test model is a model"),
            )),
            zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
        },
    )
    .with_shared_calendar(ModelName::parse("calendar").expect("a test model is a model"));
    let resolved = |by_region: bool| {
        let ratio_alone = Resolution {
            cross: Some(CrossModel {
                second: &visits,
                calendar: &calendar,
                second_calendar: &calendar,
                own_to_calendar: &facts_calendar,
                second_to_calendar: &visits_calendar,
                second_relationships: vec![&visits_customer],
            }),
            ..corpus.asking(vec![&ratio], by_region)
        };
        Resolved {
            whole: corpus.asking(vec![&revenue, &ratio], by_region),
            per_metric: vec![corpus.asking(vec![&revenue], by_region), ratio_alone],
        }
    };
    let planned = across(plan_resolved(&resolved(true)));
    let seconds: Vec<bool> = planned
        .arms()
        .iter()
        .map(|arm| match arm {
            Arm::Joined(joined) => joined.second_fact().is_some(),
            Arm::Local(_) => panic!("both metrics reach a remote dimension"),
        })
        .collect();
    assert_eq!(seconds, [false, true], "only the ratio reads a second fact");
    assert_eq!(planned.legs().len(), 5, "two legs for the sum and three for the ratio");
    // With no remote dimension the ratio has no link to join its two facts on, so it is refused by name.
    let flat = plan_resolved(&resolved(false));
    assert!(
        matches!(
            flat,
            Err(PlanError::Refused(
                RefusalReason::CrossModelRatioWithoutSharedDimension { .. }
            ))
        ),
        "{:?}",
        flat.err()
    );
}

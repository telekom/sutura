//! New-source calendar and source-ceiling cases for cross-model ratios.

use std::collections::BTreeSet;

use sutura_domain::model::SourceName;
use sutura_domain::plan::leg::LegPlan;
use sutura_domain::query::RefusalReason;

use super::{ONE_SOURCE, Placed, ask, ask_within, ceilings, federated, refusal, two_facts};

/// A second fact on another data system is its own statement there, joined to the calendar listed on
/// that system and bucketed and bounded on it: the first fact's calendar is another system's table.
#[test]
fn a_second_fact_on_another_source_joins_the_calendar_listed_there() {
    let plan = federated(ask(
        &two_facts(&Placed {
            visits_on: "remote",
            ..ONE_SOURCE
        }),
        &["region"],
    ));
    let second = plan.second_fact().expect("a cross-model ratio carries a second fact leg");
    let LegPlan::Fact {
        tables,
        bucket,
        bindings,
        ..
    } = second
    else {
        panic!("the second fact is a fact leg");
    };
    assert_eq!(
        second.source().as_str(),
        "remote",
        "the second fact reads its own model's source"
    );
    let joined: Vec<&str> = tables.joins().iter().map(|join| join.table().name().as_str()).collect();
    assert_eq!(
        joined,
        ["dim_visits_side_calendar"],
        "the second fact joins the calendar on its own source"
    );
    assert_eq!(bucket.column().table().as_str(), "dim_visits_side_calendar");
    assert_eq!(
        plan.bucket().column().table().as_str(),
        "dim_calendar",
        "the answer's bucket is the first fact's"
    );
    let bound = format!("{bindings:?}");
    assert!(
        bound.contains("dim_visits_side_calendar") && !bound.contains("dim_calendar"),
        "the second fact's range bounds name its own calendar and no table of another system: {bound}"
    );
}

/// The second fact's own source counts against the deployment's source ceiling: local and `remote`
/// are two systems, and a second fact on a third makes three.
#[test]
fn a_second_fact_on_a_third_source_counts_against_the_source_ceiling() {
    let pinned = two_facts(&Placed {
        visits_on: "visits_remote",
        ..ONE_SOURCE
    });
    let over = ask_within(&pinned, &["region"], ceilings(2, 3, 6));
    assert_eq!(refusal(over), RefusalReason::PlanSpansTooManySources { sources: 3, limit: 2 });
    let within = federated(ask_within(&pinned, &["region"], ceilings(3, 3, 6)));
    let sources: BTreeSet<&str> = within.sources().map(SourceName::as_str).collect();
    assert_eq!(sources, BTreeSet::from(["local", "remote", "visits_remote"]));
}

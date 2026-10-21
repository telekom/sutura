//! A cross-model ratio over more than one lookup leg (`telekom/sutura#1493`): each lookup leg that
//! joins onto the fact is linked to BOTH facts, and a second fact that does not name one
//! relationship for a leg is refused rather than guessed.

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::Definitions;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::{KeyValue, PlanKey};

use super::*;

/// `revenue_per_visit` over `facts` and `visits`, with four remote dimensions: `region` and
/// `billing_region` reach `customers` through two relationships, `tier` reaches `accounts`, and
/// `segment` reaches `segments` through `customers`, a chain that crosses twice. `visits` links to
/// `customers` always, to `accounts` when `visits_account`, and to `customers` a second time when
/// `visits_referrer`.
fn ratio_over_lookups(visits_account: bool, visits_referrer: bool) -> PinnedDefinitions {
    let models = vec![
        model(
            "facts",
            "local",
            &["amount_cents", "customer_key", "billing_key", "account_key", "day_key", "day"],
        ),
        model(
            "visits",
            "local",
            &["visit_id", "customer_key", "referrer_key", "account_key", "day_key"],
        ),
        model("calendar", "local", &["day_key", "day"]),
        model("customers", "remote", &["customer_key", "segment_key", "region_code"]),
        model("accounts", "accounts_remote", &["account_key", "tier"]),
        model("segments", "segments_remote", &["segment_key", "segment_name"]),
    ];
    let mut relationships = vec![
        relationship("facts_customer", ("facts", "customer_key"), ("customers", "customer_key")),
        relationship("facts_billing", ("facts", "billing_key"), ("customers", "customer_key")),
        relationship("facts_account", ("facts", "account_key"), ("accounts", "account_key")),
        relationship("customers_segment", ("customers", "segment_key"), ("segments", "segment_key")),
        relationship("facts_calendar", ("facts", "day_key"), ("calendar", "day_key")),
        relationship("visits_calendar", ("visits", "day_key"), ("calendar", "day_key")),
        relationship("visits_customer", ("visits", "customer_key"), ("customers", "customer_key")),
    ];
    if visits_account {
        relationships.push(relationship(
            "visits_account",
            ("visits", "account_key"),
            ("accounts", "account_key"),
        ));
    }
    if visits_referrer {
        relationships.push(relationship(
            "visits_referrer",
            ("visits", "referrer_key"),
            ("customers", "customer_key"),
        ));
    }
    let metric = Metric::new(
        MetricName::parse("revenue_per_visit").expect("a test metric is a metric"),
        ModelName::parse("facts").expect("a test model is a model"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::Count,
                column("visit_id"),
                ModelName::parse("visits").expect("a test model is a model"),
            )),
            zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
        },
        Vec::new(),
        column("day"),
        BTreeSet::from([Grain::Month]),
        vec![
            declared("region", "region_code", &["facts_customer"]),
            declared("billing_region", "region_code", &["facts_billing"]),
            declared("tier", "tier", &["facts_account"]),
            declared("segment", "segment_name", &["facts_customer", "customers_segment"]),
        ],
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("these dimensions are distinct")
    .with_shared_calendar(ModelName::parse("calendar").expect("a test model is a model"));
    let definitions =
        Definitions::assemble(models, relationships, vec![metric]).expect("both facts reach the calendar they share");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("ratio-over-lookups").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}

/// A key's label, and the table and column it reads.
type ReadLink = (String, String, String);

/// The labels of a leg's keys, in order, beside the physical column each reads.
fn links_of(keys: &[PlanKey]) -> Vec<ReadLink> {
    keys.iter()
        .map(|key| {
            let KeyValue::Column(column) = key.value() else {
                panic!("a link key reads a column");
            };
            (
                String::from(key.label()),
                String::from(column.table().as_str()),
                String::from(column.column().as_str()),
            )
        })
        .collect()
}

fn triple(label: InternalLabel, table: &str, column: &str) -> ReadLink {
    (label.label(), String::from(table), String::from(column))
}

/// Dimensions on two remote systems, both facts linked to each: both fact statements group by the
/// two links, in the order of the legs the planner sorted by path (`accounts` before `customers`),
/// and each lookup leg joins onto the fact.
#[test]
fn a_ratio_grouped_by_two_remote_models_plans_both_facts_on_both_links() {
    let planned = federated(ask(&ratio_over_lookups(true, false), &["region", "tier"]));
    let second = planned.second_fact().expect("a cross-model ratio carries a second fact leg");
    assert_eq!(
        links_of(planned.fact().keys()),
        [
            triple(InternalLabel::link(0), "dim_facts", "account_key"),
            triple(InternalLabel::link(1), "dim_facts", "customer_key"),
        ]
    );
    assert_eq!(
        links_of(second.keys()),
        [
            triple(InternalLabel::link(0), "dim_visits", "account_key"),
            triple(InternalLabel::link(1), "dim_visits", "customer_key"),
        ]
    );
    let onto: Vec<LegAt> = planned.lookups().legs().iter().map(LookupLeg::onto).collect();
    assert_eq!(onto, [LegAt::Fact, LegAt::Fact]);
}

/// A leg that joins onto another lookup leg needs no link on either fact: it joins inside the lookup
/// side both facts share. A second fact keyed by every leg would look for a relationship into
/// `segments` it does not have.
#[test]
fn a_ratio_over_a_chain_that_crosses_twice_links_both_facts_on_the_first_leg_only() {
    let planned = federated(ask(&ratio_over_lookups(false, false), &["region", "segment"]));
    let second = planned.second_fact().expect("a cross-model ratio carries a second fact leg");
    assert_eq!(
        links_of(planned.fact().keys()),
        [triple(InternalLabel::link(0), "dim_facts", "customer_key")]
    );
    assert_eq!(
        links_of(second.keys()),
        [triple(InternalLabel::link(0), "dim_visits", "customer_key")]
    );
    let onto: Vec<LegAt> = planned.lookups().legs().iter().map(LookupLeg::onto).collect();
    assert_eq!(onto, [LegAt::Fact, LegAt::Lookup(0)]);
}

/// A second fact that reaches the first leg's model and not the second's has no link for it, and the
/// ratio is refused by the existing reason that says to regroup.
#[test]
fn a_second_fact_that_does_not_reach_a_lookup_leg_is_refused_as_without_a_shared_dimension() {
    let reason = refusal(ask(&ratio_over_lookups(false, false), &["region", "tier"]));
    assert!(
        matches!(reason, RefusalReason::CrossModelRatioWithoutSharedDimension { ref model, .. } if model.as_str() == "visits"),
        "expected CrossModelRatioWithoutSharedDimension naming visits, got {reason:?}"
    );
}

/// A second fact with two relationships that both fit one lookup leg (a customer and a referrer) does
/// not say which serves it, and the first of them was used.
#[test]
fn a_second_fact_with_two_relationships_into_one_model_is_refused_not_guessed() {
    let reason = refusal(ask(&ratio_over_lookups(false, true), &["region"]));
    assert!(
        matches!(reason, RefusalReason::CrossModelRatioLinkAmbiguous { ref model, .. } if model.as_str() == "visits"),
        "expected CrossModelRatioLinkAmbiguous naming visits, got {reason:?}"
    );
}

/// The first fact reaches `customers` twice (as a customer and as a billing address) and the second
/// fact once: its one relationship would serve both legs, and so the second fact would be joined to
/// the billing region by its customer.
#[test]
fn a_second_fact_relationship_serving_two_lookup_legs_is_refused() {
    let reason = refusal(ask(&ratio_over_lookups(false, false), &["region", "billing_region"]));
    assert!(
        matches!(reason, RefusalReason::CrossModelRatioLinkAmbiguous { ref model, .. } if model.as_str() == "visits"),
        "expected CrossModelRatioLinkAmbiguous naming visits, got {reason:?}"
    );
}

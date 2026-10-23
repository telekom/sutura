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
/// `segment` reaches `segments` through `customers`, a chain that crosses twice. `visits` carries
/// the relationships of `visits_links`, out of `visits_customer`, `visits_account` and
/// `visits_referrer` (a second relationship into `customers`).
fn ratio_over_lookups(visits_links: &[&str]) -> PinnedDefinitions {
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
    ];
    for (name, from, to_model, to) in [
        ("visits_customer", "customer_key", "customers", "customer_key"),
        ("visits_account", "account_key", "accounts", "account_key"),
        ("visits_referrer", "referrer_key", "customers", "customer_key"),
    ] {
        if visits_links.contains(&name) {
            relationships.push(relationship(name, ("visits", from), (to_model, to)));
        }
    }
    pinned(
        "ratio-over-lookups",
        models,
        relationships,
        revenue_per_visit(vec![
            declared("region", "region_code", &["facts_customer"]),
            declared("billing_region", "region_code", &["facts_billing"]),
            declared("tier", "tier", &["facts_account"]),
            declared("segment", "segment_name", &["facts_customer", "customers_segment"]),
        ]),
    )
}

/// `revenue_per_visit`: the sum of `facts.amount_cents` over the count of `visits.visit_id`, bucketed
/// through the shared `calendar`.
fn revenue_per_visit(dimensions: Vec<Dimension>) -> Metric {
    Metric::new(
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
        dimensions,
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("these dimensions are distinct")
    .with_shared_calendar(ModelName::parse("calendar").expect("a test model is a model"))
}

fn pinned(version: &str, models: Vec<Model>, relationships: Vec<Relationship>, metric: Metric) -> PinnedDefinitions {
    let definitions =
        Definitions::assemble(models, relationships, vec![metric]).expect("both facts reach the calendar they share");
    PinnedDefinitions::pin(
        DefinitionVersion::parse(version).expect("a test version is a version"),
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
    let planned = federated(ask(
        &ratio_over_lookups(&["visits_customer", "visits_account"]),
        &["region", "tier"],
    ));
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
    let planned = federated(ask(&ratio_over_lookups(&["visits_customer"]), &["region", "segment"]));
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

/// A second fact that misses one of the lookup legs has no link for it, and the ratio is refused by
/// the existing reason that says to regroup, whichever leg it misses. The legs sort `accounts`
/// before `customers`: a second fact linked to `customers` alone misses the first, one linked to
/// `accounts` alone reaches the first and misses a later one. The cell asserts on the answer itself,
/// because a planner that stops at the first leg answers an error and not a refusal.
#[test]
fn a_second_fact_that_does_not_reach_a_lookup_leg_is_refused_as_without_a_shared_dimension() {
    for visits_links in [&["visits_customer"][..], &["visits_account"]] {
        let answer = ask(&ratio_over_lookups(visits_links), &["region", "tier"]);
        assert!(
            matches!(
                answer,
                Ok(Compiled::Refused {
                    reason: RefusalReason::CrossModelRatioWithoutSharedDimension { ref model, .. }
                }) if model.as_str() == "visits"
            ),
            "{visits_links:?}: expected CrossModelRatioWithoutSharedDimension naming visits, got {answer:?}"
        );
    }
}

/// A second fact with two relationships that both fit one lookup leg (a customer and a referrer) does
/// not say which serves it, and the first of them was used.
#[test]
fn a_second_fact_with_two_relationships_into_one_model_is_refused_not_guessed() {
    let reason = refusal(ask(&ratio_over_lookups(&["visits_customer", "visits_referrer"]), &["region"]));
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
    let reason = refusal(ask(&ratio_over_lookups(&["visits_customer"]), &["region", "billing_region"]));
    assert!(
        matches!(reason, RefusalReason::CrossModelRatioLinkAmbiguous { ref model, .. } if model.as_str() == "visits"),
        "expected CrossModelRatioLinkAmbiguous naming visits, got {reason:?}"
    );
}

/// A relationship from `from` to `to` that joins on every `(origin, target)` pair, in the order given.
fn compound(name: &str, from: &str, to: &str, keys: &[(&str, &str)]) -> Relationship {
    Relationship::new(
        RelationshipName::parse(name).expect("a test relationship is a relationship"),
        ModelName::parse(from).expect("a test model is a model"),
        ModelName::parse(to).expect("a test model is a model"),
        JoinType::ManyToOne,
        JoinKeys::of(
            keys.iter()
                .map(|(origin, target)| JoinKey::Equal {
                    origin: column(origin),
                    target: column(target),
                })
                .collect(),
        )
        .expect("a test relationship declares a key"),
    )
}

/// `revenue_per_visit` with `region` through `customers` and `tier` through `accounts`, each crossing
/// joined on a plain pair of keys. `visits` names its own columns and declares each pair in the other
/// order, so only a match on the crossing's keys reads the right column for each part;
/// `visits_account_keys` is the pair `visits_account` joins on.
fn ratio_over_compound_lookups(visits_account_keys: &[(&str, &str)]) -> PinnedDefinitions {
    let models = vec![
        model(
            "facts",
            "local",
            &[
                "amount_cents",
                "customer_key",
                "customer_site",
                "account_key",
                "account_site",
                "day_key",
                "day",
            ],
        ),
        model(
            "visits",
            "local",
            &["visit_id", "cust_key", "cust_site", "acct_key", "acct_site", "day_key"],
        ),
        model("calendar", "local", &["day_key", "day"]),
        model("customers", "remote", &["customer_key", "customer_site", "region_code"]),
        model("accounts", "accounts_remote", &["account_key", "account_site", "tier"]),
    ];
    let relationships = vec![
        compound(
            "facts_customer",
            "facts",
            "customers",
            &[("customer_key", "customer_key"), ("customer_site", "customer_site")],
        ),
        compound(
            "facts_account",
            "facts",
            "accounts",
            &[("account_key", "account_key"), ("account_site", "account_site")],
        ),
        compound(
            "visits_customer",
            "visits",
            "customers",
            &[("cust_site", "customer_site"), ("cust_key", "customer_key")],
        ),
        compound("visits_account", "visits", "accounts", visits_account_keys),
        relationship("facts_calendar", ("facts", "day_key"), ("calendar", "day_key")),
        relationship("visits_calendar", ("visits", "day_key"), ("calendar", "day_key")),
    ];
    pinned(
        "ratio-over-compound-lookups",
        models,
        relationships,
        revenue_per_visit(vec![
            declared("region", "region_code", &["facts_customer"]),
            declared("tier", "tier", &["facts_account"]),
        ]),
    )
}

/// A compound crossing on each of two lookup legs links BOTH facts on every part of every leg, under
/// the label of its leg and part. The legs sort `accounts` before `customers`; the second fact reads
/// its own columns in the crossing's order, not the order it declares them in.
///
/// Asserts on the answer itself: a planner that matches one part of a key answers an error, and a
/// shared helper that asserts "a plan" would panic before the cell's own assertion.
#[test]
fn a_ratio_over_two_compound_crossings_links_both_facts_on_every_part_of_every_leg() {
    let pinned = ratio_over_compound_lookups(&[("acct_site", "account_site"), ("acct_key", "account_key")]);
    let answer = ask(&pinned, &["region", "tier"]);
    let Ok(Compiled::Federated { plan: planned }) = answer else {
        panic!("expected a federated plan, got {answer:?}");
    };
    let second = planned.second_fact().expect("a cross-model ratio carries a second fact leg");
    let part = |leg, part| InternalLabel::Link { leg, part };
    assert_eq!(
        links_of(planned.fact().keys()),
        [
            triple(part(0, 0), "dim_facts", "account_key"),
            triple(part(0, 1), "dim_facts", "account_site"),
            triple(part(1, 0), "dim_facts", "customer_key"),
            triple(part(1, 1), "dim_facts", "customer_site"),
        ]
    );
    assert_eq!(
        links_of(second.keys()),
        [
            triple(part(0, 0), "dim_visits", "acct_key"),
            triple(part(0, 1), "dim_visits", "acct_site"),
            triple(part(1, 0), "dim_visits", "cust_key"),
            triple(part(1, 1), "dim_visits", "cust_site"),
        ]
    );
}

/// A second fact whose relationship carries only the first part of the compound crossing into
/// `accounts` has no link for the whole key, and the ratio is refused by the reason that says to
/// regroup, not planned on half a key. It asks for `tier` alone, so the one leg the refusal can name
/// is the one whose key is short.
#[test]
fn a_second_fact_relationship_carrying_part_of_a_compound_crossing_is_refused() {
    let answer = ask(&ratio_over_compound_lookups(&[("acct_key", "account_key")]), &["tier"]);
    assert!(
        matches!(
            answer,
            Ok(Compiled::Refused {
                reason: RefusalReason::CrossModelRatioWithoutSharedDimension { ref model, .. }
            }) if model.as_str() == "visits"
        ),
        "expected CrossModelRatioWithoutSharedDimension naming visits, got {answer:?}"
    );
}

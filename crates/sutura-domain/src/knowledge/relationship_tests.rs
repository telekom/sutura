//! A caveat written about a relationship rather than about a metric: what it expands into, who sees
//! each part of it, and every way one does not load.
//!
//! Its own fixture, because the shared one declares no relationship. Three models and two
//! relationships, of which `customer_region` is declared and reached by no dimension; three metrics,
//! two reaching the same `region` dimension through `subscription_customer` - one of them restricted
//! to an audience - and one reaching nothing.

use std::collections::BTreeSet;

use super::tests::{audience_id, body, column, dimension_name, metric_name, note_name, only_caveats};
use super::{Caveat, InconsistentKnowledge, Knowledge, MAX_NOTE_BODY_BYTES, NoteBody, Referent};
use crate::capabilities::MetadataCapabilities;
use crate::catalog::{
    Audience, AudienceGrant, Definitions, Description, Dimension, GrantedAudiences, JoinKey, JoinKeys, Metric, Model,
    Relationship, ViaChain,
};
use crate::measure::{AggregatedColumn, Measure, Term};
use crate::model::{Aggregate, Grain, InvalidIdentifier, JoinType, ModelName, RelationshipName, SourceName, TableName};
use crate::pinned::view::ScopedView;
use crate::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

fn model_name(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model is a model")
}

fn relationship(raw: &str) -> RelationshipName {
    RelationshipName::parse(raw).expect("a test relationship is one")
}

/// `open` reaches `segment` and `region` through `subscription_customer`, `restricted` reaches
/// `region` through it and is visible only to `finance`, and `own_total` reaches nothing.
fn definitions_with(open: &str, restricted: &str) -> Definitions {
    let source = SourceName::parse("local").expect("a test source is a source");
    let table = |raw: &str| TableName::parse(raw).expect("a test table is a table");
    let models = vec![
        Model::new(
            model_name("subscriptions"),
            source.clone(),
            table("fct_subscription_monthly"),
            BTreeSet::from([column("month"), column("mrr_cents"), column("customer_key")]),
            Description::default(),
        ),
        Model::new(
            model_name("customers"),
            source.clone(),
            table("dim_customer"),
            BTreeSet::from([column("customer_key"), column("segment"), column("region")]),
            Description::default(),
        ),
        Model::new(
            model_name("regions"),
            source,
            table("dim_region"),
            BTreeSet::from([column("region")]),
            Description::default(),
        ),
    ];
    let join = |name: &str, origin: &str, target: &str, key: &str| {
        Relationship::new(
            relationship(name),
            model_name(origin),
            model_name(target),
            JoinType::ManyToOne,
            JoinKeys::of(vec![JoinKey::Equal {
                origin: column(key),
                target: column(key),
            }])
            .expect("a test relationship declares one key"),
        )
    };
    let through_customer = |name: &str| {
        Dimension::new(
            dimension_name(name),
            column(name),
            Some(ViaChain::of(vec![relationship("subscription_customer")]).expect("a one-hop chain has hops")),
            None,
            Description::default(),
        )
    };
    let metric = |name: &str, dimensions: Vec<Dimension>, audience: Audience| {
        Metric::new(
            metric_name(name),
            model_name("subscriptions"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_cents")))),
            Vec::new(),
            column("month"),
            BTreeSet::from([Grain::Month]),
            dimensions,
            None,
            Description::default(),
            audience,
        )
        .expect("these fixture dimensions are distinct")
    };
    let finance = Audience::Restricted(AudienceGrant::parse(BTreeSet::from([audience_id("finance")])).expect("one id grants"));
    Definitions::assemble(
        models,
        vec![
            join("subscription_customer", "subscriptions", "customers", "customer_key"),
            join("customer_region", "customers", "regions", "region"),
        ],
        vec![
            metric(
                open,
                vec![through_customer("segment"), through_customer("region")],
                Audience::Open,
            ),
            metric(restricted, vec![through_customer("region")], finance),
            metric("own_total", Vec::new(), Audience::Open),
        ],
    )
    .expect("the relationship fixture is consistent")
}

fn definitions() -> Definitions {
    definitions_with("recurring_revenue", "churned_revenue")
}

fn through(name: &str, relationships: &[&str]) -> Caveat {
    Caveat::new(note_name(name), Vec::new(), body()).through(relationships.iter().map(|raw| relationship(raw)).collect())
}

fn about_own_total(name: &str) -> Caveat {
    Caveat::new(
        note_name(name),
        vec![Referent::Metric {
            metric: metric_name("own_total"),
        }],
        body(),
    )
}

fn refused(notes: Vec<Caveat>) -> InconsistentKnowledge {
    Knowledge::assemble(&definitions(), only_caveats(notes)).expect_err("these caveats must not load")
}

#[test]
fn a_caveat_about_a_relationship_becomes_one_caveat_per_metric_reaching_a_dimension_through_it() {
    let knowledge = Knowledge::assemble(
        &definitions(),
        only_caveats(vec![through("customer_is_current", &["subscription_customer"])]),
    )
    .expect("two metrics reach a dimension through it");
    let dimensions = |metric: &str, names: &[&str]| -> Vec<Referent> {
        names
            .iter()
            .map(|name| Referent::Dimension {
                metric: metric_name(metric),
                dimension: dimension_name(name),
            })
            .collect()
    };
    let expanded = knowledge
        .caveats()
        .iter()
        .map(|(name, note)| (String::from(name.as_str()), note.about().to_vec()))
        .collect::<Vec<_>>();
    // One per metric, about exactly the dimensions it reaches through the relationship. Nothing is
    // filed under the authored name, and `own_total`, which reaches no dimension at all, gets none.
    assert_eq!(
        expanded,
        vec![
            (
                String::from("customer_is_current__churned_revenue"),
                dimensions("churned_revenue", &["region"])
            ),
            (
                String::from("customer_is_current__recurring_revenue"),
                dimensions("recurring_revenue", &["region", "segment"])
            ),
        ]
    );
    assert!(
        knowledge
            .caveats()
            .values()
            .all(|note| note.body() == &body() && note.relationships().is_empty()),
        "each part carries the authored body and no relationship of its own: {knowledge:?}"
    );
}

#[test]
fn a_caller_who_sees_one_of_two_metrics_sharing_a_dimension_gets_exactly_that_metrics_caveat() {
    let definitions = definitions();
    let knowledge = Knowledge::assemble(
        &definitions,
        only_caveats(vec![through("customer_is_current", &["subscription_customer"])]),
    )
    .expect("two metrics reach a dimension through it");
    let pinned = PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        knowledge.clone(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::produced(&definitions, &knowledge)),
        ),
    )
    .expect("the test definitions hash");
    let names = |view: &ScopedView<'_>| -> Vec<String> {
        knowledge
            .scoped(view)
            .caveats()
            .keys()
            .map(|name| String::from(name.as_str()))
            .collect()
    };
    // No `finance` grant, so `churned_revenue` is invisible - and so is its caveat, though this
    // caller sees `region`, the dimension both caveats are about. Its name carries the metric's.
    let ungranted = ScopedView::granted_by(&pinned, GrantedAudiences::none());
    assert_eq!(names(&ungranted), vec!["customer_is_current__recurring_revenue"]);
    // The `Debug` form holds every name the scoped bundle does, and the bundle is all a renderer of
    // it reads - so nothing rendered from it can name the hidden metric either.
    let shown = format!("{:?}", knowledge.scoped(&ungranted));
    assert!(shown.contains("recurring_revenue"), "{shown}");
    assert!(!shown.contains("churned_revenue"), "{shown}");
    assert_eq!(
        names(&ScopedView::granted_by(
            &pinned,
            GrantedAudiences::of(BTreeSet::from([audience_id("finance")]))
        )),
        vec![
            "customer_is_current__churned_revenue",
            "customer_is_current__recurring_revenue"
        ]
    );
    assert_eq!(names(&ScopedView::everything(&pinned)).len(), 2);
}

#[test]
fn a_caveat_about_an_undeclared_relationship_does_not_load() {
    assert_eq!(
        refused(vec![through("customer_is_current", &["subscription_custmer"])]),
        InconsistentKnowledge::CaveatUnknownRelationship {
            name: note_name("customer_is_current"),
            relationship: relationship("subscription_custmer"),
        }
    );
}

#[test]
fn a_caveat_about_a_relationship_no_dimension_is_reached_through_does_not_load() {
    // Declared, and no `via` names it: the caveat would expand into nothing and be read by nobody.
    assert_eq!(
        refused(vec![through("region_is_current", &["customer_region"])]),
        InconsistentKnowledge::CaveatRelationshipReachesNoMetric {
            name: note_name("region_is_current"),
            relationship: relationship("customer_region"),
        }
    );
}

#[test]
fn a_caveat_about_both_a_metric_and_a_relationship_does_not_load() {
    let mixed = about_own_total("customer_is_current").through(vec![relationship("subscription_customer")]);
    assert_eq!(
        refused(vec![mixed]),
        InconsistentKnowledge::CaveatAboutAndThroughRelationships {
            name: note_name("customer_is_current"),
        }
    );
}

#[test]
fn a_derived_caveat_name_past_the_identifier_limit_does_not_load() {
    // Forty-six characters: under `churned_revenue` the derived name is exactly 63, the most an
    // identifier may have, and under `recurring_revenue` it is 65.
    let name = "c".repeat(46);
    assert_eq!(
        refused(vec![through(&name, &["subscription_customer"])]),
        InconsistentKnowledge::DerivedCaveatNotAName {
            caveat: note_name(&name),
            metric: metric_name("recurring_revenue"),
            cause: InvalidIdentifier::TooLong { len: 65, limit: 63 },
        }
    );
}

#[test]
fn a_derived_caveat_name_that_is_already_a_caveat_does_not_load() {
    // Taken by an authored caveat, listed AFTER the relationship caveat - every authored name is
    // claimed before anything is derived, so the order the documents arrived in decides nothing.
    assert_eq!(
        refused(vec![
            through("customer_is_current", &["subscription_customer"]),
            about_own_total("customer_is_current__recurring_revenue"),
        ]),
        InconsistentKnowledge::DerivedCaveatNameTaken {
            name: note_name("customer_is_current__recurring_revenue"),
            caveat: note_name("customer_is_current"),
            metric: metric_name("recurring_revenue"),
        }
    );
    // Taken by another derived name: `joins` under `net__revenue` and `joins__net` under `revenue`
    // are both `joins__net__revenue`.
    assert_eq!(
        Knowledge::assemble(
            &definitions_with("revenue", "net__revenue"),
            only_caveats(vec![
                through("joins", &["subscription_customer"]),
                through("joins__net", &["subscription_customer"]),
            ]),
        )
        .expect_err("two derived caveats cannot share a name"),
        InconsistentKnowledge::DerivedCaveatNameTaken {
            name: note_name("joins__net__revenue"),
            caveat: note_name("joins__net"),
            metric: metric_name("revenue"),
        }
    );
}

#[test]
fn the_byte_cap_counts_what_a_relationship_caveat_expands_into() {
    // Seven bodies at the per-note cap are under the aggregate cap once each, and over it twice each.
    let full = NoteBody::parse("x".repeat(MAX_NOTE_BODY_BYTES)).expect("exactly the cap is a body");
    let once: Vec<Caveat> = (0..7)
        .map(|index| {
            Caveat::new(
                note_name(&format!("note_{index}")),
                vec![Referent::Metric {
                    metric: metric_name("own_total"),
                }],
                full.clone(),
            )
        })
        .collect();
    drop(Knowledge::assemble(&definitions(), only_caveats(once)).expect("seven bodies once each fit"));
    let twice: Vec<Caveat> = (0..7)
        .map(|index| {
            Caveat::new(note_name(&format!("note_{index}")), Vec::new(), full.clone())
                .through(vec![relationship("subscription_customer")])
        })
        .collect();
    assert!(
        matches!(refused(twice), InconsistentKnowledge::KnowledgeTooLarge { .. }),
        "fourteen bodies are over the cap"
    );
}

//! The compound join: an ordered, non-empty list of typed [`JoinKey`] terms rather than one column
//! pair, and the two checks that generalise from a single key to the whole set.
//!
//! A concept module rather than a split of [`super`], [`chain`](super::chain)'s own reason.

use super::*;

#[test]
fn a_join_key_list_must_name_at_least_one_key() {
    assert_eq!(JoinKeys::of(Vec::new()).unwrap_err(), InvalidJoinKeys::Empty);
}

/// `daily_usage` (daily) joined to `subscriptions` (monthly) on the subscription key AND the usage
/// date truncated to the month - the running example the compound join exists for.
fn usage_and_snapshot() -> ModelsAndJoins {
    let usage = model("daily_usage", "local", &["usage_date", "subscription_key"]);
    let subscriptions = model("subscriptions", "local", &["month", "subscription_key", "contract_term"]);
    let compound = Relationship::new(
        relationship_name("daily_usage_subscription"),
        model_name("daily_usage"),
        model_name("subscriptions"),
        JoinKeys::of(vec![
            JoinKey::Equal {
                origin: column("subscription_key"),
                target: column("subscription_key"),
            },
            JoinKey::TruncatedEqual {
                origin: column("usage_date"),
                grain: Grain::Month,
                target: column("month"),
            },
        ])
        .expect("two keys are a non-empty list"),
        JoinType::ManyToOne,
    );
    (vec![usage, subscriptions], vec![compound])
}

#[test]
fn a_compound_relationship_assembles_and_a_dimension_reaches_through_it() {
    let (models, joins) = usage_and_snapshot();
    let m = metric(
        "data_gb",
        vec![dimension(
            "contract_term",
            "contract_term",
            Some(&["daily_usage_subscription"]),
            None,
        )],
    );
    Definitions::assemble(models, joins, vec![m]).expect("two typed key terms are a relationship this catalog accepts");
}

/// The load check reasons over EVERY key, not just the first - a column named wrong on the SECOND
/// term must be caught exactly like one named wrong on the first.
#[test]
fn an_unknown_column_on_the_second_key_term_is_refused_naming_it() {
    let (models, _) = usage_and_snapshot();
    let broken = vec![Relationship::new(
        relationship_name("daily_usage_subscription"),
        model_name("daily_usage"),
        model_name("subscriptions"),
        JoinKeys::of(vec![
            JoinKey::Equal {
                origin: column("subscription_key"),
                target: column("subscription_key"),
            },
            JoinKey::TruncatedEqual {
                origin: column("usage_date"),
                grain: Grain::Month,
                target: column("not_there"),
            },
        ])
        .expect("two keys are a non-empty list"),
        JoinType::ManyToOne,
    )];
    assert_eq!(
        Definitions::assemble(models, broken, vec![]).unwrap_err(),
        InconsistentDefinitions::RelationshipUnknownColumn {
            relationship: relationship_name("daily_usage_subscription"),
            model: model_name("subscriptions"),
            column: column("not_there"),
        }
    );
}

/// A relationship whose two models sit on different sources may still cross - that is the
/// federated case - but only with exactly one `equal` key, because the plan layer's link between a
/// fact leg and a lookup leg carries one value. More than one key is refused at load rather than
/// reaching a splitter that has no way to render it.
#[test]
fn a_relationship_crossing_sources_with_more_than_one_key_is_refused() {
    let local = model("daily_usage", "local", &["usage_date", "subscription_key"]);
    let remote = model("subscriptions", "elsewhere", &["month", "subscription_key"]);
    let crossing = vec![Relationship::new(
        relationship_name("daily_usage_subscription"),
        model_name("daily_usage"),
        model_name("subscriptions"),
        JoinKeys::of(vec![
            JoinKey::Equal {
                origin: column("subscription_key"),
                target: column("subscription_key"),
            },
            JoinKey::TruncatedEqual {
                origin: column("usage_date"),
                grain: Grain::Month,
                target: column("month"),
            },
        ])
        .expect("two keys are a non-empty list"),
        JoinType::ManyToOne,
    )];
    assert_eq!(
        Definitions::assemble(vec![local, remote], crossing, vec![]).unwrap_err(),
        InconsistentDefinitions::CrossSourceRelationshipNotSingleEqualKey {
            relationship: relationship_name("daily_usage_subscription"),
            origin: model_name("daily_usage"),
            origin_source: SourceName::parse("local").expect("a test source is a source"),
            target: model_name("subscriptions"),
            target_source: SourceName::parse("elsewhere").expect("a test source is a source"),
        }
    );
}

/// The same refusal for a single key that is NOT `equal` - a lone `truncated_equal` still promises
/// something the splitter's single carried value cannot render, so it does not earn the single-key
/// exemption either.
#[test]
fn a_relationship_crossing_sources_with_a_single_truncated_key_is_refused() {
    let local = model("daily_usage", "local", &["usage_date"]);
    let remote = model("subscriptions", "elsewhere", &["month"]);
    let crossing = vec![Relationship::new(
        relationship_name("daily_usage_subscription"),
        model_name("daily_usage"),
        model_name("subscriptions"),
        JoinKeys::of(vec![JoinKey::TruncatedEqual {
            origin: column("usage_date"),
            grain: Grain::Month,
            target: column("month"),
        }])
        .expect("one key is a non-empty list"),
        JoinType::ManyToOne,
    )];
    assert_eq!(
        Definitions::assemble(vec![local, remote], crossing, vec![]).unwrap_err(),
        InconsistentDefinitions::CrossSourceRelationshipNotSingleEqualKey {
            relationship: relationship_name("daily_usage_subscription"),
            origin: model_name("daily_usage"),
            origin_source: SourceName::parse("local").expect("a test source is a source"),
            target: model_name("subscriptions"),
            target_source: SourceName::parse("elsewhere").expect("a test source is a source"),
        }
    );
}

/// The exemption itself: one `equal` key crossing sources is the federated case, and it must still
/// assemble - a check that refused every crossing relationship would refuse federation outright.
#[test]
fn a_relationship_crossing_sources_with_a_single_equal_key_still_assembles() {
    let local = model("orders", "local", &["customer_id"]);
    let remote = model("customers", "elsewhere", &["id"]);
    let crossing = vec![Relationship::new(
        relationship_name("order_customer"),
        model_name("orders"),
        model_name("customers"),
        JoinKeys::single_equal(column("customer_id"), column("id")),
        JoinType::ManyToOne,
    )];
    Definitions::assemble(vec![local, remote], crossing, vec![]).expect("a single equal key may cross sources");
}

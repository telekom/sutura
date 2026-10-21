//! A relationship's declared key comparison: `telekom/sutura#1482`.

use super::*;
use crate::catalog::InvalidComparison;
use crate::plan::LinkKind;

fn equal(origin: &str, target: &str) -> JoinKey {
    JoinKey::Equal {
        origin: column(origin),
        target: column(target),
    }
}

fn orders_customer(keys: JoinKeys) -> Relationship {
    Relationship::new(
        relationship_name("orders_customer"),
        model_name("orders"),
        model_name("customers"),
        JoinType::ManyToOne,
        keys,
    )
}

#[test]
fn a_comparison_on_a_compound_relationship_is_refused() {
    let compound = JoinKeys::of(vec![equal("customer_id", "id"), equal("order_date", "since")]).expect("two keys are a set");
    assert_eq!(
        orders_customer(compound).with_comparison(LinkKind::ExactInteger).unwrap_err(),
        InvalidComparison::CompoundKey { keys: 2 }
    );
    let single = orders_customer(JoinKeys::single(equal("customer_id", "id")))
        .with_comparison(LinkKind::Text)
        .expect("one key takes a comparison");
    assert_eq!(single.comparison(), Some(LinkKind::Text));
}

#[test]
fn a_relationship_with_no_comparison_serialises_without_the_field() {
    let bare = orders_customer(JoinKeys::single(equal("customer_id", "id")));
    assert_eq!(bare.comparison(), None);
    let written = serde_json::to_value(&bare).expect("a relationship serialises");
    assert!(
        written.get("comparison").is_none(),
        "None must not enter the digest's bytes: {written}"
    );
    let declared =
        serde_json::to_value(bare.with_comparison(LinkKind::ExactDecimal).expect("one key")).expect("a relationship serialises");
    assert_eq!(declared.get("comparison"), Some(&serde_json::json!("exact_decimal")));
}

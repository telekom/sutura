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

/// A column's name and the type it declares, if any.
type Declared<'a> = (&'a str, Option<&'a str>);

/// A model on `local` whose columns each declare the type given, or none.
fn typed(name: &str, columns: &[Declared<'_>]) -> Model {
    Model::new(
        model_name(name),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse(name).expect("a test table is a table"),
        columns.iter().map(|&(column_name, declared)| {
            Column::new(
                column(column_name),
                declared.map(|raw| ColumnType::parse(raw).expect("a test column type is a column type")),
                Description::default(),
                None,
            )
        }),
        Description::default(),
    )
}

/// `orders.customer_id` against `customers.id`, declared `origin` and `target`, joined on `key`
/// and declared to compare as `declared`.
fn loading(
    key: JoinKey,
    origin: Option<&str>,
    target: Option<&str>,
    declared: LinkKind,
) -> Result<Definitions, InconsistentDefinitions> {
    Definitions::assemble(
        vec![
            typed("orders", &[("customer_id", origin), ("opened_on", origin)]),
            typed("customers", &[("id", target), ("month", target)]),
        ],
        vec![declared_on(key, declared)],
        vec![],
    )
}

fn declared_on(key: JoinKey, declared: LinkKind) -> Relationship {
    orders_customer(JoinKeys::single(key))
        .with_comparison(declared)
        .expect("one key takes a comparison")
}

fn contradicted(declared: LinkKind) -> InconsistentDefinitions {
    InconsistentDefinitions::RelationshipComparisonContradicted {
        relationship: relationship_name("orders_customer"),
        declared,
    }
}

/// A declaration the declared column types contradict refuses the whole catalog at load, in either
/// column order, and a declaration they do not contradict loads: both kinds known and joined by the
/// declaration, one kind known and reached by it, or no kind known and left to the run.
#[test]
fn a_comparison_contradicting_a_declared_column_type_is_refused_at_load() {
    let key = || equal("customer_id", "id");
    for (origin, target, declared) in [
        (Some("VARCHAR"), Some("VARCHAR"), LinkKind::ExactInteger),
        (Some("BIGINT"), Some("VARCHAR"), LinkKind::ExactDecimal),
        (Some("VARCHAR"), Some("BIGINT"), LinkKind::Text),
        (Some("DATE"), Some("DATE"), LinkKind::ExactInteger),
    ] {
        assert_eq!(
            loading(key(), origin, target, declared).unwrap_err(),
            contradicted(declared),
            "{origin:?} against {target:?} declared {declared:?}"
        );
    }
    for (origin, target, declared) in [
        (Some("VARCHAR"), Some("BIGINT"), LinkKind::ExactInteger),
        (Some("BIGINT"), Some("VARCHAR"), LinkKind::ExactInteger),
        (Some("BIGINT"), Some("NUMERIC(38,9)"), LinkKind::ExactDecimal),
        (Some("DATE"), Some("DATE"), LinkKind::Date),
        (None, Some("BIGINT"), LinkKind::ExactInteger),
        (Some("VARCHAR"), None, LinkKind::ExactInteger),
        (None, None, LinkKind::ExactInteger),
        (Some("DOUBLE"), Some("BIGINT"), LinkKind::ExactInteger),
    ] {
        loading(key(), origin, target, declared)
            .unwrap_or_else(|refusal| panic!("{origin:?} against {target:?} declared {declared:?} should load: {refusal}"));
    }
}

/// A decimal never reads as an integer, and a single known side is held to the declaration too.
#[test]
fn a_declared_decimal_column_under_exact_integer_is_refused_at_load() {
    let key = || equal("customer_id", "id");
    for (origin, target) in [
        (Some("BIGINT"), Some("NUMERIC(38,9)")),
        (Some("NUMERIC(38,9)"), Some("BIGINT")),
        (Some("NUMERIC(38,9)"), None),
        (None, Some("NUMERIC(38,9)")),
    ] {
        assert_eq!(
            loading(key(), origin, target, LinkKind::ExactInteger).unwrap_err(),
            contradicted(LinkKind::ExactInteger),
            "{origin:?} against {target:?}"
        );
    }
    // A key truncated to a grain is a date to the combiner, whatever its origin is declared as.
    let truncated = || JoinKey::TruncatedEqual {
        origin: column("opened_on"),
        grain: Grain::Month,
        target: column("month"),
    };
    assert_eq!(
        loading(truncated(), Some("BIGINT"), Some("VARCHAR"), LinkKind::ExactInteger).unwrap_err(),
        contradicted(LinkKind::ExactInteger)
    );
    loading(truncated(), Some("TIMESTAMP"), Some("DATE"), LinkKind::Date)
        .expect("a truncated key under `date` over a date target loads");
}

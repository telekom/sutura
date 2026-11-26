use super::*;

/// `telekom/sutura#2082` ask 1: `on_invalid_comment: omit` drops an unusable native column comment
/// instead of refusing the whole load - the column is still served, with no description.
#[test]
fn on_invalid_comment_omit_drops_an_unusable_native_column_comment_and_serves_the_column_without_prose() {
    let crlf = SparseReader(Dictionary::new(
        vec![table("orders", vec!["order_id".to_owned()], None).with_column_metadata([(
            "order_id".to_owned(),
            crate::ColumnMetadata::new(None, Some("The order's id.\r\nOne per row.".to_owned())),
        )])],
        Vec::new(),
    ));
    let pinned = RdbmsCatalog::new(name(), version(), crlf)
        .with_invalid_comment_policy(descriptions::InvalidDescriptionPolicy::Omit)
        .load()
        .expect("omit serves the column without prose rather than refusing");
    let orders = pinned
        .definitions()
        .models()
        .get(&ModelName::parse("orders").expect("a test model is a model"))
        .expect("orders is a model");
    let order_id = sutura_domain::model::ColumnName::parse("order_id").expect("a test column is a column");
    let column = orders.column(&order_id).expect("order_id is declared");
    assert_eq!(column.description(), "");
}

/// `telekom/sutura#2082` ask 3: the explicit C1-sanitization opt-in replaces a C1 control character
/// (here U+0096) with U+FFFD before the comment is validated, so a comment that would otherwise
/// refuse (or be omitted) is instead served, sanitized.
#[test]
fn control_character_sanitization_lets_a_c1_control_comment_through_replaced() {
    let c1 = SparseReader(Dictionary::new(
        vec![table("orders", vec!["order_id".to_owned()], None).with_column_metadata([(
            "order_id".to_owned(),
            crate::ColumnMetadata::new(None, Some("The order\u{0096}s id.".to_owned())),
        )])],
        Vec::new(),
    ));
    let pinned = RdbmsCatalog::new(name(), version(), c1)
        .with_control_character_sanitization(true)
        .load()
        .expect("a sanitized C1 control is a loadable description");
    let orders = pinned
        .definitions()
        .models()
        .get(&ModelName::parse("orders").expect("a test model is a model"))
        .expect("orders is a model");
    let order_id = sutura_domain::model::ColumnName::parse("order_id").expect("a test column is a column");
    let column = orders.column(&order_id).expect("order_id is declared");
    assert_eq!(column.description(), "The order\u{FFFD}s id.");
}

/// `telekom/sutura#2082` ask 1: `on_invalid_comment: omit` drops an unusable native table comment
/// instead of refusing the whole load - the table is still served, with no description.
#[test]
fn on_invalid_comment_omit_drops_an_unusable_native_table_comment_and_serves_the_table_without_prose() {
    let crlf = SparseReader(Dictionary::new(
        vec![table(
            "orders",
            vec!["order_id".to_owned()],
            Some("Orders placed by customers.\r\nOne row per order.".to_owned()),
        )],
        Vec::new(),
    ));
    let pinned = RdbmsCatalog::new(name(), version(), crlf)
        .with_invalid_comment_policy(descriptions::InvalidDescriptionPolicy::Omit)
        .load()
        .expect("omit serves the table without prose rather than refusing");
    let orders = pinned
        .definitions()
        .models()
        .get(&ModelName::parse("orders").expect("a test model is a model"))
        .expect("orders is a model");
    assert_eq!(orders.description(), "");
}

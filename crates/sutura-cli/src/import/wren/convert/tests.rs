use super::convert;
use crate::import::wren::wire::Manifest;

/// A manifest naming one instance of every refusal kind alongside one mapped model,
/// relationship and pair of metrics - the unit-level twin of the golden fixture under
/// `crates/sutura-cli/tests/fixtures/wren-import`, small enough to read in one sitting.
const MANIFEST: &str = r#"{
  "catalog": "c",
  "schema": "s",
  "models": [
    {
      "name": "orders",
      "tableReference": { "catalog": null, "schema": null, "table": "orders" },
      "primaryKey": "id",
      "rowLevelAccessControls": [
        { "name": "regional_only", "requiredProperties": [], "condition": "region = 'x'" }
      ],
      "columns": [
        { "name": "id", "type": "BIGINT", "isCalculated": false, "notNull": true, "isHidden": false },
        { "name": "customer_id", "type": "BIGINT", "isCalculated": false, "notNull": true, "isHidden": false },
        { "name": "amount_cents", "type": "BIGINT", "isCalculated": false, "notNull": true, "isHidden": false },
        { "name": "order_date", "type": "DATE", "isCalculated": false, "notNull": true, "isHidden": false },
        { "name": "amount_dollars", "type": "DOUBLE", "isCalculated": true, "expression": "amount_cents / 100", "notNull": false, "isHidden": false },
        { "name": "notes", "type": "VARCHAR", "isCalculated": false, "notNull": false, "isHidden": false,
          "columnLevelAccessControl": { "name": "finance_only", "requiredProperties": [], "operator": "EQUALS", "threshold": "1" } },
        { "name": "customer", "type": "customers", "relationship": "orders_customer", "isCalculated": false, "notNull": false, "isHidden": true }
      ]
    },
    {
      "name": "customers",
      "tableReference": { "catalog": null, "schema": null, "table": "customers" },
      "columns": [
        { "name": "id", "type": "BIGINT", "isCalculated": false, "notNull": true, "isHidden": false }
      ]
    },
    {
      "name": "recent_orders",
      "refSql": "SELECT * FROM orders WHERE order_date > now() - interval '30' day",
      "columns": []
    }
  ],
  "relationships": [
    { "name": "orders_customer", "models": ["orders", "customers"], "joinType": "MANY_TO_ONE", "condition": "orders.customer_id = customers.id" },
    { "name": "orders_customer_legacy", "models": ["orders", "customers"], "joinType": "MANY_TO_ONE", "condition": "orders.customer_id = customers.id OR orders.status = 'legacy'" },
    { "name": "customer_products_history", "models": ["customers", "orders"], "joinType": "MANY_TO_MANY", "condition": "customers.id = orders.customer_id" }
  ],
  "views": [
    { "name": "recent_orders_view", "statement": "SELECT * FROM orders" }
  ],
  "cubes": [
    {
      "name": "sales_summary",
      "baseObject": "orders",
      "measures": [
        { "name": "total_revenue", "expression": "SUM(amount_cents)", "type": "INTEGER" },
        { "name": "average_order_value", "expression": "SUM(amount_cents) / COUNT(DISTINCT customer_id)", "type": "DOUBLE" },
        { "name": "inflated_revenue", "expression": "SUM(amount_cents) * 1.1", "type": "DOUBLE" }
      ],
      "dimensions": [
        { "name": "status", "expression": "status", "type": "VARCHAR" },
        { "name": "region_shout", "expression": "upper(region_code)", "type": "VARCHAR" }
      ],
      "timeDimensions": [
        { "name": "order_date", "expression": "order_date", "type": "DATE" }
      ],
      "hierarchies": { "calendar": ["order_date"] }
    }
  ]
}"#;

fn refused(kinds_and_names: &[(&str, &str)], kind: &str, name: &str) -> bool {
    kinds_and_names.iter().any(|(k, n)| *k == kind && *n == name)
}

#[test]
fn every_named_refusal_kind_fires_once_over_the_fixture_manifest() {
    let manifest: Manifest = serde_json::from_str(MANIFEST).expect("the fixture manifest is valid JSON for this wire shape");
    let converted = convert(&manifest);

    assert_eq!(
        converted.models.len(),
        2,
        "orders and customers map; recent_orders is a named refusal"
    );
    assert_eq!(converted.relationships.len(), 1, "only orders_customer recognises");
    assert_eq!(
        converted.metrics.len(),
        2,
        "total_revenue and average_order_value recognise; inflated_revenue does not"
    );

    let refusals: Vec<(&str, &str)> = converted.refusals.iter().map(|r| (r.kind, r.name.as_str())).collect();

    assert!(refused(&refusals, "ref_sql_model", "recent_orders"));
    assert!(refused(&refusals, "sql_view", "recent_orders_view"));
    assert!(refused(&refusals, "calculated_column", "orders.amount_dollars"));
    assert!(refused(&refusals, "column_access_control", "orders.notes"));
    assert!(refused(&refusals, "row_access_control", "orders.regional_only"));
    assert!(refused(&refusals, "many_to_many_relationship", "customer_products_history"));
    assert!(refused(&refusals, "unrecognised_join_condition", "orders_customer_legacy"));
    assert!(refused(&refusals, "free_sql_dimension", "sales_summary.region_shout"));
    assert!(refused(&refusals, "free_sql_measure", "sales_summary.inflated_revenue"));
    assert!(
        refusals
            .iter()
            .any(|(k, n)| *k == "cube_hierarchy" && n.starts_with("sales_summary"))
    );

    assert!(
        converted
            .notes
            .iter()
            .any(|note| note.contains("orders.customer") && note.contains("orders_customer")),
        "the relationship-navigation column is a note, not a refusal: {:?}",
        converted.notes
    );
}

#[test]
fn a_cube_with_no_recognised_time_dimension_is_refused_whole() {
    let manifest: Manifest = serde_json::from_str(
        r#"{
        "catalog": "c", "schema": "s",
        "models": [{ "name": "orders", "tableReference": { "table": "orders" },
            "columns": [{ "name": "amount_cents", "type": "BIGINT", "isCalculated": false, "notNull": true, "isHidden": false }] }],
        "cubes": [{ "name": "sales", "baseObject": "orders",
            "measures": [{ "name": "total", "expression": "SUM(amount_cents)", "type": "INTEGER" }],
            "timeDimensions": [{ "name": "bad", "expression": "date_trunc('month', order_date)", "type": "DATE" }] }]
    }"#,
    )
    .expect("valid JSON");
    let converted = convert(&manifest);
    assert!(converted.metrics.is_empty());
    assert!(
        converted
            .refusals
            .iter()
            .any(|r| r.kind == "cube_without_time_dimension" && r.name == "sales")
    );
}

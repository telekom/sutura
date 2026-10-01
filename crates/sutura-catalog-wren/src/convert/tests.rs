use super::{Converted, convert};
use crate::wire::Manifest;

/// A manifest naming one instance of most refusal kinds alongside two mapped models, one
/// relationship and one cube of two measures - the unit-level twin of the golden fixture under
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
        { "name": "inflated_revenue", "expression": "SUM(amount_cents) * 1.1", "type": "DOUBLE" },
        { "name": "average_order_value", "expression": "SUM(amount_cents) / COUNT(DISTINCT customer_id)", "type": "DOUBLE" }
      ],
      "dimensions": [
        { "name": "status", "expression": "status", "type": "VARCHAR" },
        { "name": "region_shout", "expression": "upper(region_code)", "type": "VARCHAR" }
      ],
      "timeDimensions": [
        { "name": "order_date", "expression": "order_date", "type": "DATE" },
        { "name": "shipped_on", "expression": "shipped_date", "type": "DATE" }
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
    let [cube] = converted.cubes.as_slice() else {
        panic!("one cube, not {}", converted.cubes.len());
    };
    let measures: Vec<&str> = cube.measures.iter().map(|measure| measure.name.as_str()).collect();
    assert_eq!(
        measures,
        ["total_revenue", "average_order_value"],
        "inflated_revenue refuses only itself"
    );
    assert_eq!(cube.time_column.as_str(), "order_date", "the first time dimension is kept");

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
    assert!(refused(&refusals, "cube_extra_time_dimension", "sales_summary.shipped_on"));
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
    assert!(converted.cubes.is_empty());
    assert!(
        converted
            .refusals
            .iter()
            .any(|r| r.kind == "cube_without_time_dimension" && r.name == "sales")
    );
}

/// The loader fails the whole catalog over a measure name that is not a metric name, or whose
/// `<cube>_<measure>` join is past the identifier cap - so each is refused here as itself alone.
#[test]
fn a_measure_name_the_cube_document_cannot_hold_refuses_only_that_measure() {
    let long = "a".repeat(60);
    let manifest: Manifest = serde_json::from_str(&format!(
        r#"{{
        "catalog": "c", "schema": "s",
        "models": [{{ "name": "orders", "tableReference": {{ "table": "orders" }}, "columns": [] }}],
        "cubes": [{{ "name": "sales", "baseObject": "orders",
            "measures": [
              {{ "name": "total", "expression": "SUM(amount_cents)", "type": "INTEGER" }},
              {{ "name": "1st", "expression": "SUM(amount_cents)", "type": "INTEGER" }},
              {{ "name": "{long}", "expression": "SUM(amount_cents)", "type": "INTEGER" }}
            ],
            "timeDimensions": [{{ "name": "order_date", "expression": "order_date", "type": "DATE" }}] }}]
    }}"#
    ))
    .expect("valid JSON");
    let converted = convert(&manifest);
    let [cube] = converted.cubes.as_slice() else {
        panic!("one cube, not {}", converted.cubes.len());
    };
    let measures: Vec<&str> = cube.measures.iter().map(|measure| measure.name.as_str()).collect();
    assert_eq!(measures, ["total"]);
    let refusals: Vec<(&str, &str)> = converted.refusals.iter().map(|r| (r.kind, r.name.as_str())).collect();
    assert!(refused(&refusals, "unusable_identifier", "sales.1st"), "{refusals:?}");
    assert!(
        refused(&refusals, "unusable_identifier", &format!("sales.{long}")),
        "{refusals:?}"
    );
}

/// A cube's name and its measures' names.
type Cube<'a> = (&'a str, &'a [&'a str]);

/// Each `(cube, measures)` as a wren cube on one mapped `orders` model, with one time dimension and
/// every measure `SUM(amount_cents)` - so a name is the only thing a refusal can be about.
fn converted_cubes(cubes: &[Cube<'_>]) -> Converted {
    let cubes: Vec<String> = cubes
        .iter()
        .map(|(name, measures)| {
            let measures: Vec<String> = measures
                .iter()
                .map(|measure| format!(r#"{{ "name": "{measure}", "expression": "SUM(amount_cents)", "type": "INTEGER" }}"#))
                .collect();
            format!(
                r#"{{ "name": "{name}", "baseObject": "orders", "measures": [{}],
                    "timeDimensions": [{{ "name": "order_date", "expression": "order_date", "type": "DATE" }}] }}"#,
                measures.join(", ")
            )
        })
        .collect();
    let manifest: Manifest = serde_json::from_str(&format!(
        r#"{{ "catalog": "c", "schema": "s",
            "models": [{{ "name": "orders", "tableReference": {{ "table": "orders" }}, "columns": [] }}],
            "cubes": [{}] }}"#,
        cubes.join(", ")
    ))
    .expect("valid JSON");
    convert(&manifest)
}

type Planned<'a> = (&'a str, Vec<&'a str>);

/// Each planned cube's name and its measures' names, in order.
fn planned(converted: &Converted) -> Vec<Planned<'_>> {
    converted
        .cubes
        .iter()
        .map(|cube| {
            (
                cube.name.as_str(),
                cube.measures.iter().map(|measure| measure.name.as_str()).collect(),
            )
        })
        .collect()
}

fn kinds_and_names(converted: &Converted) -> Vec<(&str, &str)> {
    converted.refusals.iter().map(|r| (r.kind, r.name.as_str())).collect()
}

/// The loader refuses `measures: []`, so a cube left with no measure is not planned at all - and
/// each measure says why.
#[test]
fn a_cube_whose_every_measure_is_refused_is_not_planned() {
    let converted = converted_cubes(&[("sales", &["1st", "2nd"])]);
    assert!(planned(&converted).is_empty(), "{:?}", planned(&converted));
    let refusals = kinds_and_names(&converted);
    assert!(refused(&refusals, "unusable_identifier", "sales.1st"), "{refusals:?}");
    assert!(refused(&refusals, "unusable_identifier", "sales.2nd"), "{refusals:?}");
}

#[test]
fn a_cube_name_that_is_not_a_metric_name_refuses_the_cube_by_name() {
    let converted = converted_cubes(&[("1sales", &["total"]), ("sales", &["total"])]);
    assert_eq!(planned(&converted), [("sales", vec!["total"])]);
    assert_eq!(kinds_and_names(&converted), [("unusable_identifier", "1sales")]);
}

/// Both cubes would be written as `metrics/sales.md`, and the second would replace the first.
#[test]
fn a_second_cube_of_one_name_is_refused_by_name() {
    let converted = converted_cubes(&[("sales", &["total"]), ("sales", &["margin"])]);
    assert_eq!(planned(&converted), [("sales", vec!["total"])]);
    assert_eq!(kinds_and_names(&converted), [("duplicate_cube", "sales")]);
}

/// A measure repeated in one cube, and two cubes whose joins meet (`sales` + `net_total`, and
/// `sales_net` + `total`): the loader fails the whole catalog over a metric declared twice, so the
/// later measure is refused, alone.
#[test]
fn a_measure_converting_to_an_already_planned_metric_refuses_only_itself() {
    let converted = converted_cubes(&[
        ("sales", &["total", "total", "net_total"]),
        ("sales_net", &["total", "margin"]),
    ]);
    assert_eq!(
        planned(&converted),
        [("sales", vec!["total", "net_total"]), ("sales_net", vec!["margin"])]
    );
    assert_eq!(
        kinds_and_names(&converted),
        [("duplicate_metric", "sales.total"), ("duplicate_metric", "sales_net.total")]
    );
}

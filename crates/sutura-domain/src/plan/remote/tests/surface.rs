//! A remote leg may only name what its metric declares: its joins, dimensions, links, filters and
//! the relationship it reaches its calendar through.

use super::*;

/// The receiver's verdict, read by the cell itself: no helper here asserts a refusal.
fn judged(json: Value, params: Vec<ParamValue>) -> Result<LegPlan, LegNotCertified> {
    parsed(json, &pinned(ORDINARY), params)
}

fn lookup() -> (Value, Vec<ParamValue>) {
    let (leg, params) = region_leg(&pinned(ORDINARY));
    (sent(&leg, &pinned(ORDINARY)), params)
}

fn refunds() -> (Value, Vec<ParamValue>) {
    let (leg, params) = refund_leg(&pinned(ORDINARY));
    (sent(&leg, &pinned(ORDINARY)), params)
}

fn key(label: &str, of_table: &str, of_column: &str) -> Value {
    json!({"label": label, "column": {"table": of_table, "column": of_column}})
}

fn requested(of_table: &str, of_column: &str, param: usize) -> Value {
    json!({"origin": "requested", "predicate": {"equals": {"column": {"table": of_table, "column": of_column}, "param": param}}})
}

/// `net_revenue`'s own leg, bucketed on the shared calendar reached through `joins`.
fn own_ratio_leg(joins: &[&str]) -> (Value, Vec<ParamValue>) {
    let (mut json, params) = fact();
    let sender = pinned(ORDINARY);
    let (refund, _) = refund_leg(&sender);
    let refund = sent(&refund, &sender);
    json["metric"] = refund["metric"].clone();
    json["joins"] = json!(joins);
    json["bucket"] = refund["bucket"].clone();
    json["keys"] = json!([]);
    json["terms"][0]["label"] = json!("0_leaf_0");
    for at in 0..2 {
        json["filters"][at] = refund["filters"][at].clone();
    }
    (json, params)
}

#[test]
fn a_dimension_key_names_only_a_declared_dimension_on_its_own_column() {
    let (json, params) = fact();
    for (label, of_table, of_column) in [
        ("channel_name", "channels", "id"),
        ("region_name", "channels", "channel_name"),
        ("status", "orders", "status"),
    ] {
        let mut tampered = json.clone();
        tampered["keys"][0] = key(label, of_table, of_column);
        let refused = judged(tampered, params.clone());
        assert!(
            matches!(refused, Err(LegNotCertified::DimensionNotDeclared { ref dimension, .. }) if dimension.as_str() == label),
            "{label} on {of_table}.{of_column}: {refused:?}"
        );
    }
}

#[test]
fn a_fact_leg_joins_only_what_its_metric_declares() {
    let (mut revenue, params) = fact();
    revenue["joins"] = json!(["orders_channel", "orders_calendar"]);
    let refused = judged(revenue, params);
    assert!(
        matches!(refused, Err(LegNotCertified::RelationshipNotCertified { ref relationship }) if *relationship == relationship_name("orders_calendar")),
        "{refused:?}"
    );
    for joins in [
        json!(["refunds_calendar", "refunds_order"]),
        json!(["refunds_order", "orders_calendar"]),
    ] {
        let (mut refund, params) = refunds();
        refund["joins"] = joins.clone();
        let refused = judged(refund, params);
        assert!(
            matches!(refused, Err(LegNotCertified::RelationshipNotCertified { ref relationship }) if *relationship == relationship_name("refunds_order")),
            "{joins}: {refused:?}"
        );
    }
}

#[test]
fn a_second_model_leg_keys_nothing_on_the_metrics_own_table() {
    let (mut json, params) = refunds();
    json["joins"] = json!(["refunds_calendar", "refunds_order"]);
    json["keys"]
        .as_array_mut()
        .expect("keys are a list")
        .push(key("0_leaf_0", "orders", "amount_cents"));
    let refused = judged(json, params);
    assert!(
        matches!(refused, Err(LegNotCertified::RelationshipNotCertified { ref relationship }) if *relationship == relationship_name("refunds_order")),
        "{refused:?}"
    );
}

#[test]
fn a_measure_key_names_only_a_column_the_metric_measures_on_its_root() {
    let (mut json, params) = fact();
    json["keys"]
        .as_array_mut()
        .expect("keys are a list")
        .push(key("0_leaf_0", "orders", "customer_id"));
    let refused = judged(json, params);
    assert!(
        matches!(refused, Err(LegNotCertified::TermNotMetrics { ref column, .. }) if *column == self::column("customer_id")),
        "{refused:?}"
    );
    let (mut json, params) = lookup();
    json["keys"]
        .as_array_mut()
        .expect("keys are a list")
        .push(key("0_leaf_0", "customers", "region_id"));
    let refused = judged(json, params);
    assert!(matches!(refused, Err(LegNotCertified::KeyNotDeclared { .. })), "{refused:?}");
}

#[test]
fn a_link_key_names_only_a_key_of_a_relationship_between_two_sources() {
    let (json, params) = fact();
    for of_column in ["status", "channel_id"] {
        let mut tampered = json.clone();
        tampered["keys"][1] = key("0_link", "orders", of_column);
        let refused = judged(tampered, params.clone());
        assert!(
            matches!(refused, Err(LegNotCertified::LinkNotDeclared { ref column, .. }) if column.as_str() == of_column),
            "{of_column}: {refused:?}"
        );
    }
    let (mut json, params) = lookup();
    json["keys"][0] = key("0_link", "customers", "region_id");
    let refused = judged(json, params);
    assert!(
        matches!(refused, Err(LegNotCertified::LinkNotDeclared { ref column, .. }) if column.as_str() == "region_id"),
        "{refused:?}"
    );
}

#[test]
fn a_filter_names_only_a_filterable_dimension_and_its_allowed_values() {
    let (json, params) = lookup();
    let mut elsewhere = params.clone();
    elsewhere[0] = ParamValue::Text(String::from("MARS"));
    let refused = judged(json.clone(), elsewhere);
    assert!(matches!(refused, Err(LegNotCertified::ValueNotAllowed { .. })), "{refused:?}");
    let mut defined = json;
    defined["filters"][0]["origin"] = json!("definition");
    let refused = judged(defined, params);
    assert!(
        matches!(refused, Err(LegNotCertified::FilterNotDeclared { .. })),
        "{refused:?}"
    );

    let (json, mut params) = fact();
    params.push(ParamValue::Text(String::from("web")));
    for (of_table, of_column) in [("channels", "channel_name"), ("orders", "customer_id")] {
        let mut tampered = json.clone();
        tampered["filters"]
            .as_array_mut()
            .expect("filters are a list")
            .push(requested(of_table, of_column, 3));
        let refused = judged(tampered, params.clone());
        assert!(
            matches!(refused, Err(LegNotCertified::FilterNotDeclared { ref column, .. }) if column.as_str() == of_column),
            "{of_table}.{of_column}: {refused:?}"
        );
    }
    let mut refunded = json;
    let mut status = refunded["filters"][2].clone();
    status["predicate"]["equals"]["param"] = json!(3);
    refunded["filters"].as_array_mut().expect("filters are a list").push(status);
    params[3] = ParamValue::Text(String::from("refunded"));
    let refused = judged(refunded, params);
    assert!(
        matches!(refused, Err(LegNotCertified::FilterNotDeclared { ref column, .. }) if column.as_str() == "status"),
        "{refused:?}"
    );
}

#[test]
fn a_lookup_leg_is_rooted_only_on_a_model_its_metric_crosses_into() {
    let (region, _) = lookup();
    for (source_name, root) in [("sales", "orders"), ("crm", "regions")] {
        let json = json!({
            "leg": "lookup",
            "source": source_name,
            "metric": region["metric"],
            "table": root,
            "joins": [],
            "keys": [key("0_link", root, "id")],
            "filters": [],
        });
        let refused = judged(json, Vec::new());
        assert!(
            matches!(refused, Err(LegNotCertified::TableNotCertified { .. })),
            "{source_name}.{root}: {refused:?}"
        );
    }
    let json = json!({
        "leg": "lookup",
        "source": "sales",
        "metric": region["metric"],
        "table": "orders",
        "joins": [],
        "keys": [key("0_link", "orders", "amount_cents")],
        "filters": [],
    });
    let refused = judged(json, Vec::new());
    assert!(
        matches!(refused, Err(LegNotCertified::TableNotCertified { .. })),
        "{refused:?}"
    );
}

#[test]
fn a_lookup_leg_reads_only_its_own_metrics_dimensions() {
    let (mut json, params) = lookup();
    let (refund, _) = refunds();
    json["metric"] = refund["metric"].clone();
    let refused = judged(json, params);
    assert!(
        matches!(refused, Err(LegNotCertified::TableNotCertified { ref table, .. }) if table.to_string() == "customers"),
        "{refused:?}"
    );
}

/// A lookup leg of `metric` on `accounts`, linked on its crossing and keyed on `dimension`.
fn on_accounts(metric: &Value, dimension: &str) -> Value {
    json!({
        "leg": "lookup",
        "source": "crm",
        "metric": metric,
        "table": "accounts",
        "joins": [],
        "keys": [key("0_link", "accounts", "id"), key(dimension, "accounts", dimension)],
        "filters": [],
    })
}

#[test]
fn a_lookup_leg_keys_only_its_own_metrics_dimensions_where_two_metrics_cross() {
    let (refund, _) = refunds();
    let accepted = judged(on_accounts(&refund["metric"], "segment"), Vec::new());
    assert!(accepted.is_ok(), "{accepted:?}");
    let refused = judged(on_accounts(&refund["metric"], "tier"), Vec::new());
    assert!(
        matches!(refused, Err(LegNotCertified::DimensionNotDeclared { ref dimension, .. }) if dimension.as_str() == "tier"),
        "{refused:?}"
    );
}

#[test]
fn a_lookup_leg_is_rooted_only_where_its_own_metrics_chains_cross() {
    let (refund, _) = refunds();
    let json = json!({
        "leg": "lookup",
        "source": "crm",
        "metric": refund["metric"],
        "table": "customers",
        "joins": [],
        "keys": [key("0_link", "customers", "id")],
        "filters": [],
    });
    let refused = judged(json, Vec::new());
    assert!(
        matches!(refused, Err(LegNotCertified::TableNotCertified { ref table, .. }) if table.to_string() == "customers"),
        "{refused:?}"
    );
}

#[test]
fn a_fact_leg_links_only_on_a_crossing_its_own_metric_declares() {
    let (mut json, params) = own_ratio_leg(&["orders_calendar"]);
    json["keys"] = json!([key("0_link", "orders", "customer_id")]);
    let refused = judged(json, params);
    assert!(
        matches!(refused, Err(LegNotCertified::LinkNotDeclared { ref column, .. }) if column.as_str() == "customer_id"),
        "{refused:?}"
    );
    let (mut json, params) = own_ratio_leg(&["orders_calendar"]);
    json["keys"] = json!([key("0_link", "orders", "account_id")]);
    let accepted = judged(json, params);
    assert!(accepted.is_ok(), "{accepted:?}");
}

#[test]
fn the_metrics_own_leg_of_a_ratio_joins_its_calendar_through_the_planners_relationship() {
    let (json, params) = own_ratio_leg(&["orders_calendar"]);
    let accepted = parsed(json, &pinned(ORDINARY), params);
    assert!(accepted.is_ok(), "{accepted:?}");
}

#[test]
fn a_fact_leg_joins_its_calendar_only_through_the_relationship_the_planner_picks() {
    let (json, params) = own_ratio_leg(&["orders_ship_calendar"]);
    let refused = judged(json, params);
    assert!(
        matches!(refused, Err(LegNotCertified::CalendarNotJoined { ref metric }) if *metric == metric_name("net_revenue")),
        "{refused:?}"
    );
}

/// The refund leg of `net_revenue`, linked on `links` and nothing else.
fn refunds_linked(links: &[(&str, &str)]) -> (Value, Vec<ParamValue>) {
    let (mut json, params) = refunds();
    json["keys"] = Value::Array(
        links
            .iter()
            .map(|&(label, of_column)| key(label, "refunds", of_column))
            .collect(),
    );
    (json, params)
}

#[test]
fn a_second_fact_links_on_every_part_of_a_compound_crossing_in_the_crossings_order() {
    let (json, params) = refunds_linked(&[("0_link", "ref_a"), ("0_link_1", "ref_b")]);
    let accepted = judged(json, params);
    assert!(accepted.is_ok(), "{accepted:?}");
}

#[test]
fn a_second_fact_link_part_off_its_metrics_crossing_is_refused() {
    let (json, params) = refunds_linked(&[("0_link", "ref_a"), ("0_link_1", "amount_cents")]);
    let refused = judged(json, params);
    assert!(
        matches!(refused, Err(LegNotCertified::LinkNotDeclared { ref column, .. }) if column.as_str() == "amount_cents"),
        "{refused:?}"
    );
    let (json, params) = refunds_linked(&[("0_link", "ref_b"), ("0_link_1", "ref_a")]);
    let refused = judged(json, params);
    assert!(
        matches!(refused, Err(LegNotCertified::LinkNotDeclared { ref column, .. }) if column.as_str() == "ref_b"),
        "{refused:?}"
    );
}

#[test]
fn a_link_carries_every_part_of_its_crossing_each_once() {
    for links in [vec![("0_link", "ref_a")], vec![("0_link", "ref_a"), ("0_link", "ref_a")]] {
        let (json, params) = refunds_linked(&links);
        let refused = judged(json, params);
        assert!(
            matches!(refused, Err(LegNotCertified::LinkIncomplete { leg: 0 })),
            "{links:?}: {refused:?}"
        );
    }
    let (mut json, params) = own_ratio_leg(&["orders_calendar"]);
    json["keys"] = json!([key("0_link", "orders", "ref_a")]);
    let refused = judged(json, params);
    assert!(
        matches!(refused, Err(LegNotCertified::LinkIncomplete { leg: 0 })),
        "{refused:?}"
    );
}

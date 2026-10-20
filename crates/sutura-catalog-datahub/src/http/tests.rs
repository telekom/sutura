//! A dataset urn's name read as a model and a table, and the urn a relationship endpoint must name.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{HttpReaderError, harvest_dataset, one_endpoint};

fn urn(name: &str) -> String {
    format!("urn:li:dataset:(urn:li:dataPlatform:bigquery,{name},PROD)")
}

fn entity(name: &str) -> Value {
    json!({
        "urn": urn(name),
        "schemaMetadata": { "value": { "fields": [{ "fieldPath": "order_id" }] } },
    })
}

#[test]
fn a_qualified_urn_names_its_model_by_the_last_segment_and_its_table_in_full() {
    let dataset = harvest_dataset(&entity("acme.sales.orders"), &mut BTreeMap::new()).expect("a qualified urn is read");
    assert_eq!((dataset.name(), dataset.table()), ("orders", "acme.sales.orders"));
}

#[test]
fn a_bare_urn_is_both_its_model_and_its_table() {
    let dataset = harvest_dataset(&entity("orders"), &mut BTreeMap::new()).expect("a bare urn is read");
    assert_eq!((dataset.name(), dataset.table()), ("orders", "orders"));
}

#[test]
fn two_datasets_with_one_last_segment_are_refused_naming_both_urns() {
    let mut models = BTreeMap::new();
    drop(harvest_dataset(&entity("acme.sales.orders"), &mut models).expect("the first dataset is read"));
    let refusal =
        harvest_dataset(&entity("acme.archive.orders"), &mut models).expect_err("a second dataset for one model is refused");
    assert!(
        matches!(&refusal, HttpReaderError::TwoDatasetsOneModel { model, first, second }
            if model == "orders" && *first == urn("acme.sales.orders") && *second == urn("acme.archive.orders")),
        "{refusal:?}"
    );
    let message = refusal.to_string();
    assert!(
        message.contains(&urn("acme.sales.orders")) && message.contains(&urn("acme.archive.orders")),
        "{message}"
    );
}

#[test]
fn an_endpoint_joins_a_model_only_through_the_urn_it_was_read_from() {
    let mut models = BTreeMap::new();
    drop(harvest_dataset(&entity("acme.sales.orders"), &mut models).expect("the dataset is read"));
    let relationship = json!({ "from": urn("acme.sales.orders"), "to": urn("acme.archive.orders") });
    assert_eq!(one_endpoint(&relationship, "from", &models).expect("a dataset urn"), "orders");
    assert_eq!(
        one_endpoint(&relationship, "to", &models).expect("a dataset urn"),
        "acme.archive.orders"
    );
}

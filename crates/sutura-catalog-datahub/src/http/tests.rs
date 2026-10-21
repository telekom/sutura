//! A dataset urn read as a platform and a table, and the urn a relationship endpoint must name.

use std::collections::BTreeSet;

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
fn a_qualified_urn_is_read_as_its_platform_and_its_whole_table() {
    let dataset = harvest_dataset(&entity("acme.sales.orders"), &mut BTreeSet::new()).expect("a qualified urn is read");
    assert_eq!((dataset.platform(), dataset.table()), ("bigquery", "acme.sales.orders"));
}

#[test]
fn two_datasets_with_one_last_segment_are_both_read() {
    let mut read = BTreeSet::new();
    let sales = harvest_dataset(&entity("acme.sales.orders"), &mut read).expect("the first dataset is read");
    let archive = harvest_dataset(&entity("acme.archive.orders"), &mut read).expect("the second dataset is read");
    assert_eq!((sales.table(), archive.table()), ("acme.sales.orders", "acme.archive.orders"));
    assert_eq!(read, BTreeSet::from([urn("acme.archive.orders"), urn("acme.sales.orders")]));
}

/// `one_endpoint` on a relationship whose `to` names `to`, after `acme.sales.orders` is read.
fn endpoint_after_one_read(to: &str) -> Result<String, HttpReaderError> {
    let mut read = BTreeSet::new();
    drop(harvest_dataset(&entity("acme.sales.orders"), &mut read)?);
    one_endpoint(&json!({ "to": to }), "to", &read)
}

fn assert_not_read(to: &str) {
    let refusal = endpoint_after_one_read(to).expect_err("an endpoint naming another urn joins no model");
    assert!(
        matches!(&refusal, HttpReaderError::EndpointNotADatasetRead { field: "to", urn } if urn == to),
        "{refusal:?}"
    );
}

#[test]
fn an_endpoint_joins_a_model_only_through_the_urn_it_was_read_from() {
    assert_eq!(
        endpoint_after_one_read(&urn("acme.sales.orders")).expect("the read urn"),
        "bigquery__acme__sales__orders"
    );
    assert_not_read(&urn("acme.archive.orders"));
    assert_not_read(&urn("orders"));
}

#[test]
fn an_endpoint_naming_a_bare_urn_on_another_platform_is_refused() {
    assert_not_read("urn:li:dataset:(urn:li:dataPlatform:postgres,orders,PROD)");
}

#[test]
fn an_endpoint_naming_a_bare_urn_on_the_read_platform_is_refused() {
    assert_not_read(&urn("orders"));
}

#[test]
fn an_endpoint_on_each_platform_joins_the_model_read_from_its_own_urn() {
    let bigquery = urn("acme.sales.customers");
    let postgres = "urn:li:dataset:(urn:li:dataPlatform:postgres,orders_db.public.customers,PROD)";
    let mut read = BTreeSet::new();
    drop(harvest_dataset(&entity("acme.sales.customers"), &mut read).expect("the bigquery dataset is read"));
    drop(
        harvest_dataset(
            &json!({ "urn": postgres, "schemaMetadata": { "value": { "fields": [{ "fieldPath": "order_id" }] } } }),
            &mut read,
        )
        .expect("the postgres dataset is read"),
    );
    let joined = |to: &str| one_endpoint(&json!({ "to": to }), "to", &read).expect("a read urn joins its model");
    assert_eq!(joined(&bigquery), "bigquery__acme__sales__customers");
    assert_eq!(joined(postgres), "postgres__orders_db__public__customers");
}

/// A reader that serves the datasets a test harvested.
struct Harvested(crate::document::Snapshot);

impl crate::AspectReader for Harvested {
    fn read(&self) -> Result<crate::document::Snapshot, crate::DataHubError> {
        Ok(self.0.clone())
    }
}

#[test]
fn two_urns_that_differ_only_in_their_environment_are_refused_as_a_duplicate_model() {
    let mut read = BTreeSet::new();
    let in_env = |env: &str| {
        json!({
            "urn": format!("urn:li:dataset:(urn:li:dataPlatform:bigquery,acme.sales.orders,{env})"),
            "schemaMetadata": { "value": { "fields": [{ "fieldPath": "order_id" }] } },
        })
    };
    let datasets = vec![
        harvest_dataset(&in_env("PROD"), &mut read).expect("the PROD urn is read"),
        harvest_dataset(&in_env("DEV"), &mut read).expect("the DEV urn is read"),
    ];
    let source = sutura_domain::model::SourceName::parse("local").expect("a test name is a name");
    let catalog = crate::DataHubCatalog::new(
        source.clone(),
        sutura_domain::pinned::DefinitionVersion::parse("test").expect("a test version is a version"),
        std::collections::BTreeMap::from([(String::from("bigquery"), source)]),
        Harvested(crate::document::Snapshot::new(datasets, Vec::new(), Vec::new())),
    );
    let refused = sutura_domain::pinned::SemanticCatalog::load(&catalog).expect_err("one model twice is refused");
    assert!(
        matches!(
            refused,
            crate::DataHubError::Inconsistent {
                cause: sutura_domain::catalog::InconsistentDefinitions::DuplicateModel { ref model }
            } if model.as_str() == "bigquery__acme__sales__orders"
        ),
        "{refused:?}"
    );
}

//! The happy-path `DataHub` wire pages this crate's own tests need.
//!
//! Over the loopback fake `sutura-http-client::test_support` now hosts (issue #970's review: this
//! file's own `FakeServer`/`Scripted`/plumbing was byte-for-byte identical to
//! `sutura-catalog-openmetadata`'s copy, `cargo xtask check-jscpd` measured, once a second reader
//! crate carried one). Re-exported here so `sutura-cli`'s served-binary suite
//! (`crates/sutura-cli/tests/served/datahub.rs`) keeps building `test_support::FakeServer` off
//! THIS crate's public API - it takes `sutura-catalog-datahub` as a dependency, not
//! `sutura-http-client` directly.
//!
//! `#[cfg(feature = "http")]`, not `#[cfg(test)]`, for the reason `sutura_http_client::test_support`'s
//! own header gives: an integration test binary cannot see another crate's `tests/` directory, so
//! the only way to share a fake across crates is through a library, `pub`, reachable at compile
//! time from whichever feature both a reader and its composition root's tests turn on.

#![expect(
    clippy::expect_used,
    clippy::too_long_first_doc_paragraph,
    reason = "test: the page builders below carry the same long-form fixture prose this crate's \
              other modules do, and read the crate's own recorded corpus - a test invariant \
              rather than a production path."
)]

pub use sutura_http_client::test_support::{CapturedAuthorizations, FakeServer, Scripted};

use crate::AspectReader as _;

/// The structured property name the happy-path pages below register the certified metric's content
/// under - a fixed test constant, deliberately independent of the deployment's OWN choice, the same
/// way `tests/provisioned.rs`'s `DEPLOYMENT_PROPERTY` is: a fake registering the adapter's own field
/// name would pass equally whether the name were the deployment's choice or a constant this crate
/// requires.
pub const DEPLOYMENT_PROPERTY: &str = "deployment_metric_document";

/// One `dataset` page, over the two models the certified fixture metric needs: `orders` (carrying
/// every column the metric's measure, time column and required filter name) and `customers`
/// (carrying the dimension's column). Model name and table are the same string, because
/// `HttpAspectReader::read_datasets`'s own doc names that as a real limit rather than hiding it.
#[must_use]
pub fn dataset_page() -> serde_json::Value {
    serde_json::json!({
        "entities": [
            {
                "urn": "urn:li:dataset:(urn:li:dataPlatform:bigquery,orders,PROD)",
                "schemaMetadata": { "value": { "fields": [
                    {"fieldPath": "order_id"},
                    {"fieldPath": "customer_id"},
                    {"fieldPath": "amount_cents"},
                    {"fieldPath": "order_date"},
                    {"fieldPath": "status"},
                ] } },
                "datasetProperties": { "value": { "description": "Net revenue orders, in minor units." } },
            },
            {
                "urn": "urn:li:dataset:(urn:li:dataPlatform:bigquery,customers,PROD)",
                "schemaMetadata": { "value": { "fields": [
                    {"fieldPath": "customer_id"},
                    {"fieldPath": "segment"},
                ] } },
                "datasetProperties": { "value": { "description": "The customer dimension." } },
            },
        ]
    })
}

/// One `semanticModel` page, over the one relationship the certified fixture metric's dimension
/// reaches `customers` through. Served in the shape a REAL `DataHub` carries - the relationship
/// nested inside the `semanticModel` entity's own `semanticModelInfo.value.relationships[]`, not as
/// a top-level aspect (GMS drops that on write; measured against the docker tier, 2026-09-16) - so
/// the fake and the live tier serve one content over two transports, and
/// `HttpAspectReader::read_relationships` walks both identically.
#[must_use]
pub fn relationship_page() -> serde_json::Value {
    serde_json::json!({
        "entities": [
            {
                "urn": "urn:li:semanticModel:(urn:li:dataPlatform:bigquery,PROD,orders_to_customer)",
                "semanticModelInfo": { "value": {
                    "name": "orders_to_customer",
                    "relationships": [
                        {
                            "name": "orders_to_customer",
                            "from": "urn:li:dataset:(urn:li:dataPlatform:bigquery,orders,PROD)",
                            "fromColumns": ["customer_id"],
                            "to": "urn:li:dataset:(urn:li:dataPlatform:bigquery,customers,PROD)",
                            "toColumns": ["customer_id"],
                            "cardinality": "N_ONE",
                        },
                    ],
                } },
            },
        ]
    })
}

/// One `metric` page carrying the recorded fixture's OWN certified metric, read through the crate's
/// public port rather than restated here - the recorded corpus and this page cannot drift.
#[must_use]
pub fn metric_page() -> serde_json::Value {
    let recorded = crate::fixture::FixtureReader.read().expect("the recorded fixture reads");
    let certified = recorded
        .metrics()
        .iter()
        .find(|metric| metric.sutura().is_some())
        .expect("the fixture carries one certified metric");
    let property = certified.sutura().expect("the metric just found carries the property");
    serde_json::json!({
        "entities": [
            {
                "urn": "urn:li:metric:(urn:li:dataPlatform:bigquery,orders,revenue)",
                "metricInfo": { "value": {
                    "name": certified.name(),
                    "expression": { "dialects": [{ "dialect": certified.dialect(), "expression": certified.expression() }] },
                } },
                "structuredProperties": { "value": { "properties": [
                    { "propertyUrn": format!("urn:li:structuredProperty:{DEPLOYMENT_PROPERTY}"), "values": [{ "string": property.string_value() }] },
                ] } },
            },
        ]
    })
}

/// The three pages a `read()` call makes, in order, all answering `200` - what a real `DataHub`
/// carrying exactly the recorded fixture's content would serve.
#[must_use]
pub fn happy_path_answers() -> Vec<Scripted> {
    vec![
        Scripted::ok(&dataset_page()),
        Scripted::ok(&relationship_page()),
        Scripted::ok(&metric_page()),
    ]
}

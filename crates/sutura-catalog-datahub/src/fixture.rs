//! The recorded fixture corpus and the fake reader that serves it.
//!
//! This is the only [`AspectReader`] implementor today, and it is the **fake** the port is tested
//! against - recorded aspect documents, not mocked HTTP. Until a real reader exists this is what a
//! [`crate::DataHubCatalog`] reads. The corpus is a bundle of models, one relationship and **one
//! certified metric** - the metric is `revenue`, and it carries the deployment-defined structured
//! property that the adapter decodes into a domain `Metric`, which is the issue #202 claim:
//! `DataHub` provides metrics for a metric that carries the custom shape. The raw expression string
//! beside it stays the promotion-candidate half and is never converted.
//!
//! **The corpus is not only recorded, it is CONFIRMED against the platform.**
//! `tests/provisioned.rs` writes this metric's scalar into a provisioned `DataHub`, reads the aspect
//! back, and asserts the decoded [`crate::document::MetricAspect`] equals the one recorded here - so
//! the fixture is faithful to the platform rather than only to itself.
//!
//! **The metric content is recorded in the FLAT form, and that is the point of keeping it as text.**
//! `DataHub`'s `structuredProperty` is scalar-only, so a deployment defines metric content as one
//! string-valued property, under a name of its own; the corpus records the shape a reader hands over
//! once it has mapped that property - `"sutura": {
//! "string_value": "..." }` with the closed-vocabulary document as the scalar's text - and the read
//! path exercises `document::SuturaProperty::assemble`, the scalar-to-nested step issue #202 is
//! about, on every load. The documents are decoded through `serde_json` at read time, so the same
//! deserialization path a real reader would use is exercised, and `deny_unknown_fields` on the wire
//! shapes holds over these recorded documents.

use crate::document::Snapshot;
use crate::{AspectReader, DataHubCatalog, DataHubError};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

/// The deployment-defined metric content a real `DataHub` would store as the scalar value of the
/// `sutura` structured property: one JSON document over the domain's closed vocabularies.
///
/// This is the *flat* form issue #202 is about: the measure, the time axis, the definitional
/// filter, a dimension with its allowlist, an anchor and prose, spelled exactly as a markdown
/// metric spells them. It is embedded as the `string_value` field of the recorded `MetricAspect`
/// below, exactly as a service would store it, so the recorded document and the deployment grammar
/// cannot drift.
///
/// `cfg(test)` because the corpus is what production reads; the document's only reader is the test
/// that proves the corpus embeds it verbatim.
#[cfg(test)]
const SUTURA_DOCUMENT: &str = r#"{"model":"orders","description":"Net revenue in minor units, from active orders.","measure":{"simple":{"aggregate":"sum","column":"amount_cents"}},"time_column":"order_date","grains":["month"],"required_filters":[{"equals":{"column":"status","value":"active"}}],"dimensions":[{"name":"segment","column":"segment","via":"orders_to_customer","allowed_values":["retail","wholesale"],"description":"The customer's segment."}],"anchor":{"range":{"start":"2026-06-01","end":"2026-07-01"},"value":"412345"}}"#;

/// Two models - one fact, one lookup - one relationship whose cardinality this adapter CAN
/// represent, and one certified metric carrying the `sutura` structured property. The recorded
/// documents are the crate's own, in the narrow shape [`crate::document::Snapshot`] defines.
const CORPUS: &str = r#"{
  "datasets": [
    {
      "name": "orders",
      "table": "fct_order",
      "platform": "bigquery",
      "columns": ["order_id", "customer_id", "amount_cents", "order_date", "status"],
      "description": "Net revenue orders, in minor units."
    },
    {
      "name": "customers",
      "table": "dim_customer",
      "platform": "bigquery",
      "columns": ["customer_id", "segment"],
      "description": "The customer dimension."
    }
  ],
  "relationships": [
    {
      "name": "orders_to_customer",
      "from_model": "orders",
      "from_column": "customer_id",
      "to_model": "customers",
      "to_column": "customer_id",
      "cardinality": "n_one"
    }
  ],
  "metrics": [
    {
      "name": "revenue",
      "dialect": "ANSI_SQL",
      "expression": "SUM(amount_cents)",
      "sutura": {
        "string_value": "{\"model\":\"orders\",\"description\":\"Net revenue in minor units, from active orders.\",\"measure\":{\"simple\":{\"aggregate\":\"sum\",\"column\":\"amount_cents\"}},\"time_column\":\"order_date\",\"grains\":[\"month\"],\"required_filters\":[{\"equals\":{\"column\":\"status\",\"value\":\"active\"}}],\"dimensions\":[{\"name\":\"segment\",\"column\":\"segment\",\"via\":\"orders_to_customer\",\"allowed_values\":[\"retail\",\"wholesale\"],\"description\":\"The customer's segment.\"}],\"anchor\":{\"range\":{\"start\":\"2026-06-01\",\"end\":\"2026-07-01\"},\"value\":\"412345\"}}"
      }
    }
  ]
}"#;

/// The fake [`AspectReader`] that serves the recorded corpus.
#[derive(Debug, Clone)]
pub struct FixtureReader;

impl AspectReader for FixtureReader {
    fn read(&self) -> Result<Snapshot, DataHubError> {
        serde_json::from_str(CORPUS).map_err(|cause| DataHubError::Read { cause: Box::new(cause) })
    }
}

/// A [`crate::DataHubCatalog`] over the recorded corpus.
///
/// The source mapping answers the one platform the corpus names - `bigquery` - with the deployment's
/// declared source, which is what lets a model on that platform be opened. This is the constructor
/// the conformance registry uses to register the adapter; it is `pub` because an integration suite is
/// a separate crate and cannot reach a `#[cfg(test)]` item.
pub fn over_fixture_source(name: SourceName, version: DefinitionVersion) -> DataHubCatalog<FixtureReader> {
    let mut sources = std::collections::BTreeMap::new();
    drop(sources.insert(String::from("bigquery"), name.clone()));
    DataHubCatalog::new(name, version, sources, FixtureReader)
}

#[cfg(test)]
mod tests {
    /// The recorded flat form and the deployment grammar are the same string: the corpus embeds
    /// exactly what [`crate::document::SuturaProperty::assemble`] decodes, so the two cannot drift.
    #[test]
    fn the_recorded_sutura_document_is_embedded_in_the_corpus() {
        let corpus = serde_json::from_str::<serde_json::Value>(super::CORPUS).expect("the corpus is json");
        let string_value = corpus["metrics"][0]["sutura"]["string_value"]
            .as_str()
            .expect("the recorded sutura property is a scalar string");
        assert_eq!(
            string_value,
            super::SUTURA_DOCUMENT,
            "the corpus embeds exactly the deployment document"
        );
    }
}

//! The recorded fixture corpus and the fake reader that serves it.
//!
//! This is the only [`AspectReader`] implementor today, and it is the **fake** the port is tested
//! against - recorded aspect documents, not mocked HTTP. `docs/adr/0016`'s transport note leaves the
//! read path's cost open until a provisioned instance exists; until then this is what a
//! [`crate::DataHubCatalog`] reads. The corpus is deliberately a **bundle of models and no metrics**:
//! that is the requirement `docs/adr/0016` checks - a DataHub-only deployment loads, pins and
//! validates with no certified metric layer - so the standalone declaration ([`crate::DataHubCatalog`]
//! provides `Structure`, `Descriptions`, `Relationships` and nothing else) is faithful to it.
//!
//! The documents are embedded as text and decoded through `serde_json` at read time, so the same
//! deserialization path a real reader would use is exercised, and `deny_unknown_fields` on the wire
//! shapes holds over these recorded documents.

use crate::document::Snapshot;
use crate::{AspectReader, DataHubCatalog, DataHubError};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

/// Two models - one fact, one lookup - and one relationship whose cardinality this adapter CAN
/// represent. No metrics, no knowledge. The recorded documents are the crate's own, in the narrow
/// shape [`crate::document::Snapshot`] defines.
const CORPUS: &str = r#"{
  "datasets": [
    {
      "name": "orders",
      "table": "fct_order",
      "platform": "bigquery",
      "columns": ["order_id", "customer_id", "amount_cents", "order_date"],
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
  "metrics": []
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

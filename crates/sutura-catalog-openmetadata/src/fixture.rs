//! The recorded fixture corpus and the fake reader that serves it.
//!
//! This is the only [`SnapshotReader`] implementor today, and it is the **fake** the port is tested
//! against - recorded documents, not mocked HTTP. The corpus is a bundle of two models and one
//! declared non-duplicating join, plus one metric whose measure is an expression string; the metric
//! is carried and never minted, because its bound column is not resolvable from a foreign-dialect
//! expression (the reported-not-defined half this crate's declaration promises).
//!
//! The documents are decoded through `serde_json` at read time, so the same deserialization path a
//! real reader over `OpenMetadata`'s `REST` API would use is exercised, and `deny_unknown_fields` on
//! the wire shapes holds over these recorded documents. `over_fixture_source` is what the conformance
//! registry calls to register the adapter; it is `pub` because an integration suite is a separate
//! crate and cannot reach a `#[cfg(test)]` item.

use std::collections::BTreeMap;

use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

use crate::document::Snapshot;
use crate::{OpenMetadataCatalog, OpenMetadataError, SnapshotReader};

/// The recorded corpus: two models, one declared one-to-many join, one metric reported-not-defined.
///
/// Every model carries a description (Descriptions is a provided kind); the relationship declares a
/// non-duplicating cardinality (so it converts); the metric carries a decidable `metricType` and an
/// expression-string binding that never becomes a domain `Measure`.
const CORPUS: &str = r#"{
  "tables": [
    {"service":"warehouse","name":"orders","columns":["order_id","customer_id","amount_cents","order_date","status"],"description":"Net revenue orders, in minor units."},
    {"service":"warehouse","name":"customers","columns":["customer_id","segment"],"description":"The customer dimension."}
  ],
  "relationships": {
    "orders_to_customer": {"origin_model":"orders","origin_column":"customer_id","target_model":"customers","target_column":"customer_id","relationship_type":"ONE_TO_MANY"}
  },
  "metrics": [
    {"name":"revenue","metricType":"SUM","granularity":"DAY","expression":"SUM(amount_cents)"}
  ]
}"#;

/// The fake [`SnapshotReader`] that serves the recorded corpus.
#[derive(Debug, Clone)]
pub struct FixtureReader;

impl SnapshotReader for FixtureReader {
    type Error = OpenMetadataError;

    fn read(&self) -> Result<Snapshot, Self::Error> {
        serde_json::from_str(CORPUS).map_err(|cause| OpenMetadataError::Read(Box::new(cause)))
    }
}

/// An [`OpenMetadataCatalog`] over the recorded corpus.
///
/// The source mapping answers the one service the corpus names - `warehouse` - with the deployment's
/// declared source, which is what lets a model on that platform be opened. This is the constructor
/// the conformance registry uses to register the adapter.
pub fn over_fixture_source(name: SourceName, version: DefinitionVersion) -> OpenMetadataCatalog<FixtureReader> {
    let mut sources = BTreeMap::new();
    drop(sources.insert(String::from("warehouse"), name.clone()));
    OpenMetadataCatalog::new(name, version, sources, FixtureReader)
}

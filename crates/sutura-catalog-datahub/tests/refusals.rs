#![forbid(unsafe_code)]
//! Every reachable `DataHubError` refusal on a dataset that `src/tests.rs` does not provoke, each
//! driven through the public [`SemanticCatalog::load`] port over a stub reader and asserted by
//! variant, the contract a caller matches on.
//!
//! Not provoked, because no snapshot reaches them: `Knowledge` (this adapter supplies
//! `KnowledgeInput::none()`) and `Digest`.
//!
//! Wrapped in `#[cfg(test)] mod tests` so `allow-expect-in-tests` reaches the helpers, the shape
//! `tests/http_reader.rs` uses.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sutura_catalog_datahub::document::{ColumnMetadata, DatasetAspect, Snapshot};
    use sutura_catalog_datahub::{AspectReader, DataHubCatalog, DataHubError};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    /// A reader that serves exactly the snapshot a test hands it.
    struct Stub(Snapshot);

    impl AspectReader for Stub {
        fn read(&self) -> Result<Snapshot, DataHubError> {
            Ok(self.0.clone())
        }
    }

    fn dataset(name: &str, platform: &str, description: &str) -> DatasetAspect {
        DatasetAspect::new(
            name.to_owned(),
            String::from("fct_order"),
            platform.to_owned(),
            vec![String::from("order_id")],
            description.to_owned(),
        )
    }

    /// Loads one dataset through a catalog that maps only the `bigquery` platform, returning its
    /// refusal.
    fn refused(dataset: DatasetAspect) -> Option<DataHubError> {
        let source = SourceName::parse("local").expect("a test name is a name");
        let sources = BTreeMap::from([(String::from("bigquery"), source.clone())]);
        let version = DefinitionVersion::parse("test").expect("a test version is a version");
        let snapshot = Snapshot::new(vec![dataset], Vec::new(), Vec::new());
        DataHubCatalog::new(source, version, sources, Stub(snapshot)).load().err()
    }

    #[test]
    fn a_dataset_on_an_unmapped_platform_is_refused() {
        let outcome = refused(dataset("orders", "postgres", "Orders."));
        assert!(
            matches!(outcome, Some(DataHubError::UnknownPlatform { ref platform, .. }) if platform == "postgres"),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_dataset_with_an_unparseable_name_is_refused() {
        let outcome = refused(dataset("123bad", "bigquery", "Orders."));
        assert!(
            matches!(outcome, Some(DataHubError::Identifier { kind: "model", ref value, .. }) if value == "123bad"),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_dataset_with_an_unusable_description_is_refused() {
        let outcome = refused(dataset("orders", "bigquery", "Bad\rdescription"));
        assert!(
            matches!(outcome, Some(DataHubError::Description { ref on, .. }) if on == "orders"),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_column_with_an_unusable_description_is_refused() {
        let orders = dataset("orders", "bigquery", "Orders.").with_column_metadata([(
            String::from("order_id"),
            ColumnMetadata::new(None, Some(String::from("Bad\rdescription"))),
        )]);
        let outcome = refused(orders);
        assert!(
            matches!(outcome, Some(DataHubError::ColumnDescription { ref column, .. }) if column.as_str() == "order_id"),
            "{outcome:?}"
        );
    }
}

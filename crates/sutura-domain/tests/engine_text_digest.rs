#![forbid(unsafe_code)]
//! A schema's digest covers how its engine compares text, through the public pin alone.
//!
//! This uses no collation type: an engine with byte-wise text comparison and no per-column collation
//! records that fact on every text column it pins, so the digest of a `DataFusion`
//! schema with a text column is the value below, and a dialect that records nothing keeps the value
//! the schema had before any collation fact existed. Nothing here names an API added with the fact,
//! so a base tree that lacks it compiles this file and answers a different digest.

#[cfg(test)]
mod engine_text_digest {
    use std::collections::BTreeMap;

    use sutura_domain::catalog::{Column, Definitions, Description, Model};
    use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
    use sutura_domain::warehouse::schema::{PinnedSchemas, TypeDialect};

    /// The digest of `notes(id bigint, body text)` under `dialect`.
    fn digest(dialect: TypeDialect) -> String {
        let column = |name: &str, declared: &str| {
            Column::from_metadata(
                ColumnName::parse(name).expect("a test column is a column"),
                Some(declared),
                None,
                None,
            )
            .expect("a test column has no description to refuse")
        };
        let source = SourceName::parse("lake").expect("a test source is a source");
        let model = Model::new(
            ModelName::parse("notes").expect("a test model is a model"),
            source.clone(),
            TableName::parse("notes").expect("a test table is a table"),
            vec![column("id", "bigint"), column("body", "text")],
            Description::default(),
        );
        let definitions = Definitions::assemble(vec![model], vec![], vec![]).expect("a test bundle holds together");
        let pinned = PinnedSchemas::pin(&definitions, &BTreeMap::from([(source, dialect)])).expect("a typed bundle pins");
        pinned.digest().as_str().to_owned()
    }

    #[test]
    fn a_text_column_pinned_for_an_engine_with_byte_wise_text_carries_that_in_the_digest() {
        assert_eq!(
            digest(TypeDialect::DataFusion),
            "278d18d8d3fff5a15b7c46f23430cd493ed218173a7de8aecf8ac90e944234c3"
        );
        assert_ne!(
            digest(TypeDialect::DataFusion),
            digest(TypeDialect::Postgres),
            "an engine that records no collation hashes as it did before the fact existed"
        );
    }
}

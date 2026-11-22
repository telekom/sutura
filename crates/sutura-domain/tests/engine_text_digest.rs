#![forbid(unsafe_code)]
//! A schema's digest covers how its engine compares text, through the public pin alone.
//!
//! This uses no collation type: an engine with byte-wise text comparison and no per-column collation
//! records that fact on every text column it pins, so the digest of a `DataFusion`
//! schema with a text column includes both that fact and declared scalar-text provenance. A dialect
//! that records no collation shares scalar-text provenance and still has a different digest. This
//! file uses only the public pin, so the base compiles it and answers a different digest.

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
            "b31c93825ff69304b9517d5683730c94c8e383d62a8539c735bce0236adcc754"
        );
        assert_ne!(
            digest(TypeDialect::DataFusion),
            digest(TypeDialect::Postgres),
            "the engine collation changes identity even with the same declared scalar-text provenance"
        );
    }
}

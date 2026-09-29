//! Guidance for a bundle that carries physical structure and no semantic metric.

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Column, Definitions, Description, Model};
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use super::{CatalogProse, PromptInputs, Tool, render};

fn physical_schema() -> PinnedDefinitions {
    let source = SourceName::parse("physical").expect("a test source is a source");
    let model = Model::new(
        ModelName::parse("physical_model").expect("a test model is a model"),
        source.clone(),
        TableName::parse("physical_table").expect("a test table is a table"),
        [Column::from_metadata(
            ColumnName::parse("physical_column").expect("a test column is a column"),
            None,
            Some("A database author's column description."),
            None,
        )
        .expect("a column description")],
        Description::parse("A database author's description.").expect("a test description is a description"),
    );
    let definitions = Definitions::assemble(vec![model], Vec::new(), Vec::new())
        .expect("physical structure without a metric is a consistent bundle");
    let capabilities = MetadataCapabilities::of(
        DefinitionCapabilities::of([DefinitionKind::Structure])
            .and_may_provide([DefinitionKind::Descriptions, DefinitionKind::Relationships]),
        KnowledgeCapabilities::none(),
    );
    PinnedDefinitions::pin(
        DefinitionVersion::parse("physical-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(source, Contribution::of(capabilities)),
    )
    .expect("the physical bundle hashes")
}

#[test]
fn enabled_physical_schema_lists_models_and_columns_with_quoted_prose() {
    let bundle = physical_schema();
    let model_description = "A database author's description.";
    let column_description = "A database author's column description.";
    let quoted = render(
        &ScopedView::everything(&bundle),
        &PromptInputs::new(Tool::ALL, CatalogProse::Quoted, None).listing_physical_schema(true),
    );
    assert!(quoted.contains("physical_model"), "{quoted}");
    assert!(quoted.contains("physical_table"), "{quoted}");
    assert!(quoted.contains("physical_column"), "{quoted}");
    assert!(quoted.contains(&format!("> {model_description}")), "{quoted}");
    assert!(quoted.contains(&format!("> {column_description}")), "{quoted}");

    let omitted = render(
        &ScopedView::everything(&bundle),
        &PromptInputs::new(Tool::ALL, CatalogProse::Omitted, None).listing_physical_schema(true),
    );
    assert!(omitted.contains("physical_column"), "{omitted}");
    assert!(
        !omitted.contains(model_description) && !omitted.contains(column_description),
        "{omitted}"
    );
}

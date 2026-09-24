//! Tests for [`super`].
//!
//! A file rather than an inline `mod tests`, which is this repository's usual shape over the
//! thousand-line gate. The scratch-directory helpers follow `sutura-catalog-local`'s: each test
//! writes real Table Schema descriptors to a per-process temp directory and loads through
//! [`super::OkfCatalog::load`], so a cell asserts on what the adapter reads, not on source text.

use std::path::{Path, PathBuf};

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};

use crate::{OkfCatalog, OkfCatalogError};

fn test_name() -> SourceName {
    SourceName::parse("test").expect("a test name is a name")
}

fn catalog(root: PathBuf) -> OkfCatalog {
    OkfCatalog::new(
        test_name(),
        root,
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sutura-catalog-okf-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
    dir
}

/// Writes the given `(file, document)` pairs into a fresh scratch directory and opens a catalog over it.
///
/// Split from [`outcome_of`] so the file-writes (which may panic on a broken temp dir) stay in a
/// function that returns no `Result`, leaving the `Result`-returning helper to contain only the
/// catalog's own verdict.
fn catalog_of(name: &str, documents: &[(&str, &str)]) -> (PathBuf, OkfCatalog) {
    let root = scratch(name);
    for &(file, document) in documents {
        std::fs::write(root.join(file), document).expect("a descriptor is writable");
    }
    (root.clone(), catalog(root))
}

/// Loads a catalog built over the given descriptors, cleaning up the scratch directory on the way out.
fn outcome_of(name: &str, documents: &[(&str, &str)]) -> Result<PinnedDefinitions, OkfCatalogError> {
    let (root, catalog) = catalog_of(name, documents);
    let outcome = catalog.load();
    drop(std::fs::remove_dir_all(&root));
    outcome
}

fn error_for(name: &str, document: &str) -> OkfCatalogError {
    outcome_of(name, &[("model.yaml", document)]).expect_err("the descriptor must be refused")
}

const ORDERS: &str =
    "description: Orders, one row per order.\nfields:\n  - name: id\n    type: integer\n  - name: total\n    type: number\n";

const CUSTOMERS: &str = "title: Customers\ndescription: The people who place orders.\nfields:\n  - name: id\n    type: integer\n  - name: region\n    type: string\n";

#[test]
fn a_bundle_from_this_vocabulary_loads_pins_and_validates() {
    let pinned = outcome_of("loads", &[("orders.yaml", ORDERS), ("customers.yaml", CUSTOMERS)])
        .expect("a directory of Table Schema descriptors loads");
    let models = pinned.definitions().models();
    assert_eq!(models.len(), 2, "two descriptors load as two models");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = models.get(&orders_name).expect("orders is a model");
    assert_eq!(orders.columns().len(), 2, "orders carries its two columns");
    assert!(!orders.description().is_empty(), "orders carries its description");
}

/// `it_provides_exactly_what_it_declares`, lived in the adapter's own crate too: the bundle the
/// adapter produces and the declaration it publishes agree in both directions.
#[test]
fn it_declares_what_it_cannot_supply_and_the_bundle_agrees() {
    let pinned = outcome_of("fidelity", &[("orders.yaml", ORDERS)]).expect("the descriptor loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert_eq!(
        <OkfCatalog as SemanticCatalog>::capabilities().checked_against(&produced),
        Ok(()),
        "the OKF catalog's declaration and its bundle disagree"
    );
    // And the honest half of "declares what it cannot supply": a numeric column type is a data type,
    // NOT a measure, so a bundle with a `type: number` column still carries no metric and the
    // declaration says so.
    assert!(
        pinned.definitions().metrics().is_empty(),
        "a Table Schema column type is not a measure"
    );
}

const ORDERS_WITH_COLUMN_METADATA: &str = "description: Orders, one row per order.\nprimaryKey: id\nfields:\n  - name: id\n    type: integer\n    description: The order's unique identifier.\n  - name: total\n    type: number\n";

#[test]
fn a_field_s_type_and_description_arrive_on_the_model_s_column() {
    let pinned = outcome_of("column-metadata", &[("orders.yaml", ORDERS_WITH_COLUMN_METADATA)])
        .expect("a descriptor with typed, described fields loads");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = pinned.definitions().models().get(&orders_name).expect("orders is a model");
    let id = sutura_domain::model::ColumnName::parse("id").expect("a fixture column is a column");
    let column = orders.column(&id).expect("id is a declared column");
    assert_eq!(
        column.data_type().map(sutura_domain::catalog::ColumnType::as_str),
        Some("integer")
    );
    assert_eq!(column.description(), "The order's unique identifier.");
    // `total` carries a type and no description - the two kinds are independent, over one column.
    let total = sutura_domain::model::ColumnName::parse("total").expect("a fixture column is a column");
    assert_eq!(orders.column(&total).expect("total is declared").description(), "");
    // `primaryKey: id` is evidence only: it arrives on the model and licenses nothing else.
    assert_eq!(orders.primary_key(), &std::collections::BTreeSet::from([id]));
}

/// A field's `title` describes it when there is no `description` - the same fallback the model's
/// own description already had, extended to the field level.
#[test]
fn a_field_s_title_describes_it_when_there_is_no_description() {
    let yaml = "description: Orders, one row per order.\nfields:\n  - name: id\n    title: The order's unique identifier.\n";
    let pinned = outcome_of("field-title-fallback", &[("orders.yaml", yaml)]).expect("a title-only field loads");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = pinned.definitions().models().get(&orders_name).expect("orders is a model");
    let id = sutura_domain::model::ColumnName::parse("id").expect("a fixture column is a column");
    assert_eq!(orders.column(&id).expect("id is declared").description(), "The order's unique identifier.");
}

/// A type this crate cannot represent - here, one over `MAX_COLUMN_TYPE_CHARS` - is dropped, not
/// refused: the load still succeeds and the column simply carries no type. `ColumnType`'s own doc
/// carries the argument; this is the load-level proof that a whole catalog does not go down for
/// one long type spelling, the defect found in review over a real `DataHub` field.
#[test]
fn a_column_type_this_crate_cannot_represent_is_dropped_rather_than_refusing_the_load() {
    let long_type = "STRUCT<".to_owned() + &"field STRING, ".repeat(80) + "last STRING>";
    assert!(
        long_type.len() > sutura_domain::catalog::MAX_COLUMN_TYPE_CHARS,
        "the fixture must actually exceed the bound to prove anything"
    );
    let yaml = format!("description: Orders, one row per order.\nfields:\n  - name: id\n    type: \"{long_type}\"\n");
    let pinned = outcome_of("long-column-type", &[("orders.yaml", &yaml)]).expect("a long column type still loads");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = pinned.definitions().models().get(&orders_name).expect("orders is a model");
    let id = sutura_domain::model::ColumnName::parse("id").expect("a fixture column is a column");
    assert_eq!(orders.column(&id).expect("id is declared").data_type(), None);
}

#[test]
fn a_primary_key_naming_an_undeclared_column_fails_the_load() {
    let err = error_for(
        "badkey",
        "description: A descriptor whose primaryKey is not one of its own fields.\nprimaryKey: nope\nfields:\n  - name: id\n",
    );
    assert!(
        matches!(err, OkfCatalogError::Inconsistent { .. }),
        "a primaryKey naming an unknown column is a cross-reference failure: {err}"
    );
}

#[test]
fn an_unusable_primary_key_shape_is_refused() {
    let err = error_for(
        "listkey",
        "description: A descriptor whose primaryKey is a number.\nprimaryKey: 1\nfields:\n  - name: id\n",
    );
    assert!(
        matches!(err, OkfCatalogError::InvalidPrimaryKey { .. }),
        "a primaryKey that is neither a string nor a list of strings is refused: {err}"
    );
}

#[test]
fn a_document_field_it_does_not_declare_fails_the_load() {
    let err = error_for(
        "unknown",
        "description: A descriptor with a stray key.\nfields:\n  - name: id\nunknown_thing: 1\n",
    );
    assert!(
        matches!(err, OkfCatalogError::Malformed { .. }),
        "an unknown top-level key is refused: {err}"
    );
}

#[test]
fn a_descriptor_without_a_description_is_refused() {
    let err = error_for("nodescription", "fields:\n  - name: id\n");
    assert!(
        matches!(err, OkfCatalogError::MissingDescription { .. }),
        "a descriptor that would leave Descriptions unproduced is refused: {err}"
    );
}

#[test]
fn duplicate_column_names_are_refused() {
    let err = error_for(
        "duplicate",
        "description: A descriptor listing the same column twice.\nfields:\n  - name: id\n  - name: id\n",
    );
    assert!(
        matches!(err, OkfCatalogError::DuplicateColumn { .. }),
        "a doubled column is refused rather than silently deduplicated: {err}"
    );
}

#[test]
fn an_empty_directory_is_an_error() {
    let root = scratch("empty");
    let err = catalog(root.clone()).load();
    drop(std::fs::remove_dir_all(&root));
    assert!(
        matches!(err, Err(OkfCatalogError::Empty { .. })),
        "an empty root loads no catalog: {err:?}"
    );
}

#[test]
fn the_example_corpus_loads_and_digests_stably() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/okf");
    let first = catalog(root.clone()).load().expect("the example corpus loads");
    let second = catalog(root).load().expect("the example corpus loads twice");
    assert_eq!(first.digest(), second.digest(), "the same corpus pins to the same digest");
    assert!(!first.definitions().models().is_empty(), "the example corpus has models");
}

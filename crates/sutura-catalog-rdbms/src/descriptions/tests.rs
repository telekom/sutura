use super::*;
use crate::{ColumnMetadata, Table};
use std::num::NonZeroU64;

fn bounds(rows: u64, bytes: u64) -> DictionaryBounds {
    DictionaryBounds::new(
        NonZeroU64::new(rows).expect("nonzero"),
        NonZeroU64::new(bytes).expect("nonzero"),
    )
}

fn dictionary(schema: &str, values: &[(&str, &str)]) -> Dictionary {
    let table = Table::new(
        String::from("orders"),
        TableAddress::in_schema(schema.to_owned(), String::from("orders")),
        values.iter().map(|(name, _)| (*name).to_owned()).collect(),
        Some(String::from("Native table")),
    )
    .with_column_metadata(values.iter().map(|(name, prose)| {
        (
            (*name).to_owned(),
            ColumnMetadata::new(Some(String::from("INTEGER")), Some((*prose).to_owned())),
        )
    }));
    Dictionary::new(vec![table], Vec::new())
}

#[test]
fn ordered_prose_overlays_keep_dictionary_structure_and_count_unexposed_columns() {
    let base = dictionary(
        "public",
        &[
            ("id", "Native id"),
            ("amount", "Native amount"),
            ("fallback", "Native fallback"),
        ],
    );
    let first = dictionary("public", &[("id", "First id"), ("amount", "  "), ("missing", "Unknown")]);
    let second = dictionary(
        "public",
        &[("id", "Second id"), ("amount", "Second amount"), ("excluded", "Excluded")],
    );
    let overlays = [first, second]
        .iter()
        .map(|dictionary| DescriptionOverlay::from_dictionary(dictionary, bounds(100, 10000)).expect("bounded prose"))
        .collect::<Vec<_>>();
    let (actual, ignored) = base.clone().overlay_descriptions(&overlays).expect("overlaid");
    assert_eq!(ignored, 2);
    let table = actual.tables().first().expect("base table");
    assert_eq!(table.columns(), base.tables().first().expect("base table").columns());
    assert_eq!(
        table.column_metadata("id").and_then(crate::ColumnMetadata::description),
        Some("First id")
    );
    assert_eq!(
        table.column_metadata("amount").and_then(crate::ColumnMetadata::description),
        Some("Second amount")
    );
    assert_eq!(
        table.column_metadata("fallback").and_then(crate::ColumnMetadata::description),
        Some("Native fallback")
    );
    assert_eq!(
        table.column_metadata("amount").and_then(crate::ColumnMetadata::data_type),
        Some("INTEGER")
    );
    assert!(table.column_metadata("missing").is_none());
}

#[test]
fn overlay_physical_identity_does_not_fold_case_or_drop_catalog_qualifiers() {
    let mut docs = dictionary("PUBLIC", &[("id", "Wrong case")]);
    docs.tables.extend(dictionary("public", &[("id", "Wrong catalog")]).tables);
    docs.tables.last_mut().expect("second table").address.catalog = Some(String::from("other"));
    let overlay = DescriptionOverlay::from_dictionary(&docs, bounds(100, 10000)).expect("exact records");
    let (actual, ignored) = dictionary("public", &[("id", "Native")])
        .overlay_descriptions(&[overlay])
        .expect("overlaid");
    assert_eq!(ignored, 2);
    assert_eq!(
        actual
            .tables()
            .first()
            .and_then(|table| table.column_metadata("id"))
            .and_then(crate::ColumnMetadata::description),
        Some("Native")
    );
}

#[test]
fn description_projection_refuses_duplicate_keys_caps_and_control_characters() {
    let mut repeated = dictionary("public", &[("id", "One")]);
    repeated.tables.extend(dictionary("public", &[("id", "Two")]).tables);
    assert!(matches!(
        DescriptionOverlay::from_dictionary(&repeated, bounds(100, 10000)),
        Err(DescriptionError::DuplicateTable { .. })
    ));
    assert!(matches!(
        DescriptionOverlay::from_dictionary(&dictionary("public", &[("id", "One"), ("id", "Two")]), bounds(100, 10000)),
        Err(DescriptionError::Duplicate { .. })
    ));
    assert!(matches!(
        DescriptionOverlay::from_dictionary(
            &dictionary("public", &[("id", "Prose"), ("extra", "Prose")]),
            bounds(1, 10000)
        ),
        Err(DescriptionError::Rows { .. })
    ));
    assert!(matches!(
        DescriptionOverlay::from_dictionary(&dictionary("public", &[("id", "Prose")]), bounds(100, 1)),
        Err(DescriptionError::Bytes { .. })
    ));
    assert!(matches!(
        DescriptionOverlay::from_dictionary(&dictionary("public", &[("id", "Bad\u{1b}prose")]), bounds(100, 10000)),
        Err(DescriptionError::Prose { .. })
    ));
}

#[test]
fn a_catalog_projection_uses_physical_names_and_discards_the_contributors_structure() {
    use sutura_domain::pinned::SemanticCatalog as _;
    struct Recorded(Dictionary);
    impl crate::DictionaryReader for Recorded {
        fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
            Ok(self.0.clone())
        }
    }
    let mut source = dictionary(
        "public",
        &[("id", "Catalog documentation."), ("unknown", "Unknown documentation.")],
    );
    source.tables.first_mut().unwrap().model = String::from("different_semantic_alias");
    let catalog = crate::RdbmsCatalog::new(
        sutura_domain::model::SourceName::parse("documentation").unwrap(),
        sutura_domain::pinned::DefinitionVersion::parse("test").unwrap(),
        Recorded(source),
    );
    let bundle = catalog.load().unwrap();
    let overlay = DescriptionOverlay::from_definitions(bundle.definitions(), bounds(100, 10000)).unwrap();
    let (actual, ignored) = dictionary("public", &[("id", "Native")])
        .overlay_descriptions(&[overlay])
        .unwrap();
    let table = actual.tables().first().unwrap();
    assert_eq!(table.model(), "orders");
    assert_eq!(table.columns(), &[String::from("id")]);
    assert_eq!(
        table.column_metadata("id").and_then(crate::ColumnMetadata::description),
        Some("Catalog documentation.")
    );
    assert_eq!(ignored, 1);
}

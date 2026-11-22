//! Bounded prose keyed by exact physical identity. No structure crosses this boundary.

#[cfg(feature = "live")]
mod mapping;
#[cfg(feature = "live")]
pub use mapping::{MappedDescriptions, MappedKeySource, MappedPredicate, MappedTable, MappedVia};

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::catalog::{Definitions, Description};
use sutura_domain::model::{ColumnName, DatasetName, ProjectName, QualifiedTable, TableName, TableQualifier};

use crate::{Dictionary, DictionaryBounds, RdbmsError, TableAddress};

#[derive(Debug, thiserror::Error)]
pub enum DescriptionError {
    #[error("description source exceeds its row bound of {limit}")]
    Rows { limit: u64 },
    #[error("description source exceeds its byte bound of {limit}")]
    Bytes { limit: u64 },
    #[error("description source repeats physical column {table}.{column}")]
    Duplicate { table: QualifiedTable, column: ColumnName },
    #[error("description source repeats physical table {table}")]
    DuplicateTable { table: QualifiedTable },
    #[error("description source has an invalid physical identifier")]
    Identifier {
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    #[error("description source contains invalid prose")]
    Prose {
        #[source]
        cause: sutura_domain::catalog::InvalidDescription,
    },
}

type PhysicalColumn = (QualifiedTable, ColumnName);

/// A dictionary with the number of description rows outside its visible columns.
pub type OverlaidDictionary = (Dictionary, u64);

/// One ordered source's complete prose projection, already checked and bounded.
#[derive(Debug)]
pub struct DescriptionOverlay {
    tables: BTreeMap<QualifiedTable, Description>,
    columns: BTreeMap<PhysicalColumn, Description>,
}

struct Projection {
    overlay: DescriptionOverlay,
    bounds: DictionaryBounds,
    rows: u64,
    bytes: u64,
}

impl Projection {
    const fn new(bounds: DictionaryBounds) -> Self {
        Self {
            overlay: DescriptionOverlay {
                tables: BTreeMap::new(),
                columns: BTreeMap::new(),
            },
            bounds,
            rows: 0,
            bytes: 0,
        }
    }

    fn admit(&mut self, bytes: usize, row: bool) -> Result<(), DescriptionError> {
        self.rows = self.rows.saturating_add(u64::from(row));
        self.bytes = self.bytes.saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
        if self.rows > self.bounds.max_rows().get() {
            return Err(DescriptionError::Rows {
                limit: self.bounds.max_rows().get(),
            });
        }
        if self.bytes > self.bounds.max_bytes().get() {
            return Err(DescriptionError::Bytes {
                limit: self.bounds.max_bytes().get(),
            });
        }
        Ok(())
    }

    fn column(&mut self, table: &QualifiedTable, column: &str, prose: &str) -> Result<(), DescriptionError> {
        let column = exact(column)
            .and_then(ColumnName::parse)
            .map_err(|cause| DescriptionError::Identifier { cause })?;
        let prose = Description::parse(prose).map_err(|cause| DescriptionError::Prose { cause })?;
        if self.overlay.columns.insert((table.clone(), column.clone()), prose).is_some() {
            return Err(DescriptionError::Duplicate {
                table: table.clone(),
                column,
            });
        }
        Ok(())
    }
}

fn exact(raw: &str) -> Result<&str, sutura_domain::model::InvalidIdentifier> {
    if let Some(offending) = raw.chars().find(|character| character.is_whitespace()) {
        return Err(sutura_domain::model::InvalidIdentifier::IllegalCharacter { offending });
    }
    Ok(raw)
}

fn physical(address: &TableAddress) -> Result<QualifiedTable, DescriptionError> {
    let parse = || {
        let table = TableName::parse(exact(address.table())?)?;
        let schema = DatasetName::parse(exact(address.schema())?)?;
        let qualifier = match address.catalog() {
            Some(catalog) => TableQualifier::in_project(ProjectName::parse(exact(catalog)?)?, schema),
            None => TableQualifier::in_dataset(schema),
        };
        Ok(QualifiedTable::new(Some(qualifier), table))
    };
    parse().map_err(|cause| DescriptionError::Identifier { cause })
}

impl DescriptionOverlay {
    pub fn from_dictionary(dictionary: &Dictionary, bounds: DictionaryBounds) -> Result<Self, DescriptionError> {
        let mut projection = Projection::new(bounds);
        for table in dictionary.tables() {
            let address_bytes = table.address.to_string().len();
            projection.admit(
                address_bytes.saturating_add(table.description.as_deref().unwrap_or("").len()),
                table.columns.is_empty(),
            )?;
            let address = physical(&table.address)?;
            let description = Description::parse(table.description.as_deref().unwrap_or(""))
                .map_err(|cause| DescriptionError::Prose { cause })?;
            if projection.overlay.tables.insert(address.clone(), description).is_some() {
                return Err(DescriptionError::DuplicateTable { table: address });
            }
            for column in &table.columns {
                let prose = table
                    .column_metadata(column)
                    .and_then(crate::ColumnMetadata::description)
                    .unwrap_or("");
                projection.admit(address_bytes.saturating_add(column.len()).saturating_add(prose.len()), true)?;
                projection.column(&address, column, prose)?;
            }
        }
        Ok(projection.overlay)
    }

    pub fn from_definitions(definitions: &Definitions, bounds: DictionaryBounds) -> Result<Self, DescriptionError> {
        let mut projection = Projection::new(bounds);
        for model in definitions.models().values() {
            let address = model.table();
            let address_bytes = address.to_string().len();
            projection.admit(
                address_bytes.saturating_add(model.description().len()),
                model.columns().next().is_none(),
            )?;
            if projection
                .overlay
                .tables
                .insert(
                    address.clone(),
                    Description::parse(model.description()).map_err(|cause| DescriptionError::Prose { cause })?,
                )
                .is_some()
            {
                return Err(DescriptionError::DuplicateTable { table: address.clone() });
            }
            for column in model.columns() {
                projection.admit(
                    address_bytes
                        .saturating_add(column.name().as_str().len())
                        .saturating_add(column.description().len()),
                    true,
                )?;
                projection.column(address, column.name().as_str(), column.description())?;
            }
        }
        Ok(projection.overlay)
    }
}

/// Applies only prose after dictionary selection. Returns the number of ignored column rows,
/// including empty rows and repeated unknown columns from different ordered sources.
pub(crate) fn apply(mut dictionary: Dictionary, sources: &[DescriptionOverlay]) -> Result<OverlaidDictionary, RdbmsError> {
    let mut visible = BTreeSet::new();
    for table in &mut dictionary.tables {
        let address = physical(&table.address).map_err(|cause| RdbmsError::Read(Box::new(cause)))?;
        if let Some(prose) = sources
            .iter()
            .filter_map(|source| source.tables.get(&address))
            .find(|prose| !prose.is_empty())
        {
            table.description = Some(prose.as_str().to_owned());
        }
        for column in &table.columns {
            let key = (
                address.clone(),
                ColumnName::parse(column).map_err(|cause| RdbmsError::ColumnName {
                    table: address.to_string(),
                    column: column.clone(),
                    cause,
                })?,
            );
            if let Some(prose) = sources
                .iter()
                .filter_map(|source| source.columns.get(&key))
                .find(|prose| !prose.is_empty())
            {
                table.column_metadata.entry(column.clone()).or_default().description = Some(prose.as_str().to_owned());
            }
            visible.insert(key);
        }
    }
    let ignored = sources
        .iter()
        .flat_map(|source| source.columns.keys())
        .filter(|key| !visible.contains(*key))
        .count();
    Ok((dictionary, u64::try_from(ignored).unwrap_or(u64::MAX)))
}

#[cfg(test)]
mod tests {
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
}

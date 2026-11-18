//! Bounded prose keyed by exact physical identity. No structure crosses this boundary.

#[cfg(feature = "live")]
mod mapping;
#[cfg(feature = "live")]
pub use mapping::MappedDescriptions;

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
mod tests;

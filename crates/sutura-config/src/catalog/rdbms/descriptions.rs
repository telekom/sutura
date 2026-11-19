//! Prose-only catalog references and closed plain-table mappings.

use sutura_domain::model::{ColumnName, QualifiedTable, SourceName};

use super::{CatalogConnection, InvalidRdbmsCatalog, LiveRowPredicate, non_zero};
use crate::raw::{RawCatalogConnection, RawLiveRowPredicate};
use std::num::NonZeroU64;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDescriptionSource {
    pub(crate) catalog: Option<String>,
    pub(crate) connection: Option<RawCatalogConnection>,
    pub(crate) table: Option<String>,
    pub(crate) key: Option<RawDescriptionKey>,
    pub(crate) description: Option<String>,
    pub(crate) live_row_predicate: Option<RawLiveRowPredicate>,
    pub(crate) max_dictionary_rows: Option<u64>,
    pub(crate) max_dictionary_bytes: Option<u64>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDescriptionKey {
    pub(crate) catalog: Option<String>,
    pub(crate) schema: String,
    pub(crate) table: String,
    pub(crate) column: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptionSource {
    Catalog(SourceName),
    Table(Box<DescriptionMapping>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptionMapping {
    connection: CatalogConnection,
    table: QualifiedTable,
    catalog_key: Option<ColumnName>,
    schema_key: ColumnName,
    table_key: ColumnName,
    column_key: ColumnName,
    description: ColumnName,
    predicate: Option<LiveRowPredicate>,
    max_rows: Option<NonZeroU64>,
    max_bytes: Option<NonZeroU64>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDescriptionSource {
    #[error("descriptions_from must name a catalog OR a complete table mapping, without fields from the other shape")]
    Shape,
    #[error("description mapping field {field} is not an exact identifier")]
    Identifier { field: &'static str },
    #[error("description mapping table requires an exact schema-qualified physical name")]
    Table,
    #[error(
        "description catalog reference must name another declared rdbms or datahub catalog without a reference cycle (depth at most 32 and at most 256 expanded references per consumer)"
    )]
    Reference,
    #[error("a description source list may contain at most {limit} entries")]
    TooMany { limit: usize },
}

pub(crate) fn parse(name: &SourceName, raw: &[RawDescriptionSource]) -> Result<Vec<DescriptionSource>, InvalidRdbmsCatalog> {
    if raw.len() > 32 {
        return Err(invalid(InvalidDescriptionSource::TooMany { limit: 32 }));
    }
    raw.iter()
        .map(|raw| {
            if let Some(catalog) = &raw.catalog {
                if raw.connection.is_some()
                    || raw.table.is_some()
                    || raw.key.is_some()
                    || raw.description.is_some()
                    || raw.live_row_predicate.is_some()
                    || raw.max_dictionary_rows.is_some()
                    || raw.max_dictionary_bytes.is_some()
                {
                    return Err(invalid(InvalidDescriptionSource::Shape));
                }
                let catalog = SourceName::parse(catalog).map_err(|_cause| invalid(InvalidDescriptionSource::Reference))?;
                if &catalog == name {
                    return Err(invalid(InvalidDescriptionSource::Reference));
                }
                return Ok(DescriptionSource::Catalog(catalog));
            }
            let connection = raw
                .connection
                .as_ref()
                .ok_or_else(|| invalid(InvalidDescriptionSource::Shape))?;
            let key = raw.key.as_ref().ok_or_else(|| invalid(InvalidDescriptionSource::Shape))?;
            let table = raw.table.as_deref().ok_or_else(|| invalid(InvalidDescriptionSource::Shape))?;
            if table.chars().any(char::is_whitespace) {
                return Err(invalid(InvalidDescriptionSource::Table));
            }
            let table = QualifiedTable::parse(table).map_err(|_cause| invalid(InvalidDescriptionSource::Table))?;
            if table.qualifier().is_none() {
                return Err(invalid(InvalidDescriptionSource::Table));
            }
            let description = raw
                .description
                .as_deref()
                .ok_or_else(|| invalid(InvalidDescriptionSource::Shape))?;
            let column = |raw: &str, field| {
                if raw.chars().any(char::is_whitespace) {
                    return Err(invalid(InvalidDescriptionSource::Identifier { field }));
                }
                ColumnName::parse(raw).map_err(|_cause| invalid(InvalidDescriptionSource::Identifier { field }))
            };
            Ok(DescriptionSource::Table(Box::new(DescriptionMapping {
                connection: CatalogConnection::parse(name, connection)
                    .map_err(|cause| InvalidRdbmsCatalog::Connection { cause })?,
                table,
                catalog_key: key.catalog.as_deref().map(|raw| column(raw, "key.catalog")).transpose()?,
                schema_key: column(&key.schema, "key.schema")?,
                table_key: column(&key.table, "key.table")?,
                column_key: column(&key.column, "key.column")?,
                description: column(description, "description")?,
                predicate: raw.live_row_predicate.as_ref().map(LiveRowPredicate::parse).transpose()?,
                max_rows: non_zero("max_dictionary_rows", raw.max_dictionary_rows)?,
                max_bytes: non_zero("max_dictionary_bytes", raw.max_dictionary_bytes)?,
            })))
        })
        .collect()
}

const fn invalid(cause: InvalidDescriptionSource) -> InvalidRdbmsCatalog {
    InvalidRdbmsCatalog::Descriptions { cause }
}

impl DescriptionMapping {
    pub const fn connection(&self) -> &CatalogConnection {
        &self.connection
    }
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }
    pub const fn catalog_key(&self) -> Option<&ColumnName> {
        self.catalog_key.as_ref()
    }
    pub const fn schema_key(&self) -> &ColumnName {
        &self.schema_key
    }
    pub const fn table_key(&self) -> &ColumnName {
        &self.table_key
    }
    pub const fn column_key(&self) -> &ColumnName {
        &self.column_key
    }
    pub const fn description(&self) -> &ColumnName {
        &self.description
    }
    pub const fn predicate(&self) -> Option<&LiveRowPredicate> {
        self.predicate.as_ref()
    }
    pub const fn max_rows(&self) -> Option<NonZeroU64> {
        self.max_rows
    }
    pub const fn max_bytes(&self) -> Option<NonZeroU64> {
        self.max_bytes
    }
}

/// Checks the complete graph after all names have been parsed. A repeated path is a cycle;
/// a catalog referenced as prose does not become a structural contributor by this check.
pub(crate) fn check_references(entries: &[super::super::CatalogSettings]) -> Result<(), InvalidRdbmsCatalog> {
    fn visit(
        entries: &[super::super::CatalogSettings],
        name: &SourceName,
        path: &mut Vec<SourceName>,
        expanded: &mut usize,
    ) -> Result<(), InvalidRdbmsCatalog> {
        if path.len() >= 32 || path.contains(name) {
            return Err(invalid(InvalidDescriptionSource::Reference));
        }
        *expanded = expanded.saturating_add(1);
        if *expanded > 256 {
            return Err(invalid(InvalidDescriptionSource::Reference));
        }
        let entry = entries
            .iter()
            .find(|entry| entry.name() == name)
            .ok_or_else(|| invalid(InvalidDescriptionSource::Reference))?;
        if !matches!(
            entry.kind(),
            super::super::CatalogKind::Rdbms | super::super::CatalogKind::Datahub
        ) {
            return Err(invalid(InvalidDescriptionSource::Reference));
        }
        path.push(name.clone());
        if let Some(rdbms) = entry.rdbms() {
            for source in rdbms.descriptions_from() {
                if let DescriptionSource::Catalog(name) = source {
                    visit(entries, name, path, expanded)?;
                }
            }
        }
        path.pop();
        Ok(())
    }
    for entry in entries {
        if let Some(rdbms) = entry.rdbms() {
            let mut expanded = 0;
            for source in rdbms.descriptions_from() {
                if let DescriptionSource::Catalog(name) = source {
                    visit(entries, name, &mut vec![entry.name().clone()], &mut expanded)?;
                }
            }
        }
    }
    Ok(())
}

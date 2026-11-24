//! Prose-only catalog references and closed plain-table mappings.

use sutura_domain::model::{ColumnName, QualifiedTable, SourceName};

use super::{CatalogConnection, InvalidRdbmsCatalog, LiveRowPredicate, non_zero};
use crate::raw::{RawCatalogConnection, RawLiveRowPredicate, RawTablePredicate};
use std::collections::BTreeMap;
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
    #[serde(default)]
    pub(crate) live_row_predicates: Option<Vec<RawTablePredicate>>,
    pub(crate) max_dictionary_rows: Option<u64>,
    pub(crate) max_dictionary_bytes: Option<u64>,
    /// What a matched row's unusable prose does - `refuse` (default) or `omit`.
    pub(crate) on_invalid_description: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDescriptionKey {
    pub(crate) catalog: Option<String>,
    #[serde(default)]
    pub(crate) schema: Option<String>,
    #[serde(default)]
    pub(crate) table: Option<String>,
    pub(crate) column: String,
    #[serde(default)]
    pub(crate) via: Option<RawDescriptionKeyVia>,
    /// How a mapped row's own `schema_name`/`table_name`/`column_name` values are matched against
    /// the main dictionary's physical identity: `exact` (default) or `case_insensitive`
    /// (`telekom/sutura#1975`, `telekom/sutura#2040`). Never changes how this table's own SQL
    /// identifiers are quoted or bound - only how the two sides are compared after readback.
    #[serde(default, rename = "match")]
    pub(crate) key_match: Option<String>,
}

/// A key reached through a declared parent table (a SQL join), for a documentation system that
/// normalises what `descriptions_from`'s flat form stores on one row - `telekom/sutura#2040`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDescriptionKeyVia {
    pub(crate) table: String,
    pub(crate) on: BTreeMap<String, String>,
    pub(crate) schema: String,
    pub(crate) table_name: String,
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
    key_source: DescriptionKeySource,
    column_key: ColumnName,
    description: ColumnName,
    predicates: Vec<DescriptionPredicate>,
    max_rows: Option<NonZeroU64>,
    max_bytes: Option<NonZeroU64>,
    case_insensitive: bool,
    on_invalid_description: InvalidDescriptionPolicy,
}

/// What a matched row's unusable prose does, once its key has matched a visible table or column -
/// an unmatched row is ignored and never checked regardless of this policy.
///
/// `Refuse` is the default, unchanged. `Omit` is the opt-in that drops just that row, falling back
/// to the next ordered source or the dictionary's own native comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InvalidDescriptionPolicy {
    #[default]
    Refuse,
    Omit,
}
impl InvalidDescriptionPolicy {
    const ALL: [Self; 2] = [Self::Refuse, Self::Omit];
    pub(crate) fn parse(written: &str) -> Option<Self> {
        let written = written.trim();
        Self::ALL.into_iter().find(|policy| policy.as_str() == written)
    }
    const fn as_str(self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::Omit => "omit",
        }
    }
}

/// Where a mapped row's schema and physical table name come from: either columns on the mapped
/// table itself, or columns on a declared parent table reached by a join on declared columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptionKeySource {
    Direct { schema: ColumnName, table: ColumnName },
    Via(DescriptionVia),
}

/// The parent table a description key is resolved through, and the columns the join and the
/// schema/table-name projection read off it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptionVia {
    table: QualifiedTable,
    on: Vec<(ColumnName, ColumnName)>,
    schema: ColumnName,
    table_name: ColumnName,
}

/// Which physical table a [`DescriptionPredicate`] filters: the mapped table, or its `key.via`
/// parent. There is never a third table to name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateTable {
    Main,
    Via,
}

/// One closed-form live-row predicate naming the physical table it filters, so a mapping whose key
/// is reached through a parent table can filter rows on either side of the join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptionPredicate {
    table: PredicateTable,
    predicate: LiveRowPredicate,
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
    #[error("description key must set either schema and table, or via, never both or neither")]
    KeyShape,
    #[error("description key.via requires a nonempty on: mapping of exact column names")]
    ViaJoin,
    #[error("a description source may set live_row_predicate OR live_row_predicates, not both")]
    PredicateShape,
    #[error("live_row_predicates must be nonempty when set")]
    PredicateListEmpty,
    #[error("live_row_predicates entries' table must name the mapped table or its key.via table")]
    PredicateTable,
    #[error("key.match must be exact or case_insensitive")]
    KeyMatch,
    #[error("on_invalid_description must be one of refuse, omit")]
    InvalidDescriptionPolicy,
}

pub(crate) fn parse(name: &SourceName, raw: &[RawDescriptionSource]) -> Result<Vec<DescriptionSource>, InvalidRdbmsCatalog> {
    if raw.len() > 32 {
        return Err(invalid(InvalidDescriptionSource::TooMany { limit: 32 }));
    }
    raw.iter().map(|raw| parse_one(name, raw)).collect()
}

fn parse_one(name: &SourceName, raw: &RawDescriptionSource) -> Result<DescriptionSource, InvalidRdbmsCatalog> {
    if let Some(catalog) = &raw.catalog {
        if raw.connection.is_some()
            || raw.table.is_some()
            || raw.key.is_some()
            || raw.description.is_some()
            || raw.live_row_predicate.is_some()
            || raw.live_row_predicates.is_some()
            || raw.max_dictionary_rows.is_some()
            || raw.max_dictionary_bytes.is_some()
            || raw.on_invalid_description.is_some()
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
    let table = physical_table(table)?;
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
    let key_source = key_source(key, &column)?;
    let via_table = match &key_source {
        DescriptionKeySource::Via(via) => Some(&via.table),
        DescriptionKeySource::Direct { .. } => None,
    };
    if via_table == Some(&table) {
        // A `key.via` naming the mapped table itself gives every predicate exactly one physical
        // table to resolve to, so `PredicateTable::Via` could never be produced - a join declared
        // only to immediately collapse back onto its own starting table is always a mistake, not a
        // no-op worth accepting silently.
        return Err(invalid(InvalidDescriptionSource::ViaJoin));
    }
    let predicates = predicates(raw, &table, via_table)?;
    Ok(DescriptionSource::Table(Box::new(DescriptionMapping {
        connection: CatalogConnection::parse(name, connection).map_err(|cause| InvalidRdbmsCatalog::Connection { cause })?,
        table,
        catalog_key: key.catalog.as_deref().map(|raw| column(raw, "key.catalog")).transpose()?,
        key_source,
        column_key: column(&key.column, "key.column")?,
        description: column(description, "description")?,
        predicates,
        max_rows: non_zero("max_dictionary_rows", raw.max_dictionary_rows)?,
        max_bytes: non_zero("max_dictionary_bytes", raw.max_dictionary_bytes)?,
        case_insensitive: key_match(key)?,
        on_invalid_description: raw
            .on_invalid_description
            .as_deref()
            .map(|written| {
                InvalidDescriptionPolicy::parse(written)
                    .ok_or_else(|| invalid(InvalidDescriptionSource::InvalidDescriptionPolicy))
            })
            .transpose()?
            .unwrap_or_default(),
    })))
}

fn key_match(key: &RawDescriptionKey) -> Result<bool, InvalidRdbmsCatalog> {
    match key.key_match.as_deref() {
        None | Some("exact") => Ok(false),
        Some("case_insensitive") => Ok(true),
        Some(_) => Err(invalid(InvalidDescriptionSource::KeyMatch)),
    }
}

fn physical_table(raw: &str) -> Result<QualifiedTable, InvalidRdbmsCatalog> {
    if raw.chars().any(char::is_whitespace) {
        return Err(invalid(InvalidDescriptionSource::Table));
    }
    let table = QualifiedTable::parse(raw).map_err(|_cause| invalid(InvalidDescriptionSource::Table))?;
    if table.qualifier().is_none() {
        return Err(invalid(InvalidDescriptionSource::Table));
    }
    Ok(table)
}

fn key_source(
    key: &RawDescriptionKey,
    column: &impl Fn(&str, &'static str) -> Result<ColumnName, InvalidRdbmsCatalog>,
) -> Result<DescriptionKeySource, InvalidRdbmsCatalog> {
    match (&key.schema, &key.table, &key.via) {
        (Some(schema), Some(table), None) => Ok(DescriptionKeySource::Direct {
            schema: column(schema, "key.schema")?,
            table: column(table, "key.table")?,
        }),
        (None, None, Some(via)) => {
            if via.on.is_empty() {
                return Err(invalid(InvalidDescriptionSource::ViaJoin));
            }
            let on = via
                .on
                .iter()
                .map(|(main, parent)| Ok((column(main, "key.via.on")?, column(parent, "key.via.on")?)))
                .collect::<Result<Vec<_>, InvalidRdbmsCatalog>>()?;
            Ok(DescriptionKeySource::Via(DescriptionVia {
                table: physical_table(&via.table)?,
                on,
                schema: column(&via.schema, "key.via.schema")?,
                table_name: column(&via.table_name, "key.via.table_name")?,
            }))
        }
        _ => Err(invalid(InvalidDescriptionSource::KeyShape)),
    }
}

fn predicates(
    raw: &RawDescriptionSource,
    table: &QualifiedTable,
    via_table: Option<&QualifiedTable>,
) -> Result<Vec<DescriptionPredicate>, InvalidRdbmsCatalog> {
    match (&raw.live_row_predicate, &raw.live_row_predicates) {
        (Some(_), Some(_)) => Err(invalid(InvalidDescriptionSource::PredicateShape)),
        (Some(single), None) => Ok(vec![DescriptionPredicate {
            table: PredicateTable::Main,
            predicate: LiveRowPredicate::parse(single)?,
        }]),
        (None, Some(list)) => {
            if list.is_empty() {
                return Err(invalid(InvalidDescriptionSource::PredicateListEmpty));
            }
            list.iter()
                .map(|raw: &RawTablePredicate| {
                    let named = physical_table(&raw.table)?;
                    let target = if &named == table {
                        PredicateTable::Main
                    } else if Some(&named) == via_table {
                        PredicateTable::Via
                    } else {
                        return Err(invalid(InvalidDescriptionSource::PredicateTable));
                    };
                    Ok(DescriptionPredicate {
                        table: target,
                        predicate: LiveRowPredicate::parse_fields(&raw.column, &raw.operator, raw.value.as_deref())?,
                    })
                })
                .collect()
        }
        (None, None) => Ok(Vec::new()),
    }
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
    pub const fn key_source(&self) -> &DescriptionKeySource {
        &self.key_source
    }
    pub const fn column_key(&self) -> &ColumnName {
        &self.column_key
    }
    pub const fn description(&self) -> &ColumnName {
        &self.description
    }
    pub fn predicates(&self) -> &[DescriptionPredicate] {
        &self.predicates
    }
    pub const fn max_rows(&self) -> Option<NonZeroU64> {
        self.max_rows
    }
    pub const fn max_bytes(&self) -> Option<NonZeroU64> {
        self.max_bytes
    }
    /// Whether a mapped row's own identity columns match the main dictionary case-insensitively.
    pub const fn case_insensitive(&self) -> bool {
        self.case_insensitive
    }
    /// What a matched row's unusable prose does: `refuse` (default) or `omit`.
    pub const fn on_invalid_description(&self) -> InvalidDescriptionPolicy {
        self.on_invalid_description
    }
}

impl DescriptionVia {
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }
    pub fn on(&self) -> &[(ColumnName, ColumnName)] {
        &self.on
    }
    pub const fn schema(&self) -> &ColumnName {
        &self.schema
    }
    pub const fn table_name(&self) -> &ColumnName {
        &self.table_name
    }
}

impl DescriptionPredicate {
    pub const fn table(&self) -> PredicateTable {
        self.table
    }
    pub const fn predicate(&self) -> &LiveRowPredicate {
        &self.predicate
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

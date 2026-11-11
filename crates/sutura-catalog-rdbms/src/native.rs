//! Native selection and the physical allowlist used by composed catalogs.

use std::collections::{BTreeMap, BTreeSet};
use sutura_domain::catalog::{ColumnType, DictionarySelection, Model};
use sutura_domain::model::{ColumnName, QualifiedTable, SourceName};
use sutura_domain::warehouse::TypeDialect;
use crate::{Dictionary, RdbmsCatalog, RdbmsError};

#[cfg(feature = "live")]
pub(crate) mod sql;

#[derive(Debug, thiserror::Error)]
pub enum NativeSelectionError {
    #[error("native include {index} ({schema}, {table}) matches no visible table")]
    UnmatchedInclude { index: usize, schema: String, table: String },
    #[error("native table {table} has no included column")]
    NoColumns { table: String },
    #[error("native column {table}.{column} has no exact supported mapping for {data_type}")]
    UnsupportedType { table: String, column: String, data_type: String },
    #[error("composed model {model} on source {source_alias} names table {table} outside its native allowlist")]
    TableExcluded { model: String, source_alias: SourceName, table: QualifiedTable },
    #[error("composed model {model} names column {table}.{column} outside its native allowlist")]
    ColumnExcluded { model: String, table: QualifiedTable, column: ColumnName },
}

/// Effective physical columns from one successful native read; semantic aliases cannot widen it.
#[derive(Debug)]
pub struct NativeAllowlist {
    source: SourceName,
    tables: BTreeMap<QualifiedTable, BTreeSet<ColumnName>>,
}

impl NativeAllowlist {
    pub const fn source(&self) -> &SourceName { &self.source }

    pub fn check_model(&self, model: &Model) -> Result<(), NativeSelectionError> {
        if model.source() != &self.source { return Ok(()); }
        let allowed = self.tables.get(model.table()).ok_or_else(|| NativeSelectionError::TableExcluded {
            model: model.name().as_str().to_owned(), source_alias: self.source.clone(), table: model.table().clone(),
        })?;
        for column in model.columns() {
            if !allowed.contains(column.name()) {
                return Err(NativeSelectionError::ColumnExcluded {
                    model: model.name().as_str().to_owned(), table: model.table().clone(), column: column.name().clone(),
                });
            }
        }
        Ok(())
    }
}

pub(crate) fn select(
    dictionary: Dictionary,
    selection: &DictionarySelection,
    dialect: TypeDialect,
) -> Result<Dictionary, RdbmsError> {
    let mut matched = vec![false; selection.include().len()];
    let mut tables = Vec::new();
    for mut table in dictionary.tables {
        let mut included = false;
        for (index, include) in selection.include().iter().enumerate() {
            if include.schema().matches(table.address.schema()) && include.table().matches(table.address.table()) {
                matched[index] = true;
                included = true;
            }
        }
        if !included || selection.excludes_table(table.address.table()) { continue; }
        let mut columns = Vec::new();
        for column in &table.columns {
            let native = table.column_metadata(column).and_then(crate::ColumnMetadata::data_type).unwrap_or("");
            if selection.excludes_column(column) || selection.excludes_type(native) { continue; }
            let precise = ColumnType::parse(native).ok().and_then(|declared| dialect.arrow(&declared)).is_some();
            // Arrow mapping also includes Float32/Binary, which the row reader refuses.
            // Oracle normalizes timestamps, but FLOAT declares decimal values with binary precision.
            let lower = native.to_ascii_lowercase();
            let name = lower.split('(').next().unwrap_or("").trim_end();
            let unreadable = match dialect {
                TypeDialect::Oracle => matches!(name, "float" | "binary_float" | "raw" | "blob"),
                TypeDialect::Postgres => matches!(name, "real" | "float4" | "bytea" | "blob" | "timestamp" | "timestamp without time zone" | "timestamptz" | "timestamp with time zone"),
                _ => true,
            };
            let timestamp_too_precise = lower.strip_prefix("timestamp(")
                .and_then(|rest| rest.split_once(')')).and_then(|(precision, _)| precision.parse::<u8>().ok())
                .is_some_and(|precision| precision > 6);
            if !precise || unreadable || timestamp_too_precise {
                return Err(NativeSelectionError::UnsupportedType {
                    table: table.address.to_string(), column: column.clone(), data_type: native.to_owned(),
                }.into());
            }
            columns.push(column.clone());
        }
        if columns.is_empty() { return Err(NativeSelectionError::NoColumns { table: table.address.to_string() }.into()); }
        table.column_metadata.retain(|column, _| columns.contains(column));
        table.primary_key.retain(|column| columns.contains(column));
        table.columns = columns;
        tables.push(table);
    }
    for (index, include) in selection.include().iter().enumerate() {
        if !matched[index] {
            return Err(NativeSelectionError::UnmatchedInclude {
                index, schema: include.schema().as_str().to_owned(), table: include.table().as_str().to_owned(),
            }.into());
        }
    }
    if tables.is_empty() { return Err(RdbmsError::NoVisibleTables); }
    Ok(Dictionary::new(tables, Vec::new()))
}

impl<R: crate::DictionaryReader> RdbmsCatalog<R> {
    /// Loads once, returning the matching allowlist for composition before publication.
    pub fn load_with_allowlist(&self) -> Result<(sutura_domain::pinned::PinnedDefinitions, Option<NativeAllowlist>), RdbmsError> {
        let dictionary = self.reader.read_dictionary()?;
        let (definitions, knowledge) = self.assemble(&dictionary)?;
        let allowlist = dictionary.native.then(|| NativeAllowlist {
            source: self.source_alias.clone(),
            tables: definitions.models().values().map(|model| (model.table().clone(), model.columns().map(|column| column.name().clone()).collect())).collect(),
        });
        use sutura_domain::pinned::{Contribution, ContributionManifest, PinnedDefinitions, SemanticCatalog};
        let pinned = PinnedDefinitions::pin(self.version.clone(), definitions, knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())))
            .map_err(|cause| RdbmsError::Digest { cause })?;
        Ok((pinned, allowlist))
    }
}

#[cfg(test)]
mod tests {
    use crate::{ColumnMetadata, Dictionary, DictionaryReader, NativeSelectionError, RdbmsCatalog, RdbmsError, Table, TableAddress};
    use sutura_domain::catalog::{Column, Description, DictionaryInclude, DictionaryPattern, DictionarySelection, Model};
    use sutura_domain::model::{ColumnName, ModelName, QualifiedTable, SourceName};
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
    use sutura_domain::warehouse::TypeDialect;

    fn pattern(text: &str) -> DictionaryPattern {
        DictionaryPattern::parse(text).expect("a fixture pattern parses")
    }

    fn selection(table: &str, columns: &[&str], types: &[&str]) -> DictionarySelection {
        DictionarySelection::parse(
            vec![DictionaryInclude::new(pattern("public"), pattern(table))],
            vec![pattern("events_old")],
            columns.iter().map(|value| pattern(value)).collect(),
            types.iter().map(|value| pattern(value)).collect(),
        ).expect("the fixture selection is bounded")
    }

    fn table(name: &str, columns: &[(&str, &str)]) -> Table {
        Table::new(
            String::from(name), TableAddress::in_schema(String::from("public"), String::from(name)),
            columns.iter().map(|(name, _)| String::from(*name)).collect(), Some(String::from("Visible metadata.")),
        ).with_column_metadata(columns.iter().map(|(name, kind)| (String::from(*name), ColumnMetadata::new(Some(String::from(*kind)), Some(String::from("A column comment."))))))
    }

    struct NativeReader;

    impl DictionaryReader for NativeReader {
        fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
            Dictionary::new(vec![
                table("events", &[("id", "bigint"), ("secret", "text"), ("opaque", "jsonb")]),
                table("events_old", &[("id", "bigint")]),
            ], vec![]).native(&selection("event*", &["secret"], &["jsonb"]), TypeDialect::Postgres)
        }
    }

    #[test]
    fn native_selection_crosses_the_reader_port_with_only_supported_included_metadata() {
        let catalog = RdbmsCatalog::new(SourceName::parse("dictionary").unwrap(), DefinitionVersion::parse("dict-1").unwrap(), NativeReader)
            .with_source_alias(SourceName::parse("warehouse").unwrap());
        let pinned = catalog.load().expect("the selected dictionary loads through its port");
        assert_eq!(pinned.definitions().models().len(), 1);
        let model = pinned.definitions().model(&ModelName::parse("events").unwrap()).unwrap();
        assert_eq!(model.source().as_str(), "warehouse");
        assert_eq!(model.description(), "Visible metadata.");
        assert_eq!(model.columns().map(|column| column.name().as_str()).collect::<Vec<_>>(), ["id"]);
        let id = model.column(&ColumnName::parse("id").unwrap()).unwrap();
        assert_eq!(id.data_type().map(|kind| kind.as_str()), Some("bigint"));
        assert_eq!(id.description(), "A column comment.");
        assert!(pinned.manifest().get(&SourceName::parse("dictionary").unwrap()).is_some());
        assert!(pinned.manifest().get(&SourceName::parse("warehouse").unwrap()).is_none());
    }

    #[test]
    fn native_selection_refuses_unmatched_empty_and_inexact_types() {
        for (name, selected, expected) in [
            ("events", selection("absent", &[], &[]), "unmatched"),
            ("events", selection("events", &["*"], &[]), "empty"),
            ("events", selection("events", &[], &[]), "type"),
        ] {
            let error = Dictionary::new(vec![table(name, &[("opaque", "jsonb")])], vec![])
                .native(&selected, TypeDialect::Postgres).expect_err("a native dictionary cannot be partially admitted");
            assert!(matches!((expected, error),
                ("unmatched", RdbmsError::Native { cause: NativeSelectionError::UnmatchedInclude { .. } }) |
                ("empty", RdbmsError::Native { cause: NativeSelectionError::NoColumns { .. } }) |
                ("type", RdbmsError::Native { cause: NativeSelectionError::UnsupportedType { .. } })
            ));
        }
        for kind in ["NUMBER", "NUMBER(39,0)", "NUMBER(12,-2)", "FLOAT(20)", "TIMESTAMP(9)", "BINARY_FLOAT", "RAW(16)", "BLOB"] {
            assert!(matches!(Dictionary::new(vec![table("events", &[("value", kind)])], vec![])
                .native(&selection("events", &[], &[]), TypeDialect::Oracle),
                Err(RdbmsError::Native { cause: NativeSelectionError::UnsupportedType { .. } })), "{kind}");
        }
        for kind in ["real", "float4", "bytea", "blob", "timestamp", "timestamp without time zone", "timestamptz", "timestamp with time zone"] {
            assert!(matches!(Dictionary::new(vec![table("events", &[("value", kind)])], vec![])
                .native(&selection("events", &[], &[]), TypeDialect::Postgres),
                Err(RdbmsError::Native { cause: NativeSelectionError::UnsupportedType { .. } })), "{kind}");
        }
        assert!(matches!(Dictionary::new(vec![table("events_old", &[("id", "bigint")])], vec![])
            .native(&selection("event*", &[], &[]), TypeDialect::Postgres), Err(RdbmsError::NoVisibleTables)));
    }

    fn alias(source: &str, address: &str, column: &str) -> Model {
        Model::new(
            ModelName::parse("different_alias").unwrap(), SourceName::parse(source).unwrap(), QualifiedTable::parse(address).unwrap(),
            [Column::from_metadata(ColumnName::parse(column).unwrap(), Some(if column == "secret" { "text" } else { "bigint" }), None, None).unwrap()],
            Description::parse("").unwrap(),
        )
    }

    #[test]
    fn native_physical_allowlists_refuse_semantic_aliases_that_reintroduce_a_column_or_table() {
        let catalog = RdbmsCatalog::new(SourceName::parse("dictionary").unwrap(), DefinitionVersion::parse("dict-1").unwrap(), NativeReader)
            .with_source_alias(SourceName::parse("warehouse").unwrap());
        let (_, allowlist) = catalog.load_with_allowlist().unwrap();
        let allowlist = allowlist.expect("native reads carry their own effective authority");
        allowlist.check_model(&alias("warehouse", "public.events", "id")).expect("the positive control stays inside authority");
        assert!(matches!(allowlist.check_model(&alias("warehouse", "public.events", "secret")), Err(NativeSelectionError::ColumnExcluded { .. })));
        assert!(matches!(allowlist.check_model(&alias("warehouse", "public.events_old", "id")), Err(NativeSelectionError::TableExcluded { .. })));
        allowlist.check_model(&alias("another_source", "public.events", "secret")).expect("a dictionary is authoritative only for its source");
    }

    #[test]
    fn native_identifier_globs_preserve_case_unicode_and_sql_metacharacters() {
        for (glob, yes, no) in [("Event?", "Events", "events"), ("a*b?", "axxbz", "axxb"), ("a%b_\\c'", "a%b_\\c'", "axxbzc'"), ("?", "é", "ab"), ("*ab*cd", "zabxcd", "abdc")] {
            assert!(pattern(glob).matches(yes), "{glob} must match {yes}");
            assert!(!pattern(glob).matches(no), "{glob} must not match {no}");
        }
        assert_eq!(pattern("a%_\\'*").sql_like(), "a\\%\\_\\\\'%");
        assert!(!pattern("*").matches(&"x".repeat(257)));
    }
}

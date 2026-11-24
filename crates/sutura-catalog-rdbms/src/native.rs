//! Native selection and the physical allowlist used by composed catalogs.

use crate::{Dictionary, RdbmsCatalog, RdbmsError};
use std::collections::BTreeMap;
use sutura_domain::catalog::{ColumnType, DictionarySelection, EmptyTablePolicy, Model, UnsupportedColumnPolicy};
use sutura_domain::model::{ColumnName, QualifiedTable, SourceName};
use sutura_domain::pinned::{Contribution, ContributionManifest, PinnedDefinitions, SemanticCatalog};
use sutura_domain::warehouse::schema::TypeDialect;

#[cfg(feature = "live")]
pub(crate) mod sql;

/// One native column refused for carrying no exact supported mapping. Named in full, with its
/// table, so a single refusal can tell an operator every offending column at once rather than one
/// per restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedColumn {
    table: String,
    column: String,
    data_type: String,
}

impl UnsupportedColumn {
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }
    #[must_use]
    pub fn column(&self) -> &str {
        &self.column
    }
    #[must_use]
    pub fn data_type(&self) -> &str {
        &self.data_type
    }
}

impl std::fmt::Display for UnsupportedColumn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{} has no exact supported mapping for {}",
            self.table, self.column, self.data_type
        )
    }
}

/// How many offending columns or tables a refusal message names before it switches to a count. A
/// misconfigured catalog over a large dictionary could otherwise produce an unreadable refusal.
const REFUSAL_PREVIEW: usize = 20;

fn render_refusal(columns: &[UnsupportedColumn], empty_tables: &[String]) -> String {
    let mut parts = Vec::new();
    for column in columns.iter().take(REFUSAL_PREVIEW) {
        parts.push(column.to_string());
    }
    if columns.len() > REFUSAL_PREVIEW {
        parts.push(format!(
            "... and {} more unsupported column(s)",
            columns.len() - REFUSAL_PREVIEW
        ));
    }
    for table in empty_tables.iter().take(REFUSAL_PREVIEW) {
        parts.push(format!("{table} has no included column"));
    }
    if empty_tables.len() > REFUSAL_PREVIEW {
        parts.push(format!(
            "... and {} more table(s) with no included column",
            empty_tables.len() - REFUSAL_PREVIEW
        ));
    }
    parts.join("; ")
}

#[derive(Debug, thiserror::Error)]
pub enum NativeSelectionError {
    #[error("column {column} matches conflicting text policies")]
    ConflictingTextPolicy { column: String },
    #[error("native include {index} ({schema}, {table}) matches no visible table")]
    UnmatchedInclude { index: usize, schema: String, table: String },
    /// Every unsupported column and every table left with no included column, collected over one
    /// read rather than refused one at a time across restarts. `columns.len()` and
    /// `empty_tables.len()` are always stated even when the listed detail is truncated.
    #[error(
        "native dictionary refused {} unsupported column(s) and {} table(s) with no included column: {}",
        columns.len(),
        empty_tables.len(),
        render_refusal(columns, empty_tables)
    )]
    Unsupported {
        columns: Vec<UnsupportedColumn>,
        empty_tables: Vec<String>,
    },
    #[error("composed model {model} on source {source_alias} names table {table} outside its native allowlist")]
    TableExcluded {
        model: String,
        source_alias: SourceName,
        table: Box<QualifiedTable>,
    },
    #[error("composed model {model} names column {table}.{column} outside its native allowlist")]
    ColumnExcluded {
        model: String,
        table: Box<QualifiedTable>,
        column: ColumnName,
    },
}

type ColumnPolicies = BTreeMap<ColumnName, sutura_domain::catalog::TextPolicy>;

/// Effective physical columns from one successful native read; semantic aliases cannot widen it.
#[derive(Debug)]
pub struct NativeAllowlist {
    source: SourceName,
    tables: BTreeMap<QualifiedTable, ColumnPolicies>,
}

impl NativeAllowlist {
    pub const fn source(&self) -> &SourceName {
        &self.source
    }

    pub fn check_model(&self, model: &Model) -> Result<(), NativeSelectionError> {
        if model.source() != &self.source {
            return Ok(());
        }
        let allowed = self
            .tables
            .get(model.table())
            .ok_or_else(|| NativeSelectionError::TableExcluded {
                model: model.name().as_str().to_owned(),
                source_alias: self.source.clone(),
                table: Box::new(model.table().clone()),
            })?;
        for column in model.columns() {
            if allowed.get(column.name()) != Some(column.text_policy()) {
                return Err(NativeSelectionError::ColumnExcluded {
                    model: model.name().as_str().to_owned(),
                    table: Box::new(model.table().clone()),
                    column: column.name().clone(),
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
    unbounded_number: Option<sutura_domain::warehouse::schema::UnboundedNumber>,
) -> Result<Dictionary, RdbmsError> {
    let mut matched = vec![false; selection.include().len()];
    let mut tables = Vec::new();
    let mut unsupported = Vec::new();
    let mut empty_tables = Vec::new();
    for mut table in dictionary.tables {
        let mut included = false;
        for (matched, include) in matched.iter_mut().zip(selection.include()) {
            if include.schema().matches(table.address.schema()) && include.table().matches(table.address.table()) {
                *matched = true;
                included = true;
            }
        }
        if !included || selection.excludes_table(table.address.table()) {
            continue;
        }
        let mut columns = Vec::new();
        for column in &table.columns {
            let native = table
                .column_metadata(column)
                .and_then(crate::ColumnMetadata::data_type)
                .unwrap_or("");
            if selection.excludes_column(column) || selection.excludes_type(native) {
                continue;
            }
            let mut policy = None;
            for (pattern, rule) in selection.text_rules() {
                if pattern.matches(column) {
                    if policy.is_some_and(|previous| previous != rule) {
                        return Err(NativeSelectionError::ConflictingTextPolicy { column: column.clone() }.into());
                    }
                    policy = Some(rule);
                }
            }
            let bounded = policy.is_some_and(|rule| rule.max_text_bytes().is_some());
            let precise = ColumnType::parse(native)
                .ok()
                .and_then(|declared| dialect.arrow_with_unbounded_number(&declared, unbounded_number))
                .is_some();
            // Arrow mapping also includes Float32/Binary, which the row reader refuses.
            // Oracle normalizes timestamps, but FLOAT declares decimal values with binary precision.
            let lower = native.to_ascii_lowercase();
            let name = lower.split('(').next().unwrap_or("").trim_end();
            let unreadable = match dialect {
                TypeDialect::Oracle => {
                    matches!(name, "float" | "binary_float" | "raw" | "blob") || (matches!(name, "clob" | "nclob") && !bounded)
                }
                TypeDialect::Postgres => matches!(
                    name,
                    "real"
                        | "float4"
                        | "bytea"
                        | "blob"
                        | "timestamp"
                        | "timestamp without time zone"
                        | "timestamptz"
                        | "timestamp with time zone"
                ),
                _ => true,
            };
            let timestamp_too_precise = lower
                .strip_prefix("timestamp(")
                .and_then(|rest| rest.split_once(')'))
                .and_then(|(precision, _)| precision.parse::<u8>().ok())
                .is_some_and(|precision| precision > 6);
            if !precise || unreadable || timestamp_too_precise {
                match selection.on_unsupported_column() {
                    UnsupportedColumnPolicy::Omit => {
                        tracing::warn!(
                            table = %table.address,
                            column = %column,
                            native_type = native,
                            "omitting native column: no exact supported mapping"
                        );
                    }
                    UnsupportedColumnPolicy::Refuse => {
                        unsupported.push(UnsupportedColumn {
                            table: table.address.to_string(),
                            column: column.clone(),
                            data_type: native.to_owned(),
                        });
                    }
                }
                continue;
            }
            if let Some(policy) = policy {
                table.column_metadata.entry(column.clone()).or_default().text = policy.clone();
            }
            columns.push(column.clone());
        }
        if columns.is_empty() {
            match selection.on_empty_table() {
                EmptyTablePolicy::Omit => {
                    tracing::warn!(table = %table.address, "omitting native table: no included column remains");
                }
                EmptyTablePolicy::Refuse => {
                    empty_tables.push(table.address.to_string());
                }
            }
            continue;
        }
        table.column_metadata.retain(|column, _| columns.contains(column));
        if table.primary_key.iter().any(|column| !columns.contains(column)) {
            table.primary_key.clear();
        }
        table.columns = columns;
        tables.push(table);
    }
    if !unsupported.is_empty() || !empty_tables.is_empty() {
        return Err(NativeSelectionError::Unsupported {
            columns: unsupported,
            empty_tables,
        }
        .into());
    }
    for (index, (matched, include)) in matched.iter().zip(selection.include()).enumerate() {
        if !matched {
            return Err(NativeSelectionError::UnmatchedInclude {
                index,
                schema: include.schema().as_str().to_owned(),
                table: include.table().as_str().to_owned(),
            }
            .into());
        }
    }
    if tables.is_empty() {
        return Err(RdbmsError::NoVisibleTables);
    }
    let relationships = dictionary
        .relationships
        .into_iter()
        .filter(|relationship| {
            let origin = tables.iter().find(|table| table.address() == relationship.origin_table());
            let target = tables.iter().find(|table| table.address() == relationship.target_table());
            origin.zip(target).is_some_and(|(origin, target)| {
                relationship.pairs().all(|(left, right)| {
                    origin.columns().iter().any(|column| column == left) && target.columns().iter().any(|column| column == right)
                })
            })
        })
        .collect();
    Ok(Dictionary::new(tables, relationships))
}

/// A pinned bundle and the authority produced by the same native read.
pub type NativeLoad = (PinnedDefinitions, Option<NativeAllowlist>);

pub(crate) fn load<R>(catalog: &RdbmsCatalog<R>) -> Result<NativeLoad, RdbmsError>
where
    R: crate::DictionaryReader,
{
    let dictionary = catalog.reader.read_dictionary()?;
    let (definitions, knowledge) = catalog.assemble(&dictionary)?;
    let allowlist = dictionary.native.then(|| NativeAllowlist {
        source: catalog.source_alias.clone(),
        tables: definitions
            .models()
            .values()
            .map(|model| {
                (
                    model.table().clone(),
                    model
                        .columns()
                        .map(|column| (column.name().clone(), column.text_policy().clone()))
                        .collect(),
                )
            })
            .collect(),
    });
    let pinned = PinnedDefinitions::pin(
        catalog.version.clone(),
        definitions,
        knowledge,
        ContributionManifest::single(
            catalog.name.clone(),
            Contribution::of(<RdbmsCatalog<R> as SemanticCatalog>::capabilities()),
        ),
    )
    .map_err(|cause| RdbmsError::Digest { cause })?;
    Ok((pinned, allowlist))
}

#[cfg(test)]
mod tests {
    use crate::{
        ColumnMetadata, Dictionary, DictionaryReader, NativeSelectionError, RdbmsCatalog, RdbmsError, Table, TableAddress,
    };
    use sutura_domain::catalog::{
        Column, Description, DictionaryInclude, DictionaryPattern, DictionarySelection, EmptyTablePolicy, Model,
        UnsupportedColumnPolicy,
    };
    use sutura_domain::model::{ColumnName, ModelName, QualifiedTable, SourceName};
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
    use sutura_domain::warehouse::schema::TypeDialect;

    fn pattern(text: &str) -> DictionaryPattern {
        DictionaryPattern::parse(text).expect("a fixture pattern parses")
    }

    fn selection(table: &str, columns: &[&str], types: &[&str]) -> DictionarySelection {
        DictionarySelection::parse(
            vec![DictionaryInclude::new(pattern("public"), pattern(table))],
            vec![pattern("events_old")],
            columns.iter().map(|value| pattern(value)).collect(),
            types.iter().map(|value| pattern(value)).collect(),
        )
        .expect("the fixture selection is bounded")
    }

    fn table(name: &str, columns: &[(&str, &str)]) -> Table {
        Table::new(
            String::from(name),
            TableAddress::in_schema(String::from("public"), String::from(name)),
            columns.iter().map(|(name, _)| String::from(*name)).collect(),
            Some(String::from("Visible metadata.")),
        )
        .with_column_metadata(columns.iter().map(|(name, kind)| {
            (
                String::from(*name),
                ColumnMetadata::new(Some(String::from(*kind)), Some(String::from("A column comment."))),
            )
        }))
    }

    struct NativeReader;

    impl DictionaryReader for NativeReader {
        fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
            Dictionary::new(
                vec![
                    table("events", &[("id", "bigint"), ("secret", "text"), ("opaque", "jsonb")]),
                    table("events_old", &[("id", "bigint")]),
                ],
                vec![],
            )
            .native(&selection("event*", &["secret"], &["jsonb"]), TypeDialect::Postgres)
        }
    }

    #[test]
    fn native_selection_crosses_the_reader_port_with_only_supported_included_metadata() {
        let catalog = RdbmsCatalog::new(
            SourceName::parse("dictionary").unwrap(),
            DefinitionVersion::parse("dict-1").unwrap(),
            NativeReader,
        )
        .with_source_alias(SourceName::parse("warehouse").unwrap());
        let pinned = catalog.load().expect("the selected dictionary loads through its port");
        assert_eq!(pinned.definitions().models().len(), 1);
        let model = pinned.definitions().model(&ModelName::parse("events").unwrap()).unwrap();
        assert_eq!(model.source().as_str(), "warehouse");
        assert_eq!(model.description(), "Visible metadata.");
        assert_eq!(
            model.columns().map(|column| column.name().as_str()).collect::<Vec<_>>(),
            ["id"]
        );
        let id = model.column(&ColumnName::parse("id").unwrap()).unwrap();
        assert_eq!(id.data_type().map(sutura_domain::catalog::ColumnType::as_str), Some("bigint"));
        assert_eq!(id.description(), "A column comment.");
        assert!(pinned.manifest().get(&SourceName::parse("dictionary").unwrap()).is_some());
        assert!(pinned.manifest().get(&SourceName::parse("warehouse").unwrap()).is_none());
    }

    #[test]
    fn oracle_native_admission_uses_the_number_policy_without_rewriting_declared_metadata() {
        use sutura_domain::warehouse::schema::UnboundedNumber;
        for kind in ["NUMBER", "TIMESTAMP(6) WITH TIME ZONE", "TIMESTAMP(3) WITH LOCAL TIME ZONE"] {
            let selected = Dictionary::new(vec![table("events", &[("value", kind)])], vec![])
                .native_with_unbounded_number(
                    &selection("events", &[], &[]),
                    TypeDialect::Oracle,
                    Some(UnboundedNumber::Utf8),
                )
                .expect("the explicit representation and supported UTC timestamps are admitted");
            assert_eq!(selected.tables()[0].column_metadata("value").unwrap().data_type(), Some(kind));
        }
        let refused = Dictionary::new(vec![table("events", &[("value", "TIMESTAMP(9) WITH TIME ZONE")])], vec![])
            .native_with_unbounded_number(
                &selection("events", &[], &[]),
                TypeDialect::Oracle,
                Some(UnboundedNumber::Utf8),
            )
            .expect_err("sub-microsecond precision is refused");
        assert!(matches!(
            refused,
            RdbmsError::Native {
                cause: NativeSelectionError::Unsupported { .. }
            }
        ));
    }

    #[test]
    fn native_selection_refuses_unmatched_empty_and_inexact_types() {
        for (name, selected, expected) in [
            ("events", selection("absent", &[], &[]), "unmatched"),
            ("events", selection("events", &["*"], &[]), "empty"),
            ("events", selection("events", &[], &[]), "type"),
        ] {
            let error = Dictionary::new(vec![table(name, &[("opaque", "jsonb")])], vec![])
                .native(&selected, TypeDialect::Postgres)
                .expect_err("a native dictionary cannot be partially admitted");
            assert!(matches!(
                (expected, error),
                (
                    "unmatched",
                    RdbmsError::Native {
                        cause: NativeSelectionError::UnmatchedInclude { .. }
                    }
                ) | (
                    "empty",
                    RdbmsError::Native {
                        cause: NativeSelectionError::Unsupported { .. }
                    }
                ) | (
                    "type",
                    RdbmsError::Native {
                        cause: NativeSelectionError::Unsupported { .. }
                    }
                )
            ));
        }
        for kind in [
            "NUMBER",
            "NUMBER(39,0)",
            "NUMBER(12,-2)",
            "FLOAT(20)",
            "TIMESTAMP(9)",
            "BINARY_FLOAT",
            "RAW(16)",
            "BLOB",
        ] {
            assert!(
                matches!(
                    Dictionary::new(vec![table("events", &[("value", kind)])], vec![])
                        .native(&selection("events", &[], &[]), TypeDialect::Oracle),
                    Err(RdbmsError::Native {
                        cause: NativeSelectionError::Unsupported { .. }
                    })
                ),
                "{kind}"
            );
        }
        for kind in [
            "real",
            "float4",
            "bytea",
            "blob",
            "timestamp",
            "timestamp without time zone",
            "timestamptz",
            "timestamp with time zone",
        ] {
            assert!(
                matches!(
                    Dictionary::new(vec![table("events", &[("value", kind)])], vec![])
                        .native(&selection("events", &[], &[]), TypeDialect::Postgres),
                    Err(RdbmsError::Native {
                        cause: NativeSelectionError::Unsupported { .. }
                    })
                ),
                "{kind}"
            );
        }
        assert!(matches!(
            Dictionary::new(vec![table("events_old", &[("id", "bigint")])], vec![])
                .native(&selection("event*", &[], &[]), TypeDialect::Postgres),
            Err(RdbmsError::NoVisibleTables)
        ));
    }

    #[test]
    fn native_selection_refuses_with_the_complete_list_in_one_pass() {
        let error = Dictionary::new(
            vec![
                table("events", &[("id", "bigint"), ("a", "real"), ("b", "bytea")]),
                table("totals", &[("c", "blob")]),
            ],
            vec![],
        )
        .native(&selection("*", &["c"], &[]), TypeDialect::Postgres)
        .expect_err("two unsupported columns and one emptied table refuse together");
        let RdbmsError::Native {
            cause: NativeSelectionError::Unsupported { columns, empty_tables },
        } = error
        else {
            panic!("expected Unsupported, got {error:?}");
        };
        assert_eq!(
            columns
                .iter()
                .map(|column| (column.table(), column.column(), column.data_type()))
                .collect::<Vec<_>>(),
            [("public.events", "a", "real"), ("public.events", "b", "bytea")]
        );
        assert_eq!(empty_tables, ["public.totals"]);
    }

    #[test]
    fn native_selection_omits_an_unsupported_column_instead_of_refusing_when_opted_in() {
        let selected = Dictionary::new(vec![table("events", &[("id", "bigint"), ("junk", "bytea")])], vec![])
            .native(
                &selection("events", &[], &[]).with_unsupported_column_policy(UnsupportedColumnPolicy::Omit),
                TypeDialect::Postgres,
            )
            .expect("an omitted unsupported column no longer refuses the load");
        let model = &selected.tables()[0];
        assert_eq!(model.columns(), [String::from("id")]);
        assert!(model.column_metadata("junk").is_none());
    }

    #[test]
    fn native_selection_omits_an_empty_table_instead_of_refusing_when_opted_in() {
        let selected = Dictionary::new(
            vec![table("events", &[("id", "bigint")]), table("totals", &[("junk", "bigint")])],
            vec![],
        )
        .native(
            &selection("*", &["junk"], &[]).with_empty_table_policy(EmptyTablePolicy::Omit),
            TypeDialect::Postgres,
        )
        .expect("a table left with no included column is dropped, not refused");
        assert_eq!(
            selected
                .tables()
                .iter()
                .map(|table| table.address().to_string())
                .collect::<Vec<_>>(),
            ["public.events"]
        );
    }

    #[test]
    fn native_physical_allowlist_omits_an_unsupported_column_so_a_composed_model_cannot_reintroduce_it() {
        let catalog = RdbmsCatalog::new(
            SourceName::parse("dictionary").unwrap(),
            DefinitionVersion::parse("dict-1").unwrap(),
            OmittingReader,
        )
        .with_source_alias(SourceName::parse("warehouse").unwrap());
        let (_, allowlist) = catalog.load_with_allowlist().unwrap();
        let allowlist = allowlist.expect("native reads carry their own effective authority");
        let refused = allowlist
            .check_model(&alias("warehouse", "public.events", "junk"))
            .expect_err("the omitted column is outside the allowlist, exactly like an excluded one");
        assert!(matches!(refused, NativeSelectionError::ColumnExcluded { .. }));
    }

    struct OmittingReader;

    impl DictionaryReader for OmittingReader {
        fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
            Dictionary::new(vec![table("events", &[("id", "bigint"), ("junk", "bytea")])], vec![]).native(
                &selection("events", &[], &[]).with_unsupported_column_policy(UnsupportedColumnPolicy::Omit),
                TypeDialect::Postgres,
            )
        }
    }

    fn alias(source: &str, address: &str, column: &str) -> Model {
        Model::new(
            ModelName::parse("different_alias").unwrap(),
            SourceName::parse(source).unwrap(),
            QualifiedTable::parse(address).unwrap(),
            [Column::from_metadata(
                ColumnName::parse(column).unwrap(),
                Some(if column == "secret" { "text" } else { "bigint" }),
                None,
                None,
            )
            .unwrap()],
            Description::parse("").unwrap(),
        )
    }

    #[test]
    fn native_physical_allowlists_refuse_semantic_aliases_that_reintroduce_a_column_or_table() {
        let catalog = RdbmsCatalog::new(
            SourceName::parse("dictionary").unwrap(),
            DefinitionVersion::parse("dict-1").unwrap(),
            NativeReader,
        )
        .with_source_alias(SourceName::parse("warehouse").unwrap());
        let (_, allowlist) = catalog.load_with_allowlist().unwrap();
        let allowlist = allowlist.expect("native reads carry their own effective authority");
        allowlist
            .check_model(&alias("warehouse", "public.events", "id"))
            .expect("the positive control stays inside authority");
        assert!(matches!(
            allowlist.check_model(&alias("warehouse", "public.events", "secret")),
            Err(NativeSelectionError::ColumnExcluded { .. })
        ));
        assert!(matches!(
            allowlist.check_model(&alias("warehouse", "public.events_old", "id")),
            Err(NativeSelectionError::TableExcluded { .. })
        ));
        allowlist
            .check_model(&alias("another_source", "public.events", "secret"))
            .expect("a dictionary is authoritative only for its source");
    }
}

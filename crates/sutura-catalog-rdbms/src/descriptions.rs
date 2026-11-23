//! Bounded prose keyed by exact physical identity. No structure crosses this boundary.

#[cfg(feature = "live")]
mod mapping;
#[cfg(feature = "live")]
pub use mapping::{MappedDescriptions, MappedKeySource, MappedPredicate, MappedTable, MappedVia};

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::catalog::{Definitions, Description};
use sutura_domain::model::{ColumnName, DatasetName, ProjectName, QualifiedTable, TableName, TableQualifier};

use crate::{Dictionary, DictionaryBounds, RdbmsError, TableAddress};

/// The colliding table, and the two column identifiers that fold to the same identity under it.
type AmbiguousColumn = (QualifiedTable, ColumnName, ColumnName);

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
    #[error("case-insensitive matching is ambiguous: {} and {} fold to the same table identity", .0.0, .0.1)]
    AmbiguousTableCaseFold(Box<(QualifiedTable, QualifiedTable)>),
    #[error("case-insensitive matching is ambiguous: {}.{} and {}.{} fold to the same column identity", .0.0, .0.1, .0.0, .0.2)]
    AmbiguousColumnCaseFold(Box<AmbiguousColumn>),
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
    /// Whether this source's own keys were folded to lower case at construction, so [`apply`]
    /// knows to probe it with a folded physical identity instead of the dictionary's exact one.
    case_insensitive: bool,
}

struct Projection {
    overlay: DescriptionOverlay,
    bounds: DictionaryBounds,
    rows: u64,
    bytes: u64,
}

impl Projection {
    const fn new(bounds: DictionaryBounds, case_insensitive: bool) -> Self {
        Self {
            overlay: DescriptionOverlay {
                tables: BTreeMap::new(),
                columns: BTreeMap::new(),
                case_insensitive,
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
        let folded = if self.overlay.case_insensitive {
            column.to_lowercase()
        } else {
            column.to_owned()
        };
        let column = exact(&folded)
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
    physical_folded(address, false)
}

/// Like [`physical`], but when `fold` is set, lower-cases every identifier segment before parsing.
/// Used only to compare a case-insensitive description source against the dictionary's own
/// physical identity, never to change what SQL this crate emits.
fn physical_folded(address: &TableAddress, fold: bool) -> Result<QualifiedTable, DescriptionError> {
    let lower = |raw: &str| if fold { raw.to_lowercase() } else { raw.to_owned() };
    let parse = || {
        let table = TableName::parse(exact(&lower(address.table()))?)?;
        let schema = DatasetName::parse(exact(&lower(address.schema()))?)?;
        let qualifier = match address.catalog() {
            Some(catalog) => TableQualifier::in_project(ProjectName::parse(exact(&lower(catalog))?)?, schema),
            None => TableQualifier::in_dataset(schema),
        };
        Ok(QualifiedTable::new(Some(qualifier), table))
    };
    parse().map_err(|cause| DescriptionError::Identifier { cause })
}

impl DescriptionOverlay {
    pub fn from_dictionary(
        dictionary: &Dictionary,
        bounds: DictionaryBounds,
        case_insensitive: bool,
    ) -> Result<Self, DescriptionError> {
        let mut projection = Projection::new(bounds, case_insensitive);
        for table in dictionary.tables() {
            let address_bytes = table.address.to_string().len();
            projection.admit(
                address_bytes.saturating_add(table.description.as_deref().unwrap_or("").len()),
                table.columns.is_empty(),
            )?;
            let address = physical_folded(&table.address, case_insensitive)?;
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
        let mut projection = Projection::new(bounds, false);
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
    // Folding runs only when some source actually asked for it: with none, this must be
    // byte-identical to the exact-only path, and a served identifier whose lower case does not
    // parse must never newly refuse a dictionary that loads today.
    let fold_active = sources.iter().any(|source| source.case_insensitive);
    let mut visible = BTreeSet::new();
    let mut folded_tables: BTreeMap<QualifiedTable, QualifiedTable> = BTreeMap::new();
    for table in &mut dictionary.tables {
        let address = physical(&table.address).map_err(|cause| RdbmsError::Read(Box::new(cause)))?;
        let folded_address = if fold_active {
            let folded = physical_folded(&table.address, true).map_err(|cause| RdbmsError::Read(Box::new(cause)))?;
            if let Some(first) = folded_tables.insert(folded.clone(), address.clone())
                && first != address
            {
                return Err(RdbmsError::Read(Box::new(DescriptionError::AmbiguousTableCaseFold(
                    Box::new((first, address)),
                ))));
            }
            Some(folded)
        } else {
            None
        };
        if let Some(prose) = sources
            .iter()
            .filter_map(|source| {
                let probe = match (source.case_insensitive, &folded_address) {
                    (true, Some(folded)) => folded,
                    _ => &address,
                };
                source.tables.get(probe)
            })
            .find(|prose| !prose.is_empty())
        {
            table.description = Some(prose.as_str().to_owned());
        }
        let mut folded_columns: BTreeMap<ColumnName, ColumnName> = BTreeMap::new();
        for column in &table.columns {
            let column_key = ColumnName::parse(column).map_err(|cause| RdbmsError::ColumnName {
                table: address.to_string(),
                column: column.clone(),
                cause,
            })?;
            let folded_key = if fold_active {
                let folded_column = ColumnName::parse(column.to_lowercase()).map_err(|cause| RdbmsError::ColumnName {
                    table: address.to_string(),
                    column: column.clone(),
                    cause,
                })?;
                if let Some(first) = folded_columns.insert(folded_column.clone(), column_key.clone())
                    && first != column_key
                {
                    return Err(RdbmsError::Read(Box::new(DescriptionError::AmbiguousColumnCaseFold(
                        Box::new((address, first, column_key)),
                    ))));
                }
                Some((folded_address.clone().unwrap_or_else(|| address.clone()), folded_column))
            } else {
                None
            };
            let key = (address.clone(), column_key);
            if let Some(prose) = sources
                .iter()
                .filter_map(|source| {
                    let probe = match (source.case_insensitive, &folded_key) {
                        (true, Some(folded)) => folded,
                        _ => &key,
                    };
                    source.columns.get(probe)
                })
                .find(|prose| !prose.is_empty())
            {
                table.column_metadata.entry(column.clone()).or_default().description = Some(prose.as_str().to_owned());
            }
            visible.insert(key);
            // Visible under its folded identity too, so a case-insensitive source's already-
            // matched key is never miscounted as an ignored, unmatched row below - never done
            // when `fold_active` is false, which keeps the exact-only count unchanged.
            if let Some(folded) = folded_key {
                visible.insert(folded);
            }
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
            .map(|dictionary| DescriptionOverlay::from_dictionary(dictionary, bounds(100, 10000), false).expect("bounded prose"))
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
        let overlay = DescriptionOverlay::from_dictionary(&docs, bounds(100, 10000), false).expect("exact records");
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

    /// `key.match: case_insensitive` (`telekom/sutura#1975`, `telekom/sutura#2040`) is opt-in: it
    /// relaxes only this source's own comparison, never the SQL this crate emits elsewhere, and
    /// never the pinned exact-match default above.
    #[test]
    fn a_case_insensitive_source_matches_a_differently_cased_physical_identity() {
        let docs = dictionary("PUBLIC", &[("ID", "Folded match")]);
        let overlay = DescriptionOverlay::from_dictionary(&docs, bounds(100, 10000), true).expect("folded records");
        let (actual, ignored) = dictionary("public", &[("id", "Native")])
            .overlay_descriptions(&[overlay])
            .expect("overlaid");
        assert_eq!(ignored, 0);
        assert_eq!(
            actual
                .tables()
                .first()
                .and_then(|table| table.column_metadata("id"))
                .and_then(crate::ColumnMetadata::description),
            Some("Folded match")
        );
    }

    /// Folding only ever runs when some source asked for it (`fold_active` in `apply`): with every
    /// source exact, an exact-only source row that merely shares a folded identity with the served
    /// column must stay ignored, not newly match - byte-identical to pre-fold-feature behaviour.
    /// Red at `671af707e`, which folded unconditionally and miscounted this row as matched.
    #[test]
    fn exact_mode_counts_a_source_row_matching_only_by_fold_as_ignored() {
        let served = dictionary("PUBLIC", &[("ID", "Served")]);
        let source = dictionary("public", &[("id", "Should not attach")]);
        let overlay = DescriptionOverlay::from_dictionary(&source, bounds(100, 10000), false).expect("exact records");
        let (actual, ignored) = served.overlay_descriptions(&[overlay]).expect("overlaid");
        assert_eq!(ignored, 1, "an exact-only source must never match by a folded identity");
        assert_eq!(
            actual
                .tables()
                .first()
                .and_then(|table| table.column_metadata("ID"))
                .and_then(crate::ColumnMetadata::description),
            Some("Served")
        );
    }

    /// An exact source and a case-insensitive source are independent: each is probed by its own
    /// comparison, and one source opting in never relaxes the other.
    #[test]
    fn an_exact_source_and_a_case_insensitive_source_are_probed_independently() {
        let exact_docs = dictionary("public", &[("id", "Exact match")]);
        let folded_docs = dictionary("PUBLIC", &[("AMOUNT", "Folded match")]);
        let exact_overlay = DescriptionOverlay::from_dictionary(&exact_docs, bounds(100, 10000), false).expect("exact records");
        let folded_overlay = DescriptionOverlay::from_dictionary(&folded_docs, bounds(100, 10000), true).expect("folded records");
        let served = dictionary("public", &[("id", "Native id"), ("amount", "Native amount")]);
        let (actual, ignored) = served
            .overlay_descriptions(&[exact_overlay, folded_overlay])
            .expect("overlaid");
        assert_eq!(ignored, 0);
        let table = actual.tables().first().expect("table");
        assert_eq!(
            table.column_metadata("id").and_then(crate::ColumnMetadata::description),
            Some("Exact match")
        );
        assert_eq!(
            table.column_metadata("amount").and_then(crate::ColumnMetadata::description),
            Some("Folded match")
        );
    }

    /// Two source rows for the same table that differ only by case are a `Duplicate`/`DuplicateTable`
    /// under case-insensitive matching, exactly as two byte-identical rows already are - never a
    /// silent last-write-wins.
    #[test]
    fn two_case_insensitive_source_tables_differing_only_by_case_are_refused_as_duplicates() {
        let mut source = dictionary("public", &[("id", "One")]);
        source.tables.extend(dictionary("PUBLIC", &[("id", "Two")]).tables);
        assert!(matches!(
            DescriptionOverlay::from_dictionary(&source, bounds(100, 10000), true),
            Err(DescriptionError::DuplicateTable { .. })
        ));
    }

    #[test]
    fn two_case_insensitive_source_columns_differing_only_by_case_are_refused_as_duplicates() {
        let source = dictionary("public", &[("id", "One"), ("ID", "Two")]);
        assert!(matches!(
            DescriptionOverlay::from_dictionary(&source, bounds(100, 10000), true),
            Err(DescriptionError::Duplicate { .. })
        ));
    }

    /// Blocking defect: a served dictionary with both `ID` and `id` columns (or `Orders`/`orders`
    /// tables) must never let a case-insensitive source write its one description onto both -
    /// that is silent wrong attachment. Refuse instead, naming the colliding identifiers.
    #[test]
    fn a_dictionary_with_two_columns_differing_only_by_case_refuses_a_case_insensitive_source() {
        let served = dictionary("public", &[("ID", "Upper"), ("id", "Lower")]);
        let source = dictionary("public", &[("whatever", "Prose")]);
        let overlay = DescriptionOverlay::from_dictionary(&source, bounds(100, 10000), true).expect("folded records");
        let error = served
            .overlay_descriptions(&[overlay])
            .expect_err("colliding columns must refuse, not silently attach to both");
        let RdbmsError::Read(cause) = error else {
            panic!("expected RdbmsError::Read, got {error:?}");
        };
        assert!(
            cause
                .downcast_ref::<DescriptionError>()
                .is_some_and(|cause| matches!(cause, DescriptionError::AmbiguousColumnCaseFold(..)))
        );
    }

    #[test]
    fn two_tables_differing_only_by_case_refuse_a_case_insensitive_source() {
        let mut served = dictionary("public", &[("id", "Lower table")]);
        served.tables.extend(dictionary("PUBLIC", &[("id", "Upper table")]).tables);
        let source = dictionary("public", &[("whatever", "Prose")]);
        let overlay = DescriptionOverlay::from_dictionary(&source, bounds(100, 10000), true).expect("folded records");
        let error = served
            .overlay_descriptions(&[overlay])
            .expect_err("colliding tables must refuse, not silently attach to both");
        let RdbmsError::Read(cause) = error else {
            panic!("expected RdbmsError::Read, got {error:?}");
        };
        assert!(
            cause
                .downcast_ref::<DescriptionError>()
                .is_some_and(|cause| matches!(cause, DescriptionError::AmbiguousTableCaseFold(..)))
        );
    }

    #[test]
    fn description_projection_refuses_duplicate_keys_caps_and_control_characters() {
        let mut repeated = dictionary("public", &[("id", "One")]);
        repeated.tables.extend(dictionary("public", &[("id", "Two")]).tables);
        assert!(matches!(
            DescriptionOverlay::from_dictionary(&repeated, bounds(100, 10000), false),
            Err(DescriptionError::DuplicateTable { .. })
        ));
        assert!(matches!(
            DescriptionOverlay::from_dictionary(
                &dictionary("public", &[("id", "One"), ("id", "Two")]),
                bounds(100, 10000),
                false
            ),
            Err(DescriptionError::Duplicate { .. })
        ));
        assert!(matches!(
            DescriptionOverlay::from_dictionary(
                &dictionary("public", &[("id", "Prose"), ("extra", "Prose")]),
                bounds(1, 10000),
                false
            ),
            Err(DescriptionError::Rows { .. })
        ));
        assert!(matches!(
            DescriptionOverlay::from_dictionary(&dictionary("public", &[("id", "Prose")]), bounds(100, 1), false),
            Err(DescriptionError::Bytes { .. })
        ));
        assert!(matches!(
            DescriptionOverlay::from_dictionary(&dictionary("public", &[("id", "Bad\u{1b}prose")]), bounds(100, 10000), false),
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

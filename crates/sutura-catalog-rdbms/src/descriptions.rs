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

/// What a matched row's unusable prose does.
///
/// Governs only a row whose key has already matched a visible table or column in the served
/// dictionary - a row whose key resolves to no selected column is never checked at all.
///
/// `Refuse` is the default, unchanged. `Omit` is the opt-in that drops just that row, falling back
/// to the next ordered source or the dictionary's own native comment - the same fallback an
/// unmatched row already takes. An omitted row is logged with its key (schema, table, column),
/// never with its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InvalidDescriptionPolicy {
    #[default]
    Refuse,
    Omit,
}

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
    /// A matched table's description is not usable. Names the key, never the offending text.
    #[error("the description of table {table} is not usable")]
    TableProse {
        table: QualifiedTable,
        #[source]
        cause: sutura_domain::catalog::InvalidDescription,
    },
    /// A matched column's description is not usable. Names the key, never the offending text.
    #[error("the description of column {table}.{column} is not usable")]
    ColumnProse {
        table: QualifiedTable,
        column: ColumnName,
        #[source]
        cause: sutura_domain::catalog::InvalidDescription,
    },
}

type PhysicalColumn = (QualifiedTable, ColumnName);

/// A dictionary with the number of description rows outside its visible columns.
pub type OverlaidDictionary = (Dictionary, u64);

/// One ordered source's complete prose projection, bounded but **not yet validated**.
///
/// Prose is parsed only once its key has matched a visible table or column in [`apply`], so an
/// unmatched row's invalid prose never reaches [`Description::parse`] at all.
#[derive(Debug)]
pub struct DescriptionOverlay {
    tables: BTreeMap<QualifiedTable, String>,
    columns: BTreeMap<PhysicalColumn, String>,
    /// Whether this source's own keys were folded to lower case at construction, so [`apply`]
    /// knows to probe it with a folded physical identity instead of the dictionary's exact one.
    case_insensitive: bool,
    /// What a matched row of this source does if its prose fails [`Description::parse`].
    on_invalid_description: InvalidDescriptionPolicy,
}

struct Projection {
    overlay: DescriptionOverlay,
    bounds: DictionaryBounds,
    rows: u64,
    bytes: u64,
}

impl Projection {
    const fn new(bounds: DictionaryBounds, case_insensitive: bool, on_invalid_description: InvalidDescriptionPolicy) -> Self {
        Self {
            overlay: DescriptionOverlay {
                tables: BTreeMap::new(),
                columns: BTreeMap::new(),
                case_insensitive,
                on_invalid_description,
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

    /// Records a column's raw prose unparsed - only its physical identifier is checked here, never
    /// its text. [`apply`] parses it lazily, after matching.
    fn column(&mut self, table: &QualifiedTable, column: &str, prose: &str) -> Result<(), DescriptionError> {
        let folded = if self.overlay.case_insensitive {
            column.to_lowercase()
        } else {
            column.to_owned()
        };
        let column = exact(&folded)
            .and_then(ColumnName::parse)
            .map_err(|cause| DescriptionError::Identifier { cause })?;
        if self
            .overlay
            .columns
            .insert((table.clone(), column.clone()), prose.to_owned())
            .is_some()
        {
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
        on_invalid_description: InvalidDescriptionPolicy,
    ) -> Result<Self, DescriptionError> {
        let mut projection = Projection::new(bounds, case_insensitive, on_invalid_description);
        for table in dictionary.tables() {
            let address_bytes = table.address.to_string().len();
            projection.admit(
                address_bytes.saturating_add(table.description.as_deref().unwrap_or("").len()),
                table.columns.is_empty(),
            )?;
            let address = physical_folded(&table.address, case_insensitive)?;
            let description = table.description.as_deref().unwrap_or("").to_owned();
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

    /// A catalog reference never carries its own `on_invalid_description` - that knob is `Table`
    /// mapping-only - so a row here keeps the one behaviour this crate always had: refuse, once
    /// matched. The deferral below still holds, so an unmatched row's invalid prose is still
    /// ignored exactly as a mapped source's is.
    pub fn from_definitions(definitions: &Definitions, bounds: DictionaryBounds) -> Result<Self, DescriptionError> {
        let mut projection = Projection::new(bounds, false, InvalidDescriptionPolicy::Refuse);
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
                .insert(address.clone(), model.description().to_owned())
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

/// The first nonempty, valid prose across the ordered candidates, applying each candidate's own
/// policy to an invalid row before falling through to the next one. Never returns the offending
/// text, and an `Ok(description)` whose text trims to empty is treated as "no prose" here - the
/// same fallback an absent row already takes - so a blank override still lets the next source show
/// through. `refuse` builds the typed refusal naming only the key; `warn` logs that same key under
/// `omit`.
fn resolve_prose<'a>(
    candidates: impl Iterator<Item = (&'a str, InvalidDescriptionPolicy)>,
    refuse: impl Fn(sutura_domain::catalog::InvalidDescription) -> DescriptionError,
    warn: impl Fn(&sutura_domain::catalog::InvalidDescription),
) -> Result<Option<String>, DescriptionError> {
    for (raw, policy) in candidates {
        match Description::parse(raw) {
            Ok(description) if description.is_empty() => {}
            Ok(description) => return Ok(Some(description.as_str().to_owned())),
            Err(cause) => match policy {
                InvalidDescriptionPolicy::Refuse => return Err(refuse(cause)),
                InvalidDescriptionPolicy::Omit => warn(&cause),
            },
        }
    }
    Ok(None)
}

/// [`resolve_prose`] for a table's matched candidates.
fn resolve_table_prose<'a>(
    candidates: impl Iterator<Item = (&'a str, InvalidDescriptionPolicy)>,
    table: &QualifiedTable,
) -> Result<Option<String>, DescriptionError> {
    resolve_prose(
        candidates,
        |cause| DescriptionError::TableProse {
            table: table.clone(),
            cause,
        },
        |cause| {
            tracing::warn!(table = %table, cause = %cause, "omitting description overlay row: table prose is not usable");
        },
    )
}

/// [`resolve_prose`] for a column's matched candidates.
fn resolve_column_prose<'a>(
    candidates: impl Iterator<Item = (&'a str, InvalidDescriptionPolicy)>,
    table: &QualifiedTable,
    column: &ColumnName,
) -> Result<Option<String>, DescriptionError> {
    resolve_prose(
        candidates,
        |cause| DescriptionError::ColumnProse {
            table: table.clone(),
            column: column.clone(),
            cause,
        },
        |cause| {
            tracing::warn!(table = %table, column = %column, cause = %cause, "omitting description overlay row: column prose is not usable");
        },
    )
}

/// Applies only prose after dictionary selection: a source row is parsed and validated only once
/// its key has matched a table or column the selection actually kept. Returns the number of
/// ignored column rows, including empty rows and repeated unknown columns from different ordered
/// sources.
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
        let table_candidates = sources.iter().filter_map(|source| {
            let probe = match (source.case_insensitive, &folded_address) {
                (true, Some(folded)) => folded,
                _ => &address,
            };
            source
                .tables
                .get(probe)
                .map(|raw| (raw.as_str(), source.on_invalid_description))
        });
        if let Some(prose) = resolve_table_prose(table_candidates, &address).map_err(|cause| RdbmsError::Read(Box::new(cause)))? {
            table.description = Some(prose);
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
            let key = (address.clone(), column_key.clone());
            let column_candidates = sources.iter().filter_map(|source| {
                let probe = match (source.case_insensitive, &folded_key) {
                    (true, Some(folded)) => folded,
                    _ => &key,
                };
                source
                    .columns
                    .get(probe)
                    .map(|raw| (raw.as_str(), source.on_invalid_description))
            });
            if let Some(prose) = resolve_column_prose(column_candidates, &address, &column_key)
                .map_err(|cause| RdbmsError::Read(Box::new(cause)))?
            {
                table.column_metadata.entry(column.clone()).or_default().description = Some(prose);
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

    fn from_dictionary(dictionary: &Dictionary, case_insensitive: bool) -> Result<DescriptionOverlay, DescriptionError> {
        DescriptionOverlay::from_dictionary(
            dictionary,
            bounds(100, 10000),
            case_insensitive,
            InvalidDescriptionPolicy::Refuse,
        )
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
            .map(|dictionary| from_dictionary(dictionary, false).expect("bounded prose"))
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
        let overlay = from_dictionary(&docs, false).expect("exact records");
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
        let overlay = from_dictionary(&docs, true).expect("folded records");
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
        let overlay = from_dictionary(&source, false).expect("exact records");
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
        let exact_overlay = from_dictionary(&exact_docs, false).expect("exact records");
        let folded_overlay = from_dictionary(&folded_docs, true).expect("folded records");
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
            from_dictionary(&source, true),
            Err(DescriptionError::DuplicateTable { .. })
        ));
    }

    #[test]
    fn two_case_insensitive_source_columns_differing_only_by_case_are_refused_as_duplicates() {
        let source = dictionary("public", &[("id", "One"), ("ID", "Two")]);
        assert!(matches!(
            from_dictionary(&source, true),
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
        let overlay = from_dictionary(&source, true).expect("folded records");
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
        let overlay = from_dictionary(&source, true).expect("folded records");
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
    fn description_projection_refuses_duplicate_keys_and_caps() {
        let mut repeated = dictionary("public", &[("id", "One")]);
        repeated.tables.extend(dictionary("public", &[("id", "Two")]).tables);
        assert!(matches!(
            from_dictionary(&repeated, false),
            Err(DescriptionError::DuplicateTable { .. })
        ));
        assert!(matches!(
            from_dictionary(&dictionary("public", &[("id", "One"), ("id", "Two")]), false),
            Err(DescriptionError::Duplicate { .. })
        ));
        assert!(matches!(
            DescriptionOverlay::from_dictionary(
                &dictionary("public", &[("id", "Prose"), ("extra", "Prose")]),
                bounds(1, 10000),
                false,
                InvalidDescriptionPolicy::Refuse
            ),
            Err(DescriptionError::Rows { .. })
        ));
        assert!(matches!(
            DescriptionOverlay::from_dictionary(
                &dictionary("public", &[("id", "Prose")]),
                bounds(100, 1),
                false,
                InvalidDescriptionPolicy::Refuse
            ),
            Err(DescriptionError::Bytes { .. })
        ));
    }

    /// Construction never validates prose - a row with a control character must still be
    /// recorded, so matching (and only matching) decides whether it is ever checked.
    #[test]
    fn construction_defers_prose_validation_past_control_characters() {
        let source = dictionary("public", &[("id", "Bad\u{96}prose")]);
        assert!(
            from_dictionary(&source, false).is_ok(),
            "construction must defer prose validation until after key matching"
        );
    }

    /// A row whose key resolves to no selected column is ignored and counted, and its prose is
    /// never checked.
    #[test]
    fn invalid_prose_for_an_unmatched_column_is_ignored_and_never_refuses() {
        let source = dictionary("public", &[("unknown", "Bad\u{96}prose")]);
        let overlay = from_dictionary(&source, false).expect("construction never validates prose");
        let (actual, ignored) = dictionary("public", &[("id", "Native")])
            .overlay_descriptions(&[overlay])
            .expect("an unmatched row's invalid prose must never refuse the load");
        assert_eq!(ignored, 1);
        assert_eq!(
            actual
                .tables()
                .first()
                .and_then(|table| table.column_metadata("id"))
                .and_then(crate::ColumnMetadata::description),
            Some("Native")
        );
    }

    /// Ask 3: under the default `Refuse` policy, a matched row's invalid prose still refuses the
    /// load, but the refusal names the key (schema, table, column), never the text.
    #[test]
    fn invalid_prose_for_a_matched_column_refuses_by_default_naming_the_key_not_the_text() {
        let source = dictionary("public", &[("id", "Bad\u{96}prose")]);
        let overlay = from_dictionary(&source, false).expect("construction never validates prose");
        let error = dictionary("public", &[("id", "Native")])
            .overlay_descriptions(&[overlay])
            .expect_err("a matched row's invalid prose must refuse under the default policy");
        let RdbmsError::Read(cause) = error else {
            panic!("expected RdbmsError::Read, got {error:?}");
        };
        let cause = cause.downcast_ref::<DescriptionError>().expect("DescriptionError");
        match cause {
            DescriptionError::ColumnProse { table, column, .. } => {
                assert_eq!(table.to_string(), "public.orders");
                assert_eq!(column.as_str(), "id");
            }
            other => panic!("expected ColumnProse, got {other:?}"),
        }
        assert!(
            !cause.to_string().contains("Bad") && !format!("{cause:?}").contains("Bad"),
            "the refusal must never carry the offending prose"
        );
    }

    /// Ask 2: `on_invalid_description: Omit` drops a matched row whose prose does not parse,
    /// falling back to the next ordered source's prose for the same key.
    #[test]
    fn omit_policy_drops_an_invalid_row_and_falls_back_to_the_next_source() {
        let first = dictionary("public", &[("id", "Bad\u{96}prose")]);
        let second = dictionary("public", &[("id", "Second id")]);
        let overlays = [
            DescriptionOverlay::from_dictionary(&first, bounds(100, 10000), false, InvalidDescriptionPolicy::Omit)
                .expect("omit never refuses at construction"),
            from_dictionary(&second, false).expect("bounded prose"),
        ];
        let (actual, ignored) = dictionary("public", &[("id", "Native")])
            .overlay_descriptions(&overlays)
            .expect("an omitted row must never refuse the load");
        assert_eq!(ignored, 0);
        assert_eq!(
            actual
                .tables()
                .first()
                .and_then(|table| table.column_metadata("id"))
                .and_then(crate::ColumnMetadata::description),
            Some("Second id")
        );
    }

    /// Ask 2: with no other source left, an omitted row falls back to the dictionary's own native
    /// comment, exactly as an unmatched row already does.
    #[test]
    fn omit_policy_falls_back_to_the_dictionarys_own_native_comment_when_no_source_is_left() {
        let source = dictionary("public", &[("id", "Bad\u{96}prose")]);
        let overlay = DescriptionOverlay::from_dictionary(&source, bounds(100, 10000), false, InvalidDescriptionPolicy::Omit)
            .expect("omit never refuses at construction");
        let (actual, ignored) = dictionary("public", &[("id", "Native")])
            .overlay_descriptions(&[overlay])
            .expect("an omitted row with no other source must fall back to the native comment");
        assert_eq!(ignored, 0);
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

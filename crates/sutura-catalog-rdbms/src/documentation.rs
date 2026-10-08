//! The documentation-schema contract both live readers hold, independent of the driver that
//! streams it: the constructor checks, the inline row and byte caps, and the assembly of rows into
//! [`Table`]s. [`crate::postgres_reader`]'s header carries the column table this module decodes.
//!
//! A reader asks its driver for one row's columns, hands them over as a [`Row`] after
//! [`Assembly::admit`] has counted the row against the declared caps (or [`Assembly::bill`] a whole
//! batch and [`Assembly::count_row`] each of its rows), and gets a [`Dictionary`] back from
//! [`Assembly::finish`]. What a reader still owns is its SQL, its transaction and how
//! it measures a row's size.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::num::NonZeroU64;

use crate::postgres_reader::{DEFAULT_MAX_DICTIONARY_BYTES, DEFAULT_MAX_DICTIONARY_ROWS, InvalidReaderConfig, RowPredicate};
use crate::{ColumnMetadata, Dictionary, DictionaryBounds, RdbmsError, Table, TableAddress};

/// The documentation view within the documentation schema.
pub(crate) const DOCUMENTATION_VIEW: &str = "columns";

fn identifier(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// The checks both readers' constructors make before any SQL exists: the schema and the predicate
/// column are identifiers, so quoting them cannot be a vehicle for SQL, and an absent cap selects
/// its documented default.
pub(crate) fn checked_bounds(
    documentation_schema: &str,
    predicate: &RowPredicate,
    row_cap: Option<NonZeroU64>,
    byte_cap: Option<NonZeroU64>,
) -> Result<DictionaryBounds, InvalidReaderConfig> {
    if !identifier(documentation_schema) {
        return Err(InvalidReaderConfig::Schema);
    }
    let predicate_column = match predicate {
        RowPredicate::None => None,
        RowPredicate::IsNull(column) | RowPredicate::IsNotNull(column) | RowPredicate::Equals { column, .. } => {
            Some(column.as_str())
        }
    };
    if predicate_column.is_some_and(|column| !identifier(column)) {
        return Err(InvalidReaderConfig::PredicateColumn);
    }
    Ok(DictionaryBounds::new(
        row_cap
            .or_else(|| NonZeroU64::new(DEFAULT_MAX_DICTIONARY_ROWS))
            .ok_or(InvalidReaderConfig::DefaultBound)?,
        byte_cap
            .or_else(|| NonZeroU64::new(DEFAULT_MAX_DICTIONARY_BYTES))
            .ok_or(InvalidReaderConfig::DefaultBound)?,
    ))
}

/// One documentation row as the driver decoded it, before the contract is checked.
#[derive(Debug, Default)]
pub(crate) struct Row {
    pub(crate) environment: Option<String>,
    pub(crate) catalog_name: Option<String>,
    pub(crate) schema_name: Option<String>,
    pub(crate) table_name: Option<String>,
    pub(crate) model_name: Option<String>,
    pub(crate) table_description: Option<String>,
    pub(crate) column_name: Option<String>,
    pub(crate) column_type: Option<String>,
    pub(crate) column_description: Option<String>,
    pub(crate) is_primary_key: Option<bool>,
}

impl Row {
    /// The decoded text's UTF-8 length plus one byte for the key flag - never zero, so every row
    /// spends from the byte cap.
    pub(crate) fn decoded_len(&self) -> u64 {
        let text = [
            &self.environment,
            &self.catalog_name,
            &self.schema_name,
            &self.table_name,
            &self.model_name,
            &self.table_description,
            &self.column_name,
            &self.column_type,
            &self.column_description,
        ];
        let bytes: usize = text.iter().map(|cell| cell.as_deref().map_or(0, str::len)).sum();
        u64::try_from(bytes).unwrap_or(u64::MAX).saturating_add(1)
    }
}

type PhysicalKey = (Option<String>, String, String);

/// The rows of one read, gathered into tables under the declared bounds.
pub(crate) struct Assembly<'env> {
    environment: &'env str,
    bounds: DictionaryBounds,
    rows_read: u64,
    bytes_remaining: u64,
    tables: BTreeMap<PhysicalKey, TableAccumulator>,
}

impl<'env> Assembly<'env> {
    pub(crate) const fn new(environment: &'env str, bounds: DictionaryBounds) -> Self {
        Self {
            environment,
            bounds,
            rows_read: 0,
            bytes_remaining: bounds.max_bytes().get(),
            tables: BTreeMap::new(),
        }
    }

    /// Counts one row of `size` bytes against the declared caps, refusing the row that crosses
    /// either, so the caller abandons its stream there rather than after it.
    pub(crate) fn admit(&mut self, size: u64) -> Result<(), RdbmsError> {
        self.count_row()?;
        self.bill(size)
    }

    /// Counts one row against the row cap, refusing the row that crosses it.
    pub(crate) fn count_row(&mut self) -> Result<(), RdbmsError> {
        self.rows_read = self.rows_read.saturating_add(1);
        if self.rows_read > self.bounds.max_rows().get() {
            return Err(RdbmsError::Read(Box::new(CapExceeded::Rows {
                limit: self.bounds.max_rows().get(),
            })));
        }
        Ok(())
    }

    /// Spends `size` bytes from the byte cap, refusing what crosses it - a row, or a whole batch
    /// for a reader whose driver hands rows over in batches.
    pub(crate) fn bill(&mut self, size: u64) -> Result<(), RdbmsError> {
        if self.bytes_remaining < size {
            return Err(RdbmsError::Read(Box::new(CapExceeded::Bytes {
                limit: self.bounds.max_bytes().get(),
            })));
        }
        self.bytes_remaining -= size;
        Ok(())
    }

    /// Checks one admitted row against the view's contract and adds it to its table.
    pub(crate) fn push(&mut self, row: Row) -> Result<(), RdbmsError> {
        let decoded = self.decode(row)?;
        let key = (
            decoded.address.catalog().map(str::to_owned),
            decoded.address.schema().to_owned(),
            decoded.address.table().to_owned(),
        );
        match self.tables.entry(key) {
            Entry::Occupied(mut slot) => {
                if slot.get().model != decoded.model || slot.get().description != decoded.description {
                    return Err(RdbmsError::Read(Box::new(ContractViolation::ConflictingTable)));
                }
                slot.get_mut().push(decoded)
            }
            Entry::Vacant(slot) => {
                slot.insert(TableAccumulator::new(decoded));
                Ok(())
            }
        }
    }

    /// No foreign keys in this first slice: the documentation schema carries none, and neither
    /// reader invents a relationship or a target-uniqueness claim.
    pub(crate) fn finish(self) -> Dictionary {
        Dictionary::new(
            self.tables.into_values().map(TableAccumulator::into_table).collect(),
            Vec::new(),
        )
    }

    fn decode(&self, row: Row) -> Result<DecodedColumn, RdbmsError> {
        // The SQL already filters on the environment; a row still arriving with a different value
        // violates the view's contract and is refused rather than read past.
        if row.environment.as_deref() != Some(self.environment) {
            return Err(RdbmsError::Read(Box::new(ContractViolation::Environment)));
        }
        let schema = row.schema_name.unwrap_or_default();
        let table = row.table_name.unwrap_or_default();
        if schema.is_empty() || table.is_empty() {
            return Err(RdbmsError::Read(Box::new(ContractViolation::MissingTableIdentity)));
        }
        let model = row.model_name.unwrap_or_else(|| table.clone());
        Ok(DecodedColumn {
            address: TableAddress::new(row.catalog_name, schema, table),
            model,
            description: row.table_description,
            column: row.column_name.unwrap_or_default(),
            metadata: ColumnMetadata::new(row.column_type, row.column_description),
            is_primary_key: row
                .is_primary_key
                .ok_or_else(|| RdbmsError::Read(Box::new(ContractViolation::MissingPrimaryKeyEvidence)))?,
        })
    }
}

/// One documentation row, decoded into the parts a table accumulation needs.
struct DecodedColumn {
    address: TableAddress,
    model: String,
    description: Option<String>,
    column: String,
    metadata: ColumnMetadata,
    is_primary_key: bool,
}

/// A physical table gathered from the streamed documentation rows: its address, model name, table
/// description, ordered columns with their metadata, and the primary-key evidence.
struct TableAccumulator {
    address: TableAddress,
    model: String,
    description: Option<String>,
    columns: Vec<String>,
    column_metadata: BTreeMap<String, ColumnMetadata>,
    primary_key: Vec<String>,
}

impl TableAccumulator {
    fn new(decoded: DecodedColumn) -> Self {
        let mut table = Self {
            address: decoded.address,
            model: decoded.model,
            description: decoded.description,
            columns: Vec::new(),
            column_metadata: BTreeMap::new(),
            primary_key: Vec::new(),
        };
        table.add(decoded.column, decoded.metadata, decoded.is_primary_key);
        table
    }

    fn push(&mut self, decoded: DecodedColumn) -> Result<(), RdbmsError> {
        if self.columns.contains(&decoded.column) {
            return Err(RdbmsError::Read(Box::new(ContractViolation::DuplicateColumn {
                column: decoded.column,
            })));
        }
        self.add(decoded.column, decoded.metadata, decoded.is_primary_key);
        Ok(())
    }

    fn add(&mut self, column: String, metadata: ColumnMetadata, is_primary_key: bool) {
        if metadata.data_type().is_some() || metadata.description().is_some() {
            self.column_metadata.insert(column.clone(), metadata);
        }
        if is_primary_key && !self.primary_key.contains(&column) {
            self.primary_key.push(column.clone());
        }
        self.columns.push(column);
    }

    fn into_table(self) -> Table {
        Table::new(self.model, self.address, self.columns, self.description)
            .with_column_metadata(self.column_metadata)
            .with_primary_key(self.primary_key)
    }
}

/// A row cap or byte cap was crossed. A row-by-row reader abandons its cursor at the crossing row.
/// A batch reader refuses a batch that crosses the byte cap before it decodes any row of it, and
/// the row that crosses the row cap while it decodes one.
#[derive(Debug, thiserror::Error)]
enum CapExceeded {
    #[error("the dictionary stream reached the declared maximum of {limit} rows")]
    Rows { limit: u64 },
    #[error("the dictionary stream reached the declared maximum of {limit} bytes")]
    Bytes { limit: u64 },
}

/// The documentation view violated its declared contract - a row a reader can only refuse, not heal.
#[derive(Debug, thiserror::Error)]
enum ContractViolation {
    #[error("a documentation row's environment did not match the declared environment")]
    Environment,
    #[error("a documentation row carried no schema and table identity")]
    MissingTableIdentity,
    #[error("documentation rows disagree about one table's model or description")]
    ConflictingTable,
    #[error("a documentation row carried no primary-key evidence")]
    MissingPrimaryKeyEvidence,
    #[error("a documentation table repeated column {column}")]
    DuplicateColumn { column: String },
}

#[cfg(test)]
mod tests;

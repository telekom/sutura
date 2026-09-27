//! The live Postgres documentation-schema reader, behind the default-off `live` feature.
//!
//! This is the reader the crate's module header has said, since #151, "a real implementor" would
//! be: one that speaks to a Postgres socket, reads a documentation schema, decodes into
//! [`crate::Dictionary`], and maps its own failures into [`crate::RdbmsError::Read`]. It is the
//! companion to [`crate::fixture::FixtureReader`] - the fake is the recorded corpus this port was
//! tested against, and this is the live implementor over a real connection.
//!
//! # The documentation-schema contract
//!
//! A **documented fixed schema** - one column per documented dictionary row, so the reader never
//! guesses at structure, named `columns` inside the declared documentation schema:
//!
//! | Column | Meaning |
//! | --- | --- |
//! | `environment` | The deployment environment key this row's descriptions apply to. |
//! | `catalog_name` | The physical catalog above the schema, when generated statements need one. |
//! | `schema_name` | The physical schema the described table lives in. |
//! | `table_name` | The physical table. |
//! | `model_name` | The semantic model name to bind the table to. |
//! | `table_description` | Authored table prose; `NULL` for none. |
//! | `column_name` | The physical column. |
//! | `column_ordinal` | The column's stable order within the table. |
//! | `column_type` | The physical data type, as `information_schema` reports it. |
//! | `column_description` | Authored column prose; `NULL` for none. |
//! | `is_primary_key` | A `boolean`: is this column the sole column of a primary or unique key. |
//! | `is_deleted` | A soft-delete marker: `false` selects live rows on every read. |
//!
//! The reader selects `is_deleted = false` unconditionally and binds the declared `environment` and
//! the equals-predicate value as SQL parameters - **never interpolating configuration values into
//! the statement**. The schema and predicate column are checked at the reader's constructor,
//! then quoted as identifiers, so neither can be the vehicle for SQL. The predicate
//! operator comes from a closed set rendered as fixed text.
//!
//! # Foreign keys are unsupported in this first slice - and the reader does not pretend otherwise
//!
//! The documented schema carries no foreign-key columns, so this reader emits a
//! [`crate::Dictionary`] with **no [`crate::Relationship`]s**. That is a real, explicit limit,
//! stated here. It is not an invented uniqueness assertion: single-column primary-key evidence is
//! read per column (`is_primary_key`) and that alone is ever emitted; nothing in this reader
//! fabricates a foreign key or a target-uniqueness claim on the reader's behalf.
//!
//! # Read-only, streamed, bounded
//!
//! The read runs inside a single read-only transaction (`read_only`, `RepeatableRead`). Rows are
//! streamed with `query_raw` - the driver's extended-protocol portal, which does not materialise
//! the result set up front - and the row cap and byte cap are enforced **inline**, abandoning the
//! stream the moment the declared ceiling is crossed. This bounds the streamed row payload;
//! the converter's separate post-decode guard bounds the assembled dictionary. The driver still
//! materialises one row before its size is checked. Neither bound limits elapsed read time.
//!
//! # Connection and transport policy
//!
//! The reader uses the catalog's own declared connection and transport policy. Anchor/identity
//! material is resolved by a composition root into a `rustls::ClientConfig` for `verified`/`mutual`
//! channels, or `None` for `plaintext`. When a `ClientConfig` is supplied the reader forces
//! `SslMode::Require` so a server declining TLS cannot silently downgrade the verifier to
//! cleartext - the same hardening `sutura-exec-postgres::connect_secured` applies. There is no
//! unconditional `NoTls`: plaintext is reached only through the declared `plaintext` mode, which
//! the transport layer already refuses for a remote host.
//!
//! # Feature gating
//!
//! This module is `#[cfg(feature = "live")]`. A build without the feature links no
//! `tokio-postgres`/`tokio-postgres-rustls`/`rustls`/`ring` stack, and the composition root refuses
//! the catalog by name.
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::num::NonZeroU64;

use tokio_postgres::config::SslMode;
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, NoTls, Row};

use futures_util::StreamExt as _;

use crate::{ColumnMetadata, Dictionary, DictionaryBounds, DictionaryReader, RdbmsError, Table, TableAddress};

/// The documented default documentation schema, used when no `dictionary_schema` is configured.
pub const DEFAULT_DOCUMENTATION_SCHEMA: &str = "sutura_dictionary";

/// The documented default row cap on one dictionary read.
pub const DEFAULT_MAX_DICTIONARY_ROWS: u64 = 10_000;

/// The documented default byte cap on one dictionary read.
pub const DEFAULT_MAX_DICTIONARY_BYTES: u64 = 8 * 1024 * 1024;

/// The documentation view within the documentation schema.
const DOCUMENTATION_VIEW: &str = "columns";

/// The soft-delete filter: only live rows are ever read.
const SOFT_DELETE_SQL: &str = "is_deleted = false";

type OpenConnection = (tokio::runtime::Runtime, Client);
type PhysicalKey = (Option<String>, String, String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidReaderConfig {
    #[error("the dictionary schema is not a nonempty SQL identifier")]
    Schema,
    #[error("the live-row predicate column is not a nonempty SQL identifier")]
    PredicateColumn,
    #[error("a default dictionary bound is zero")]
    DefaultBound,
}

fn identifier(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// A live-row predicate rendered into the dictionary query and bound as parameters.
///
/// [`PostgresReader::new`] validates the column identifier before it can reach SQL text. The
/// operator is from a fixed set rendered as fixed text; an `equals` value is bound as a parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowPredicate {
    /// No live-row filter beyond the soft-delete marker.
    None,
    /// `column IS NULL`.
    IsNull(String),
    /// `column IS NOT NULL`.
    IsNotNull(String),
    /// `column = $N`, value bound as a parameter.
    Equals { column: String, value: String },
}

/// A live [`crate::DictionaryReader`] over a Postgres documentation schema.
///
/// Owns the driver configuration and the optional TLS verifier, plus the environment key, the
/// optional live-row predicate and the read bounds. The TLS `ClientConfig` is supplied by a
/// composition root that resolved the declared `transport_mode`; `None` selects the `plaintext`
/// channel. The read-only transaction, parameter binding and inline caps are all this reader's own.
#[derive(Debug, Clone)]
pub struct PostgresReader {
    config: tokio_postgres::Config,
    tls: Option<rustls::ClientConfig>,
    documentation_schema: String,
    environment: String,
    predicate: RowPredicate,
    bounds: DictionaryBounds,
}

impl PostgresReader {
    /// Builds the reader. `tls` is `Some(rustls::ClientConfig)` for a `verified`/`mutual` channel
    /// and `None` for a declared `plaintext` one. `documentation_schema` and `environment` are
    /// validated identifiers supplied by the composition root. An absent `row_cap`/`byte_cap`
    /// selects the reader's own documented defaults.
    pub fn new(
        config: tokio_postgres::Config,
        tls: Option<rustls::ClientConfig>,
        documentation_schema: String,
        environment: String,
        predicate: RowPredicate,
        row_cap: Option<NonZeroU64>,
        byte_cap: Option<NonZeroU64>,
    ) -> Result<Self, InvalidReaderConfig> {
        if !identifier(&documentation_schema) {
            return Err(InvalidReaderConfig::Schema);
        }
        let predicate_column = match &predicate {
            RowPredicate::None => None,
            RowPredicate::IsNull(column) | RowPredicate::IsNotNull(column) | RowPredicate::Equals { column, .. } => {
                Some(column.as_str())
            }
        };
        if predicate_column.is_some_and(|column| !identifier(column)) {
            return Err(InvalidReaderConfig::PredicateColumn);
        }
        let bounds = DictionaryBounds::new(
            row_cap
                .or_else(|| NonZeroU64::new(DEFAULT_MAX_DICTIONARY_ROWS))
                .ok_or(InvalidReaderConfig::DefaultBound)?,
            byte_cap
                .or_else(|| NonZeroU64::new(DEFAULT_MAX_DICTIONARY_BYTES))
                .ok_or(InvalidReaderConfig::DefaultBound)?,
        );
        Ok(Self {
            config,
            tls,
            documentation_schema,
            environment,
            predicate,
            bounds,
        })
    }

    #[must_use]
    pub const fn bounds(&self) -> DictionaryBounds {
        self.bounds
    }

    fn connect(&self) -> Result<OpenConnection, RdbmsError> {
        let mut config = self.config.clone();
        if self.tls.is_some() {
            // A supplied verifier means the caller declared TLS - require the handshake so a server
            // declining TLS cannot silently downgrade the channel to cleartext.
            config.ssl_mode(SslMode::Require);
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|cause| RdbmsError::Read(Box::new(cause)))?;
        let client = if let Some(client_config) = &self.tls {
            let connector = tokio_postgres_rustls::MakeRustlsConnect::new(client_config.clone());
            let (client, connection) = runtime
                .block_on(config.connect(connector))
                .map_err(|cause| RdbmsError::Read(Box::new(cause)))?;
            #[expect(
                clippy::let_underscore_must_use,
                clippy::let_underscore_untyped,
                reason = "the connection driver task's own error has no caller this read can route to"
            )]
            runtime.spawn(async move {
                let _ = connection.await;
            });
            client
        } else {
            let (client, connection) = runtime
                .block_on(config.connect(NoTls))
                .map_err(|cause| RdbmsError::Read(Box::new(cause)))?;
            #[expect(
                clippy::let_underscore_must_use,
                clippy::let_underscore_untyped,
                reason = "the connection driver task's own error has no caller this read can route to"
            )]
            runtime.spawn(async move {
                let _ = connection.await;
            });
            client
        };
        Ok((runtime, client))
    }
    /// Streams the documentation rows and assembles [`crate::Table`]s, enforcing the row and byte
    /// caps inline. The schema name and predicate column are validated identifiers quoted as such;
    /// the environment and any equals value are bound parameters.
    async fn stream_columns(&self, transaction: &tokio_postgres::Transaction<'_>) -> Result<Vec<Table>, RdbmsError> {
        let mut params: Vec<&(dyn ToSql + Sync)> = vec![&self.environment];
        let predicate_sql = match &self.predicate {
            RowPredicate::None => String::new(),
            RowPredicate::IsNull(column) => format!(" AND \"{column}\" IS NULL"),
            RowPredicate::IsNotNull(column) => format!(" AND \"{column}\" IS NOT NULL"),
            RowPredicate::Equals { column, value } => {
                params.push(value);
                format!(" AND \"{column}\" = $2")
            }
        };

        let statement = format!(
            "SELECT environment, catalog_name, schema_name, table_name, model_name, table_description, \
                    column_name, column_type, column_description, is_primary_key \
             FROM \"{}\".\"{}\" \
             WHERE {SOFT_DELETE_SQL} AND environment = $1{predicate_sql} \
             ORDER BY catalog_name, schema_name, table_name, column_ordinal",
            self.documentation_schema, DOCUMENTATION_VIEW,
        );

        let stream = transaction.query_raw(&statement, params).await.map_err(read_err)?;

        futures_util::pin_mut!(stream);
        let mut tables: BTreeMap<PhysicalKey, TableAccumulator> = BTreeMap::new();
        let mut rows_read = 0u64;
        let mut bytes_remaining = self.bounds.max_bytes().get();

        while let Some(row) = stream.next().await {
            let row = row.map_err(read_err)?;
            rows_read = rows_read.saturating_add(1);
            if rows_read > self.bounds.max_rows().get() {
                return Err(RdbmsError::Read(Box::new(CapExceeded::Rows {
                    limit: self.bounds.max_rows().get(),
                })));
            }
            let row_bytes = u64::try_from(row.raw_size_bytes().max(1)).unwrap_or(u64::MAX);
            if bytes_remaining < row_bytes {
                return Err(RdbmsError::Read(Box::new(CapExceeded::Bytes {
                    limit: self.bounds.max_bytes().get(),
                })));
            }
            bytes_remaining -= row_bytes;

            let decoded = self.decode_row(&row)?;
            let key = (
                decoded.address.catalog().map(str::to_owned),
                decoded.address.schema().to_owned(),
                decoded.address.table().to_owned(),
            );
            match tables.entry(key) {
                Entry::Occupied(mut slot) => {
                    if slot.get().model != decoded.model || slot.get().description != decoded.description {
                        return Err(RdbmsError::Read(Box::new(ContractViolation::ConflictingTable)));
                    }
                    slot.get_mut().push(decoded)?;
                }
                Entry::Vacant(slot) => {
                    slot.insert(TableAccumulator::new(decoded));
                }
            }
        }

        Ok(tables.into_values().map(TableAccumulator::into_table).collect())
    }

    /// Decodes one documentation row into the parts a table accumulation needs.
    fn decode_row(&self, row: &Row) -> Result<DecodedColumn, RdbmsError> {
        let environment: Option<String> = row.try_get("environment").map_err(read_err)?;
        let catalog_name: Option<String> = row.try_get("catalog_name").map_err(read_err)?;
        let schema_name: Option<String> = row.try_get("schema_name").map_err(read_err)?;
        let table_name: Option<String> = row.try_get("table_name").map_err(read_err)?;
        let model_name: Option<String> = row.try_get("model_name").map_err(read_err)?;
        let table_description: Option<String> = row.try_get("table_description").map_err(read_err)?;
        let column_name: Option<String> = row.try_get("column_name").map_err(read_err)?;
        let column_type: Option<String> = row.try_get("column_type").map_err(read_err)?;
        let column_description: Option<String> = row.try_get("column_description").map_err(read_err)?;
        let is_primary_key: Option<bool> = row.try_get("is_primary_key").map_err(read_err)?;

        // The SQL already filters `environment = $1`; a row still arriving with a different value
        // violates the view's contract and is refused rather than read past.
        if environment.as_deref() != Some(self.environment.as_str()) {
            return Err(RdbmsError::Read(Box::new(ContractViolation::Environment)));
        }
        let schema = schema_name.unwrap_or_default();
        let table = table_name.unwrap_or_default();
        if schema.is_empty() || table.is_empty() {
            return Err(RdbmsError::Read(Box::new(ContractViolation::MissingTableIdentity)));
        }
        let model = model_name.unwrap_or_else(|| table.clone());
        let address = TableAddress::new(catalog_name, schema, table);
        let column = column_name.unwrap_or_default();
        let metadata = ColumnMetadata::new(column_type, column_description);
        Ok(DecodedColumn {
            address,
            model,
            description: table_description,
            column,
            metadata,
            is_primary_key: is_primary_key
                .ok_or_else(|| RdbmsError::Read(Box::new(ContractViolation::MissingPrimaryKeyEvidence)))?,
        })
    }
}

impl DictionaryReader for PostgresReader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        let (runtime, client) = self.connect()?;
        runtime
            .block_on(async {
                let mut client = client;
                let transaction = client
                    .build_transaction()
                    .read_only(true)
                    .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
                    .deferrable(false)
                    .start()
                    .await
                    .map_err(read_err)?;
                let tables = self.stream_columns(&transaction).await?;
                transaction.commit().await.map_err(read_err)?;
                Ok(tables)
            })
            .map(|tables| {
                // No foreign keys in this first slice: the documentation schema carries none, and this
                // reader never invents a relationship or a target-uniqueness claim.
                Dictionary::new(tables, Vec::new())
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
        let mut columns = Vec::new();
        let mut column_metadata = BTreeMap::new();
        let mut primary_key = Vec::new();
        Self::push_into(
            decoded.column,
            decoded.metadata,
            decoded.is_primary_key,
            &mut columns,
            &mut column_metadata,
            &mut primary_key,
        );
        Self {
            address: decoded.address,
            model: decoded.model,
            description: decoded.description,
            columns,
            column_metadata,
            primary_key,
        }
    }

    fn push(&mut self, decoded: DecodedColumn) -> Result<(), RdbmsError> {
        if self.columns.contains(&decoded.column) {
            return Err(RdbmsError::Read(Box::new(ContractViolation::DuplicateColumn {
                column: decoded.column,
            })));
        }
        Self::push_into(
            decoded.column,
            decoded.metadata,
            decoded.is_primary_key,
            &mut self.columns,
            &mut self.column_metadata,
            &mut self.primary_key,
        );
        Ok(())
    }

    fn push_into(
        column: String,
        metadata: ColumnMetadata,
        is_primary_key: bool,
        columns: &mut Vec<String>,
        column_metadata: &mut BTreeMap<String, ColumnMetadata>,
        primary_key: &mut Vec<String>,
    ) {
        columns.push(column.clone());
        if metadata.data_type().is_some() || metadata.description().is_some() {
            column_metadata.insert(column.clone(), metadata);
        }
        if is_primary_key && !primary_key.contains(&column) {
            primary_key.push(column);
        }
    }

    fn into_table(self) -> Table {
        Table::new(self.model, self.address, self.columns, self.description)
            .with_column_metadata(self.column_metadata)
            .with_primary_key(self.primary_key)
    }
}

fn read_err(cause: tokio_postgres::Error) -> RdbmsError {
    RdbmsError::Read(Box::new(cause))
}

/// A row cap or byte cap was met mid-stream. The stream was abandoned, so the reader holds against
/// the declared ceiling rather than against a post-decode row count.
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
mod tests {
    use super::{InvalidReaderConfig, PostgresReader, RowPredicate};

    #[test]
    fn the_reader_rejects_untrusted_sql_identifiers_before_opening_a_connection() {
        let config = tokio_postgres::Config::new();
        let schema = PostgresReader::new(
            config.clone(),
            None,
            String::from("dictionary\"; DROP SCHEMA public; --"),
            String::from("test"),
            RowPredicate::None,
            None,
            None,
        );
        assert!(matches!(schema, Err(InvalidReaderConfig::Schema)));

        let predicate = PostgresReader::new(
            config,
            None,
            String::from("dictionary"),
            String::from("test"),
            RowPredicate::Equals {
                column: String::from("state\" OR true --"),
                value: String::from("live"),
            },
            None,
            None,
        );
        assert!(matches!(predicate, Err(InvalidReaderConfig::PredicateColumn)));
    }
}

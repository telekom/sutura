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
//! cleartext - the libpq `sslmode=verify-full` the Postgres data source's `Conninfo` writes holds
//! the same line. There is no unconditional `NoTls`: plaintext is reached only through the
//! declared `plaintext` mode, which the transport layer already refuses for a remote host.
//!
//! # Feature gating
//!
//! This module is `#[cfg(feature = "live")]`. A build without the feature links no
//! `tokio-postgres`/`tokio-postgres-rustls`/`rustls`/`ring` stack, and the composition root refuses
//! the catalog by name.
use std::num::NonZeroU64;

use tokio_postgres::config::SslMode;
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, NoTls, Row};

use futures_util::StreamExt as _;

use crate::documentation::{Assembly, DOCUMENTATION_VIEW, Row as DocumentationRow, checked_bounds};
use crate::{Dictionary, DictionaryBounds, DictionaryReader, RdbmsError};

/// The documented default documentation schema, used when no `dictionary_schema` is configured.
pub const DEFAULT_DOCUMENTATION_SCHEMA: &str = "sutura_dictionary";

/// The documented default row cap on one dictionary read.
pub const DEFAULT_MAX_DICTIONARY_ROWS: u64 = 10_000;

/// The documented default byte cap on one dictionary read.
pub const DEFAULT_MAX_DICTIONARY_BYTES: u64 = 8 * 1024 * 1024;

/// The soft-delete filter: only live rows are ever read.
const SOFT_DELETE_SQL: &str = "is_deleted = false";

type OpenConnection = (tokio::runtime::Runtime, Client);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidReaderConfig {
    #[error("the dictionary schema is not a nonempty SQL identifier")]
    Schema,
    #[error("the live-row predicate column is not a nonempty SQL identifier")]
    PredicateColumn,
    #[error("a default dictionary bound is zero")]
    DefaultBound,
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
        let bounds = checked_bounds(&documentation_schema, &predicate, row_cap, byte_cap)?;
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
    async fn stream_columns(&self, transaction: &tokio_postgres::Transaction<'_>) -> Result<Dictionary, RdbmsError> {
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
        let mut assembly = Assembly::new(&self.environment, self.bounds);
        while let Some(row) = stream.next().await {
            let row = row.map_err(read_err)?;
            assembly.admit(u64::try_from(row.raw_size_bytes().max(1)).unwrap_or(u64::MAX))?;
            assembly.push(Self::decode_row(&row)?)?;
        }
        Ok(assembly.finish())
    }

    /// Reads one documentation row's columns; the contract is [`Assembly::push`]'s to check.
    fn decode_row(row: &Row) -> Result<DocumentationRow, RdbmsError> {
        Ok(DocumentationRow {
            environment: row.try_get("environment").map_err(read_err)?,
            catalog_name: row.try_get("catalog_name").map_err(read_err)?,
            schema_name: row.try_get("schema_name").map_err(read_err)?,
            table_name: row.try_get("table_name").map_err(read_err)?,
            model_name: row.try_get("model_name").map_err(read_err)?,
            table_description: row.try_get("table_description").map_err(read_err)?,
            column_name: row.try_get("column_name").map_err(read_err)?,
            column_type: row.try_get("column_type").map_err(read_err)?,
            column_description: row.try_get("column_description").map_err(read_err)?,
            is_primary_key: row.try_get("is_primary_key").map_err(read_err)?,
        })
    }
}

impl DictionaryReader for PostgresReader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        let (runtime, client) = self.connect()?;
        runtime.block_on(async {
            let mut client = client;
            let transaction = client
                .build_transaction()
                .read_only(true)
                .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
                .deferrable(false)
                .start()
                .await
                .map_err(read_err)?;
            let dictionary = self.stream_columns(&transaction).await?;
            transaction.commit().await.map_err(read_err)?;
            Ok(dictionary)
        })
    }
}

fn read_err(cause: tokio_postgres::Error) -> RdbmsError {
    RdbmsError::Read(Box::new(cause))
}

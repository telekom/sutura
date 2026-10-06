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
//! # Read-only, bounded
//!
//! One read is one connection and one transaction: autocommit off (the driver issues `BEGIN` before
//! the first statement), [`TRANSACTION_MODE`] as that first statement, the select, and a rollback
//! whatever happened - nothing this reader sends commits. The select asks for one row past the row
//! cap (`LIMIT`), so the server never sends more rows than the cap can refuse, and each Arrow batch
//! is billed WHOLE against the byte cap (`RecordBatch::get_array_memory_size`) before its rows are
//! counted against the row cap and decoded. The converter's separate post-decode guard bounds the
//! assembled dictionary.
//!
//! **The limits.** With bound parameters the pinned driver (`apache-arrow-adbc-24`) executes through
//! `PQexecPrepared` and hands the whole result back as one batch (`bind_stream.h`,
//! `result_reader.cc`), so the byte cap refuses a result libpq already holds: what bounds the read
//! itself is the `LIMIT`, in rows and not in bytes. Neither bound limits elapsed read time. That the
//! driver's `BEGIN` precedes [`TRANSACTION_MODE`] is read off its source; `tests/provisioned.rs`
//! observes the effect - a documentation view that writes is refused `25006`.
//!
//! # Connection and transport policy
//!
//! The reader dials the [`Conninfo`] a composition root built from the catalog's own declared
//! connection, through the shared connector `sutura_adbc_postgres` - the one `sutura-exec-postgres`
//! dials a source through, so both refuse the same declarations and libpq is told the same posture:
//! `sslmode=verify-full` for a `verified` or `mutual` channel, `disable` only for a declared
//! `plaintext` one, which the settings refuse for a remote host. A connection opens per read.
//!
//! # Feature gating
//!
//! This module is `#[cfg(feature = "live")]`. A build without the feature links no ADBC stack, and
//! the composition root refuses the catalog by name.
use std::num::NonZeroU64;

use adbc_core::options::{OptionConnection, OptionValue};
use adbc_core::{Connection, Statement as _};
use arrow_array::cast::AsArray as _;
use arrow_array::{Array as _, RecordBatch, StringArray};
use sutura_adbc_postgres::{AdbcError, Conninfo, PostgresDriver};
use sutura_domain::warehouse::ParamValue;

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

/// The first statement of every read's transaction - a fixed literal, sent through the simple
/// protocol (`clippy.toml`'s `execute_update` row).
const TRANSACTION_MODE: &str = "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY";

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

/// A documentation-view column that did not arrive as the type the contract names - a driver or
/// view defect, since the statement names every column it reads.
#[derive(Debug, thiserror::Error)]
#[error("the documentation view's `{column}` did not arrive as {expected}")]
struct UnexpectedColumn {
    column: &'static str,
    expected: &'static str,
}

/// A live [`crate::DictionaryReader`] over a Postgres documentation schema.
///
/// Owns the driver and the connection string a composition root built from the declared channel,
/// plus the environment key, the optional live-row predicate and the read bounds. The read-only
/// transaction, parameter binding and caps are all this reader's own.
///
/// Not `Clone`: the connection string is a secret, and the one long-lived holder - the serve
/// refresh loop - takes the opened catalog by value.
#[derive(Debug)]
pub struct PostgresReader {
    driver: PostgresDriver,
    conninfo: Conninfo,
    documentation_schema: String,
    environment: String,
    predicate: RowPredicate,
    bounds: DictionaryBounds,
}

impl PostgresReader {
    /// Builds the reader over `driver` and `conninfo`, which already carries the declared channel.
    /// `documentation_schema` and `environment` are validated identifiers supplied by the
    /// composition root. An absent `row_cap`/`byte_cap` selects the reader's own documented
    /// defaults.
    pub fn new(
        driver: PostgresDriver,
        conninfo: Conninfo,
        documentation_schema: String,
        environment: String,
        predicate: RowPredicate,
        row_cap: Option<NonZeroU64>,
        byte_cap: Option<NonZeroU64>,
    ) -> Result<Self, InvalidReaderConfig> {
        let bounds = checked_bounds(&documentation_schema, &predicate, row_cap, byte_cap)?;
        Ok(Self {
            driver,
            conninfo,
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

    /// Selects the documentation rows and assembles [`crate::Table`]s under the caps. The schema
    /// name and predicate column are validated identifiers quoted as such; the environment and any
    /// equals value are bound parameters, and the `LIMIT` is the row cap's own number.
    fn select<C>(&self, connection: &mut C) -> Result<Dictionary, RdbmsError>
    where
        C: Connection,
    {
        let mut params = vec![ParamValue::Text(self.environment.clone())];
        let predicate_sql = match &self.predicate {
            RowPredicate::None => String::new(),
            RowPredicate::IsNull(column) => format!(" AND \"{column}\" IS NULL"),
            RowPredicate::IsNotNull(column) => format!(" AND \"{column}\" IS NOT NULL"),
            RowPredicate::Equals { column, value } => {
                params.push(ParamValue::Text(value.clone()));
                format!(" AND \"{column}\" = $2")
            }
        };

        let statement = format!(
            "SELECT environment, catalog_name, schema_name, table_name, model_name, table_description, \
                    column_name, column_type, column_description, is_primary_key \
             FROM \"{}\".\"{}\" \
             WHERE {SOFT_DELETE_SQL} AND environment = $1{predicate_sql} \
             ORDER BY catalog_name, schema_name, table_name, column_ordinal \
             LIMIT {}",
            self.documentation_schema,
            DOCUMENTATION_VIEW,
            self.bounds.max_rows().get().saturating_add(1),
        );

        let mut select = connection.new_statement().map_err(adbc)?;
        select.set_sql_query(statement).map_err(adbc)?;
        if let Some(bound) = sutura_adbc::parameter_batch(&params).map_err(|cause| read_err(AdbcError::Parameters(cause)))? {
            select.bind(bound).map_err(adbc)?;
        }
        let mut assembly = Assembly::new(&self.environment, self.bounds);
        for batch in select.execute().map_err(adbc)? {
            admit(&mut assembly, &batch.map_err(|cause| read_err(AdbcError::Batch(cause)))?)?;
        }
        Ok(assembly.finish())
    }
}

impl DictionaryReader for PostgresReader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        let mut connection = self.driver.connect(&self.conninfo).map_err(read_err)?;
        read_only(&mut connection, |connection| self.select(connection))
    }
}

/// `step` inside the transaction the driver opens once autocommit is off, put in
/// [`TRANSACTION_MODE`] first, and always rolled back.
fn read_only<C, T>(connection: &mut C, step: impl FnOnce(&mut C) -> Result<T, RdbmsError>) -> Result<T, RdbmsError>
where
    C: Connection,
{
    connection
        .set_option(OptionConnection::AutoCommit, OptionValue::from(false))
        .map_err(adbc)?;
    let outcome = transaction_mode(connection).and_then(|()| step(connection));
    // A failed rollback leaves a transaction the connection's drop discards anyway, so its error is
    // not the caller's.
    drop(connection.rollback());
    outcome
}

fn transaction_mode<C>(connection: &mut C) -> Result<(), RdbmsError>
where
    C: Connection,
{
    let mut statement = connection.new_statement().map_err(adbc)?;
    statement.set_sql_query(TRANSACTION_MODE).map_err(adbc)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "the reader's one execute_update: its text is `TRANSACTION_MODE`, a fixed literal"
    )]
    let set = statement.execute_update();
    set.map(drop).map_err(adbc)
}

/// Bills `batch` whole against the byte cap, then counts and decodes its rows; the row contract is
/// [`Assembly::push`]'s to check.
fn admit(assembly: &mut Assembly<'_>, batch: &RecordBatch) -> Result<(), RdbmsError> {
    assembly.bill(u64::try_from(batch.get_array_memory_size()).unwrap_or(u64::MAX))?;
    let unexpected = |column, expected| read_err(UnexpectedColumn { column, expected });
    let text = |column: &'static str| -> Result<&StringArray, RdbmsError> {
        batch
            .column_by_name(column)
            .and_then(|array| array.as_string_opt::<i32>())
            .ok_or_else(|| unexpected(column, "text"))
    };
    let environment = text("environment")?;
    let catalog_name = text("catalog_name")?;
    let schema_name = text("schema_name")?;
    let table_name = text("table_name")?;
    let model_name = text("model_name")?;
    let table_description = text("table_description")?;
    let column_name = text("column_name")?;
    let column_type = text("column_type")?;
    let column_description = text("column_description")?;
    let is_primary_key = batch
        .column_by_name("is_primary_key")
        .and_then(|array| array.as_boolean_opt())
        .ok_or_else(|| unexpected("is_primary_key", "boolean"))?;
    for row in 0..batch.num_rows() {
        assembly.count_row()?;
        let cell = |array: &StringArray| array.is_valid(row).then(|| array.value(row).to_owned());
        assembly.push(DocumentationRow {
            environment: cell(environment),
            catalog_name: cell(catalog_name),
            schema_name: cell(schema_name),
            table_name: cell(table_name),
            model_name: cell(model_name),
            table_description: cell(table_description),
            column_name: cell(column_name),
            column_type: cell(column_type),
            column_description: cell(column_description),
            is_primary_key: is_primary_key.is_valid(row).then(|| is_primary_key.value(row)),
        })?;
    }
    Ok(())
}

fn adbc(cause: adbc_core::error::Error) -> RdbmsError {
    read_err(AdbcError::Adbc(cause))
}

fn read_err(cause: impl std::error::Error + Send + Sync + 'static) -> RdbmsError {
    RdbmsError::Read(Box::new(cause))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::sync::Arc;

    use arrow_array::{ArrayRef, BooleanArray, RecordBatch, StringArray};

    use crate::DictionaryBounds;
    use crate::documentation::Assembly;

    /// Two documentation rows of one table as the driver delivers them, one with NULL prose.
    fn batch() -> RecordBatch {
        let text = |values: [Option<&str>; 2]| -> ArrayRef { Arc::new(StringArray::from(values.to_vec())) };
        RecordBatch::try_from_iter([
            ("environment", text([Some("test"), Some("test")])),
            ("catalog_name", text([None, None])),
            ("schema_name", text([Some("public"), Some("public")])),
            ("table_name", text([Some("orders"), Some("orders")])),
            ("model_name", text([Some("orders"), Some("orders")])),
            (
                "table_description",
                text([Some("Customer orders."), Some("Customer orders.")]),
            ),
            ("column_name", text([Some("order_id"), Some("amount")])),
            ("column_type", text([Some("bigint"), None])),
            ("column_description", text([None, None])),
            ("is_primary_key", Arc::new(BooleanArray::from(vec![true, false])) as ArrayRef),
        ])
        .expect("a documentation batch builds")
    }

    fn admitted(bytes: NonZeroU64) -> Result<crate::Dictionary, crate::RdbmsError> {
        let mut assembly = Assembly::new("test", DictionaryBounds::new(NonZeroU64::MAX, bytes));
        super::admit(&mut assembly, &batch())?;
        Ok(assembly.finish())
    }

    #[test]
    fn a_batch_is_billed_whole_against_the_byte_cap_before_a_row_is_read() {
        let size = u64::try_from(batch().get_array_memory_size()).expect("a batch size fits a u64");
        let cap = |bytes| NonZeroU64::new(bytes).expect("a batch is more than one byte");
        let dictionary = admitted(cap(size)).expect("a cap of exactly the batch's size admits it");
        let [orders] = dictionary.tables() else {
            panic!("one table: {dictionary:?}")
        };
        assert_eq!(orders.columns(), ["order_id", "amount"]);
        assert_eq!(orders.primary_key(), ["order_id"]);
        assert_eq!(
            admitted(cap(size - 1)).expect_err("one byte short refuses").to_string(),
            format!(
                "reading the dictionary failed: the dictionary stream reached the declared maximum of {} bytes",
                size - 1
            ),
        );
    }
}

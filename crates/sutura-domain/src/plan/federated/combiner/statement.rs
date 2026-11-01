//! The SQL tool's question to the combiner: one caller statement over the tables of the sources
//! `tools.run_sql` lists, planned by the engine, each source's part run through the raw port.
//!
//! A port beside [`super::FederationCombiner::combine`] and not a second trait, because the engine
//! that cuts a statement is the one that joins its legs, and a deployment holds one. The combiner
//! holds no credential and runs no leg itself: it hands each leg's statement to [`StatementLegs`],
//! which the application implements over `Warehouse::execute_raw`, and plans over the rows that came
//! back.

use std::collections::BTreeSet;

use arrow_schema::SchemaRef;

use crate::model::{QualifiedTable, SourceName};
use crate::raw::RawStatement;
use crate::warehouse::RawRows;
use crate::warehouse::schema::TypeDialect;

/// One table a statement may read: a model's table on a listed source, typed by its pinned schema.
#[derive(Debug, Clone)]
pub struct StatementTable {
    source: SourceName,
    table: QualifiedTable,
    dialect: TypeDialect,
    schema: SchemaRef,
}

impl StatementTable {
    /// A table at `source`, read in that source's `dialect`, with the columns `schema` pins.
    #[must_use]
    pub const fn new(source: SourceName, table: QualifiedTable, dialect: TypeDialect, schema: SchemaRef) -> Self {
        Self {
            source,
            table,
            dialect,
            schema,
        }
    }

    /// The source the table lives at.
    #[must_use]
    pub const fn source(&self) -> &SourceName {
        &self.source
    }

    /// The table's path at its source, which is the name a caller writes.
    #[must_use]
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }

    /// The source's dialect, which a leg's statement is written in.
    #[must_use]
    pub const fn dialect(&self) -> TypeDialect {
        self.dialect
    }

    /// The table's pinned columns.
    #[must_use]
    pub const fn schema(&self) -> &SchemaRef {
        &self.schema
    }
}

/// What the SQL tool asks the combiner.
#[derive(Debug, Clone, Copy)]
pub struct StatementQuestion<'a> {
    statement: &'a RawStatement,
    tables: &'a [StatementTable],
    federation_allowed: bool,
    working_set_bytes: u64,
}

impl<'a> StatementQuestion<'a> {
    /// `statement` over `tables`, the local part of its plan under `working_set_bytes`.
    #[must_use]
    pub const fn new(
        statement: &'a RawStatement,
        tables: &'a [StatementTable],
        federation_allowed: bool,
        working_set_bytes: u64,
    ) -> Self {
        Self {
            statement,
            tables,
            federation_allowed,
            working_set_bytes,
        }
    }

    /// The caller's statement.
    #[must_use]
    pub const fn statement(&self) -> &'a RawStatement {
        self.statement
    }

    /// Every table the caller may read.
    #[must_use]
    pub const fn tables(&self) -> &'a [StatementTable] {
        self.tables
    }

    /// `tools.run_sql.federation_allowed`. `false` refuses a plan over more than one source before
    /// any leg runs.
    #[must_use]
    pub const fn federation_allowed(&self) -> bool {
        self.federation_allowed
    }

    /// The working-set ceiling the local part of the plan runs under.
    #[must_use]
    pub const fn working_set_bytes(&self) -> u64 {
        self.working_set_bytes
    }
}

/// Runs the part of a statement one source answers, under the caller, through the raw port.
pub trait StatementLegs {
    /// Why a leg did not answer. The runner's own, and opaque to the combiner.
    type Stop;

    /// Readies every source the plan names, once, before any leg runs: the one mint of the call.
    ///
    /// # Errors
    ///
    /// [`Self::Stop`], and no leg runs.
    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop>;

    /// Runs one leg's statement at its source.
    ///
    /// # Errors
    ///
    /// [`Self::Stop`], which ends the answer.
    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop>;
}

/// Why a statement got no answer from the combiner.
#[derive(Debug)]
pub enum StatementStopped<S> {
    /// A leg did not answer.
    Leg(S),
    /// The plan reads more than one source, and the tool does not federate. No leg ran.
    SpansMoreThanOneSource,
    /// The statement did not plan, was not a query, or a leg's rows did not fit its table. The
    /// engine's words name columns, so they reach the log and never this value.
    NotPlanned,
    /// The local part of the plan outgrew the working set.
    AnswerTooLarge,
}

/// The combiner's answer to a statement: `None` where it plans none.
pub type StatementAnswer<S> = Option<Result<RawRows, StatementStopped<S>>>;

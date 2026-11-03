//! Cells for `execute_leg_partitions` and `read_partition_to_deadline` - both sides of partitioned
//! execution. Driven directly, the way `batch_cells.rs` drives `run_to_deadline`, through fake
//! `adbc_core::Statement`/`Connection` implementors that never open a network connection of their
//! own.
//!
//! **None of this compiles at base.** `execute_leg_partitions`, `read_partition_to_deadline` and
//! `PartitionDescriptor` are this branch's own new names (`adbc/partitions.rs`, declared via
//! `mod partitions;` in `adbc.rs`, which the base tree lacks) - there is no prior Rust-side
//! behaviour to pin a red cell against, so every cell in this file is declared `Claim-Cell:` on the
//! commit that adds it, with its killing mutation at `devco/claim-mutations/<fn>.patch`, rather than
//! left for the ordinary base/head proof to measure.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use adbc_core::error::{Error, Result as AdbcResult, Status};
use adbc_core::options::{OptionConnection, OptionStatement, OptionValue};
use adbc_core::{Connection, Optionable, PartitionedResult, Statement};
use arrow_array::{ArrayRef, Int64Array, RecordBatch, RecordBatchIterator, RecordBatchReader, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use super::{AdbcError, PartitionDescriptor, execute_leg_partitions, read_partition_to_deadline};
use crate::transport::JobDeadline;

/// A port deadline opened at `opened` with `budget` to spend - the same helper `tests.rs` uses.
fn deadline(opened: Instant, budget: Duration) -> Deadline {
    Deadline::opened_at(opened, Budget::parse(budget).expect("a non-zero budget parses"))
}

fn not_asked(what: &str) -> Error {
    Error::with_message_and_status(
        format!("the reading connection was asked to {what}, which `read_partition_to_deadline` does not do"),
        Status::NotImplemented,
    )
}

/// Named because `Result<(SchemaRef, RecordBatch), Error>` is over the `type_complexity`
/// threshold this workspace tightened - the same reason `crate::Mapped` exists.
type Delivers = Result<(SchemaRef, RecordBatch), Error>;

/// A connection whose only real method is `read_partition`, answering with `delivers` or,
/// mimicking a descriptor the pinned driver refused, `Err`.
#[derive(Clone)]
struct Reading {
    delivers: Delivers,
}

/// Stands in for [`Reading::StatementType`]. Never constructed: no cell here calls
/// `new_statement`, so every method would be unreachable rather than exercised.
#[derive(Clone)]
struct Unreachable;

impl Optionable for Unreachable {
    type Option = OptionStatement;

    fn set_option(&mut self, _key: Self::Option, _value: OptionValue) -> AdbcResult<()> {
        Err(not_asked("set a statement option"))
    }

    fn get_option_string(&self, _key: Self::Option) -> AdbcResult<String> {
        Err(not_asked("read a statement string option"))
    }

    fn get_option_bytes(&self, _key: Self::Option) -> AdbcResult<Vec<u8>> {
        Err(not_asked("read a statement bytes option"))
    }

    fn get_option_int(&self, _key: Self::Option) -> AdbcResult<i64> {
        Err(not_asked("read a statement integer option"))
    }

    fn get_option_double(&self, _key: Self::Option) -> AdbcResult<f64> {
        Err(not_asked("read a statement double option"))
    }
}

impl Statement for Unreachable {
    fn bind(&mut self, _batch: RecordBatch) -> AdbcResult<()> {
        Err(not_asked("bind"))
    }

    fn bind_stream(&mut self, _reader: Box<dyn RecordBatchReader + Send>) -> AdbcResult<()> {
        Err(not_asked("bind a stream"))
    }

    fn execute(&mut self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        Err(not_asked("execute"))
    }

    fn execute_update(&mut self) -> AdbcResult<Option<i64>> {
        Err(not_asked("execute an update"))
    }

    fn execute_schema(&mut self) -> AdbcResult<Schema> {
        Err(not_asked("read a result schema"))
    }

    fn execute_partitions(&mut self) -> AdbcResult<adbc_core::PartitionedResult> {
        Err(not_asked("execute partitions"))
    }

    fn get_parameter_schema(&self) -> AdbcResult<Schema> {
        Err(not_asked("read a parameter schema"))
    }

    fn prepare(&mut self) -> AdbcResult<()> {
        Err(not_asked("prepare"))
    }

    fn set_sql_query(&mut self, _query: impl AsRef<str>) -> AdbcResult<()> {
        Err(not_asked("set the SQL query"))
    }

    fn set_substrait_plan(&mut self, _plan: impl AsRef<[u8]>) -> AdbcResult<()> {
        Err(not_asked("set a Substrait plan"))
    }

    fn cancel(&mut self) -> AdbcResult<()> {
        Err(not_asked("cancel"))
    }
}

impl Optionable for Reading {
    type Option = OptionConnection;

    fn set_option(&mut self, _key: Self::Option, _value: OptionValue) -> AdbcResult<()> {
        Err(not_asked("set an option"))
    }

    fn get_option_string(&self, _key: Self::Option) -> AdbcResult<String> {
        Err(not_asked("read a string option"))
    }

    fn get_option_bytes(&self, _key: Self::Option) -> AdbcResult<Vec<u8>> {
        Err(not_asked("read a bytes option"))
    }

    fn get_option_int(&self, _key: Self::Option) -> AdbcResult<i64> {
        Err(not_asked("read an integer option"))
    }

    fn get_option_double(&self, _key: Self::Option) -> AdbcResult<f64> {
        Err(not_asked("read a double option"))
    }
}

/// The `Connection` surface no fake in this file is asked to exercise - every method but `cancel`
/// and `read_partition`, which is the one pair each fake answers differently.
macro_rules! unasked_connection_surface {
    () => {
        fn new_statement(&mut self) -> AdbcResult<Self::StatementType> {
            Err(not_asked("make a new statement"))
        }

        fn get_info(
            &self,
            _codes: Option<std::collections::HashSet<adbc_core::options::InfoCode>>,
        ) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("get info"))
        }

        fn get_objects(
            &self,
            _depth: adbc_core::options::ObjectDepth,
            _catalog: Option<&str>,
            _db_schema: Option<&str>,
            _table_name: Option<&str>,
            _table_type: Option<Vec<&str>>,
            _column_name: Option<&str>,
        ) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("get objects"))
        }

        fn get_table_schema(&self, _catalog: Option<&str>, _db_schema: Option<&str>, _table_name: &str) -> AdbcResult<Schema> {
            Err(not_asked("get a table schema"))
        }

        fn get_table_types(&self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("get table types"))
        }

        fn get_statistic_names(&self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("get statistic names"))
        }

        fn get_statistics(
            &self,
            _catalog: Option<&str>,
            _db_schema: Option<&str>,
            _table_name: Option<&str>,
            _approximate: bool,
        ) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("get statistics"))
        }

        fn commit(&mut self) -> AdbcResult<()> {
            Err(not_asked("commit"))
        }

        fn rollback(&mut self) -> AdbcResult<()> {
            Err(not_asked("roll back"))
        }
    };
}

impl Connection for Reading {
    type StatementType = Unreachable;

    unasked_connection_surface!();

    fn cancel(&mut self) -> AdbcResult<()> {
        Err(not_asked("cancel"))
    }

    fn read_partition(&self, _partition: impl AsRef<[u8]>) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        match &self.delivers {
            Ok((announced, delivered)) => Ok(Box::new(RecordBatchIterator::new(
                core::iter::once(Ok(delivered.clone())),
                Arc::clone(announced),
            ))),
            Err(refused) => Err(refused.clone()),
        }
    }
}

#[test]
fn a_partition_stream_past_the_row_ceiling_is_refused_at_the_crossing_batch() {
    // One batch of `MOST_RESULT_ROWS + 1` narrow rows - cheap to build and, per
    // `execute_to_completion`'s own doc, refused at this one batch rather than after a full drain.
    let schema: SchemaRef = Arc::new(Schema::new(vec![Field::new("value", DataType::Int64, false)]));
    let rows = super::MOST_RESULT_ROWS + 1;
    let array: ArrayRef = Arc::new(Int64Array::from(vec![0_i64; rows]));
    let delivered = RecordBatch::try_new(Arc::clone(&schema), vec![array]).expect("one int64 column is rectangular");
    let conn = Reading {
        delivers: Ok((schema, delivered)),
    };
    let refused = read_partition_to_deadline(
        conn,
        PartitionDescriptor::for_test(vec![1]),
        (),
        JobDeadline::Boot,
        Instant::now(),
    );
    assert!(
        matches!(
            refused,
            Err(AdbcError::Unannounced(sutura_domain::warehouse::UnannouncedBatch::OverBound { most })) if most == super::MOST_RESULT_ROWS
        ),
        "{refused:?}"
    );
}

#[test]
fn a_partition_stream_past_the_byte_budget_is_refused_without_crossing_the_row_ceiling() {
    // One row holding a string past the byte budget - `MOST_RESULT_BYTES` is charged at double the
    // buffer it costs to hold (`materialised`'s own doc), so a value just over half the budget
    // crosses it on its own, with nowhere near `MOST_RESULT_ROWS` rows in the same batch.
    let schema: SchemaRef = Arc::new(Schema::new(vec![Field::new("value", DataType::Utf8, false)]));
    // `clippy::integer_division` is denied here - a right shift is the same halving without it.
    let past_half_the_budget = (super::MOST_RESULT_BYTES >> 1) + 1;
    let huge = "x".repeat(past_half_the_budget);
    let array: ArrayRef = Arc::new(StringArray::from(vec![huge]));
    let delivered = RecordBatch::try_new(Arc::clone(&schema), vec![array]).expect("one utf8 column is rectangular");
    let conn = Reading {
        delivers: Ok((schema, delivered)),
    };
    let refused = read_partition_to_deadline(
        conn,
        PartitionDescriptor::for_test(vec![1]),
        (),
        JobDeadline::Boot,
        Instant::now(),
    );
    assert!(
        matches!(
            refused,
            Err(AdbcError::Unannounced(sutura_domain::warehouse::UnannouncedBatch::OverBudget { most_bytes }))
                if most_bytes == super::MOST_RESULT_BYTES
        ),
        "{refused:?}"
    );
}

#[test]
fn a_descriptor_the_driver_refuses_maps_to_the_same_adbc_error_every_other_driver_refusal_does() {
    // `nix/bigquery-adbc-partitions.patch`'s own `decodePartitionDescriptor` answers an empty or
    // version-mismatched descriptor `StatusInvalidArgument` - decoded entirely inside the driver,
    // never by this crate, so what this cell proves is the MAPPING: whatever `read_partition`
    // refuses lands as `AdbcError::Adbc`, exactly like every other ADBC call failure in this
    // transport, and not as a silently-swallowed or newly-invented variant.
    let conn = Reading {
        delivers: Err(Error::with_message_and_status(
            "unknown or invalid partition descriptor",
            Status::InvalidArguments,
        )),
    };
    let refused = read_partition_to_deadline(
        conn,
        PartitionDescriptor::for_test(Vec::new()),
        (),
        JobDeadline::Boot,
        Instant::now(),
    );
    assert!(matches!(refused, Err(AdbcError::Adbc(_))), "{refused:?}");
}

/// A connection whose `read_partition` never returns on its own, standing in for a Storage Read API
/// stream this process cannot finish waiting on.
///
/// **Mirrors `tests.rs`'s `Hanging` statement, over `Connection` instead of `Statement`.** One flag
/// shared through the `Arc` every clone holds: the only way `read_partition` below ever returns is a
/// `cancel` call reaching the SAME shared state, not a timeout of its own - exactly what
/// `read_partition_to_deadline`'s own doc says a real driver's `ConnectionCancel` would have to do.
#[derive(Clone)]
struct HangingConnection {
    cancelled: Arc<AtomicBool>,
}

impl HangingConnection {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Optionable for HangingConnection {
    type Option = OptionConnection;

    fn set_option(&mut self, _key: Self::Option, _value: OptionValue) -> AdbcResult<()> {
        Err(not_asked("set an option"))
    }

    fn get_option_string(&self, _key: Self::Option) -> AdbcResult<String> {
        Err(not_asked("read a string option"))
    }

    fn get_option_bytes(&self, _key: Self::Option) -> AdbcResult<Vec<u8>> {
        Err(not_asked("read a bytes option"))
    }

    fn get_option_int(&self, _key: Self::Option) -> AdbcResult<i64> {
        Err(not_asked("read an integer option"))
    }

    fn get_option_double(&self, _key: Self::Option) -> AdbcResult<f64> {
        Err(not_asked("read a double option"))
    }
}

impl Connection for HangingConnection {
    type StatementType = Unreachable;

    unasked_connection_surface!();

    fn cancel(&mut self) -> AdbcResult<()> {
        self.cancelled.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn read_partition(&self, _partition: impl AsRef<[u8]>) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        while !self.cancelled.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        Err(not_asked("read a partition past being told to stop"))
    }
}

#[test]
fn a_hung_partition_read_is_cancelled_and_the_call_returns_by_the_deadline() {
    // **The reading side of `telekom/sutura#929`'s deadline finding.** Nothing here ever finishes
    // `read_partition` on its own - the ONLY way `read_partition_to_deadline` below returns at all
    // is the deadline firing and asking this fake to `cancel`, which is what unblocks it.
    let hanging = HangingConnection::new();
    let cancelled = Arc::clone(&hanging.cancelled);
    let opened = Instant::now();
    let port = JobDeadline::Port(deadline(opened, Duration::from_millis(20)));
    let started = Instant::now();
    let result = read_partition_to_deadline(hanging, PartitionDescriptor::for_test(vec![1]), (), port, opened);
    assert!(matches!(result, Err(AdbcError::DeadlineElapsed)), "{result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the call outlived its 20ms deadline by {:?}",
        started.elapsed()
    );
    // The cancel a fired deadline asks for is deliberately ASYNCHRONOUS, the same reason
    // `tests.rs`'s own hanging-statement cell gives - give it a moment to land before reading the
    // flag it sets.
    let latest = Instant::now() + Duration::from_secs(10);
    while !cancelled.load(Ordering::SeqCst) && Instant::now() < latest {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        cancelled.load(Ordering::SeqCst),
        "the deadline elapsing must ask the reading connection to cancel"
    );
}

/// A statement whose `execute_partitions` never returns on its own - the statement-side counterpart
/// to [`HangingConnection`], for [`execute_leg_partitions`]'s own deadline.
#[derive(Clone)]
struct HangingStatement {
    cancelled: Arc<AtomicBool>,
}

impl HangingStatement {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Optionable for HangingStatement {
    type Option = OptionStatement;

    fn set_option(&mut self, _key: Self::Option, _value: OptionValue) -> AdbcResult<()> {
        Err(not_asked("set a statement option"))
    }

    fn get_option_string(&self, _key: Self::Option) -> AdbcResult<String> {
        Err(not_asked("read a statement string option"))
    }

    fn get_option_bytes(&self, _key: Self::Option) -> AdbcResult<Vec<u8>> {
        Err(not_asked("read a statement bytes option"))
    }

    fn get_option_int(&self, _key: Self::Option) -> AdbcResult<i64> {
        Err(not_asked("read a statement integer option"))
    }

    fn get_option_double(&self, _key: Self::Option) -> AdbcResult<f64> {
        Err(not_asked("read a statement double option"))
    }
}

impl Statement for HangingStatement {
    fn bind(&mut self, _batch: RecordBatch) -> AdbcResult<()> {
        Err(not_asked("bind"))
    }

    fn bind_stream(&mut self, _reader: Box<dyn RecordBatchReader + Send>) -> AdbcResult<()> {
        Err(not_asked("bind a stream"))
    }

    fn execute(&mut self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        Err(not_asked("execute"))
    }

    fn execute_update(&mut self) -> AdbcResult<Option<i64>> {
        Err(not_asked("execute an update"))
    }

    fn execute_schema(&mut self) -> AdbcResult<Schema> {
        Err(not_asked("read a result schema"))
    }

    fn execute_partitions(&mut self) -> AdbcResult<PartitionedResult> {
        while !self.cancelled.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        Err(not_asked("execute partitions past being told to stop"))
    }

    fn get_parameter_schema(&self) -> AdbcResult<Schema> {
        Err(not_asked("read a parameter schema"))
    }

    fn prepare(&mut self) -> AdbcResult<()> {
        Err(not_asked("prepare"))
    }

    fn set_sql_query(&mut self, _query: impl AsRef<str>) -> AdbcResult<()> {
        Err(not_asked("set the SQL query"))
    }

    fn set_substrait_plan(&mut self, _plan: impl AsRef<[u8]>) -> AdbcResult<()> {
        Err(not_asked("set a Substrait plan"))
    }

    fn cancel(&mut self) -> AdbcResult<()> {
        self.cancelled.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn a_hung_execute_partitions_is_cancelled_and_the_call_returns_by_the_deadline() {
    // **`execute_leg_partitions`'s own deadline, S1's finding.** Nothing here ever finishes
    // `execute_partitions` on its own - the ONLY way `execute_leg_partitions` below returns at all
    // is the deadline firing and asking this fake to `cancel`.
    let hanging = HangingStatement::new();
    let cancelled = Arc::clone(&hanging.cancelled);
    let opened = Instant::now();
    let port = JobDeadline::Port(deadline(opened, Duration::from_millis(20)));
    let started = Instant::now();
    let result = execute_leg_partitions(hanging, (), port, opened);
    assert!(matches!(result, Err(AdbcError::DeadlineElapsed)), "{result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the call outlived its 20ms deadline by {:?}",
        started.elapsed()
    );
    let latest = Instant::now() + Duration::from_secs(10);
    while !cancelled.load(Ordering::SeqCst) && Instant::now() < latest {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        cancelled.load(Ordering::SeqCst),
        "the deadline elapsing must ask the statement to cancel"
    );
}

/// A statement whose `execute_partitions` answers immediately with a made-up
/// [`PartitionedResult`], standing in for the driver's own answer.
struct Partitioning {
    answers: PartitionedResult,
}

impl Clone for Partitioning {
    /// Hand-rolled: [`PartitionedResult`] itself derives only `Debug, PartialEq, Eq`, so `derive`
    /// cannot reach through it - cloning its three fields individually is all this needs.
    fn clone(&self) -> Self {
        Self {
            answers: PartitionedResult {
                partitions: self.answers.partitions.clone(),
                schema: self.answers.schema.clone(),
                rows_affected: self.answers.rows_affected,
            },
        }
    }
}

impl Optionable for Partitioning {
    type Option = OptionStatement;

    fn set_option(&mut self, _key: Self::Option, _value: OptionValue) -> AdbcResult<()> {
        Err(not_asked("set a statement option"))
    }

    fn get_option_string(&self, _key: Self::Option) -> AdbcResult<String> {
        Err(not_asked("read a statement string option"))
    }

    fn get_option_bytes(&self, _key: Self::Option) -> AdbcResult<Vec<u8>> {
        Err(not_asked("read a statement bytes option"))
    }

    fn get_option_int(&self, _key: Self::Option) -> AdbcResult<i64> {
        Err(not_asked("read a statement integer option"))
    }

    fn get_option_double(&self, _key: Self::Option) -> AdbcResult<f64> {
        Err(not_asked("read a statement double option"))
    }
}

impl Statement for Partitioning {
    fn bind(&mut self, _batch: RecordBatch) -> AdbcResult<()> {
        Err(not_asked("bind"))
    }

    fn bind_stream(&mut self, _reader: Box<dyn RecordBatchReader + Send>) -> AdbcResult<()> {
        Err(not_asked("bind a stream"))
    }

    fn execute(&mut self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        Err(not_asked("execute"))
    }

    fn execute_update(&mut self) -> AdbcResult<Option<i64>> {
        Err(not_asked("execute an update"))
    }

    fn execute_schema(&mut self) -> AdbcResult<Schema> {
        Err(not_asked("read a result schema"))
    }

    fn execute_partitions(&mut self) -> AdbcResult<PartitionedResult> {
        Ok(PartitionedResult {
            partitions: self.answers.partitions.clone(),
            schema: self.answers.schema.clone(),
            rows_affected: self.answers.rows_affected,
        })
    }

    fn get_parameter_schema(&self) -> AdbcResult<Schema> {
        Err(not_asked("read a parameter schema"))
    }

    fn prepare(&mut self) -> AdbcResult<()> {
        Err(not_asked("prepare"))
    }

    fn set_sql_query(&mut self, _query: impl AsRef<str>) -> AdbcResult<()> {
        Err(not_asked("set the SQL query"))
    }

    fn set_substrait_plan(&mut self, _plan: impl AsRef<[u8]>) -> AdbcResult<()> {
        Err(not_asked("set a Substrait plan"))
    }

    fn cancel(&mut self) -> AdbcResult<()> {
        Err(not_asked("cancel"))
    }
}

#[test]
fn execute_leg_partitions_returns_the_schema_every_descriptor_and_the_mapped_row_count() {
    // **N1's cell.** `PartitionedResult::rows_affected` is `-1` for "not known", not absent, and
    // this is the one place that sentinel is mapped - `-1` to `None`, everything else (including
    // `0`) to `Some`. Asserted for both ends and the middle so a mutation that maps every value the
    // same way cannot pass by only checking one.
    let mut unknown_rows = Partitioning {
        answers: PartitionedResult {
            partitions: vec![vec![1, 2, 3], vec![4, 5, 6]],
            schema: Schema::new(vec![Field::new("value", DataType::Int64, false)]),
            rows_affected: -1,
        },
    };
    let answered = execute_leg_partitions(unknown_rows.clone(), (), JobDeadline::Boot, Instant::now())
        .expect("the fake answers unconditionally");
    assert_eq!(answered.schema.fields().len(), 1);
    assert_eq!(answered.schema.field(0).name(), "value");
    assert_eq!(answered.descriptors.len(), 2);
    assert_eq!(answered.rows, None, "-1 rows_affected must map to None, not a count");

    unknown_rows.answers.rows_affected = 0;
    let zero = execute_leg_partitions(unknown_rows.clone(), (), JobDeadline::Boot, Instant::now())
        .expect("the fake answers unconditionally");
    assert_eq!(zero.rows, Some(0), "0 rows_affected is a known, zero count - not None");

    unknown_rows.answers.rows_affected = 7;
    let seven =
        execute_leg_partitions(unknown_rows, (), JobDeadline::Boot, Instant::now()).expect("the fake answers unconditionally");
    assert_eq!(seven.rows, Some(7));
}

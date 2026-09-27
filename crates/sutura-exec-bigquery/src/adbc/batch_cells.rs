//! `AdbcError::Batch` and `AdbcError::Unannounced`, raised only while `execute_to_completion`
//! drains a result stream. Driven through `run_to_deadline` as `adbc::tests` does, because
//! `JobTransport::run` loads a real driver.

use std::sync::Arc;
use std::time::Instant;

use adbc_core::Statement;
use adbc_core::error::{Error, Result as AdbcResult, Status};
use adbc_core::options::{OptionStatement, OptionValue};
use arrow_array::{ArrayRef, Int64Array, RecordBatch, RecordBatchIterator, RecordBatchReader};
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef};

use super::{AdbcError, run_to_deadline};
use crate::transport::JobDeadline;

fn named(column: &str) -> SchemaRef {
    Arc::new(Schema::new(vec![Field::new(column, DataType::Int64, true)]))
}

fn one_row(schema: SchemaRef) -> RecordBatch {
    let array: ArrayRef = Arc::new(Int64Array::from(vec![1_i64]));
    RecordBatch::try_new(schema, vec![array]).expect("a one-column batch is rectangular")
}

/// A statement whose `execute` announces `announced` and then streams `delivered`, followed by a
/// stream error when `fails_after` is set.
#[derive(Clone)]
struct Streaming {
    announced: SchemaRef,
    delivered: RecordBatch,
    fails_after: bool,
}

fn not_asked(what: &str) -> Error {
    Error::with_message_and_status(
        format!("the streaming statement was asked to {what}, which `execute_to_completion` does not do"),
        Status::NotImplemented,
    )
}

impl adbc_core::Optionable for Streaming {
    type Option = OptionStatement;

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

impl Statement for Streaming {
    fn bind(&mut self, _batch: RecordBatch) -> AdbcResult<()> {
        Err(not_asked("bind"))
    }

    fn bind_stream(&mut self, _reader: Box<dyn RecordBatchReader + Send>) -> AdbcResult<()> {
        Err(not_asked("bind a stream"))
    }

    fn execute(&mut self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        let failure = self.fails_after.then(|| {
            Err(ArrowError::ComputeError(String::from(
                "the stream failed after its first batch",
            )))
        });
        Ok(Box::new(RecordBatchIterator::new(
            core::iter::once(Ok(self.delivered.clone())).chain(failure),
            Arc::clone(&self.announced),
        )))
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

#[test]
fn a_result_stream_that_fails_after_its_first_batch_is_refused_as_batch() {
    let statement = Streaming {
        announced: named("value"),
        delivered: one_row(named("value")),
        fails_after: true,
    };
    let failed = run_to_deadline(statement, (), JobDeadline::Boot, Instant::now());
    assert!(matches!(failed, Err(AdbcError::Batch(_))), "{failed:?}");
}

#[test]
fn a_result_stream_whose_batch_disagrees_with_the_announced_schema_is_refused_as_unannounced() {
    let statement = Streaming {
        announced: named("value"),
        delivered: one_row(named("renamed")),
        fails_after: false,
    };
    let failed = run_to_deadline(statement, (), JobDeadline::Boot, Instant::now());
    assert!(matches!(failed, Err(AdbcError::Unannounced(_))), "{failed:?}");
}

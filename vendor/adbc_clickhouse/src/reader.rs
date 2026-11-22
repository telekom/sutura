use crate::TokioContext;
use crate::timeout::Deadline;
use adbc_core::error::Error;
use arrow_array::{RecordBatch, RecordBatchReader};
use arrow_schema::{ArrowError, Schema, SchemaRef};
use clickhouse_ext_arrow::ArrowCursor;

use clickhouse::error::Error as ChError;

pub(crate) struct ArrowStreamReader {
    tokio: TokioContext,
    /// `None` once the stream has ended by timeout; dropping the cursor closes the connection.
    cursor: Option<ArrowCursor>,
    schema: SchemaRef,
    first_batch: Option<RecordBatch>,
    deadline: Deadline,
}

impl ArrowStreamReader {
    /// Reads up to the first batch. Every read of this stream, this one and each later one,
    /// is bound by the same `deadline`.
    pub(crate) async fn begin(
        tokio: TokioContext,
        mut cursor: ArrowCursor,
        deadline: Deadline,
    ) -> Result<Self, Error> {
        // We need to read the schema message so that `RecordBatchReader::schema()` is infallible.
        // This entails reading up to, and storing, the first record batch if applicable.
        let first_batch = deadline
            .bound(cursor.next())
            .await?
            .map_err(|e| Error::from(ch_error_to_arrow_error(e)))?;

        let schema = cursor
            .schema()
            // Empty result, possibly because the query was DDL; don't error.
            // https://github.com/ClickHouse/adbc_clickhouse/issues/49
            .unwrap_or_else(|| Schema::empty().into());

        Ok(Self {
            tokio,
            cursor: Some(cursor),
            schema,
            first_batch,
            deadline,
        })
    }
}

impl RecordBatchReader for ArrowStreamReader {
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }
}

impl Iterator for ArrowStreamReader {
    type Item = Result<RecordBatch, ArrowError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(first_batch) = self.first_batch.take() {
            return Some(Ok(first_batch));
        }

        let cursor = self.cursor.as_mut()?;

        match self.tokio.block_on(self.deadline.bound(cursor.next())) {
            Ok(read) => read.map_err(ch_error_to_arrow_error).transpose(),
            Err(_) => {
                // The stream ends with this error; it never yields a later batch.
                self.cursor = None;
                Some(Err(ArrowError::ExternalError(STREAM_TIMED_OUT.into())))
            }
        }
    }
}

/// The error text a consumer behind the C interface sees for a read that outran the deadline.
pub(crate) const STREAM_TIMED_OUT: &str = "query result stream timed out";

fn ch_error_to_arrow_error(e: ChError) -> ArrowError {
    if let ChError::Other(e) = e {
        return e
            .downcast::<ArrowError>()
            .map_or_else(ArrowError::ExternalError, |e| *e);
    }

    ArrowError::ExternalError(e.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_server::{FakeServer, Step, ipc_pieces};
    use std::time::{Duration, Instant};

    fn open(
        server: &FakeServer,
        bound: Option<Duration>,
    ) -> (TokioContext, Result<ArrowStreamReader, Error>) {
        use clickhouse_ext_arrow::ArrowQueryExt;

        let tokio = TokioContext::new_current_thread().unwrap();
        let _guard = tokio.enter();
        let cursor = server.client().query_raw("SELECT n").fetch_arrow().unwrap();
        let deadline = Deadline::after(bound);
        let reader = tokio.block_on(ArrowStreamReader::begin(tokio.clone(), cursor, deadline));
        (tokio, reader)
    }

    #[test]
    fn a_stall_after_the_first_batch_ends_the_stream_with_an_error() {
        let mut pieces = ipc_pieces(2).into_iter();
        let server = FakeServer::streaming(vec![Step::Send(pieces.next().unwrap())]);
        let (_tokio, reader) = open(&server, Some(Duration::from_millis(1_500)));
        let mut reader = reader.unwrap();

        reader.next().unwrap().unwrap();

        let started = Instant::now();
        let error = reader.next().unwrap().unwrap_err();
        assert!(error.to_string().contains(STREAM_TIMED_OUT), "{error}");
        assert!(started.elapsed() < Duration::from_secs(4));

        assert!(
            reader.next().is_none(),
            "a timed-out stream yields nothing more"
        );
    }

    #[test]
    fn one_deadline_bounds_the_whole_stream_not_each_read() {
        // Each gap is shorter than the bound, but the stream as a whole is not.
        let mut script = Vec::new();
        for piece in ipc_pieces(5) {
            script.push(Step::Send(piece));
            script.push(Step::Wait(Duration::from_millis(600)));
        }
        script.push(Step::Finish);
        let server = FakeServer::streaming(script);
        let (_tokio, reader) = open(&server, Some(Duration::from_millis(2_000)));

        let outcome: Result<Vec<_>, _> = reader.unwrap().collect();

        let error = outcome.expect_err("the stream outran its one deadline");
        assert!(error.to_string().contains(STREAM_TIMED_OUT), "{error}");
    }

    #[test]
    fn with_no_bound_a_slow_stream_is_read_to_its_end() {
        let mut script = Vec::new();
        for piece in ipc_pieces(2) {
            script.push(Step::Send(piece));
            script.push(Step::Wait(Duration::from_millis(300)));
        }
        script.push(Step::Finish);
        let server = FakeServer::streaming(script);
        let (_tokio, reader) = open(&server, None);

        let batches: Vec<_> = reader.unwrap().collect::<Result<_, _>>().unwrap();

        assert_eq!(batches.len(), 2);
    }
}

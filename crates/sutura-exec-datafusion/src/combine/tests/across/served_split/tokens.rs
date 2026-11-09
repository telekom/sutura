//! A reading node's reader that reads only with a token.

use std::sync::Arc;

use datafusion::arrow::datatypes::SchemaRef;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use sutura_domain::model::SourceName;

use crate::combine::{Forwarded, PartitionReader, Unread};

/// Each partition a reader read with a token, and the token.
pub(super) type Handed = Arc<parking_lot::Mutex<Vec<(Vec<u8>, String)>>>;

/// A reader that reads only with a token, answering as `rows` does and logging each token. Not
/// beside its one cell: the causality gate matches a helper by name, and would read each cell there
/// that names another `read` as calling this one.
pub(super) struct ReadsWithTokens {
    pub(super) rows: Arc<dyn PartitionReader>,
    pub(super) handed: Handed,
}

impl PartitionReader for ReadsWithTokens {
    fn read(&self, _: &[u8], _: &SourceName, _: &[u8], _: &SchemaRef) -> Result<SendableRecordBatchStream, Unread> {
        Err(Unread::Failed(DataFusionError::Execution(String::from("read with no token"))))
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a test reader logs the token it was handed, so a cell can compare it with the one granted"
    )]
    fn read_forwarded(
        &self,
        wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
        forwarded: Forwarded,
    ) -> Result<SendableRecordBatchStream, Unread> {
        self.handed
            .lock()
            .push((partition.to_vec(), String::from(forwarded.into_token().expose_secret())));
        self.rows.read(wire, source, partition, schema)
    }
}

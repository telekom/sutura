//! A reading node whose reader logs the end of the grant each token it is handed was admitted
//! under, and how long that grant had left. Not beside its one cell, for the reason `tokens` gives.

use core::time::Duration;
use std::sync::Arc;
use std::time::SystemTime;

use datafusion::arrow::datatypes::SchemaRef;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use sutura_domain::model::SourceName;
use tokio::runtime::Runtime;
use url::Url;

use super::super::Authority;
use crate::combine::{Forwarded, Nodes, PartitionReader, Unread};

/// Each admitted end a reader was handed, and the time it had left then.
pub(super) type Ends = Arc<parking_lot::Mutex<Vec<(SystemTime, Option<Duration>)>>>;

/// A reader that reads only with a token, answering as `rows` does and logging its grant's end.
struct LogsTheEnd {
    rows: Arc<dyn PartitionReader>,
    ends: Ends,
}

impl PartitionReader for LogsTheEnd {
    fn read(
        &self,
        _: &[u8],
        _: &SourceName,
        _: &[u8],
        _: &SchemaRef,
        _: Option<crate::SubmissionCapture>,
    ) -> Result<SendableRecordBatchStream, Unread> {
        Err(Unread::Failed(DataFusionError::Execution(String::from("read with no token"))))
    }

    fn read_forwarded(
        &self,
        wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
        forwarded: Forwarded,
        capture: Option<crate::SubmissionCapture>,
    ) -> Result<SendableRecordBatchStream, Unread> {
        self.ends.lock().push((forwarded.ends(), forwarded.left()));
        drop(forwarded);
        self.rows.read(wire, source, partition, schema, capture)
    }
}

/// A reading node over a [`LogsTheEnd`] reader.
pub(super) struct EndPeer {
    pub(super) url: Url,
    pub(super) ends: Ends,
}

impl EndPeer {
    pub(super) fn logging_ends(servers: &Runtime, authority: &Authority) -> Self {
        let signed = authority.sign();
        let ends = Ends::default();
        let reading = LogsTheEnd {
            rows: super::reader(super::Asked::default()),
            ends: Arc::clone(&ends),
        };
        let worker = Nodes::worker_reading(super::roomy(), authority.identity(&signed), Some(Arc::new(reading)))
            .expect("a reading worker builds");
        let url = super::served_seeing(
            servers,
            super::listening(&signed, authority.peers()),
            worker,
            super::Seen::default(),
        );
        Self { url, ends }
    }
}

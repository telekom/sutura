#![forbid(unsafe_code)]
//! Arrow Flight framing for a typed remote leg.
//!
//! The remote-leg ADR chooses one `DoExchange` call per leg. This crate owns only that protocol
//! boundary: the descriptor carries the domain's [`LegWire`] document and the first stream carries
//! the parameter batches. It does not open a listener, verify identity, or execute a leg; those
//! responsibilities stay with the composition and application layers that will consume this
//! foundation in later slices.

use arrow_array::RecordBatch;
use arrow_flight::{
    FlightData, FlightDescriptor, decode::FlightRecordBatchStream, encode::FlightDataEncoderBuilder, error::FlightError,
};
use futures_util::{Stream, StreamExt as _, stream::BoxStream};
use sutura_domain::plan::LegWire;

/// The protocol's descriptor kind: a typed leg is carried as command bytes, never as a path.
const COMMAND_DESCRIPTOR: i32 = 1;

/// Why a typed leg could not become a Flight descriptor.
#[derive(Debug, thiserror::Error)]
pub enum DescriptorError {
    /// The domain document could not be encoded as JSON.
    #[error("the remote leg could not be encoded: {0}")]
    Encode(#[from] serde_json::Error),
}

/// Encode a typed leg as the command descriptor used by `DoExchange`.
///
/// The receiver must deserialize these bytes into [`LegWire`] and certify them against its own
/// pinned definitions. No parameter value or credential belongs in the descriptor.
pub fn descriptor(leg: &LegWire) -> Result<FlightDescriptor, DescriptorError> {
    Ok(FlightDescriptor {
        r#type: COMMAND_DESCRIPTOR,
        cmd: serde_json::to_vec(leg)?.into(),
        path: Vec::new(),
    })
}

/// Build the input stream for one `DoExchange` call.
///
/// `FlightDataEncoderBuilder` writes the descriptor into the first Flight message and encodes
/// each parameter [`RecordBatch`] as Arrow IPC. The stream is intentionally generic over its
/// source so the eventual Warehouse client can attach its own deadline and credential metadata.
pub fn request_stream<S>(leg: &LegWire, batches: S) -> Result<RequestStream, DescriptorError>
where
    S: Stream<Item = Result<RecordBatch, FlightError>> + Send + 'static,
{
    Ok(FlightDataEncoderBuilder::new()
        .with_flight_descriptor(Some(descriptor(leg)?))
        .build(batches)
        .boxed())
}

/// The result stream type returned by the Arrow Flight client after decoding response batches.
pub type ResponseStream = FlightRecordBatchStream;

/// The encoded request stream consumed by Arrow Flight's `DoExchange` client.
pub type RequestStream = BoxStream<'static, Result<FlightData, FlightError>>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Int64Array, RecordBatch};
    use futures_util::{TryStreamExt as _, stream};

    fn leg() -> LegWire {
        serde_json::from_value(serde_json::json!({
            "leg": "lookup",
            "source": "sales",
            "metric": {"metric": "revenue", "digest": "0000000000000000000000000000000000000000000000000000000000000000"},
            "table": "orders",
            "joins": [],
            "keys": [],
            "filters": []
        }))
        .expect("the fixture is a typed leg")
    }

    #[test]
    fn descriptor_is_a_command_and_the_first_message_carries_it() {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a test runtime")
            .block_on(async {
                let batch = RecordBatch::try_from_iter([("value", Arc::new(Int64Array::from(vec![7])) as ArrayRef)])
                    .expect("the parameter batch is valid");
                let mut encoded = request_stream(&leg(), stream::iter([Ok(batch)])).expect("the leg descriptor is encodable");
                let first = encoded
                    .next()
                    .await
                    .expect("the encoder emits a descriptor")
                    .expect("encoding succeeds");
                let descriptor = first.flight_descriptor.expect("the first message carries the descriptor");
                assert_eq!(descriptor.r#type, COMMAND_DESCRIPTOR);
                assert!(!descriptor.cmd.is_empty());
            });
    }

    #[test]
    fn parameter_batches_are_arrow_decodable_after_the_descriptor() {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a test runtime")
            .block_on(async {
                let batch = RecordBatch::try_from_iter([("value", Arc::new(Int64Array::from(vec![7])) as ArrayRef)])
                    .expect("the parameter batch is valid");
                let encoded = request_stream(&leg(), stream::iter([Ok(batch)])).expect("the leg descriptor is encodable");
                let messages: Vec<_> = encoded.try_collect().await.expect("encoding succeeds");
                assert!(messages.iter().any(|message| !message.data_body.is_empty()));
            });
    }
}

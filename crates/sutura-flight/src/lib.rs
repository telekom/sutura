#![forbid(unsafe_code)]
//! Arrow Flight framing for a typed remote leg.
//!
//! The remote-leg ADR chooses one `DoExchange` call per leg. This crate owns only that protocol
//! boundary: the descriptor carries the domain's [`LegWire`] document and the first stream carries
//! the parameter batch. It does not open a listener, verify identity, or execute a leg; those
//! responsibilities stay with the composition and application layers that will consume this
//! foundation in later slices.

mod receive;

pub use receive::{ReceiveError, receive_request};

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

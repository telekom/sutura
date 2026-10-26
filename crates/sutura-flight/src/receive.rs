//! Inbound framing only; certification and parameter interpretation belong to the caller.

use std::num::NonZeroUsize;

use arrow_array::RecordBatch;
use arrow_flight::{FlightData, decode::FlightRecordBatchStream, error::FlightError};
use futures_util::{Stream, StreamExt as _, TryStreamExt as _, stream};
use sutura_domain::plan::LegWire;

use crate::COMMAND_DESCRIPTOR;

/// A closed framing refusal. Peer-controlled JSON and Arrow errors are deliberately not rendered.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReceiveError {
    /// No message arrived.
    #[error("the remote leg request is empty")]
    Empty,
    /// Only the first message may carry a command descriptor, with no path.
    #[error("the remote leg descriptor is missing or has the wrong shape")]
    Descriptor,
    /// The descriptor exceeds the caller's bound before JSON decoding.
    #[error("the remote leg descriptor exceeds its byte limit")]
    DescriptorLimit,
    /// The command does not deserialize as a typed domain leg.
    #[error("the remote leg descriptor is invalid")]
    Leg,
    /// The encoded frames exceed the caller's bound before Arrow decoding.
    #[error("the remote leg request exceeds its frame budget")]
    FrameLimit,
    /// The stream or its Arrow IPC could not be read.
    #[error("the remote leg parameters could not be read")]
    Parameters,
    /// The protocol carries exactly one parameter batch.
    #[error("the remote leg requires exactly one parameter batch")]
    BatchCount,
}

/// Read the typed descriptor and exactly one Arrow parameter batch from a `DoExchange` request.
///
/// Both bounds apply before decoding: `descriptor_limit` caps command bytes, and `frame_budget`
/// charges frame struct sizes plus encoded buffer lengths, including metadata. This bounds
/// retained frame count and encoded length, not allocator overhead or Arrow decoded memory.
/// The listener must additionally bound messages, decode resources, and the stream's deadline.
/// The returned [`LegWire`] is untrusted until certified against this node's pinned definitions
/// through [`LegWire::parse`]; this function neither authorizes nor executes it.
#[expect(
    clippy::map_err_ignore,
    reason = "peer JSON, IPC and stream errors can contain values or credentials; framing exposes only closed refusals"
)]
pub async fn receive_request<S>(
    stream: S,
    descriptor_limit: NonZeroUsize,
    frame_budget: NonZeroUsize,
) -> Result<(LegWire, RecordBatch), ReceiveError>
where
    S: Stream<Item = Result<FlightData, FlightError>> + Send + 'static,
{
    let mut stream = std::pin::pin!(stream);
    let mut first = stream
        .next()
        .await
        .ok_or(ReceiveError::Empty)?
        .map_err(|_| ReceiveError::Parameters)?;
    let descriptor = first.flight_descriptor.take().ok_or(ReceiveError::Descriptor)?;
    if descriptor.r#type != COMMAND_DESCRIPTOR || !descriptor.path.is_empty() {
        return Err(ReceiveError::Descriptor);
    }
    if descriptor.cmd.len() > descriptor_limit.get() {
        return Err(ReceiveError::DescriptorLimit);
    }
    let mut remaining = frame_budget
        .get()
        .checked_sub(descriptor.cmd.len())
        .ok_or(ReceiveError::FrameLimit)?;
    let leg = serde_json::from_slice(&descriptor.cmd).map_err(|_| ReceiveError::Leg)?;
    let mut frames = Vec::new();
    charge(&first, &mut remaining)?;
    frames.push(first);
    while let Some(message) = stream.next().await {
        let frame = message.map_err(|_| ReceiveError::Parameters)?;
        if frame.flight_descriptor.is_some() {
            return Err(ReceiveError::Descriptor);
        }
        charge(&frame, &mut remaining)?;
        frames.push(frame);
    }
    let mut decoded = FlightRecordBatchStream::new_from_flight_data(stream::iter(frames.into_iter().map(Ok)));
    let batch = decoded
        .try_next()
        .await
        .map_err(|_| ReceiveError::Parameters)?
        .ok_or(ReceiveError::BatchCount)?;
    if decoded.try_next().await.map_err(|_| ReceiveError::Parameters)?.is_some() {
        return Err(ReceiveError::BatchCount);
    }
    Ok((leg, batch))
}

fn charge(frame: &FlightData, remaining: &mut usize) -> Result<(), ReceiveError> {
    for bytes in [
        std::mem::size_of::<FlightData>(),
        frame.data_header.len(),
        frame.data_body.len(),
        frame.app_metadata.len(),
    ] {
        *remaining = remaining.checked_sub(bytes).ok_or(ReceiveError::FrameLimit)?;
    }
    Ok(())
}

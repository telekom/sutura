#![no_main]
//! The receiving node's own `DoExchange` frame decode - `data` as the bytes a server reads from a
//! client, split into zero or more length-delimited `FlightData` protobuf messages (the framing
//! one `DoExchange` stream carries) and run through `sutura_flight::receive_request`,
//! `crates/sutura-flight/src/receive.rs`'s own untrusted-input boundary.
//!
//! The first frame's descriptor is replaced with a fixed, valid one before the call: fuzzed bytes
//! almost never decode into a `FlightData` whose descriptor also deserializes as a `LegWire`, so
//! without the overwrite `receive_request` would refuse nearly every input at the very first check
//! (`ReceiveError::Descriptor`) before the arrow-IPC reader or `compression.rs` ever ran. Overwriting
//! it with a descriptor built from a real `LegWire` lets the fuzzed bytes - the frame count, the
//! IPC body, the compression - reach past that gate.
//!
//! A message this harness cannot parse, or a length prefix past the remaining bytes, ends the
//! frame split early rather than panicking: the target measures `receive_request`'s refusal
//! surface over whatever prefix parsed, not over this harness's own parse errors.
//!
//! Polled once, not awaited on a runtime. Read alongside `receive_request`'s own body and
//! `arrow-flight`'s `FlightRecordBatchStream`: every `.await` in the call resolves against either
//! `stream::iter` (already-ready items) or that decoder's own synchronous per-item IPC parse, so a
//! single poll either completes the call or the assertion below catches a future that started
//! waiting on nothing - which would mean a future version of `receive_request` or its decoder added
//! a real await point this harness no longer measures correctly.
//!
//! **The limit.** This target does not fuzz the descriptor checks or the `LegWire` parse: the
//! descriptor is fixed and valid, and `fuzz/fuzz_targets/leg_wire.rs` fuzzes that parse instead.

use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::pin;
use std::sync::LazyLock;
use std::task::{Context, Poll, Waker};

use arrow_flight::FlightData;
use arrow_flight::error::FlightError;
use futures_util::stream;
use libfuzzer_sys::fuzz_target;
use prost::Message;
use sutura_domain::plan::LegWire;
use sutura_flight::receive_request;

/// The values `crates/sutura-flight/src/service.rs`'s own test module fixes its calls to
/// (`DESCRIPTOR_LIMIT`/`FRAME_BUDGET`, lines 306-307), restated here since that module is
/// `#[cfg(test)]` and not reachable from this separate workspace.
const DESCRIPTOR_LIMIT: usize = 1 << 16;
/// Same restatement, for the frame byte budget.
const FRAME_BUDGET: usize = 1 << 20;

/// A fixed, valid descriptor built from the committed `leg_wire` seed - a leg this harness does not
/// vary, so every `flight_descriptor` check inside `receive_request` passes the same way every run.
static DESCRIPTOR: LazyLock<arrow_flight::FlightDescriptor> = LazyLock::new(|| {
    let leg: LegWire =
        serde_json::from_slice(include_bytes!("../seeds/leg_wire/fact")).expect("the seed leg deserializes");
    sutura_flight::descriptor(&leg).expect("the seed leg serializes as a descriptor")
});

/// `data` split into `FlightData` messages, length-delimited the way one `DoExchange` stream frames
/// them. A prefix that fails to parse ends the split, rather than panicking or skipping bytes to
/// resynchronize.
fn frames(mut data: &[u8]) -> Vec<FlightData> {
    let mut out = Vec::new();
    while !data.is_empty() {
        match FlightData::decode_length_delimited(&mut data) {
            Ok(frame) => out.push(frame),
            Err(_) => break,
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let mut frames = frames(data);
    let Some(first) = frames.first_mut() else {
        return;
    };
    first.flight_descriptor = Some(DESCRIPTOR.clone());

    let stream = stream::iter(frames.into_iter().map(Ok::<_, FlightError>));
    let descriptor_limit = NonZeroUsize::new(DESCRIPTOR_LIMIT).expect("1 << 16 is nonzero");
    let frame_budget = NonZeroUsize::new(FRAME_BUDGET).expect("1 << 20 is nonzero");
    let mut future = pin!(receive_request(stream, descriptor_limit, frame_budget));
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(_) => {}
        Poll::Pending => panic!("receive_request over a synchronous stream pended on the first poll"),
    }
});

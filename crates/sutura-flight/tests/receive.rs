#![forbid(unsafe_code)]

use std::num::NonZeroUsize;
use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_flight::{FlightData, FlightDescriptor, error::FlightError};
use futures_util::{TryStreamExt as _, stream};
use sutura_domain::plan::LegWire;
use sutura_flight::{ReceiveError, descriptor, receive_request, request_stream};

#[cfg(test)]
mod tests {
    use super::*;

    const COMMAND_DESCRIPTOR: i32 = 1;

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

    fn batch() -> RecordBatch {
        RecordBatch::try_from_iter([("value", Arc::new(Int64Array::from(vec![7])) as ArrayRef)])
            .expect("the parameter batch is valid")
    }

    fn generous_limits() -> (NonZeroUsize, NonZeroUsize) {
        (
            NonZeroUsize::new(1 << 20).expect("nonzero"),
            NonZeroUsize::new(1 << 20).expect("nonzero"),
        )
    }

    fn run<F, T>(fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a test runtime")
            .block_on(fut)
    }

    /// Encode one leg and one parameter batch with [`request_stream`] and collect every frame.
    fn encode(leg: &LegWire, batches: Vec<RecordBatch>) -> Vec<FlightData> {
        run(async {
            let encoded = request_stream(leg, stream::iter(batches.into_iter().map(Ok))).expect("the leg is encodable");
            encoded.try_collect().await.expect("encoding succeeds")
        })
    }

    #[test]
    fn a_request_round_trips_through_receive() {
        let (descriptor_limit, frame_budget) = generous_limits();
        let frames = encode(&leg(), vec![batch()]);
        let (received_leg, received_batch) = run(async {
            receive_request(stream::iter(frames.into_iter().map(Ok)), descriptor_limit, frame_budget)
                .await
                .expect("a well formed request is received")
        });
        assert_eq!(received_leg, leg());
        assert_eq!(received_batch, batch());
    }

    #[test]
    fn an_empty_stream_is_refused() {
        let (descriptor_limit, frame_budget) = generous_limits();
        let error = run(async {
            receive_request(
                stream::iter(Vec::<Result<FlightData, FlightError>>::new()),
                descriptor_limit,
                frame_budget,
            )
            .await
            .expect_err("no message arrived")
        });
        assert_eq!(error, ReceiveError::Empty);
    }

    #[test]
    fn a_malformed_descriptor_is_refused() {
        let (descriptor_limit, frame_budget) = generous_limits();

        // No descriptor on the first message.
        let no_descriptor = FlightData {
            flight_descriptor: None,
            ..Default::default()
        };
        let error = run(async {
            receive_request(stream::iter([Ok(no_descriptor)]), descriptor_limit, frame_budget)
                .await
                .expect_err("the first message carries no descriptor")
        });
        assert_eq!(error, ReceiveError::Descriptor);

        // A path-bearing descriptor: the protocol carries a typed leg as command bytes only.
        let path_descriptor = FlightData {
            flight_descriptor: Some(FlightDescriptor {
                r#type: COMMAND_DESCRIPTOR,
                cmd: Vec::new().into(),
                path: vec![String::from("orders")],
            }),
            ..Default::default()
        };
        let error = run(async {
            receive_request(stream::iter([Ok(path_descriptor)]), descriptor_limit, frame_budget)
                .await
                .expect_err("a path-bearing descriptor is refused")
        });
        assert_eq!(error, ReceiveError::Descriptor);

        // A non-command descriptor type.
        let wrong_type = FlightData {
            flight_descriptor: Some(FlightDescriptor {
                r#type: 0,
                cmd: Vec::new().into(),
                path: Vec::new(),
            }),
            ..Default::default()
        };
        let error = run(async {
            receive_request(stream::iter([Ok(wrong_type)]), descriptor_limit, frame_budget)
                .await
                .expect_err("a non-command descriptor type is refused")
        });
        assert_eq!(error, ReceiveError::Descriptor);

        // A second descriptor on a later message.
        let mut frames = encode(&leg(), vec![batch()]);
        let second = frames.get_mut(1).expect("the encoding carries more than one frame");
        second.flight_descriptor = Some(FlightDescriptor {
            r#type: COMMAND_DESCRIPTOR,
            cmd: Vec::new().into(),
            path: Vec::new(),
        });
        let error = run(async {
            receive_request(stream::iter(frames.into_iter().map(Ok)), descriptor_limit, frame_budget)
                .await
                .expect_err("a descriptor on a later message is refused")
        });
        assert_eq!(error, ReceiveError::Descriptor);
    }

    #[test]
    fn a_descriptor_over_its_byte_limit_is_refused() {
        let descriptor_limit = NonZeroUsize::new(1).expect("nonzero");
        let frame_budget = NonZeroUsize::new(1 << 20).expect("nonzero");
        let oversized = FlightData {
            flight_descriptor: Some(FlightDescriptor {
                r#type: COMMAND_DESCRIPTOR,
                cmd: vec![0_u8; 2].into(),
                path: Vec::new(),
            }),
            ..Default::default()
        };
        let error = run(async {
            receive_request(stream::iter([Ok(oversized)]), descriptor_limit, frame_budget)
                .await
                .expect_err("the descriptor exceeds its byte limit")
        });
        assert_eq!(error, ReceiveError::DescriptorLimit);
    }

    #[test]
    fn a_request_over_its_frame_budget_is_refused() {
        let descriptor_limit = NonZeroUsize::new(1 << 20).expect("nonzero");
        let frame_budget = NonZeroUsize::new(1).expect("nonzero");
        let frames = encode(&leg(), vec![batch()]);
        let error = run(async {
            receive_request(stream::iter(frames.into_iter().map(Ok)), descriptor_limit, frame_budget)
                .await
                .expect_err("the request exceeds its frame budget")
        });
        assert_eq!(error, ReceiveError::FrameLimit);
    }

    /// `a_request_over_its_frame_budget_is_refused` pins only the initial blanket check against
    /// `descriptor.cmd.len()`; this cell pins the per-frame `charge()` call that bounds a
    /// parameter batch's own encoded body. The budget here admits the leg with a one-row batch
    /// but not with a five-thousand-row one, so only `charge()`'s own accounting can be what
    /// refuses the second request.
    #[test]
    fn a_parameter_body_over_the_frame_budget_is_refused() {
        let descriptor_limit = NonZeroUsize::new(1 << 20).expect("nonzero");

        let descriptor_cmd_len = descriptor(&leg()).expect("the fixture leg encodes a descriptor").cmd.len();
        let frame_cost = |frames: &[FlightData]| -> usize {
            frames
                .iter()
                .map(|frame| {
                    std::mem::size_of::<FlightData>() + frame.data_header.len() + frame.data_body.len() + frame.app_metadata.len()
                })
                .sum()
        };

        let small_frames = encode(&leg(), vec![batch()]);
        let frame_budget =
            NonZeroUsize::new(descriptor_cmd_len + frame_cost(&small_frames)).expect("a nonempty frame costs more than zero");

        let (received_leg, received_batch) = run(async {
            receive_request(stream::iter(small_frames.into_iter().map(Ok)), descriptor_limit, frame_budget)
                .await
                .expect("a batch within budget is received")
        });
        assert_eq!(received_leg, leg());
        assert_eq!(received_batch, batch());

        let large_batch = RecordBatch::try_from_iter([(
            "value",
            Arc::new(Int64Array::from((0..5_000).collect::<Vec<i64>>())) as ArrayRef,
        )])
        .expect("the oversized parameter batch is valid");
        let large_frames = encode(&leg(), vec![large_batch]);
        let error = run(async {
            receive_request(stream::iter(large_frames.into_iter().map(Ok)), descriptor_limit, frame_budget)
                .await
                .expect_err("the parameter batch exceeds the frame budget")
        });
        assert_eq!(error, ReceiveError::FrameLimit);
    }

    #[test]
    fn a_malformed_leg_is_refused() {
        let (descriptor_limit, frame_budget) = generous_limits();

        for cmd in [
            // Not a `LegWire` at all.
            serde_json::to_vec(&serde_json::json!({})).expect("json encodes"),
            // A valid shape plus a field the typed leg never declares.
            serde_json::to_vec(&serde_json::json!({
                "leg": "lookup",
                "source": "sales",
                "metric": {"metric": "revenue", "digest": "0000000000000000000000000000000000000000000000000000000000000000"},
                "table": "orders",
                "joins": [],
                "keys": [],
                "filters": [],
                "unexpected": true
            }))
            .expect("json encodes"),
        ] {
            let frame = FlightData {
                flight_descriptor: Some(FlightDescriptor {
                    r#type: COMMAND_DESCRIPTOR,
                    cmd: cmd.into(),
                    path: Vec::new(),
                }),
                ..Default::default()
            };
            let error = run(async {
                receive_request(stream::iter([Ok(frame)]), descriptor_limit, frame_budget)
                    .await
                    .expect_err("the command bytes do not decode as a typed leg")
            });
            assert_eq!(error, ReceiveError::Leg);
        }
    }

    #[test]
    fn a_stream_error_is_refused_as_parameters() {
        let (descriptor_limit, frame_budget) = generous_limits();
        let error = run(async {
            receive_request(
                stream::iter([Err(FlightError::DecodeError(String::from("boom")))]),
                descriptor_limit,
                frame_budget,
            )
            .await
            .expect_err("the stream yields an error")
        });
        assert_eq!(error, ReceiveError::Parameters);
    }

    #[test]
    fn a_wrong_batch_count_is_refused() {
        let (descriptor_limit, frame_budget) = generous_limits();

        // Zero batches: only the descriptor-carrying schema message, no parameter frame.
        let mut frames = encode(&leg(), vec![batch()]);
        frames.truncate(1);
        let error = run(async {
            receive_request(stream::iter(frames.into_iter().map(Ok)), descriptor_limit, frame_budget)
                .await
                .expect_err("no parameter batch arrived")
        });
        assert_eq!(error, ReceiveError::BatchCount);

        // Two batches: the protocol carries exactly one.
        let frames = encode(&leg(), vec![batch(), batch()]);
        let error = run(async {
            receive_request(stream::iter(frames.into_iter().map(Ok)), descriptor_limit, frame_budget)
                .await
                .expect_err("a second parameter batch arrived")
        });
        assert_eq!(error, ReceiveError::BatchCount);
    }
}

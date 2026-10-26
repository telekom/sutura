#![forbid(unsafe_code)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use futures_util::{StreamExt as _, TryStreamExt as _, stream};
use sutura_domain::plan::LegWire;
use sutura_flight::request_stream;

#[cfg(test)]
mod tests {
    use super::*;

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
                assert_eq!(descriptor.r#type, 1);
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

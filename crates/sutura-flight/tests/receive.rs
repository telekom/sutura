#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Date32Array, Int64Array, RecordBatch, StringArray};
    use arrow_flight::{FlightData, error::FlightError};
    use futures_util::{TryStreamExt as _, stream};
    use sutura_domain::plan::LegWire;
    use sutura_flight::{ReceiveError, descriptor, receive_request, request_stream};

    fn leg() -> LegWire {
        serde_json::from_value(serde_json::json!({
            "leg": "lookup",
            "source": "sales",
            "metric": {"metric": "revenue", "digest": "0000000000000000000000000000000000000000000000000000000000000000"},
            "table": "orders", "joins": [], "keys": [], "filters": []
        }))
        .expect("a typed fixture leg")
    }

    fn batch() -> RecordBatch {
        RecordBatch::try_from_iter([
            ("integer", Arc::new(Int64Array::from(vec![Some(i64::MAX), None])) as ArrayRef),
            (
                "numeric",
                Arc::new(StringArray::from(vec![Some("12345678901234567890.123456789"), None])) as ArrayRef,
            ),
            ("date", Arc::new(Date32Array::from(vec![Some(-1), Some(20_000)])) as ArrayRef),
        ])
        .expect("a parameter batch with nullable exact values")
    }

    fn limit(bytes: usize) -> NonZeroUsize {
        NonZeroUsize::new(bytes).expect("a nonzero test bound")
    }

    async fn frames() -> Vec<FlightData> {
        request_stream(&leg(), stream::iter([Ok(batch())]))
            .expect("a descriptor")
            .try_collect()
            .await
            .expect("encoded parameters")
    }

    async fn receive(messages: Vec<FlightData>) -> Result<(LegWire, RecordBatch), ReceiveError> {
        receive_request(
            stream::iter(messages.into_iter().map(Ok)),
            limit(64 * 1024),
            limit(1024 * 1024),
        )
        .await
    }

    #[tokio::test]
    async fn an_inbound_leg_preserves_the_typed_descriptor_and_exact_nullable_arrow_values() {
        let (received_leg, parameters) = receive(frames().await).await.expect("a complete inbound request");
        assert_eq!(received_leg, leg());
        assert_eq!(parameters, batch());
    }

    #[tokio::test]
    async fn an_inbound_request_needs_a_first_message() {
        assert_eq!(receive(Vec::new()).await, Err(ReceiveError::Empty));
    }

    #[tokio::test]
    async fn only_a_first_command_descriptor_without_a_path_is_accepted() {
        for mutation in 0..4 {
            let mut messages = frames().await;
            match mutation {
                0 => messages[0].flight_descriptor = None,
                1 => messages[0].flight_descriptor.as_mut().expect("a descriptor").r#type = 2,
                2 => messages[0]
                    .flight_descriptor
                    .as_mut()
                    .expect("a descriptor")
                    .path
                    .push(String::from("orders")),
                _ => {
                    messages.last_mut().expect("a batch frame").flight_descriptor =
                        Some(descriptor(&leg()).expect("a descriptor"));
                }
            }
            assert_eq!(receive(messages).await, Err(ReceiveError::Descriptor), "mutation {mutation}");
        }
    }

    #[tokio::test]
    async fn untyped_or_unknown_descriptor_content_is_refused_without_echoing_it() {
        for command in [
            b"select secret_value from orders".as_slice(),
            b"{}",
            b"{\"leg\":\"sql\",\"sql\":\"secret_value\"}",
        ] {
            let mut messages = frames().await;
            messages[0].flight_descriptor.as_mut().expect("a descriptor").cmd = command.to_vec().into();
            let refusal = receive(messages).await.expect_err("an untyped command is refused");
            assert_eq!(refusal, ReceiveError::Leg);
            assert_eq!(refusal.to_string(), "the remote leg descriptor is invalid");
        }
    }

    #[tokio::test]
    async fn a_descriptor_at_its_bound_is_read_and_one_byte_more_is_refused() {
        let messages = frames().await;
        let command_len = messages[0].flight_descriptor.as_ref().expect("a descriptor").cmd.len();
        let exact = receive_request(
            stream::iter(messages.clone().into_iter().map(Ok)),
            limit(command_len),
            limit(1024 * 1024),
        )
        .await;
        assert_eq!(exact.expect("a request at its exact bound").1, batch());
        let over = receive_request(
            stream::iter(messages.into_iter().map(Ok)),
            limit(command_len - 1),
            limit(1024 * 1024),
        )
        .await;
        assert_eq!(over, Err(ReceiveError::DescriptorLimit));
    }

    #[tokio::test]
    async fn every_encoded_buffer_and_frame_uses_the_budget_before_arrow_decoding() {
        let mut messages = frames().await;
        messages[0].app_metadata = vec![0; 37].into();
        let total = messages
            .iter()
            .map(|frame| {
                std::mem::size_of::<FlightData>()
                    + frame.data_header.len()
                    + frame.data_body.len()
                    + frame.app_metadata.len()
                    + frame.flight_descriptor.as_ref().map_or(0, |descriptor| descriptor.cmd.len())
            })
            .sum();
        let exact = receive_request(
            stream::iter(messages.clone().into_iter().map(Ok)),
            limit(64 * 1024),
            limit(total),
        )
        .await;
        assert_eq!(exact.expect("a request at its exact bound").1, batch());
        let over = receive_request(stream::iter(messages.into_iter().map(Ok)), limit(64 * 1024), limit(total - 1)).await;
        assert_eq!(over, Err(ReceiveError::FrameLimit));
        let mut empty_frames = frames().await;
        empty_frames.extend(std::iter::repeat_n(FlightData::default(), 100));
        let over = receive_request(
            stream::iter(empty_frames.into_iter().map(Ok)),
            limit(64 * 1024),
            limit(total + 1),
        )
        .await;
        assert_eq!(over, Err(ReceiveError::FrameLimit));
    }

    #[tokio::test]
    async fn a_leg_needs_exactly_one_parameter_batch() {
        let mut no_batch = frames().await;
        no_batch.truncate(1);
        assert_eq!(receive(no_batch).await, Err(ReceiveError::BatchCount));
        let two_batches = request_stream(&leg(), stream::iter([Ok(batch()), Ok(batch())]))
            .expect("a descriptor")
            .try_collect()
            .await
            .expect("two encoded batches");
        assert_eq!(receive(two_batches).await, Err(ReceiveError::BatchCount));
    }

    #[tokio::test]
    async fn invalid_arrow_and_stream_failures_are_closed_parameter_refusals() {
        let mut messages = frames().await;
        messages.last_mut().expect("a batch frame").data_header = b"invalid IPC secret_value".to_vec().into();
        assert_eq!(receive(messages).await, Err(ReceiveError::Parameters));
        let broken = stream::iter([Err(FlightError::protocol("peer secret_value"))]);
        let refusal = receive_request(broken, limit(64 * 1024), limit(1024 * 1024))
            .await
            .expect_err("a stream failure");
        assert_eq!(refusal, ReceiveError::Parameters);
        assert_eq!(refusal.to_string(), "the remote leg parameters could not be read");
    }
}

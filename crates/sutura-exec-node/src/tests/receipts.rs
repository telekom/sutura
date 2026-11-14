//! Real Flight frames exercise the receipt boundary, including schema-only answers.

use arrow_flight::FlightData;
use arrow_flight::encode::FlightDataEncoderBuilder;
use futures_util::{TryStreamExt as _, stream};

use super::{Node, NodeError, Reply, answer, client_trusting, fake, labels, run, subject};

fn frames(metadata: Vec<u8>, empty: bool) -> Vec<FlightData> {
    let batch = answer(&labels());
    let schema = batch.schema();
    let batches = if empty { Vec::new() } else { vec![batch] };
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
        .block_on(
            FlightDataEncoderBuilder::new()
                .with_schema(schema)
                .with_metadata(metadata.into())
                .build(stream::iter(batches.into_iter().map(Ok)))
                .try_collect(),
        )
        .expect("scripted frames encode")
}

#[test]
fn a_node_receipt_is_flattened_after_its_outbound_leg_even_without_rows() {
    use sutura_domain::warehouse::execution::Execution;
    let node = Node::answering([Reply::Frames(frames(fake::receipt(), true))]);
    let mut answered = run(&client_trusting(&node, node.anchor().clone()), &subject()).expect("a schema-only answer");
    assert_eq!(answered.rows(), 0);
    let records = answered.take_executions();
    assert_eq!(records.len(), 2);
    assert!(
        matches!(&records[0], Execution::Node { query, .. } if query.as_ref() == &sutura_domain::plan::remote::to_wire(&super::leg()).expect("a wire leg"))
    );
    assert!(matches!(&records[1], Execution::Sql { sql, .. } if sql == "SELECT 7"));
}

#[test]
fn a_node_refuses_missing_malformed_empty_or_excessive_receipts() {
    let excessive_depth = format!(
        r#"{{"version":1,"executions":[{{"kind":"cube","source":"s","query":{}0{}}}]}}"#,
        "[".repeat(62),
        "]".repeat(62)
    );
    let excessive_count = format!(
        r#"{{"version":1,"executions":[{}]}}"#,
        std::iter::repeat_n(r#"{"kind":"cube","source":"s","query":0}"#, (1 << 14) + 1)
            .collect::<Vec<_>>()
            .join(",")
    );
    for (case, metadata) in [
        ("missing", Vec::new()),
        ("malformed", b"{".to_vec()),
        ("empty", br#"{"version":1,"executions":[]}"#.to_vec()),
        ("unknown version", br#"{"version":2,"executions":[]}"#.to_vec()),
        ("oversized", vec![b' '; (1 << 20) + 1]),
        ("depth", excessive_depth.into_bytes()),
        ("count", excessive_count.into_bytes()),
    ] {
        let node = Node::answering([Reply::Frames(frames(metadata, false))]);
        let refused = run(&client_trusting(&node, node.anchor().clone()), &subject()).expect_err(case);
        assert!(matches!(refused, NodeError::Answer), "{case}: {refused:?}");
    }
}

#[test]
fn a_node_charges_receipt_bytes_before_retaining_its_rows() {
    use sutura_domain::warehouse::arrow::ResultBudget;
    let mut envelope: serde_json::Value = serde_json::from_slice(&fake::receipt()).expect("fixture receipt");
    envelope["executions"][0]["sql"] = serde_json::json!(format!("SELECT 7 /*{}*/", "x".repeat(20_000)));
    let metadata = serde_json::to_vec(&envelope).expect("JSON");
    let budget = ResultBudget::of_bytes(std::num::NonZeroUsize::new(metadata.len() + 1).expect("a positive budget"));
    let batch = answer(&labels());
    let mut alone = sutura_domain::warehouse::arrow::Accumulating::announcing(batch.schema(), usize::MAX, budget);
    alone.push(batch).expect("the rows alone fit");
    let node = Node::answering([Reply::Frames(frames(metadata, false))]);
    let warehouse = super::NodeWarehouse::open(
        super::source(),
        sutura_domain::source::SourcePosture::ImpersonationAtSource,
        &super::NodeEndpoint::parse(&format!("https://localhost:{}", node.port())).expect("a TLS endpoint"),
        [node.anchor().clone()],
        budget,
    )
    .expect("a node client");
    let refused = run(&warehouse, &subject()).expect_err("metadata and rows share one budget");
    assert!(matches!(refused, NodeError::Result { .. }), "{refused:?}");
}

#[test]
fn a_node_refuses_receipts_hidden_on_dictionary_frames() {
    use arrow_array::types::Int8Type;
    use arrow_array::{Array as _, ArrayRef, DictionaryArray, Int8Array, RecordBatch, StringArray};
    use arrow_schema::{Field, Schema};
    use std::sync::Arc;
    let values = DictionaryArray::<Int8Type>::try_new(Int8Array::from(vec![0]), Arc::new(StringArray::from(vec!["north"])))
        .expect("a dictionary");
    let schema = Arc::new(Schema::new(
        labels()
            .iter()
            .map(|label| Field::new(label, values.data_type().clone(), true))
            .collect::<Vec<_>>(),
    ));
    let columns: Vec<ArrayRef> = labels().iter().map(|_| Arc::new(values.clone()) as ArrayRef).collect();
    let batch = RecordBatch::try_new(Arc::clone(&schema), columns).expect("a dictionary batch");
    let mut encoded: Vec<FlightData> = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
        .block_on(
            FlightDataEncoderBuilder::new()
                .with_schema(schema)
                .with_metadata(fake::receipt().into())
                .with_dictionary_handling(arrow_flight::encode::DictionaryHandling::Resend)
                .build(stream::iter([Ok(batch)]))
                .try_collect(),
        )
        .expect("dictionary frames");
    assert!(encoded.len() >= 3, "schema, dictionaries and batch");
    encoded[1].app_metadata = fake::receipt().into();
    let node = Node::answering([Reply::Frames(encoded)]);
    let refused = run(&client_trusting(&node, node.anchor().clone()), &subject())
        .expect_err("dictionary metadata cannot hide a duplicate receipt");
    assert!(matches!(refused, NodeError::Answer), "{refused:?}");
}

#[test]
fn a_node_refuses_late_duplicate_or_repeated_schema_receipts() {
    #[derive(Debug)]
    enum Broken {
        Late,
        Duplicate,
        Schema,
    }
    for case in [Broken::Late, Broken::Duplicate, Broken::Schema] {
        let mut scripted = frames(fake::receipt(), false);
        assert_eq!(scripted.len(), 2, "schema then batch");
        match case {
            Broken::Late => {
                scripted[1].app_metadata = core::mem::take(&mut scripted[0].app_metadata);
            }
            Broken::Duplicate => {
                scripted[1].app_metadata = fake::receipt().into();
            }
            Broken::Schema => {
                let mut repeated = scripted[0].clone();
                repeated.app_metadata.clear();
                scripted.insert(1, repeated);
            }
        }
        let node = Node::answering([Reply::Frames(scripted)]);
        let refused = run(&client_trusting(&node, node.anchor().clone()), &subject()).expect_err("invalid receipt placement");
        assert!(matches!(refused, NodeError::Answer), "{case:?}: {refused:?}");
    }
}

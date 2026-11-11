//! A peer's stage whose scalar, constraint, parquet option or schema bytes make the registry
//! crate's decoder panic, rather than refuse, must not abort the worker under `panic = "abort"`.
//! `telekom/sutura#1680`.

use datafusion::error::DataFusionError;
use datafusion_distributed::{MaybeEncoded, SetPlanRequest, TaskKey};
use datafusion_proto::protobuf::physical_expr_node::ExprType;
use datafusion_proto::protobuf::physical_plan_node::PhysicalPlanType;
use datafusion_proto::protobuf::window_agg_exec_node::InputOrderMode;
use datafusion_proto::protobuf::{
    ArrowType, CsvScanExecNode, EmptyExecNode, EmptyMessage, Field, FileScanExecConf, FilterExecNode, ParquetScanExecNode,
    PartiallySortedInputOrderMode, PhysicalExprNode, PhysicalPlanNode, ScalarValue, Schema, WindowAggExecNode,
};
use datafusion_proto_common::protobuf_common::arrow_type::ArrowTypeEnum;
use datafusion_proto_common::protobuf_common::scalar_value::Value as ScalarValueKind;
use datafusion_proto_common::protobuf_common::{
    Constraint, Constraints, Decimal32, ParquetOptions, ScalarNestedValue, TableParquetOptions, Union, UnionMode,
};
use futures_util::stream;
use prost::Message as _;
use std::collections::HashMap;
use tonic::codegen::http::HeaderMap;
use url::Url;

use super::{Authority, roomy, servers};
use crate::combine::Nodes;

/// A zero-field `EmptyExec`, the cheapest child a wrapping node can decode.
fn empty_child() -> Box<PhysicalPlanNode> {
    Box::new(PhysicalPlanNode {
        physical_plan_type: Some(PhysicalPlanType::Empty(EmptyExecNode {
            schema: Some(Schema::default()),
            partitions: 0,
        })),
    })
}

/// A `FilterExec` over [`empty_child`] whose predicate is the literal `value`.
fn filter_literal(value: ScalarValue) -> PhysicalPlanNode {
    PhysicalPlanNode {
        physical_plan_type: Some(PhysicalPlanType::Filter(Box::new(FilterExecNode {
            input: Some(empty_child()),
            expr: Some(PhysicalExprNode {
                expr_id: None,
                expr_type: Some(ExprType::Literal(value)),
            }),
            default_filter_selectivity: 0,
            projection: vec![],
            batch_size: 8192,
            fetch: None,
        }))),
    }
}

/// An empty `FileScanExecConf` reading `schema`, with no constraints of its own.
fn scan_conf(schema: Schema, constraints: Option<Constraints>) -> FileScanExecConf {
    FileScanExecConf {
        file_groups: vec![],
        schema: Some(schema),
        projection: vec![],
        limit: None,
        statistics: None,
        table_partition_cols: vec![],
        object_store_url: String::new(),
        output_ordering: vec![],
        constraints,
        batch_size: None,
        projection_exprs: None,
        output_partitioning: None,
    }
}

/// Sends `plan` as a worker's one stage and returns the refusal, panicking if the worker instead
/// decoded and ran it.
fn refused(plan: &PhysicalPlanNode) -> DataFusionError {
    let authority = Authority::generate();
    let worker = Nodes::worker(roomy(), authority.identity(&authority.sign())).expect("a worker builds");
    #[expect(
        clippy::default_trait_access,
        reason = "the question id is a `uuid`, which this crate does not name; the decoder reads no id"
    )]
    let stage = SetPlanRequest {
        completion: None,
        task_key: TaskKey {
            query_id: Default::default(),
            stage_id: 1,
            task_number: 0,
        },
        task_count: 1,
        plan: MaybeEncoded::Encoded(plan.encode_to_vec()),
        work_unit_feed_declarations: vec![],
        target_worker_url: Url::parse("https://127.0.0.1:1").expect("an address is a URL"),
        query_start_time_ns: 0,
    };
    servers()
        .block_on(worker.coordinator_channel(HeaderMap::new(), stage, Box::pin(stream::empty())))
        .err()
        .expect("a node ran a stage it cannot decode")
}

fn window_without_expressions(input_order_mode: Option<InputOrderMode>) -> PhysicalPlanNode {
    PhysicalPlanNode {
        physical_plan_type: Some(PhysicalPlanType::Window(Box::new(WindowAggExecNode {
            input: Some(empty_child()),
            window_expr: vec![],
            partition_keys: vec![],
            input_order_mode,
        }))),
    }
}

#[test]
fn a_node_refuses_an_unbounded_window_stage_without_expressions() {
    let refused = refused(&window_without_expressions(None));
    assert!(
        matches!(refused.find_root(), DataFusionError::Plan(message) if message == "WindowAggExec requires at least one window expression"),
        "{refused}"
    );
}

#[test]
fn a_node_refuses_a_bounded_window_stage_without_expressions() {
    for order in [
        InputOrderMode::Linear(EmptyMessage {}),
        InputOrderMode::PartiallySorted(PartiallySortedInputOrderMode { columns: vec![] }),
        InputOrderMode::Sorted(EmptyMessage {}),
    ] {
        let refused = refused(&window_without_expressions(Some(order)));
        assert!(
            matches!(refused.find_root(), DataFusionError::Plan(message) if message == "BoundedWindowAggExec requires at least one window expression"),
            "{refused}"
        );
    }
}

#[test]
fn a_node_refuses_a_literal_whose_decimal_value_is_the_wrong_length() {
    // A `Decimal32Value` with 3 value bytes rather than the 4 `i32::from_be_bytes` needs. The
    // registry crate's `vec_to_array` unwraps that length check, which `panic = "abort"` turns
    // into a stopped process.
    let value = ScalarValue {
        value: Some(ScalarValueKind::Decimal32Value(Decimal32 {
            value: vec![0, 0, 0],
            p: 5,
            s: 0,
        })),
    };
    let refused = refused(&filter_literal(value));
    assert!(
        matches!(refused.find_root(), DataFusionError::Plan(message) if message.contains("Decimal32Value")),
        "{refused}"
    );
}

#[test]
fn a_node_refuses_a_literal_whose_list_value_column_is_not_a_list() {
    // A `ListValue` whose IPC batch's one column genuinely decodes as an `Int32Array`, not a list
    // array. The registry crate's `as_list` downcast unwraps that mismatch.
    let arrow_schema = datafusion::arrow::datatypes::Schema::new(vec![datafusion::arrow::datatypes::Field::new(
        "col0",
        datafusion::arrow::datatypes::DataType::Int32,
        true,
    )]);
    let batch = datafusion::arrow::record_batch::RecordBatch::try_new(
        std::sync::Arc::new(arrow_schema.clone()),
        vec![std::sync::Arc::new(datafusion::arrow::array::Int32Array::from(vec![Some(1)]))],
    )
    .expect("a one-column Int32 batch builds");
    let proto_schema: Schema = (&arrow_schema).try_into().expect("an arrow schema encodes to proto");
    let ipc_generator = datafusion::arrow::ipc::writer::IpcDataGenerator {};
    let mut dictionary_tracker = datafusion::arrow::ipc::writer::DictionaryTracker::new(false);
    let write_options = datafusion::arrow::ipc::writer::IpcWriteOptions::default();
    let (_, encoded) = ipc_generator
        .encode(
            &batch,
            &mut dictionary_tracker,
            &write_options,
            &mut datafusion::arrow::ipc::writer::IpcWriteContext::default(),
        )
        .expect("a record batch encodes to IPC");
    let value = ScalarValue {
        value: Some(ScalarValueKind::ListValue(ScalarNestedValue {
            ipc_message: encoded.ipc_message,
            arrow_data: encoded.arrow_data,
            schema: Some(proto_schema),
            dictionaries: vec![],
        })),
    };
    let refused = refused(&filter_literal(value));
    assert!(
        matches!(refused.find_root(), DataFusionError::Plan(message) if message.contains("ListValue")),
        "{refused}"
    );
}

#[test]
fn a_node_refuses_a_file_scan_whose_constraint_mode_is_unset() {
    // A `Constraint` naming neither `PrimaryKey` nor `Unique`. The registry crate's borrowed
    // `From<&Constraint>`, which the owned and `Constraints` impls both delegate to, unwraps that
    // oneof.
    let plan = PhysicalPlanNode {
        physical_plan_type: Some(PhysicalPlanType::CsvScan(CsvScanExecNode {
            base_conf: Some(scan_conf(
                Schema::default(),
                Some(Constraints {
                    constraints: vec![Constraint { constraint_mode: None }],
                }),
            )),
            has_header: false,
            delimiter: ",".to_owned(),
            quote: "\"".to_owned(),
            newlines_in_values: false,
            truncate_rows: false,
            optional_escape: None,
            optional_comment: None,
        })),
    };
    let refused = refused(&plan);
    assert!(
        matches!(refused.find_root(), DataFusionError::Plan(message) if message.contains("constraint_mode")),
        "{refused}"
    );
}

#[test]
fn a_node_refuses_a_parquet_scan_whose_projection_is_out_of_bounds() {
    let plan = PhysicalPlanNode::decode(&[10, 6, 10, 4, 18, 0, 32, 10][..]).expect("the fuzz input is a valid protobuf message");
    let refused = refused(&plan);
    assert!(
        matches!(refused.find_root(), DataFusionError::Internal(message)
            if message == "Parquet projection index 10 is out of bounds for 0 columns"),
        "{refused}"
    );
}

#[test]
fn a_node_refuses_a_parquet_scan_whose_global_options_fail_to_convert() {
    // `TableParquetOptions.global`'s default `writer_version` is the empty string, which fails to
    // parse. The registry crate unwraps that `Result` a second time, after a first `.unwrap()`
    // that only strips the `Option`. Its `base_conf` leaves `constraints` unset: `ParquetSource`
    // reads `parquet_options` before `FileScanConfig::try_from_proto`, so this is isolated from the
    // constraint-mode site above.
    let plan = PhysicalPlanNode {
        physical_plan_type: Some(PhysicalPlanType::ParquetScan(ParquetScanExecNode {
            base_conf: Some(scan_conf(Schema::default(), None)),
            predicate: None,
            parquet_options: Some(TableParquetOptions {
                global: Some(ParquetOptions::default()),
                column_specific_options: vec![],
                key_value_metadata: HashMap::default(),
            }),
        })),
    };
    let refused = refused(&plan);
    assert!(
        matches!(refused.find_root(), DataFusionError::Internal(message) if message.contains("writer_version")),
        "{refused}"
    );
}

#[test]
fn a_node_refuses_a_schema_whose_union_field_has_too_many_children() {
    // A `Union` field with 129 children and no explicit `type_ids`. The registry crate's
    // `UnionFields::from_fields` falls back to index-based ids and `expect`s at most 128 fields.
    // Arrow's own error names `UnionFields`, so the cell reads the decoder's context instead.
    let union_types: Vec<Field> = (0..129)
        .map(|i| Field {
            name: format!("f{i}"),
            arrow_type: Some(Box::new(ArrowType {
                arrow_type_enum: Some(ArrowTypeEnum::Int32(EmptyMessage {})),
            })),
            nullable: true,
            children: vec![],
            metadata: HashMap::default(),
        })
        .collect();
    let schema = Schema {
        columns: vec![Field {
            name: "u".to_owned(),
            arrow_type: Some(Box::new(ArrowType {
                arrow_type_enum: Some(ArrowTypeEnum::Union(Union {
                    union_types,
                    union_mode: UnionMode::Sparse as i32,
                    type_ids: vec![],
                })),
            })),
            nullable: true,
            children: vec![],
            metadata: HashMap::default(),
        }],
        metadata: HashMap::default(),
    };
    let plan = PhysicalPlanNode {
        physical_plan_type: Some(PhysicalPlanType::Empty(EmptyExecNode {
            schema: Some(schema),
            partitions: 0,
        })),
    };
    let refused = refused(&plan);
    assert!(
        matches!(refused.find_root(), DataFusionError::Plan(message) if message.contains("Deserializing Union DataType")),
        "{refused}"
    );
}

/// Cell E (arrow-rs#10959's `compression.rs` hunks): a `ListValue`'s IPC-compressed data buffer
/// whose own declared uncompressed-length prefix understates the bytes the decompressor actually
/// produces by one. The length prefix is otherwise honest (unlike cell F's implausible bomb), so
/// this exercises the registry 59.2.0 mismatch check itself rather than an allocation bound.
///
/// Corrupted field: the LZ4 frame's 8-byte little-endian uncompressed-length prefix, decremented by
/// one below the length the frame actually decompresses to.
/// Registry 59.2.0 behaviour: `arrow-ipc-59.2.0/src/compression.rs`'s `decompress_lz4` decompresses
/// successfully, then its own `if ret.len() != decompressed_size` check returns
/// `Err(ArrowError::IpcError("Expected compressed length of {decompressed_size} got {}"))`- this
/// cell's planned fragment names the future fix's own wording instead, so it is red against that
/// check's current text and green once the fix lands.
#[test]
#[expect(
    clippy::little_endian_bytes,
    reason = "Arrow IPC's wire format mandates little-endian explicitly, which this cell reads and rewrites"
)]
fn a_node_refuses_a_list_value_whose_compressed_length_prefix_is_short() {
    // A highly compressible column (4096 repeated zeros) so the writer actually emits a real LZ4
    // frame rather than the `LENGTH_NO_COMPRESSED_DATA = -1` "stored uncompressed" sentinel it
    // falls back to when compression does not shrink the input.
    let arrow_schema = datafusion::arrow::datatypes::Schema::new(vec![datafusion::arrow::datatypes::Field::new(
        "col0",
        datafusion::arrow::datatypes::DataType::Int64,
        false,
    )]);
    let batch = datafusion::arrow::record_batch::RecordBatch::try_new(
        std::sync::Arc::new(arrow_schema.clone()),
        vec![std::sync::Arc::new(datafusion::arrow::array::Int64Array::from(vec![
            0i64;
            4096
        ]))],
    )
    .expect("a one-column Int64 batch builds");
    let proto_schema: Schema = (&arrow_schema).try_into().expect("an arrow schema encodes to proto");
    let ipc_generator = datafusion::arrow::ipc::writer::IpcDataGenerator {};
    let mut dictionary_tracker = datafusion::arrow::ipc::writer::DictionaryTracker::new(false);
    let write_options = datafusion::arrow::ipc::writer::IpcWriteOptions::default()
        .try_with_compression(Some(datafusion::arrow::ipc::CompressionType::LZ4_FRAME))
        .expect("LZ4 compression is a valid IPC write option");
    let (_, encoded) = ipc_generator
        .encode(
            &batch,
            &mut dictionary_tracker,
            &write_options,
            &mut datafusion::arrow::ipc::writer::IpcWriteContext::default(),
        )
        .expect("a record batch encodes to IPC");

    let mut arrow_data = encoded.arrow_data.clone();
    let message = datafusion::arrow::ipc::root_as_message(&encoded.ipc_message)
        .expect("the hand-produced header is a valid flatbuffer message");
    let record_batch_header = message
        .header_as_record_batch()
        .expect("a RecordBatch message carries a RecordBatch header");
    let buffers = record_batch_header
        .buffers()
        .expect("a one-column RecordBatch declares its buffers");
    let data_buffer = buffers.get(1);
    let offset = usize::try_from(data_buffer.offset()).expect("a test buffer offset fits in usize");
    let prefix = &mut arrow_data[offset..offset + 8];
    let declared = i64::from_le_bytes(prefix.try_into().expect("an 8-byte slice"));
    assert!(
        declared > 0,
        "the writer's own LZ4 frame must carry a positive declared length, got {declared}"
    );
    prefix.copy_from_slice(&(declared - 1).to_le_bytes());

    let value = ScalarValue {
        value: Some(ScalarValueKind::ListValue(ScalarNestedValue {
            ipc_message: encoded.ipc_message,
            arrow_data,
            schema: Some(proto_schema),
            dictionaries: vec![],
        })),
    };
    let refused = refused(&filter_literal(value));
    assert!(
        matches!(refused.find_root(), DataFusionError::Plan(message) if message.contains("exceeds advertised size")),
        "{refused}"
    );
}

/// `datafusion-proto-common`'s nested `ScalarValue` decode round-trips the peer's proto `Schema`
/// through IPC and, after `fix (3)` made `arrow-ipc`'s own decode refuse a negative
/// `FixedSizeBinary` width, was the one remaining caller of the panicking `fb_to_schema`. A
/// `ListValue` whose schema holds `FixedSizeBinary(-1)` reached that `.expect(..)` and aborted the
/// worker; it must now be refused instead.
#[test]
fn a_node_refuses_a_list_value_whose_schema_has_a_negative_fixed_size_binary_width() {
    let arrow_schema = datafusion::arrow::datatypes::Schema::new(vec![datafusion::arrow::datatypes::Field::new(
        "col0",
        datafusion::arrow::datatypes::DataType::FixedSizeBinary(-1),
        true,
    )]);
    let proto_schema: Schema = (&arrow_schema).try_into().expect("an arrow schema encodes to proto");
    let value = ScalarValue {
        value: Some(ScalarValueKind::ListValue(ScalarNestedValue {
            ipc_message: vec![],
            arrow_data: vec![],
            schema: Some(proto_schema),
            dictionaries: vec![],
        })),
    };
    let refused = refused(&filter_literal(value));
    assert!(
        matches!(refused.find_root(), DataFusionError::Plan(message) if message.contains("Invalid IPC schema while deserializing nested ScalarValue")),
        "{refused}"
    );
}

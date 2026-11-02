//! A peer's `Explain` or `Unnest` stage, encoded with a field the registry crate's decoder used to
//! `unwrap`, must be refused rather than abort the worker under `panic = "abort"`. `telekom/sutura#1680`.

use datafusion::error::DataFusionError;
use datafusion_distributed::{MaybeEncoded, SetPlanRequest, TaskKey};
use datafusion_proto::protobuf::physical_plan_node::PhysicalPlanType;
use datafusion_proto::protobuf::{
    EmptyExecNode, ExplainExecNode, PhysicalPlanNode, RecursionUnnestOption, Schema, StringifiedPlan, UnnestExecNode,
    UnnestOptions,
};
use futures_util::stream;
use prost::Message as _;
use tonic::codegen::http::HeaderMap;
use url::Url;

use super::{Authority, roomy, servers};
use crate::combine::Nodes;

#[test]
fn a_node_refuses_an_explain_stage_whose_plan_type_is_unset() {
    let authority = Authority::generate();
    let worker = Nodes::worker(roomy(), authority.identity(&authority.sign())).expect("a worker builds");
    // An Explain node with one stringified plan whose `plan_type` is unset. The registry crate's
    // decoder unwraps that field, which `panic = "abort"` turns into a stopped process.
    let plan = PhysicalPlanNode {
        physical_plan_type: Some(PhysicalPlanType::Explain(ExplainExecNode {
            schema: Some(Schema::default()),
            stringified_plans: vec![StringifiedPlan {
                plan_type: None,
                plan: String::new(),
            }],
            verbose: false,
        })),
    };
    #[expect(
        clippy::default_trait_access,
        reason = "the question id is a `uuid`, which this crate does not name; the decoder reads no id"
    )]
    let stage = SetPlanRequest {
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
    let refused = servers()
        .block_on(worker.coordinator_channel(HeaderMap::new(), stage, Box::pin(stream::empty())))
        .err()
        .expect("a node ran a stage it cannot decode");
    assert!(
        matches!(refused.find_root(), DataFusionError::Internal(message) if message.contains("plan_type")),
        "{refused}"
    );
}

#[test]
fn a_node_refuses_an_unnest_stage_whose_recursion_input_column_is_unset() {
    let authority = Authority::generate();
    let worker = Nodes::worker(roomy(), authority.identity(&authority.sign())).expect("a worker builds");
    // An Unnest node over an empty child, whose one recursion names neither an input nor an output
    // column. The decoder unwraps `input_column` before it ever reaches `output_column`, so leaving
    // both unset still isolates the refusal to `input_column` without constructing a `Column`.
    let child = PhysicalPlanNode {
        physical_plan_type: Some(PhysicalPlanType::Empty(EmptyExecNode {
            schema: Some(Schema::default()),
            partitions: 0,
        })),
    };
    let plan = PhysicalPlanNode {
        physical_plan_type: Some(PhysicalPlanType::Unnest(Box::new(UnnestExecNode {
            input: Some(Box::new(child)),
            schema: Some(Schema::default()),
            list_type_columns: vec![],
            struct_type_columns: vec![],
            options: Some(UnnestOptions {
                null_handling: 0,
                recursions: vec![RecursionUnnestOption {
                    output_column: None,
                    input_column: None,
                    depth: 0,
                }],
            }),
        }))),
    };
    #[expect(
        clippy::default_trait_access,
        reason = "the question id is a `uuid`, which this crate does not name; the decoder reads no id"
    )]
    let stage = SetPlanRequest {
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
    let refused = servers()
        .block_on(worker.coordinator_channel(HeaderMap::new(), stage, Box::pin(stream::empty())))
        .err()
        .expect("a node ran a stage it cannot decode");
    assert!(
        matches!(refused.find_root(), DataFusionError::Internal(message) if message.contains("input_column")),
        "{refused}"
    );
}

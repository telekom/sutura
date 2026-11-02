#![no_main]
//! A worker's own decode of a coordinator-sent stage - `data` as the protobuf bytes
//! `datafusion-distributed`'s worker channel decodes before anything in the plan executes
//! (`vendor/datafusion-distributed/src/worker/impl_coordinator_channel.rs`: `request.plan.decode`,
//! which runs `decode_execution_plan`). That function is `pub(crate)`, so this harness rebuilds it
//! from the same public building blocks it composes, in the same order, over a session built the
//! same minimal way the worker's own channel builds one when nothing registers a custom session
//! builder (`SessionStateBuilder::new().with_default_features()`, same file, line 68) -
//! `crates/sutura-exec-datafusion/src/combine/across.rs`'s `worker` registers no user codec or
//! function, so neither does this harness.
//!
//! **The limit.** This rebuilds the decode, not the worker's own call: it does not reach
//! `reads_no_source`, the sampler or execution past the decode
//! (`SamplerExec::kick_off_first_sampler` dials a peer, which this harness must not trigger), and a
//! codec or function a later version of the worker's session registers is not followed here -
//! nothing checks that this harness's session stays in step with the worker's.

use std::sync::LazyLock;

use datafusion::execution::SessionStateBuilder;
use datafusion::execution::TaskContext;
use datafusion_distributed::DistributedCodec;
use datafusion_proto::bytes::physical_plan_from_bytes_with_proto_converter;
use datafusion_proto::physical_plan::DeduplicatingProtoConverter;
use libfuzzer_sys::fuzz_target;

/// Built once: a session with no registered codec or function, matching the worker's own when
/// nothing customizes its session builder.
static TASK_CTX: LazyLock<std::sync::Arc<TaskContext>> =
    LazyLock::new(|| SessionStateBuilder::new().with_default_features().build().task_ctx());

fuzz_target!(|data: &[u8]| {
    let codec = DistributedCodec::new_combined_with_user(TASK_CTX.session_config());
    let converter = DeduplicatingProtoConverter::default();
    let _ = physical_plan_from_bytes_with_proto_converter(data, &TASK_CTX, &codec, &converter);
});

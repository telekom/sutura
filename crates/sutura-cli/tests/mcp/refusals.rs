//! Refusals: a governance refusal arrives as a tool result rather than a protocol error.

use sutura_app::Capability;

use crate::harness::{a_metric_this_catalog_does_not_define, spawn};

#[test]
fn a_refusal_arrives_as_a_tool_result_rather_than_a_protocol_error() {
    // *Refusal is a result, not an error*, on the wire and on the composed process. The bug this
    // prevents is a governance refusal reaching an agent as a JSON-RPC error, which a client
    // retries and a model reads as an outage rather than as an answer it may not have.
    let mut agent = spawn();
    drop(agent.initialize());
    // `request` already fails if the reply carried an `error`, which is half the claim.
    let result = agent.call(Capability::AskMetric, &a_metric_this_catalog_does_not_define());
    assert_ne!(
        result["isError"],
        serde_json::json!(true),
        "a refusal was reported as a failure: {result}"
    );

    let content = &result["structuredContent"];
    assert_eq!(content["outcome"], "refusal", "{result}");
    // The code, not the sentence: the code is the contract a client branches on and the sentence
    // is written for a person. `metric_unknown` is `RefusalReason::MetricUnknown` in snake_case,
    // which is the derivation `sutura_mcp::refusal`'s own test pins.
    assert_eq!(content["reason"]["code"], "metric_unknown", "{result}");
    assert!(
        content["reason"]["detail"].as_str().is_some_and(|detail| !detail.is_empty()),
        "a refusal arrived with no sentence for a person: {result}"
    );

    assert!(agent.close().success(), "the process did not exit cleanly");
}

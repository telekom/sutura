//! The `run_sql` malformed-input path, over the wire through a real client.
//!
//! `crate::NotServed` has no cell, and is held by review alone: `serve_stdio` hardcodes
//! `rmcp::transport::stdio()`, so `Handshake` is reachable only by building the variant in the test,
//! which asserts nothing `serve_stdio` returned; `Interrupted` wraps the `JoinError` of a panicked
//! service task, and nothing this crate publishes makes that task panic.

use rmcp::ServiceError;
use rmcp::model::ErrorCode;

/// A `run_sql` call carrying a field the tool does not declare is a named parse error, not a call
/// answered with the extra field dropped - and the refusal does not echo the caller's key.
#[tokio::test]
async fn a_run_sql_call_with_an_undeclared_field_is_a_named_parse_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(super::raw(&serde_json::json!({
            "statement": "select 1",
            "table": "orders"
        })))
        .await
        .expect_err("the run_sql tool declares one argument");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    // `RenderedCause::outer_only`: the fixed sentence, never the serde cause that names the key.
    assert!(data.message.contains("not a statement"), "{}", data.message);
    assert!(!data.message.contains("table"), "{}", data.message);
    drop(client.cancel().await);
}

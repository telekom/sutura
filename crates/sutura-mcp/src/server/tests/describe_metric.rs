//! `describe_catalog`'s `metric` argument, over the wire through a real client -
//! `github.com/telekom/sutura#2071`'s second ask.
//!
//! A file of its own for the same 1000-line reason [`super::deadline`] is.

use rmcp::ServiceError;
use rmcp::model::{CallToolRequestParams, ErrorCode};
use sutura_app::Capability;
use sutura_app::surface::LocalService;

use crate::testing;

/// The catalog tool, narrowed to one named metric.
fn describe_metric(name: &str) -> CallToolRequestParams {
    super::call(Capability::DescribeCatalog.id(), &serde_json::json!({ "metric": name }))
}

/// A service over [`testing::TwoMetricOwningModelsCatalog`] - the metric-pool twin of
/// [`super::describe_model::two_metric_owning_models_service`]. `metric` narrowing needs a SECOND
/// metric to actually filter out, over a model this caller can see either way, or the absence this
/// test asserts is restated rather than proven.
fn two_metric_owning_models_service() -> super::CertifiedService {
    LocalService::start(
        &testing::TwoMetricOwningModelsCatalog,
        testing::fake_warehouse(),
        std::sync::Arc::new(testing::CountingSink::default()),
        testing::broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the two-metric-owning-model fixture bundle validates")
}

/// The catalog tool, naming both a model and a metric at once - refused, never one silently
/// winning over the other.
fn describe_model_and_metric(model: &str, metric: &str) -> CallToolRequestParams {
    super::call(
        Capability::DescribeCatalog.id(),
        &serde_json::json!({ "model": model, "metric": metric }),
    )
}

/// `metric` narrows `describe_catalog` to one named metric alone, never the whole catalog's worth -
/// over a bundle where another metric exists too, so its absence is actually proven, not merely
/// restated because it never had anything to filter in the first place.
#[tokio::test]
async fn describe_catalog_with_a_metric_argument_answers_only_that_metric() {
    let client = super::connected(two_metric_owning_models_service()).await;
    let result = client
        .call_tool(describe_metric("revenue"))
        .await
        .expect("a real, visible metric is not a protocol error");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let text = super::text_of(&result);
    assert!(text.contains("revenue"), "{text}");
    assert!(!text.contains("customer_count"), "{text}");
    drop(client.cancel().await);
}

/// A `metric` argument that parses but names no metric this caller may see is refused - a
/// JSON-RPC error, never an empty listing that looks like a narrowing that happened.
#[tokio::test]
async fn describe_catalog_with_an_unknown_metric_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_metric("does_not_exist"))
        .await
        .expect_err("an unknown metric is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("does_not_exist"), "{}", data.message);
    drop(client.cancel().await);
}

/// `model` and `metric` given together are refused outright, before either name is even looked up -
/// a caller must narrow one way or the other, not both.
#[tokio::test]
async fn describe_catalog_with_both_model_and_metric_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_model_and_metric("orders", "revenue"))
        .await
        .expect_err("model and metric together are refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(
        data.message.contains("model") && data.message.contains("metric"),
        "{}",
        data.message
    );
    drop(client.cancel().await);
}

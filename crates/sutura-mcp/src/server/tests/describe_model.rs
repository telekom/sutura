//! `describe_catalog`'s `model` argument, over the wire through a real client -
//! `github.com/telekom/sutura#2071`'s second ask.
//!
//! A file of its own for the same 1000-line reason [`super::deadline`] is.

use rmcp::ServiceError;
use rmcp::model::{CallToolRequestParams, ErrorCode};
use sutura_app::Capability;
use sutura_app::surface::LocalService;

use crate::testing;

/// The catalog tool, narrowed to one named model.
fn describe_model(name: &str) -> CallToolRequestParams {
    super::call(Capability::DescribeCatalog.id(), &serde_json::json!({ "model": name }))
}

/// A service over [`testing::TwoMetricOwningModelsCatalog`] - this file's own discriminating test
/// needs the EXCLUDED model to own a metric of its own, so asserting its absence proves the filter
/// ran rather than restating that the excluded model had nothing to filter in the first place.
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

/// `model` narrows `describe_catalog` to one named model's own metrics, never the whole catalog's -
/// over a bundle where the OTHER model owns its own metric too, so its absence is actually proven,
/// not merely restated because it never had anything to filter in the first place.
#[tokio::test]
async fn describe_catalog_with_a_model_argument_answers_only_that_models_metrics() {
    let client = super::connected(two_metric_owning_models_service()).await;
    let result = client
        .call_tool(describe_model("orders"))
        .await
        .expect("a real, visible model is not a protocol error");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let text = super::text_of(&result);
    assert!(text.contains("revenue"), "{text}");
    assert!(!text.contains("customer_count"), "{text}");
    assert!(!text.contains("customers"), "{text}");
    drop(client.cancel().await);
}

/// A `model` argument that parses but names no model this caller may see is refused - a JSON-RPC
/// error, never an empty listing that looks like a narrowing that happened.
#[tokio::test]
async fn describe_catalog_with_an_unknown_model_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_model("does_not_exist"))
        .await
        .expect_err("an unknown model is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("does_not_exist"), "{}", data.message);
    drop(client.cancel().await);
}

/// A `model` argument that does not even parse as a model name is refused the same way - never
/// confused with the unknown-but-well-formed case above.
#[tokio::test]
async fn describe_catalog_with_a_malformed_model_name_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_model("not a model"))
        .await
        .expect_err("a malformed model name is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(
        data.message.contains("letters, digits and underscore"),
        "expected the parse cause, got {}",
        data.message
    );
    drop(client.cancel().await);
}

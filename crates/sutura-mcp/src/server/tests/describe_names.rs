//! `describe_catalog`'s `names: true` index, `model_prefix` narrowing and the refusals that guard
//! them - `github.com/telekom/sutura#2083`'s own ask, over a real MCP client.
//!
//! A file of its own for the same 1000-line reason [`super::deadline`] is.

use rmcp::ServiceError;
use rmcp::model::{CallToolRequestParams, ErrorCode};
use sutura_app::Capability;
use sutura_app::catalog_budget::MAX_CATALOG_LISTING_BYTES;
use sutura_app::surface::LocalService;

use crate::testing;

/// The catalog tool, called with an arbitrary arguments object - this file's own calls all go
/// through here so every one is visibly a `describe_catalog` call and nothing else.
fn describe_catalog(args: &serde_json::Value) -> CallToolRequestParams {
    super::call(Capability::DescribeCatalog.id(), args)
}

/// A service over [`testing::TwoMetricOwningModelsCatalog`] - two models (`orders`, `customers`),
/// each owning its own metric, which is enough to prove `model_prefix` narrows both pools without
/// an excluded entry merely having had nothing to filter in the first place.
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

/// `names: true` answers a names-only index of both pools - name and table for a model, name and
/// owning model for a metric - never the full listing (no description, no columns).
#[tokio::test]
async fn names_true_answers_a_names_only_index_of_models_and_metrics() {
    let client = super::connected(two_metric_owning_models_service()).await;
    let result = client
        .call_tool(describe_catalog(&serde_json::json!({ "names": true })))
        .await
        .expect("names: true is a valid question");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let json = result
        .structured_content
        .clone()
        .expect("a names index carries structured content");
    let model_names: Vec<&str> = json["model_names"]
        .as_array()
        .expect("model_names is a list")
        .iter()
        .map(|entry| entry["name"].as_str().expect("a name is a string"))
        .collect();
    assert_eq!(model_names, vec!["customers", "orders"], "{json}");
    assert!(json["model_names"][0].get("description").is_none(), "{json}");
    let metric_names: Vec<&str> = json["metric_names"]
        .as_array()
        .expect("metric_names is a list")
        .iter()
        .map(|entry| entry["name"].as_str().expect("a name is a string"))
        .collect();
    assert_eq!(metric_names, vec!["customer_count", "revenue"], "{json}");
    assert!(json.get("next").is_none(), "both models fit on one page: {json}");
    assert!(json.get("next_metric").is_none(), "both metrics fit on one page: {json}");
    drop(client.cancel().await);
}

/// Every visible model is reached by walking `names: true`'s `after` cursor to exhaustion, even
/// with far more models than `sutura_app::catalog_budget::MAX_OMITTED_NAMES` ever could have named
/// - the paged index has no cap on how many PAGES it takes, only on each page's own byte cost. Each
/// page, both halves, stays under the shared bound.
#[tokio::test]
async fn walking_after_to_exhaustion_reaches_every_model_past_the_omission_cap() {
    const MODEL_COUNT: usize = 2500;
    let bundle = testing::bundle_with_many_models_and_metrics_at_max_name_len(MODEL_COUNT, 0);
    let service = LocalService::start(
        &testing::FixedBundleCatalog(bundle),
        testing::fake_warehouse(),
        std::sync::Arc::new(testing::CountingSink::default()),
        testing::broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the many-models fixture bundle validates");
    let client = super::connected(service).await;

    let mut seen = std::collections::BTreeSet::new();
    let mut after: Option<String> = None;
    let mut pages = 0_u32;
    loop {
        pages += 1;
        assert!(pages < 10_000, "the index never terminated - a cursor is not advancing");
        let mut args = serde_json::json!({ "names": true });
        if let Some(cursor) = &after {
            args["after"] = serde_json::Value::String(cursor.clone());
        }
        let result = client
            .call_tool(describe_catalog(&args))
            .await
            .expect("names: true, paged, is a valid question");
        assert_ne!(result.is_error, Some(true), "{result:?}");
        let text = super::text_of(&result);
        let json = result.structured_content.expect("a names index carries structured content");
        let structured_bytes = serde_json::to_vec(&json).expect("structured content serializes").len();
        assert!(
            text.len() + structured_bytes <= MAX_CATALOG_LISTING_BYTES,
            "page {pages}'s text ({} bytes) plus structured ({structured_bytes} bytes) half overshot the shared bound",
            text.len()
        );
        for entry in json["model_names"].as_array().expect("model_names is a list") {
            seen.insert(entry["name"].as_str().expect("a name is a string").to_owned());
        }
        match json.get("next").and_then(serde_json::Value::as_str) {
            Some(cursor) => after = Some(cursor.to_owned()),
            None => break,
        }
    }
    assert!(pages > 1, "the fixture must be large enough to force more than one page");
    assert_eq!(seen.len(), MODEL_COUNT, "every model must be reached exactly once");
    drop(client.cancel().await);
}

/// The metric half of the same index walks the same way, via `after_metric` - the metrics-side
/// twin of [`walking_after_to_exhaustion_reaches_every_model_past_the_omission_cap`].
#[tokio::test]
async fn walking_after_metric_to_exhaustion_reaches_every_metric_past_the_omission_cap() {
    const METRIC_COUNT: usize = 2500;
    let bundle = testing::bundle_with_many_models_and_metrics_at_max_name_len(1, METRIC_COUNT);
    let service = LocalService::start(
        &testing::FixedBundleCatalog(bundle),
        testing::fake_warehouse(),
        std::sync::Arc::new(testing::CountingSink::default()),
        testing::broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the many-metrics fixture bundle validates");
    let client = super::connected(service).await;

    let mut seen = std::collections::BTreeSet::new();
    let mut after_metric: Option<String> = None;
    let mut pages = 0_u32;
    loop {
        pages += 1;
        assert!(pages < 10_000, "the index never terminated - a cursor is not advancing");
        let mut args = serde_json::json!({ "names": true });
        if let Some(cursor) = &after_metric {
            args["after_metric"] = serde_json::Value::String(cursor.clone());
        }
        let result = client
            .call_tool(describe_catalog(&args))
            .await
            .expect("names: true, paged, is a valid question");
        assert_ne!(result.is_error, Some(true), "{result:?}");
        let text = super::text_of(&result);
        let json = result.structured_content.expect("a names index carries structured content");
        let structured_bytes = serde_json::to_vec(&json).expect("structured content serializes").len();
        assert!(
            text.len() + structured_bytes <= MAX_CATALOG_LISTING_BYTES,
            "page {pages}'s text ({} bytes) plus structured ({structured_bytes} bytes) half overshot the shared bound",
            text.len()
        );
        for entry in json["metric_names"].as_array().expect("metric_names is a list") {
            seen.insert(entry["name"].as_str().expect("a name is a string").to_owned());
        }
        match json.get("next_metric").and_then(serde_json::Value::as_str) {
            Some(cursor) => after_metric = Some(cursor.to_owned()),
            None => break,
        }
    }
    assert!(pages > 1, "the fixture must be large enough to force more than one page");
    assert_eq!(seen.len(), METRIC_COUNT, "every metric must be reached exactly once");
    drop(client.cancel().await);
}

/// Item 1's own test: a single bundle with BOTH more than 200 models AND more than 200 metrics,
/// walking BOTH `after` and `after_metric` to completion - the scenario a single-pool fixture never
/// exercises, since a caller continuing ONLY `after_metric` (no `after`) must not re-walk the model
/// pool from scratch and starve the metric half to roughly one entry per call
/// (`github.com/telekom/sutura#2083`'s review).
#[tokio::test]
async fn walking_both_halves_to_exhaustion_reaches_every_model_and_every_metric() {
    const MODEL_COUNT: usize = 250;
    const METRIC_COUNT: usize = 250;
    let bundle = testing::bundle_with_many_models_and_metrics_at_max_name_len(MODEL_COUNT, METRIC_COUNT);
    let service = LocalService::start(
        &testing::FixedBundleCatalog(bundle),
        testing::fake_warehouse(),
        std::sync::Arc::new(testing::CountingSink::default()),
        testing::broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the many-models-and-metrics fixture bundle validates");
    let client = super::connected(service).await;

    let mut seen_models = std::collections::BTreeSet::new();
    let mut seen_metrics = std::collections::BTreeSet::new();
    let mut after: Option<String> = None;
    let mut models_done = false;
    let mut pages = 0_u32;
    loop {
        pages += 1;
        assert!(pages < 10_000, "the index never terminated - a cursor is not advancing");
        let mut args = serde_json::json!({ "names": true });
        // Walk the model half to exhaustion first, THEN walk the metric half alone - the exact
        // transition (`after: None, after_metric: Some(_)`) the review flags as starved.
        if !models_done {
            if let Some(cursor) = &after {
                args["after"] = serde_json::Value::String(cursor.clone());
            }
        } else if let Some(cursor) = &after {
            args["after_metric"] = serde_json::Value::String(cursor.clone());
        }
        let result = client
            .call_tool(describe_catalog(&args))
            .await
            .expect("names: true, paged, is a valid question");
        assert_ne!(result.is_error, Some(true), "{result:?}");
        let text = super::text_of(&result);
        let json = result.structured_content.expect("a names index carries structured content");
        let structured_bytes = serde_json::to_vec(&json).expect("structured content serializes").len();
        assert!(
            text.len() + structured_bytes <= MAX_CATALOG_LISTING_BYTES,
            "page {pages}'s text ({} bytes) plus structured ({structured_bytes} bytes) half overshot the shared bound",
            text.len()
        );
        for entry in json["model_names"].as_array().expect("model_names is a list") {
            seen_models.insert(entry["name"].as_str().expect("a name is a string").to_owned());
        }
        for entry in json["metric_names"].as_array().expect("metric_names is a list") {
            seen_metrics.insert(entry["name"].as_str().expect("a name is a string").to_owned());
        }
        if models_done {
            match json.get("next_metric").and_then(serde_json::Value::as_str) {
                Some(cursor) if !cursor.is_empty() => after = Some(cursor.to_owned()),
                // An empty-string sentinel means "retry with no cursor" - the deferral this fix uses
                // when the model half just spent the budget this very call.
                Some(_) => after = Some(String::new()),
                None => break,
            }
        } else if let Some(cursor) = json.get("next").and_then(serde_json::Value::as_str) {
            after = Some(cursor.to_owned());
        } else {
            models_done = true;
            after = None;
        }
    }
    assert!(
        pages > 2,
        "the fixture must be large enough to force more than one page per half"
    );
    assert_eq!(seen_models.len(), MODEL_COUNT, "every model must be reached exactly once");
    assert_eq!(seen_metrics.len(), METRIC_COUNT, "every metric must be reached exactly once");
    drop(client.cancel().await);
}

/// `model_prefix` narrows the FULL (non-`names`) listing's physical-schema candidates to models
/// starting with that prefix - the excluded model owns its own metric too, so its absence actually
/// proves the filter ran.
#[tokio::test]
async fn model_prefix_narrows_the_full_listings_models() {
    let client = super::connected(two_metric_owning_models_service()).await;
    let result = client
        .call_tool(describe_catalog(&serde_json::json!({ "model_prefix": "ord" })))
        .await
        .expect("a well-formed model_prefix is a valid question");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let text = super::text_of(&result);
    assert!(text.contains("orders"), "{text}");
    assert!(!text.contains("customers"), "{text}");
    drop(client.cancel().await);
}

/// `model_prefix` narrows the full listing's METRICS pool too, by each metric's own `model` - the
/// metrics-side half of [`model_prefix_narrows_the_full_listings_models`].
#[tokio::test]
async fn model_prefix_narrows_the_full_listings_metrics() {
    let client = super::connected(two_metric_owning_models_service()).await;
    let result = client
        .call_tool(describe_catalog(&serde_json::json!({ "model_prefix": "ord" })))
        .await
        .expect("a well-formed model_prefix is a valid question");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let text = super::text_of(&result);
    assert!(text.contains("revenue"), "{text}");
    assert!(!text.contains("customer_count"), "{text}");
    drop(client.cancel().await);
}

/// `names: true` alongside `model` is refused - a different mode, not a third narrowing.
#[tokio::test]
async fn names_with_model_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(&serde_json::json!({ "names": true, "model": "orders" })))
        .await
        .expect_err("names with model is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("names"), "{}", data.message);
    drop(client.cancel().await);
}

/// `names: true` alongside `metric` is refused the same way.
#[tokio::test]
async fn names_with_metric_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(&serde_json::json!({ "names": true, "metric": "revenue" })))
        .await
        .expect_err("names with metric is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("names"), "{}", data.message);
    drop(client.cancel().await);
}

/// `model_prefix` alongside `model` is refused - both already narrow, so a prefix on top names
/// nothing new.
#[tokio::test]
async fn model_prefix_with_model_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(
            &serde_json::json!({ "model_prefix": "ord", "model": "orders" }),
        ))
        .await
        .expect_err("model_prefix with model is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("model_prefix"), "{}", data.message);
    drop(client.cancel().await);
}

/// `model_prefix` alongside `metric` is refused the same way.
#[tokio::test]
async fn model_prefix_with_metric_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(
            &serde_json::json!({ "model_prefix": "ord", "metric": "revenue" }),
        ))
        .await
        .expect_err("model_prefix with metric is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("model_prefix"), "{}", data.message);
    drop(client.cancel().await);
}

/// `after` without `names: true` is refused - a cursor into an index that was never asked for.
#[tokio::test]
async fn after_without_names_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(&serde_json::json!({ "after": "orders" })))
        .await
        .expect_err("after without names is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("names"), "{}", data.message);
    drop(client.cancel().await);
}

/// `after_metric` without `names: true` is refused the same way.
#[tokio::test]
async fn after_metric_without_names_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(&serde_json::json!({ "after_metric": "revenue" })))
        .await
        .expect_err("after_metric without names is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("names"), "{}", data.message);
    drop(client.cancel().await);
}

/// An empty `model_prefix` is refused - it would match every name, which is already what omitting
/// it does, so it is refused rather than silently accepted as a no-op.
#[tokio::test]
async fn empty_model_prefix_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(&serde_json::json!({ "model_prefix": "" })))
        .await
        .expect_err("an empty model_prefix is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("model_prefix"), "{}", data.message);
    drop(client.cancel().await);
}

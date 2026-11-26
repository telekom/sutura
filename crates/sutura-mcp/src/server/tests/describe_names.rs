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
    assert!(json.get("next").is_none(), "both halves fit on one page: {json}");
    drop(client.cancel().await);
}

/// A single bundle with BOTH more than 200 models AND more than 200 metrics, walked to completion
/// with ONE sequential `after` cursor - models first, then metrics, a single stream, crossing from
/// one half into the other within the walk rather than needing a second cursor to address either
/// half independently (`github.com/telekom/sutura#2083`'s review, round 3). Every page, both
/// halves summed, stays under the shared bound.
#[tokio::test]
async fn walking_after_to_exhaustion_reaches_every_model_and_every_metric() {
    const MODEL_COUNT: usize = 700;
    const METRIC_COUNT: usize = 700;
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
    let mut pages = 0_u32;
    let mut crossed = false;
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
        let model_entries = json["model_names"].as_array().expect("model_names is a list");
        let metric_entries = json["metric_names"].as_array().expect("metric_names is a list");
        if !model_entries.is_empty() && !metric_entries.is_empty() {
            crossed = true;
        }
        for entry in model_entries {
            seen_models.insert(entry["name"].as_str().expect("a name is a string").to_owned());
        }
        for entry in metric_entries {
            seen_metrics.insert(entry["name"].as_str().expect("a name is a string").to_owned());
        }
        match json.get("next").and_then(serde_json::Value::as_str) {
            Some(cursor) => after = Some(cursor.to_owned()),
            None => break,
        }
    }
    assert!(
        pages > 2,
        "the fixture must be large enough to force more than one page per half"
    );
    assert_eq!(seen_models.len(), MODEL_COUNT, "every model must be reached exactly once");
    assert_eq!(seen_metrics.len(), METRIC_COUNT, "every metric must be reached exactly once");
    assert!(
        crossed,
        "the walk must cross from models into metrics within one page at least once"
    );
    drop(client.cancel().await);
}

/// A model page that uses the whole shared budget leaves behind less room than one metric entry
/// costs - round 4's `first_does_not_fit` guard (`github.com/telekom/sutura#2083`). Every model
/// entry in [`testing::bundle_with_many_models_and_metrics_at_max_name_len`] costs the same number
/// of bytes, so the largest model count that still lands entirely on one page always leaves a
/// remainder smaller than one more entry - a division remainder, by construction - which is exactly
/// the crossing case the guard exists for. Binary-searches for that count against the real server
/// rather than hand-deriving the byte budget, so the test does not silently stop proving anything
/// the moment an envelope byte changes elsewhere.
#[tokio::test]
async fn a_model_page_that_spends_the_whole_budget_admits_no_metric_onto_it() {
    async fn models_fit_one_page(model_count: usize) -> bool {
        let bundle = testing::bundle_with_many_models_and_metrics_at_max_name_len(model_count, 1);
        let service = LocalService::start(
            &testing::FixedBundleCatalog(bundle),
            testing::fake_warehouse(),
            std::sync::Arc::new(testing::CountingSink::default()),
            testing::broker(),
            sutura_domain::plan::RefusingCombiner,
            1 << 30,
        )
        .expect("the probing fixture bundle validates");
        let client = super::connected(service).await;
        let result = client
            .call_tool(describe_catalog(&serde_json::json!({ "names": true })))
            .await
            .expect("names: true, probing, is a valid question");
        let json = result.structured_content.expect("a names index carries structured content");
        let fits = json["model_names"].as_array().expect("model_names is a list").len() == model_count;
        drop(client.cancel().await);
        fits
    }

    let (mut lo, mut hi) = (1_usize, 4_000_usize);
    assert!(
        models_fit_one_page(lo).await,
        "even a single max-length model must fit on its own page"
    );
    assert!(
        !models_fit_one_page(hi).await,
        "4000 max-length models must not fit on one page - widen the search"
    );
    while hi - lo > 1 {
        let mid = lo + ((hi - lo) >> 1);
        if models_fit_one_page(mid).await {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let model_count = lo;

    let bundle = testing::bundle_with_many_models_and_metrics_at_max_name_len(model_count, 2);
    let service = LocalService::start(
        &testing::FixedBundleCatalog(bundle),
        testing::fake_warehouse(),
        std::sync::Arc::new(testing::CountingSink::default()),
        testing::broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the boundary fixture bundle validates");
    let client = super::connected(service).await;
    let result = client
        .call_tool(describe_catalog(&serde_json::json!({ "names": true })))
        .await
        .expect("names: true at the boundary is a valid question");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let json = result.structured_content.expect("a names index carries structured content");
    assert_eq!(
        json["model_names"].as_array().expect("model_names is a list").len(),
        model_count,
        "{json}"
    );
    assert!(
        json["metric_names"].as_array().expect("metric_names is a list").is_empty(),
        "the leftover budget must not fit even one metric entry: {json}"
    );
    assert_eq!(json["next"].as_str(), Some("metric:"), "{json}");
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

/// A malformed `after` - neither `model:<name>` nor `metric:<name>` - is refused rather than read
/// as "start from the beginning".
#[tokio::test]
async fn malformed_after_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(&serde_json::json!({ "names": true, "after": "orders" })))
        .await
        .expect_err("a malformed after cursor is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("cursor"), "{}", data.message);
    drop(client.cancel().await);
}

/// An `after` with a recognized prefix but a name that is not itself a valid identifier is refused
/// the same way - `NamesCursor::parse` validates the name part, not just the prefix
/// (`github.com/telekom/sutura#2083` round 4).
#[tokio::test]
async fn an_after_with_an_invalid_identifier_name_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(
            &serde_json::json!({ "names": true, "after": "model:not an identifier!!" }),
        ))
        .await
        .expect_err("an after cursor whose name is not a valid identifier is refused, not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("cursor"), "{}", data.message);
    drop(client.cancel().await);
}

/// An empty name after `model:` is refused too - the empty-name sentinel is `metric:` only.
#[tokio::test]
async fn an_after_with_an_empty_model_name_is_refused_as_a_protocol_error() {
    let client = super::connected(super::certified_service()).await;
    let error = client
        .call_tool(describe_catalog(&serde_json::json!({ "names": true, "after": "model:" })))
        .await
        .expect_err("an empty model name is refused, not read as a sentinel");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("cursor"), "{}", data.message);
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

//! `catalog_visibility: per-caller` on a `foundry_login` source behind the served HTTP surface.
//!
//! The REAL leg-1 gate, the broker `build_broker` composes, and the listing
//! `super::super::visibility::declared` composes from the same settings, dialling a loopback fake of
//! Foundry's object-type list - `docs/adr/20261125083027-catalog-visibility-per-caller.md`.
//!
//! Gated on `bigquery` as well as `foundry` only to reuse `support`'s leg-1 fixtures, as
//! `foundry_login` is.
//!
//! **What this does not show:** any Foundry tenant listing object types under a brokered token, or
//! listing them differently for two users. Both fakes answer what they are scripted to.

use std::collections::BTreeSet;
use std::sync::Arc;

use sutura_config::{Environment, Settings, Sources};
use sutura_dev::issuer::{MockIssuer, PublishedKeySet};
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Definitions, Description, Metric, Model};
use sutura_domain::identity::Secret;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, QualifiedTable, SourceName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_exec_foundry::{FoundryTransport, FoundryWarehouse, OntologyIdentifier, SqlRequest};
use sutura_http_client::test_support::{FakeServer, Scripted};

use super::support::{accepted_by, catalog_of, direct_overlay};

const CALLERS: [&str; 2] = ["analyst-a@example.com", "analyst-b@example.com"];

/// One object type, the model bound to it, and the one metric over that model.
struct Object {
    api_name: &'static str,
    model: &'static str,
    metric: &'static str,
}

const OBJECTS: [Object; 2] = [
    Object {
        api_name: "Customer",
        model: "customers",
        metric: "recurring_revenue",
    },
    Object {
        api_name: "Order",
        model: "orders",
        metric: "order_count",
    },
];

fn stored_for(subject: &str) -> String {
    format!("foundry-user-token.{}", subject.replace(['@', '.'], "-"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs()
}

fn stored(token: &str) -> serde_json::Value {
    serde_json::json!({
        "access_token": token,
        "token_type": "Bearer",
        "accessTokenExpiration": now() + 300,
    })
}

/// Foundry's object-type list, holding `api_names`.
fn listed(api_names: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "data": api_names.iter().map(|name| serde_json::json!({
            "apiName": name, "displayName": name, "primaryKey": "key", "properties": {}
        })).collect::<Vec<_>>(),
    })
}

struct SecretFile(std::path::PathBuf);

impl Drop for SecretFile {
    fn drop(&mut self) {
        drop(std::fs::remove_file(&self.0));
    }
}

/// No question runs in these cells; a transport that is reached fails them.
#[derive(Debug)]
struct NeverAsked;

#[derive(Debug, thiserror::Error)]
#[error("a catalog read reached the Ontology SQL transport")]
struct Reached;

impl FoundryTransport for NeverAsked {
    type Error = Reached;

    fn execute(&self, _request: &SqlRequest, _token: &Secret, _deadline: Deadline) -> Result<Vec<u8>, Self::Error> {
        Err(Reached)
    }

    fn source_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }
}

/// Two models on `warehouse`, one per [`OBJECTS`] row, each with its metric.
fn two_objects() -> PinnedDefinitions {
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let mut models = Vec::new();
    let mut metrics = Vec::new();
    for Object { api_name, model, metric } in OBJECTS {
        let name = ModelName::parse(model).expect("a test model is a model");
        models.push(Model::new(
            name.clone(),
            SourceName::parse("warehouse").expect("a test source is a source"),
            QualifiedTable::parse(api_name).expect("a test table is a table path"),
            BTreeSet::from([column("key"), column("on")]),
            Description::default(),
        ));
        metrics.push(
            Metric::new(
                MetricName::parse(metric).expect("a test metric is a metric"),
                name,
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Count, column("key")))),
                Vec::new(),
                column("on"),
                BTreeSet::from([Grain::Month]),
                Vec::new(),
                None,
                Description::default(),
                Audience::Open,
            )
            .expect("no dimensions to duplicate"),
        );
    }
    let definitions = Definitions::assemble(models, vec![], metrics).expect("the test bundle is consistent");
    let produced = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(produced),
        ),
    )
    .expect("the test definitions hash")
}

struct Served {
    app: axum::Router,
    issuer: MockIssuer,
    _published: PublishedKeySet,
    _secret: SecretFile,
}

/// The served router over one `foundry_login` source whose broker is `idp` and whose endpoint is
/// `foundry`, declaring `catalog_visibility: per-caller` when `narrowed`.
fn served(case: &str, idp: &FakeServer, foundry: &FakeServer, narrowed: bool) -> Served {
    let issuer = MockIssuer::generating("https://issuer.example.com", "https://sutura.example.com", "the-current-key")
        .expect("a mock issuer generates a key pair");
    let published = PublishedKeySet::of(&issuer, case).expect("the key set publishes");
    let secret = SecretFile(std::env::temp_dir().join(format!("sutura-{case}-client-secret-{}", std::process::id())));
    std::fs::write(&secret.0, "a-test-client-secret\n").expect("the client secret file is writable");
    let visibility = if narrowed {
        "    catalog_visibility: \"per-caller\"\n"
    } else {
        ""
    };
    let overlay = format!(
        "{}  identity: \"multi-user\"\nsources:\n  warehouse:\n    kind: \"foundry\"\n    endpoint: \"{}\"\n    \
         ontology: \"sales_ontology\"\n    posture: \"impersonation-at-source\"\n{visibility}    foundry_login:\n      \
         token_endpoint: \"{}/realms/example/broker/foundry/token\"\n      client_id: \"sutura\"\n      \
         client_secret_file: \"{}\"\n",
        direct_overlay(&issuer, &published.path().to_string_lossy()),
        foundry.endpoint(),
        idp.endpoint(),
        secret.0.display(),
    );
    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay)).expect("the overlay loads");
    let broker =
        super::super::broker::build_broker(settings.sources(), None).expect("a direct deployment admits a foundry_login");
    let warehouse = FoundryWarehouse::each_caller(
        SourceName::parse("warehouse").expect("a test source is a source"),
        OntologyIdentifier::parse("sales_ontology").expect("the ontology parses"),
        NeverAsked,
        sutura_domain::warehouse::ResultBudget::of_bytes(core::num::NonZeroUsize::MAX),
    );
    let service = sutura_app::surface::LocalService::start(
        &catalog_of(two_objects()),
        sutura_app::Warehouses::of(warehouse),
        sutura_runtime::TracingAuditSink::new(),
        broker,
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the test bundle validates");
    // What `compose::started` attaches, from the same settings.
    let service: Arc<dyn sutura_app::surface::Surface> =
        match super::super::visibility::declared(&settings).expect("the declared listing composes") {
            Some(visibility) => Arc::new(service.with_catalog_visibility(visibility)),
            None => Arc::new(service),
        };
    let gate = sutura_http::InboundGate::from_declaration(
        settings
            .security()
            .inbound()
            .first()
            .and_then(sutura_config::InboundVerifier::identity)
            .expect("a direct deployment"),
    )
    .expect("a published key set builds a gate");
    let admission = sutura_runtime::Admission::from_settings(settings.runtime());
    let state = sutura_http::ServiceState::new(service, Arc::new(settings), admission).with_inbound_identity(Arc::new(gate));
    Served {
        app: sutura_http::router(&state).expect("the test router assembles"),
        issuer,
        _published: published,
        _secret: secret,
    }
}

/// One `GET /v1/catalog` carrying `token`: the status and the body.
async fn catalog(app: axum::Router, token: &str) -> (axum::http::StatusCode, String) {
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt as _;

    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/v1/catalog")
        .header(axum::http::header::HOST, "localhost")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("a well-formed request builds");
    let response = app.oneshot(request).await.expect("a tower service's Error is Infallible");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the response body reads");
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// Which of [`OBJECTS`]' metrics `body` lists.
fn metrics_in(body: &str) -> Vec<&'static str> {
    OBJECTS
        .iter()
        .map(|object| object.metric)
        .filter(|metric| body.contains(&format!("\"{metric}\"")))
        .collect()
}

#[tokio::test]
async fn each_caller_sees_only_the_models_their_own_foundry_token_lists() {
    let idp = FakeServer::start(
        CALLERS
            .iter()
            .map(|subject| Scripted::ok(&stored(&stored_for(subject))))
            .collect(),
    );
    let foundry = FakeServer::start(vec![Scripted::ok(&listed(&["Customer"])), Scripted::ok(&listed(&["Order"]))]);
    let Served { app, issuer, .. } = served("foundry-visibility-each-caller", &idp, &foundry, true);

    let mut seen = Vec::new();
    for subject in CALLERS {
        let token = issuer.mint(&accepted_by(subject)).expect("the issuer signs a token");
        let (status, body) = catalog(app.clone(), &token).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{subject}: {body}");
        seen.push(metrics_in(&body));
    }

    assert_eq!(
        seen,
        [vec!["recurring_revenue"], vec!["order_count"]],
        "each caller must see the metric over the object type their own token lists, and not the other's"
    );
    let listings = foundry.finish();
    assert_eq!(listings.len(), CALLERS.len(), "one listing per caller: {listings:?}");
    for (listing, subject) in listings.iter().zip(CALLERS) {
        assert_eq!(
            listing.authorization(),
            Some(format!("Bearer {}", stored_for(subject)).as_str()),
            "{subject}'s listing must carry the token the identity provider stored for {subject}"
        );
        assert!(
            listing
                .request_line()
                .contains("/api/v2/ontologies/sales_ontology/objectTypes"),
            "{}",
            listing.request_line()
        );
    }
    assert_eq!(idp.finish().len(), CALLERS.len());
}

#[tokio::test]
async fn one_callers_listing_is_never_read_for_another_caller() {
    let [first, second] = CALLERS;
    // `first` asks twice and `second` once: the cache may answer `first`'s second read, and never
    // `second`'s first.
    let idp = FakeServer::start(vec![
        Scripted::ok(&stored(&stored_for(first))),
        Scripted::ok(&stored(&stored_for(second))),
    ]);
    let foundry = FakeServer::start(vec![Scripted::ok(&listed(&["Customer"])), Scripted::ok(&listed(&["Order"]))]);
    let Served { app, issuer, .. } = served("foundry-visibility-cache", &idp, &foundry, true);
    let first_token = issuer.mint(&accepted_by(first)).expect("the issuer signs a token");
    let second_token = issuer.mint(&accepted_by(second)).expect("the issuer signs a token");

    let (_, once) = catalog(app.clone(), &first_token).await;
    let (_, again) = catalog(app.clone(), &first_token).await;
    let (status, other) = catalog(app, &second_token).await;

    assert_eq!(metrics_in(&once), ["recurring_revenue"], "{once}");
    assert_eq!(
        metrics_in(&again),
        ["recurring_revenue"],
        "a cached read answers what the listing did: {again}"
    );
    assert_eq!(status, axum::http::StatusCode::OK, "{other}");
    assert_eq!(
        metrics_in(&other),
        ["order_count"],
        "the second caller must be listed under their own token, not served the first caller's entry: {other}"
    );
    let listings = foundry.finish();
    assert_eq!(
        listings
            .iter()
            .map(|listing| listing.authorization().map(String::from))
            .collect::<Vec<_>>(),
        [
            Some(format!("Bearer {}", stored_for(first))),
            Some(format!("Bearer {}", stored_for(second))),
        ],
    );
    assert_eq!(idp.finish().len(), 2, "the first caller's second read is the cache's");
}

#[tokio::test]
async fn a_broker_failure_answers_no_catalog_rather_than_every_model() {
    let idp = FakeServer::start(vec![Scripted::status(500, r#"{"error":"server_error"}"#)]);
    let foundry = FakeServer::start(Vec::new());
    let Served { app, issuer, .. } = served("foundry-visibility-broker-down", &idp, &foundry, true);
    let token = issuer.mint(&accepted_by(CALLERS[0])).expect("the issuer signs a token");

    let (status, body) = catalog(app, &token).await;

    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("identity_unavailable"), "{body}");
    assert!(metrics_in(&body).is_empty(), "a failed narrowing must list no metric: {body}");
    assert_eq!(foundry.finish().len(), 0, "nothing is listed without a brokered token");
    assert_eq!(idp.finish().len(), 1);
}

#[tokio::test]
async fn a_foundry_listing_failure_answers_no_catalog_rather_than_every_model() {
    let idp = FakeServer::start(vec![Scripted::ok(&stored(&stored_for(CALLERS[0])))]);
    let foundry = FakeServer::start(vec![Scripted::status(403, r#"{"errorName":"PermissionDenied"}"#)]);
    let Served { app, issuer, .. } = served("foundry-visibility-listing-refused", &idp, &foundry, true);
    let token = issuer.mint(&accepted_by(CALLERS[0])).expect("the issuer signs a token");

    let (status, body) = catalog(app, &token).await;

    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(metrics_in(&body).is_empty(), "a failed listing must list no metric: {body}");
    assert!(
        !body.contains(&stored_for(CALLERS[0])),
        "the refusal must not carry the brokered token: {body}"
    );
    assert_eq!(foundry.finish().len(), 1);
    assert_eq!(idp.finish().len(), 1);
}

#[tokio::test]
async fn a_caller_with_no_linked_foundry_account_is_refused_the_catalog() {
    let idp = FakeServer::start(vec![Scripted::status(
        400,
        r#"{"error":"invalid_request","error_description":"User not associated to identity provider"}"#,
    )]);
    let foundry = FakeServer::start(Vec::new());
    let Served { app, issuer, .. } = served("foundry-visibility-not-linked", &idp, &foundry, true);
    let token = issuer.mint(&accepted_by(CALLERS[0])).expect("the issuer signs a token");

    let (status, body) = catalog(app, &token).await;

    assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("identity_not_linked"), "{body}");
    assert!(metrics_in(&body).is_empty(), "{body}");
    assert_eq!(foundry.finish().len(), 0);
    assert_eq!(idp.finish().len(), 1);
}

#[tokio::test]
async fn without_catalog_visibility_every_caller_sees_every_model_and_nothing_is_dialled() {
    // Pins the default the setting leaves alone: a claim cell, killed by narrowing every
    // `foundry` source whether or not it declares `catalog_visibility`.
    let idp = FakeServer::start(Vec::new());
    let foundry = FakeServer::start(Vec::new());
    let Served { app, issuer, .. } = served("foundry-visibility-off", &idp, &foundry, false);

    for subject in CALLERS {
        let token = issuer.mint(&accepted_by(subject)).expect("the issuer signs a token");
        let (status, body) = catalog(app.clone(), &token).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{subject}: {body}");
        assert_eq!(metrics_in(&body), ["recurring_revenue", "order_count"], "{subject}: {body}");
    }
    assert_eq!(foundry.finish().len(), 0, "an undeclared narrowing lists nothing");
    assert_eq!(idp.finish().len(), 0, "an undeclared narrowing brokers nothing");
}

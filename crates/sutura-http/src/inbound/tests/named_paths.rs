//! Signed path selection and RFC 9728 routing. The nested service is a transport fake.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse as _;
use sutura_dev::issuer::{MockIssuer, Token};
use tower::ServiceExt as _;

use super::fixtures::gate_over_multi;
use crate::inbound::VerifiedCaller;
use crate::testing::{an_issuer, broker, bundle, fake_warehouse, named_direct_entry, settings_with};

fn issuers() -> [MockIssuer; 3] {
    [
        an_issuer(),
        MockIssuer::generating("https://second.example.com", "/second", "second-key").expect("a fixture issuer"),
        MockIssuer::generating("https://gateway.example.com", "/transit", "gateway-key").expect("a fixture gateway"),
    ]
}

fn app(issuers: &[MockIssuer; 3], gateway_path: &str, gateway_header: &str) -> axum::Router {
    let [first, second, gateway] = issuers;
    let overlay = format!(
        "security:\n  inbound:\n{}{}    - id: gateway\n      path: '{gateway_path}'\n      mode: behind-gateway\n      transit_header: {gateway_header}\n      transit_issuer: '{}'\n      transit_audience: '{}'\n      transit_token_type: at+jwt\n      key_set_file: /unread.json\n      algorithms: [ES256]\n",
        named_direct_entry(first, "first", "/a/mcp", "/unread.json"),
        named_direct_entry(second, "second", "/b/mcp", "/unread.json"),
        gateway.issuer(),
        gateway.audience(),
    );
    let settings = settings_with(&overlay);
    let mut declarations = settings.security().inbound().iter();
    let primary = declarations.next().expect("the first verifier");
    let second_decl = declarations.next().expect("the second verifier");
    let gateway_decl = declarations.next().expect("the gateway verifier");
    let gate = gate_over_multi(
        primary,
        &first.key_set(),
        &[(second_decl, &second.key_set()), (gateway_decl, &gateway.key_set())],
    );
    let paths = settings
        .security()
        .inbound()
        .iter()
        .map(|declaration| declaration.path().clone())
        .collect::<std::collections::BTreeSet<_>>();
    let service = crate::surface::LocalService::start(
        &crate::testing::catalog_of(bundle()),
        fake_warehouse(),
        crate::testing::sink(),
        broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the fixture bundle validates");
    let mut state = crate::testing::state_over(Arc::new(service), settings).with_inbound_identity(Arc::new(gate));
    for path in paths {
        state = state.with_agent_surface(crate::AgentMount::at(
            tower::service_fn(|request: Request<Body>| {
                let caller = request.extensions().get::<VerifiedCaller>().expect("the outer gate established a caller");
                let value = serde_json::json!({"verifier": caller.verifier().id().as_str(), "scopes": caller.scopes().iter().collect::<Vec<_>>()});
                async { Ok::<_, std::convert::Infallible>(axum::Json(value).into_response()) }
            }), crate::SpendHeadroomPush::NoCeilingConfigured, path,
        ));
    }
    crate::router(&state).expect("the named route assembly succeeds")
}

async fn call(
    app: &axum::Router,
    path: &str,
    token: Option<&str>,
    host: &str,
) -> (StatusCode, serde_json::Value, Option<String>) {
    let mut request = Request::builder().uri(path).header("host", host);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).expect("a fixture request"))
        .await
        .expect("the router is infallible");
    let status = response.status();
    let challenge = response
        .headers()
        .get("www-authenticate")
        .map(|value| value.to_str().expect("a visible challenge").to_owned());
    let body = axum::body::to_bytes(response.into_body(), 1 << 16)
        .await
        .expect("a bounded fixture response");
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        challenge,
    )
}

#[tokio::test]
async fn named_mcp_paths_admit_only_their_own_signed_verifier_and_scopes() {
    let issuers = issuers();
    let app = app(&issuers, "/g/mcp", "Authorization");
    for (issuer, id, own, scope) in [
        (&issuers[0], "first", "/a/mcp", "sutura:catalog.read"),
        (&issuers[1], "second", "/b/mcp", "sutura:metrics.ask"),
        (&issuers[2], "gateway", "/g/mcp", "sutura:query"),
    ] {
        let token = issuer
            .mint(&Token::for_subject("same-subject").granting(scope).living_for(60))
            .expect("a signed fixture token");
        let (status, body, _) = call(&app, own, Some(&token), "sutura.example.com").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["verifier"], id);
        assert_eq!(body["scopes"], serde_json::json!([scope]));
        for other in ["/a/mcp", "/b/mcp", "/g/mcp"].into_iter().filter(|path| *path != own) {
            assert_eq!(
                call(&app, other, Some(&token), "sutura.example.com").await.0,
                StatusCode::UNAUTHORIZED
            );
        }
    }
}

#[tokio::test]
async fn each_direct_path_names_its_own_metadata_and_gateway_paths_publish_none() {
    let issuers = issuers();
    let app = app(&issuers, "/g/mcp", "Authorization");
    for (path, issuer) in [("/a/mcp", &issuers[0]), ("/b/mcp", &issuers[1])] {
        let metadata = format!("/.well-known/oauth-protected-resource{path}");
        let (status, document, _) = call(&app, &metadata, None, "sutura.example.com").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(document["resource"], format!("https://sutura.example.com{path}"));
        assert_eq!(document["authorization_servers"], serde_json::json!([issuer.issuer()]));
        let challenge = call(&app, path, None, "sutura.example.com")
            .await
            .2
            .expect("a direct challenge");
        assert!(
            challenge.contains(&format!("resource_metadata=\"https://sutura.example.com{metadata}\"")),
            "{challenge}"
        );
        let mismatch = call(&app, path, None, "SUTURA.example.com")
            .await
            .2
            .expect("a direct challenge");
        assert!(
            !mismatch.contains("resource_metadata="),
            "a mismatched resource must not advertise metadata"
        );
    }
    assert_eq!(
        call(
            &app,
            "/.well-known/oauth-protected-resource/g/mcp",
            None,
            "sutura.example.com"
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(call(&app, "/g/mcp", None, "sutura.example.com").await.2.is_none());
    assert_eq!(
        call(&app, "/v1/catalog", None, "sutura.example.com").await.2.as_deref(),
        Some("Bearer error=\"invalid_token\"")
    );
}

#[tokio::test]
async fn a_shared_direct_and_gateway_path_uses_only_the_direct_discovery_document() {
    let issuers = issuers();
    let app = app(&issuers, "/a/mcp", "Authorization");
    let (_, document, _) = call(
        &app,
        "/.well-known/oauth-protected-resource/a/mcp",
        None,
        "sutura.example.com",
    )
    .await;
    assert_eq!(document["authorization_servers"], serde_json::json!([issuers[0].issuer()]));
    let token = issuers[2]
        .mint(
            &Token::for_subject("same-subject")
                .granting("sutura:catalog.read")
                .living_for(60),
        )
        .expect("a gateway token");
    assert_eq!(
        call(&app, "/a/mcp", Some(&token), "sutura.example.com").await.1["verifier"],
        "gateway"
    );
}

#[tokio::test]
async fn a_shared_mcp_path_refuses_competing_proofs_from_distinct_locations() {
    let issuers = issuers();
    let app = app(&issuers, "/a/mcp", "X-Transit-Proof");
    let direct = issuers[0]
        .mint(
            &Token::for_subject("same-subject")
                .granting("sutura:catalog.read")
                .living_for(60),
        )
        .expect("a direct token");
    let gateway = issuers[2]
        .mint(
            &Token::for_subject("same-subject")
                .granting("sutura:catalog.read")
                .living_for(60),
        )
        .expect("a gateway assertion");
    for (bearer, transit, expected) in [
        (Some(&direct), None, StatusCode::OK),
        (None, Some(&gateway), StatusCode::OK),
        (Some(&direct), Some(&gateway), StatusCode::UNAUTHORIZED),
    ] {
        let mut request = Request::builder().uri("/a/mcp").header("host", "sutura.example.com");
        if let Some(token) = bearer {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if let Some(token) = transit {
            request = request.header("x-transit-proof", token);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).expect("a fixture request"))
            .await
            .expect("the router is infallible");
        assert_eq!(response.status(), expected);
    }
}

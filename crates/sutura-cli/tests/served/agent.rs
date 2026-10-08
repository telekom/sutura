//! The served binary's AGENT-SURFACE cells (the `agent` feature): `/mcp` behind leg 1, refused at
//! boot without it unless the deployment is `single-user`, and two verified callers seeing two
//! different tool lists.
//!
//! Split out of `served.rs` (not a second harness) because the `max-lines` gate keeps a test file
//! under a thousand lines; this module is `#[path = "served/agent.rs"]` from `served.rs`, so it
//! shares the one `harness` module and every fixture the served suite uses, and every test here is
//! `#[cfg(feature = "agent")]` - the default (no-`agent`) build has no `/mcp` at all.

use sutura_config::Environment;
use sutura_dev::issuer::{PublishedKeySet, Token};

use crate::harness::{
    accepted_by, an_issuer, example_root, refused_to_start, settings_declaring_inbound, start_configured, written,
};

// --------------------------------------------------------------------------- agent surface ---
// Every cell below needs the `agent` feature COMPILED IN, which is why they are cfg-gated: `just
// test` runs --all-features so they run there, and a default-features build has no `/mcp` at all.
// They are the composed-binary half of PR3's hermetic `http::tests` - the mount behind the REAL
// `establish_asked` and leg 1, which a fake layer cannot show.

/// One MCP JSON-RPC body, the shapes the streamable-HTTP transport and the served cells use.
#[cfg(feature = "agent")]
fn initialize(id: i64) -> String {
    serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0", "id": id, "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                   "clientInfo": {"name": "sutura-serve-served", "version": "0.0.0"}}
    }))
    .expect("the initialize body serializes")
}

#[cfg(feature = "agent")]
fn tools_list(id: i64) -> String {
    serde_json::to_string(&serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {}}))
        .expect("the tools/list body serializes")
}

#[cfg(feature = "agent")]
fn tool_names(reply: &crate::harness::Reply) -> Vec<String> {
    reply
        .json()
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("a tools/list result carries a `tools` array: {}", reply.body))
        .iter()
        .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str).map(String::from))
        .collect()
}

/// A mounted agent surface with no `security.inbound` block on a `multi-user` deployment does not
/// boot, naming the key that turned it on and the key that is missing.
///
/// `NotFitToServe::AgentSurfaceWithoutInboundIdentity`, on the composed binary: `/mcp` answering
/// every caller as the deployment is refused unless the operator declared `single-user`, and #302's
/// limit carries forward - this is its own `served.rs` cell proving exit-without-bind.
#[cfg(feature = "agent")]
#[test]
fn a_multi_user_agent_surface_with_no_inbound_identity_stops_the_process() {
    let settings = crate::harness::deployment(
        &example_root(),
        crate::harness::AGENT_LOOPBACK,
        "  identity: \"multi-user\"\n",
    );
    let said = refused_to_start(Environment::Development, written("agent-no-inbound", &settings), &[]);
    let told = said.join("\n");
    assert!(
        told.contains("server.agent_surface.enabled is true and no security.inbound is declared"),
        "the refusal did not name the mount and the missing declaration:\n{told}"
    );
    assert!(
        !told.contains("\"msg\":\"listening\""),
        "a deployment refused for an inbound-less agent surface opened a listener:\n{told}"
    );
}

/// A `single-user` loopback deployment that declares a TLS terminator in front of it does not boot
/// with no `security.inbound` and no token gate: the terminator is there for callers from beyond
/// this host, so the agent surface counts it as off-host.
///
/// The composed binary's half of `settings::tests::agent_surface`'s terminator cell, one process per
/// terminator.
#[cfg(feature = "agent")]
#[test]
fn a_single_user_loopback_agent_surface_behind_a_declared_tls_terminator_stops_the_process() {
    for terminator in ["sidecar", "ingress"] {
        let security = format!("{}  tls_termination: \"{terminator}\"\n", crate::harness::SINGLE_USER);
        let settings = crate::harness::deployment(&example_root(), crate::harness::AGENT_LOOPBACK, &security);
        let said = refused_to_start(
            Environment::Development,
            written(&format!("agent-tls-{terminator}"), &settings),
            &[],
        );
        let told = said.join("\n");
        assert!(
            told.contains("server.agent_surface.enabled is true and no security.inbound is declared"),
            "{terminator}: the refusal did not name the mount and the missing declaration:\n{told}"
        );
        assert!(
            !told.contains("\"msg\":\"listening\""),
            "{terminator}: a deployment refused for an inbound-less agent surface opened a listener:\n{told}"
        );
    }
}

/// A `single-user` loopback deployment serves `/mcp` with no `security.inbound`, as the deployment:
/// every tool, `run_sql` included, and no token asked for because none is configured.
///
/// The posture `/v1` already has there. Red on a tree that refused every inbound-less mount.
#[cfg(feature = "agent")]
#[test]
fn a_single_user_loopback_agent_surface_with_no_inbound_identity_answers_as_the_deployment() {
    let settings = crate::harness::deployment(&example_root(), crate::harness::AGENT_LOOPBACK, crate::harness::SINGLE_USER);
    let served = start_configured("agent-single-user", &settings);
    drop(served.mcp(None, &initialize(1)));
    let tools = tool_names(&served.mcp(None, &tools_list(2)));
    assert_eq!(
        tools,
        vec![
            String::from("describe_catalog"),
            String::from("ask_metric"),
            String::from("run_sql")
        ],
        "a deployment with no inbound identity answers with every tool it enables"
    );
}

/// A build WITH the `agent` feature still serves no `/mcp` unless the deployment set
/// `server.agent_surface.enabled` - the explicit-bool shape, so compiling the feature alone
/// never mounts it.
#[cfg(feature = "agent")]
#[test]
fn a_build_carrying_the_agent_feature_still_does_not_mount_it_without_the_settings_switch() {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "serve-agent-off").expect("the key set publishes");
    let served = start_configured(
        "agent-off",
        &settings_declaring_inbound(&example_root(), &issuer, published.path()),
    );
    // Leg 1 is armed (this deployment declares inbound) but no agent surface is mounted, so
    // `/mcp` is no route at all - a `404`, not the `401` a mounted-but-unguarded route would get.
    let reply = served.mcp(None, &tools_list(1));
    assert_eq!(
        reply.status, 404,
        "the agent feature alone must not mount `/mcp`: {}",
        reply.body
    );
}

/// Two VERIFIED callers over the composed binary see two different tool lists.
///
/// The cell neither PR2 nor PR3 could show with a fake layer: `establish_asked` deriving each
/// request's `Asked` from leg 1's own `VerifiedCaller`, and `AgentSurface::permitted` narrowing
/// that request's tool list accordingly, on a real binary over a real socket. A root that
/// answered every verified caller with `Permitted::every_capability()` reddens the second
/// assertion (the narrow caller would see everything).
#[cfg(feature = "agent")]
#[test]
fn two_verified_callers_over_the_composed_binary_see_two_different_tool_lists() {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "serve-agent-two-callers").expect("the key set publishes");
    let served = start_configured(
        "agent-two-callers",
        &crate::harness::settings_with_agent_surface(&example_root(), &issuer, published.path()),
    );

    let narrow = issuer
        .mint(&Token::for_subject("narrow@example.com").granting(sutura_app::Capability::DescribeCatalog.scope()))
        .expect("the issuer mints a narrow token");
    let broad = issuer
        .mint(&accepted_by("broad@example.com"))
        .expect("the issuer mints an every-token");

    drop(served.mcp(Some(&narrow), &initialize(1)));
    let narrow_tools = tool_names(&served.mcp(Some(&narrow), &tools_list(2)));
    drop(served.mcp(Some(&broad), &initialize(3)));
    let broad_tools = tool_names(&served.mcp(Some(&broad), &tools_list(4)));

    assert_eq!(
        narrow_tools,
        vec![String::from("describe_catalog")],
        "the narrow caller must see only its own grant"
    );
    assert!(
        broad_tools.len() > narrow_tools.len(),
        "the broad caller must see more than the narrow caller: {broad_tools:?}"
    );
}

#[cfg(feature = "agent")]
#[test]
fn served_initialize_does_not_publish_the_whole_physical_schema() {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "serve-agent-physical-schema").expect("the key set publishes");
    let settings = format!(
        "{}\nprompt:\n  list_physical_schema: true\n",
        crate::harness::settings_with_agent_surface(&example_root(), &issuer, published.path())
    );
    let served = start_configured("agent-physical-schema", &settings);
    let token = issuer
        .mint(&Token::for_subject("reader@example.com").granting(sutura_app::Capability::DescribeCatalog.scope()))
        .expect("the issuer mints a reader token");
    let initialized = served.mcp(Some(&token), &initialize(1)).json();
    let instructions = initialized["result"]["instructions"].as_str().expect("the prompt is text");
    assert!(!instructions.contains("fct_subscription_monthly"), "{instructions}");

    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "describe_catalog", "arguments": {}}
    });
    let listing = served.mcp(Some(&token), &call.to_string()).json();
    assert_eq!(listing["result"]["structuredContent"]["models"], serde_json::json!([]));
}

/// One caller, one deployment, two transports: `GET /v1/catalog` and `/mcp`'s `describe_catalog`
/// answer the same `knowledge`.
///
/// The text is equal rather than similar because both transports call one function over the
/// caller's view; this cell is what notices a transport that stops, or one that renders it under a
/// different prose setting or view. It reads the example's glossary, so an empty section on both
/// sides cannot pass.
#[cfg(feature = "agent")]
#[test]
fn a_caller_reads_the_same_knowledge_over_http_as_over_mcp() {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "serve-agent-knowledge-parity").expect("the key set publishes");
    let served = start_configured(
        "agent-knowledge-parity",
        &crate::harness::settings_with_agent_surface(&example_root(), &issuer, published.path()),
    );
    let token = issuer
        .mint(&accepted_by("reader@example.com"))
        .expect("the issuer mints a token");

    let over_http = served.get("/v1/catalog", Some(&token));
    assert_eq!(over_http.status, 200, "{}", over_http.body);
    drop(served.mcp(Some(&token), &initialize(1)));
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "describe_catalog", "arguments": {}}
    });
    let over_mcp = served.mcp(Some(&token), &call.to_string());

    let http = over_http.json();
    let mcp = over_mcp.json();
    let http_knowledge = http["knowledge"]
        .as_str()
        .unwrap_or_else(|| panic!("the HTTP catalog carries no knowledge: {}", over_http.body));
    let mcp_knowledge = mcp["result"]["structuredContent"]["knowledge"]
        .as_str()
        .unwrap_or_else(|| panic!("the MCP catalog carries no knowledge: {}", over_mcp.body));
    assert!(
        http_knowledge.contains("monthly recurring revenue"),
        "the glossary did not reach HTTP: {http_knowledge}"
    );
    assert_eq!(http_knowledge, mcp_knowledge, "the two transports read different knowledge");
}

/// `/mcp` reads no more of a body than `server.max_body_bytes`, the bound `/v1` reads under: the
/// same over-cap body is a `413` on both, and the same message under the cap is answered.
#[cfg(feature = "agent")]
#[test]
fn the_agent_route_reads_no_more_body_than_the_versioned_surface() {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "serve-agent-body-cap").expect("the key set publishes");
    let served = start_configured(
        "agent-body-cap",
        &crate::harness::settings_with_agent_surface(&example_root(), &issuer, published.path()),
    );
    let token = issuer
        .mint(&accepted_by("asker@example.com"))
        .expect("the issuer mints a token");
    let padded = |bytes: usize| {
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/list",
            "params": {"_meta": {"padding": "x".repeat(bytes)}}
        })
        .to_string()
    };
    // The default `server.max_body_bytes` is 65536.
    let over = padded(70 * 1024);
    let versioned = served.post("/v1/query", Some(&token), &over);
    assert_eq!(versioned.status, 413, "{}", versioned.body);
    let agent = served.mcp(Some(&token), &over);
    assert_eq!(agent.status, 413, "{}", agent.body);
    let under = served.mcp(Some(&token), &padded(1024));
    assert_eq!(under.status, 200, "{}", under.body);
    assert!(!tool_names(&under).is_empty(), "{}", under.body);
}

/// Behind the real transport, `/mcp` and `/v1` answer the same hosts: the host of the deployment's
/// own resource identifier is accepted on both, and a name nothing declared is a `403` on both.
#[cfg(feature = "agent")]
#[test]
fn the_agent_route_answers_the_same_hosts_as_the_versioned_surface() {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "serve-agent-host").expect("the key set publishes");
    let served = start_configured(
        "agent-host",
        &crate::harness::settings_with_agent_surface(&example_root(), &issuer, published.path()),
    );
    let token = issuer
        .mint(&accepted_by("asker@example.com"))
        .expect("the issuer mints a token");
    for host in ["sutura.example.com", "Sutura.Example.com:443"] {
        let versioned = served.get_as_host(host, "/v1/catalog", Some(&token));
        assert_eq!(versioned.status, 200, "/v1/catalog with Host {host}: {}", versioned.body);
        let agent = served.mcp_as_host(host, Some(&token), &tools_list(1));
        assert_eq!(agent.status, 200, "/mcp with Host {host}: {}", agent.body);
    }
    let versioned = served.get_as_host("undeclared.example.com", "/v1/catalog", Some(&token));
    assert_eq!(versioned.status, 403, "{}", versioned.body);
    let agent = served.mcp_as_host("undeclared.example.com", Some(&token), &tools_list(1));
    assert_eq!(agent.status, 403, "{}", agent.body);
}

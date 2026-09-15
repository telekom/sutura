//! The served binary's AGENT-SURFACE cells (the `agent` feature): `/mcp` hidden behind leg 1,
//! refused at boot without it, and two verified callers seeing two different tool lists.
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

/// A mounted agent surface with no `security.inbound` block does not boot, naming both the key
/// that turned it on and the key that is missing.
///
/// The assembly refusal `AgentSurfaceWithoutInboundIdentity`, on the composed binary: a surface
/// that would be reachable by whoever can route a packet has to be refused rather than skipped,
/// and #302's limit carries forward - this is its own `served.rs` cell proving exit-without-bind.
#[cfg(feature = "agent")]
#[test]
fn a_mounted_agent_surface_with_no_inbound_identity_stops_the_process() {
    let settings = crate::harness::deployment(&example_root(), crate::harness::AGENT_LOOPBACK, crate::harness::SINGLE_USER);
    let said = refused_to_start(Environment::Development, written("agent-no-inbound", &settings));
    let told = said.join("\n");
    assert!(
        told.contains("agent surface is mounted") && told.contains("security.inbound"),
        "the refusal did not name the mount and the missing declaration:\n{told}"
    );
    assert!(
        !told.contains("\"msg\":\"listening\""),
        "a deployment refused for an inbound-less agent surface opened a listener:\n{told}"
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

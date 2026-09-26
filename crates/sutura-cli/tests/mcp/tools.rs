//! The tool surface: the schemas a served `tools/list` advertises are the committed ones, a
//! certified question is answered over the binary's pipes, and the raw tool is absent unless a
//! deployment turns it on.

use std::path::Path;

use sutura_app::Capability;

use crate::harness::{SOURCE, VERSION, example_settings, recurring_revenue_june, spawn, spawn_configured};

/// The committed input schema for one capability, as JSON.
///
/// **Read from `sutura-mcp`'s own snapshot directory rather than copied here**, which is the
/// difference between two files that must be kept in step and one file with two readers: a
/// widened tool input fails that crate's byte-compare AND this one until somebody re-accepts it,
/// and re-accepting it there is what makes this test agree again.
///
/// Compared as parsed JSON and not as text. `insta` writes a YAML header, the committed bytes
/// are canonicalised with every key sorted, and `serde_json`'s `preserve_order` is unified on in
/// a workspace build - so a textual compare would be asserting key order in a served reply,
/// which is not a property anybody wants. `serde_json::Value`'s equality is map equality at
/// every depth, which is exactly the claim.
fn committed_schema(capability: Capability) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sutura-mcp/src/snapshots")
        .join(format!("sutura_mcp__tool__tests__{}_input_schema.snap", capability.id()));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|cause| {
        panic!(
            "{} is the committed schema for `{}` and it is not readable: {cause}",
            path.display(),
            capability.id()
        )
    });
    // `---` opens the header and `---` closes it; the snapshot is what follows the second one.
    // Two named failures rather than one, so a snapshot format that changed says which half of
    // the header this stopped finding.
    let Some((_, after_open)) = text.split_once("---\n") else {
        panic!("{} does not open with an insta header:\n{text}", path.display())
    };
    let Some((_, body)) = after_open.split_once("---\n") else {
        panic!("{}'s insta header is never closed:\n{text}", path.display())
    };
    serde_json::from_str(body).unwrap_or_else(|cause| panic!("{} is not a JSON schema ({cause}):\n{body}", path.display()))
}

#[test]
fn the_schemas_the_binary_advertises_are_the_committed_ones() {
    // #117's own words for this branch: *list the tools, call one, and compare the reply against
    // the committed schema snapshots*. The comparison is what makes it a contract rather than a
    // smoke test - `sutura-mcp`'s snapshot test proves the GENERATOR, and this proves that what
    // a client generator reads off the wire is the same document a reviewer accepted.
    //
    // Spawned with `tools.run_sql.enabled: true` so every capability - the raw tool included -
    // is on this list; `#129`'s own `the_raw_tool_is_absent_unless_this_deployment_turned_it_on`
    // is the test that a DEFAULT deployment does not advertise it.
    let dir = example_settings("mcp-every-tool-advertised", "tools:\n  run_sql:\n    enabled: true\n");
    let mut agent = spawn_configured(&dir);
    drop(agent.initialize());
    let result = agent.request("tools/list", &serde_json::json!({}));
    let listed = result["tools"].as_array().cloned().unwrap_or_default();

    // Derived from `sutura_app::Capability` rather than from a list written here, so a third
    // capability is covered by this test the day it is added.
    let advertised: Vec<&str> = listed.iter().filter_map(|tool| tool["name"].as_str()).collect();
    let expected: Vec<&str> = Capability::every().map(Capability::id).collect();
    assert_eq!(advertised, expected, "{listed:?}");

    for capability in Capability::every() {
        let tool = listed
            .iter()
            .find(|tool| tool["name"] == capability.id())
            .unwrap_or_else(|| panic!("`{}` is not on the served surface: {listed:?}", capability.id()));
        assert_eq!(
            tool["inputSchema"],
            committed_schema(capability),
            "the schema served for `{}` is not the committed one",
            capability.id()
        );
        // A model reads the description before it decides to call. Its WORDING is nobody's
        // assertion - `sutura_mcp::tool` says so - but its absence would be a tool no model can
        // choose.
        assert!(
            tool["description"].as_str().is_some_and(|text| !text.is_empty()),
            "`{}` is advertised with no description: {tool}",
            capability.id()
        );
    }

    assert!(agent.close().success(), "the process did not exit cleanly");
}

#[test]
fn a_certified_question_is_answered_over_the_binarys_pipes() {
    // The agent-surface half of the sentence roadmap #22 was waiting for, and the same figure
    // `tests/example.rs` asserts over the libraries and `crates/sutura-cli/tests/served.rs`
    // asserts over HTTP - here through a spawned process, the real engine and the real static
    // broker, over the protocol an agent client speaks.
    //
    // `recurring_revenue` declares an anchor, so this number was re-executed at startup before
    // the process read a byte of the pipe. Asserted by value rather than snapshotted: it is one
    // row of two cells, and a third copy of the CLI's snapshot would be a file to keep in step
    // rather than a claim.

    let mut agent = spawn();
    drop(agent.initialize());
    let result = agent.call(Capability::AskMetric, &recurring_revenue_june());
    assert_ne!(result["isError"], serde_json::json!(true), "{result}");

    let content = &result["structuredContent"];
    assert_eq!(content["outcome"], "answer", "{result}");
    assert_eq!(
        content["columns"],
        serde_json::json!(["period", "recurring_revenue"]),
        "{result}"
    );
    assert_eq!(content["rows"], serde_json::json!([["2026-06-01", "202121"]]), "{result}");
    // Provenance travels with the answer, and the version is the one this composition stamps -
    // so a bundle served from somewhere else would be visible here rather than inferred.
    assert_eq!(content["provenance"]["definition_version"], VERSION, "{result}");
    assert!(
        content["provenance"]["definition_digest"]
            .as_str()
            .is_some_and(|digest| !digest.is_empty()),
        "an answer arrived with no definition digest: {result}"
    );
    // A pipe establishes no caller, and this example's source executes under its declared
    // shared identity.
    assert_eq!(
        content["executed_as"],
        serde_json::json!([{ "source": SOURCE, "posture": "shared-service-user" }]),
        "{result}"
    );
    // The text block beside the structured content, which is what a client that renders only
    // content blocks shows a person. Both are sent; neither is redundant.
    assert!(
        result["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("202121")),
        "the answer's text block does not carry the figure: {result}"
    );

    assert!(agent.close().success(), "the process did not exit cleanly");
}

/// `#129`'s own named test: a default settings tree advertises no `run_sql` capability at all,
/// even over this transport's no-authentication case - which grants every OTHER capability to
/// whoever can reach the pipe. `docs/adr/0013`'s tool has to be absent here specifically because
/// a scope cannot keep it off: there is no header on this transport a scope could arrive in.
#[test]
fn the_raw_tool_is_absent_unless_this_deployment_turned_it_on() {
    let mut agent = spawn();
    drop(agent.initialize());
    let listed = agent.request("tools/list", &serde_json::json!({}));
    assert!(
        !listed["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["name"].as_str())
            .any(|name| name == "run_sql"),
        "{listed}"
    );

    // And calling it by name anyway is refused the same way a genuinely unknown tool is - the
    // same code, because from this caller's side the two are one thing: a tool not on its
    // surface.
    let reply = agent.exchange(
        "tools/call",
        &serde_json::json!({ "name": "run_sql", "arguments": { "statement": "select 1" } }),
    );
    assert_eq!(reply["error"]["code"], serde_json::json!(-32601), "{reply}");
    assert!(agent.close().success(), "the process did not exit cleanly");

    // And turned on, it is listed - the schema test above proves what it is listed AS.
    let dir = example_settings("mcp-run-sql-enabled-listing", "tools:\n  run_sql:\n    enabled: true\n");
    let mut agent = spawn_configured(&dir);
    drop(agent.initialize());
    let listed = agent.request("tools/list", &serde_json::json!({}));
    assert!(
        listed["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["name"].as_str())
            .any(|name| name == "run_sql"),
        "{listed}"
    );
    assert!(agent.close().success(), "the process did not exit cleanly");
}

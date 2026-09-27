//! The process itself: it speaks the protocol on its own pipes, rejects an unknown tool as a
//! protocol fault, and stops when its peer closes the pipe.

use crate::harness::{PROTOCOL, recurring_revenue_june, spawn};
use sutura_app::Capability;

// ------------------------------------------------------------------- the process itself ---

#[test]
fn the_binary_speaks_the_protocol_on_its_own_pipes() {
    // The harness's own test, and the first thing #117 asks any e2e vehicle for: something
    // starts the composed process and gets a protocol reply out of it. Everything below is
    // worth nothing if this is not true.
    let mut agent = spawn();
    let result = agent.initialize();
    // The version is echoed, so the server serves the one a client asked for rather than
    // falling back to its own - see `PROTOCOL` for why that is worth a failing test.
    assert_eq!(result["protocolVersion"], PROTOCOL, "{result}");
    // It introduces itself as THIS crate and not as the SDK, which is what `get_info` chose
    // `Implementation::new` over `from_build_env` for.
    assert_eq!(result["serverInfo"]["name"], "sutura-mcp", "{result}");
    assert!(result["capabilities"]["tools"].is_object(), "no tools capability: {result}");
    // `telekom/sutura#776`: not merely non-empty - the RENDERED bundle, carrying knowledge only
    // this catalog declares. `churn_rate_over_the_half_year` is a worked example's own `name:`
    // frontmatter (`examples/single-player/catalog/knowledge/examples/`), so a fixed sentence
    // this crate hard-coded could never contain it - only `sutura_app::prompt::render`, over
    // THIS process's own pinned bundle, can.
    assert!(
        result["instructions"]
            .as_str()
            .is_some_and(|text| text.contains("churn_rate_over_the_half_year")),
        "the surface introduced itself with no rendered instructions: {result}"
    );

    // And the limit is stated on the LOG channel. Two claims in one: the notice exists, so a
    // person launching this process is told that whoever can reach it holds every capability -
    // and it is on standard error, so it is not on the stream the protocol owns. The second
    // half is what `Agent::reply` above has already proved by parsing the line before this one.
    let notice = agent.expect_log("grants every capability");
    assert!(notice.starts_with("sutura:"), "the notice is not this binary's own: {notice}");

    assert!(agent.close().success(), "the process did not exit cleanly");
}

#[test]
fn a_tool_this_surface_does_not_have_is_a_protocol_error() {
    // The other side of the line above, and the reason both are here: a question this deployment
    // declines is a RESULT, and a tool that does not exist is a PROTOCOL fault. A surface that
    // answered the second one the way it answers the first would be inventing a refusal for a
    // call it never understood.
    //
    // **Corrected**: this used to name `run_sql` here, on the strength of "the name is the one
    // this surface may never have" - `docs/adr/0013`'s tool, landing with `#129`, is what made
    // that claim stale, and the edit is the visible one the issue's own plan called for rather
    // than a quiet deletion. `run_sql` is a real capability now; what this test still needs is a
    // name genuinely absent from `Capability::every()`, so it asserts the protocol fault rather
    // than a tool this build happens not to have turned on.
    let mut agent = spawn();
    drop(agent.initialize());
    let reply = agent.exchange(
        "tools/call",
        &serde_json::json!({ "name": "a_tool_this_surface_does_not_have", "arguments": { "sql": "select 1" } }),
    );
    // -32601 is JSON-RPC's own `method not found`, which is what `tool::named` finding nothing
    // becomes. Asserted as a number rather than by message, for the reason every refusal code in
    // this repository is: the variant is the contract and the message is not.
    assert_eq!(reply["error"]["code"], serde_json::json!(-32601), "{reply}");
    assert!(
        reply.get("result").is_none(),
        "a tool that does not exist produced a result: {reply}"
    );

    // And the process is still serving: a rejected call is not a terminated session.
    //
    // One fewer than `Capability::every().count()`: this fixture's default settings never turn
    // `tools.run_sql.enabled` on, and `#129`'s own test for that absence is
    // `the_raw_tool_is_absent_unless_this_deployment_turned_it_on`, below.
    let listed = agent.request("tools/list", &serde_json::json!({}));
    assert_eq!(
        listed["tools"].as_array().map(Vec::len),
        Some(Capability::every().count() - 1),
        "{listed}"
    );

    assert!(agent.close().success(), "the process did not exit cleanly");
}

#[test]
fn closing_the_pipe_stops_the_process() {
    // The shutdown path this transport actually has. An agent client that is finished closes its
    // end; `rmcp` sees end-of-file, `serve_stdio` returns, and `crate::mcp` spends its
    // `shutdown_timeout` and releases the engine on the main thread. A process that hung here
    // would leave an orphan behind every chat session, and the engine being released off the
    // main thread while a question was in flight is what `panic = "abort"` turns into a crash.
    //
    // Asserted after a real question rather than straight after the handshake, so the engine has
    // done work and its blocking pool has threads to settle.
    let mut agent = spawn();
    drop(agent.initialize());
    let result = agent.call(Capability::AskMetric, &recurring_revenue_june());
    assert_eq!(result["structuredContent"]["outcome"], "answer", "{result}");

    let status = agent.close();
    assert!(
        status.success(),
        "the process exited with {status} when its peer closed the pipe; standard error:\n{}",
        agent.drain_log().join("\n")
    );
}

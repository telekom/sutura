//! Settings trees: a deployment's prose setting, execution bound, and reply deadline all reach
//! the spawned surface.

use std::path::Path;

use sutura_app::Capability;

use crate::harness::{example_settings, spawn_configured};

/// One `describe_catalog` reply off the spawned binary's pipes, both halves of it.
///
/// The text block is the first content block; `sutura_mcp::wire::CatalogContent` sends both and
/// this suite reads both, because `#266`'s `H1` was one half honouring a setting the other half
/// had never been given.
fn described_catalog(config_dir: &Path) -> (serde_json::Value, String) {
    let mut agent = spawn_configured(config_dir);
    drop(agent.initialize());
    let result = agent.call(Capability::DescribeCatalog, &serde_json::json!({}));
    assert_ne!(result["isError"], serde_json::json!(true), "{result}");
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("the catalog reply carries no text block: {result}"))
        .to_owned();
    let structured = result["structuredContent"].clone();
    assert!(
        structured.is_object(),
        "the catalog reply carries no structured content: {result}"
    );
    assert!(agent.close().success(), "the process did not exit cleanly");
    (structured, text)
}

/// A dimension description the example catalog really carries, on `recurring_revenue`.
const EXAMPLE_PROSE: &str = "Where the customer is.";

#[test]
fn a_deployments_prose_setting_reaches_both_halves_of_the_agent_surface() {
    // **`#266`'s `H1`, at the only layer that can prove it: a settings FILE this process read.**
    // The chain under test is `base.yaml` -> `Settings::load` -> `crate::mcp::serve` ->
    // `mcp_service` -> `serve_stdio` -> `CatalogContent::of`, and nothing in it is supplied by
    // this test except the file. The review that blocked the first attempt is why: a test that
    // hands `mcp_service` the value it wants to see stays green with the read back to a
    // constant, which is the defect one frame out.
    //
    // Both directions in one test, because either alone is a control nobody can trust. An
    // omission that withheld everything whatever the operator asked for would be an outage, and
    // a test that only checked the omission would pass against a catalog with no prose in it.

    // The default: the example declared with no extra reading, so `prompt.catalog_prose`
    // resolves to its embedded default and the catalog's own words reach the agent on both
    // halves. `example_settings` declares the catalog and the source this command now reads -
    // there is no argument left for it to take.
    let (structured, text) = described_catalog(&example_settings("mcp-catalog-prose-default", ""));
    assert_eq!(structured["catalog_prose"], "quoted", "{structured}");
    let flat = structured.to_string();
    assert!(
        flat.contains(EXAMPLE_PROSE),
        "the default withheld the catalog's prose from the structured half: {flat}"
    );
    // Quoted, not spliced - `docs/adr/0022`'s text half, asserted through the process.
    assert!(text.contains(&format!("> {EXAMPLE_PROSE}")), "{text}");

    let dir = example_settings("mcp-catalog-prose-omitted", "prompt:\n  catalog_prose: omitted\n");
    let (structured, text) = described_catalog(&dir);
    assert_eq!(structured["catalog_prose"], "omitted", "{structured}");
    let flat = structured.to_string();
    // FLAT rather than an index into `metrics[0].description`: an index steps over a
    // description one level down on a dimension, which is the blindness the standing regression
    // test had - it read `content.first()` and never the structured half at all.
    assert!(!flat.contains(EXAMPLE_PROSE), "a description survived the omission: {flat}");
    assert!(
        !flat.contains("description"),
        "a description field survived the omission: {flat}"
    );
    assert!(!text.contains(EXAMPLE_PROSE), "{text}");
    assert!(text.contains("NOT included"), "{text}");
    // What survives is what a caller needs in order to ask a valid question. Withholding this
    // too would trade a prose channel for refusals.
    assert!(flat.contains("recurring_revenue"), "{flat}");
    assert!(flat.contains("central"), "{flat}");
}

#[test]
fn a_deployments_execution_bound_reaches_the_spawned_surface() {
    // **`#325`'s `F7` at the layer that can prove the WIRING: a settings file this process
    // read.** The chain is `base.yaml` -> `Settings::load` -> `crate::mcp::serve` ->
    // `mcp_service` -> `Admission::from_settings` -> `serve_stdio`, and the only thing this test
    // supplies is the file. What the number then DOES to a question in flight is
    // `sutura_mcp::server`'s own suite, over a fake that can be held - the same split the prose
    // test above states: this one holds the process, that one holds the behaviour. A real engine
    // answers the example in milliseconds, so nothing here could keep eight questions inside the
    // port long enough to see a ninth shed.
    //
    // Read off the startup notice, which is where the process states its own limits on the log
    // channel. Two documents, because that is what tells a read from a constant.
    let embedded = sutura_config::Settings::load(&sutura_config::Sources::defaults(sutura_config::Environment::Development))
        .expect("the embedded defaults load")
        .runtime()
        .max_concurrent_queries()
        .count();
    let example = example_settings("mcp-admission-embedded", "");
    let mut agent = spawn_configured(&example);
    let notice = agent.expect_log("grants every capability");
    assert!(
        notice.contains(&format!("at most {embedded} questions")),
        "the notice does not state the embedded bound of {embedded}: {notice}"
    );
    // Handshaken before the pipe is closed, and not for tidiness: `serve_stdio` is waiting for
    // `initialize`, so an end-of-file before it is a handshake failure and a non-zero exit. The
    // notice is printed before that wait, which is why it can be read first.
    drop(agent.initialize());
    assert!(agent.close().success(), "the process did not exit cleanly");

    // And a deployment that says something else gets what it said. Three, which no default
    let dir = example_settings("mcp-admission-bound", "runtime:\n  max_concurrent_queries: 3\n");
    let mut agent = spawn_configured(&dir);
    let notice = agent.expect_log("grants every capability");
    assert!(
        notice.contains("at most 3 questions"),
        "the configured bound did not reach the served surface: {notice}"
    );
    drop(agent.initialize());
    assert!(agent.close().success(), "the process did not exit cleanly");
}

#[test]
fn a_deployments_reply_deadline_reaches_the_spawned_surface() {
    // **`telekom/sutura#339` at the layer that can prove the WIRING: a settings file this
    // process read.** The chain is `base.yaml` -> `Settings::load` -> `crate::mcp::serve` ->
    // `mcp_service` -> `serve_stdio` -> `AgentSurface`, and the only thing this test supplies is
    // the file. What the number then DOES to a question in flight is `sutura_mcp::server`'s own
    // suite, over a fake that can be HELD - which is the only way to see a deadline fire, since
    // a real engine answers the example in milliseconds. The same split the execution bound's
    // test above states.
    //
    // Read off the startup notice, for that test's reason: it is where this process states its
    // own limits on the log channel, and `stdout` here is the protocol.
    let embedded = sutura_config::Settings::load(&sutura_config::Sources::defaults(sutura_config::Environment::Development))
        .expect("the embedded defaults load")
        .server()
        .request_timeout()
        .seconds();
    let example = example_settings("mcp-reply-embedded", "");
    let mut agent = spawn_configured(&example);
    let notice = agent.expect_log("grants every capability");
    assert!(
        notice.contains(&format!("after {embedded} seconds")),
        "the notice does not state the embedded reply deadline of {embedded}: {notice}"
    );
    drop(agent.initialize());
    assert!(agent.close().success(), "the process did not exit cleanly");

    // And a deployment that says something else gets what it said. Seven, which no default
    let dir = example_settings("mcp-reply-deadline", "server:\n  request_timeout_seconds: 7\n");
    let mut agent = spawn_configured(&dir);
    let notice = agent.expect_log("grants every capability");
    assert!(
        notice.contains("after 7 seconds"),
        "the configured reply deadline did not reach the served surface: {notice}"
    );
    drop(agent.initialize());
    assert!(agent.close().success(), "the process did not exit cleanly");
}

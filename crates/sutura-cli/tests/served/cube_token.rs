//! The Cube token, sent for real by the catalog reader and looked for everywhere the process answers
//! or writes (`telekom/sutura#1398`).
//!
//! The reader sends it when a `catalog.kind: cube` deployment boots, to a loopback Cube that accepts
//! it and to one that refuses it with a body naming it; each fake must have been sent the token as a
//! bearer, or the absence the scan reports proves nothing.
//!
//! The warehouse (`sutura-exec-cube`) sends the same token when a question is asked, and the second
//! cell reads that send and the question-time failure after it: a loopback Cube that answers the
//! question, one that answers 401 and one that answers 500, the last two with a body naming the token,
//! each asked through `/v1/query`; the third cell asks the 500 one through `/mcp`'s `ask_metric`.
//! The example catalog is copied with every anchor but `recurring_revenue`'s removed, because a
//! deployment verifies each anchor against Cube at boot and a scripted fake answers one certified
//! number in order, not six.
//!
//! **The limit:** only the Cube warehouse is made to fail at question time here. A cause chain from
//! another data system's driver is not read, and `sutura mcp` is not asked: it is a stdio process that
//! installs no tracing subscriber, so its log surface is standard error alone. `/mcp` is asked for the
//! 500 only, not for an answer or a 401.

use std::path::{Path, PathBuf};

use sutura_catalog_cube::fixture::META;
use sutura_http_client::test_support::{CapturedAuthorizations, FakeServer, Scripted};

use crate::harness::{
    LOOPBACK, Reply, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside, example_root, recurring_revenue_june,
    start_configured_with_environment, v1,
};
use crate::no_echo::{TRACE, bearer_echoed, echoed};

#[cfg(feature = "agent")]
use crate::harness::AGENT_LOOPBACK;

/// The one metric whose anchor stays: Cube must answer it at boot, with the number it certifies.
const ANCHORED: &str = "recurring_revenue.md";

const CANARY: &str = "5d27e8b1f90a4c63b7e0d9a2";

/// The recorded `meta` answer, which both readers fetch.
fn meta() -> Scripted {
    Scripted::ok(&serde_json::from_str(META).expect("the recorded meta answer is JSON"))
}

/// A directory holding the token file, removed on every path out.
struct TokenDir(std::path::PathBuf);

impl TokenDir {
    fn holding(case: &str) -> Self {
        let path = derived_beside(&config_path(case));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path).expect("the token directory is creatable");
        std::fs::write(path.join("token"), format!("{CANARY}\n")).expect("the token file is writable");
        Self(path)
    }

    fn file(&self) -> String {
        self.0.join("token").display().to_string()
    }

    /// The example catalog, copied beside the token file with every `anchor:` block but one removed.
    fn catalog_with_one_anchor(&self) -> PathBuf {
        let root = self.0.join("catalog");
        copy_tree(&example_root().join("catalog"), &root);
        let mut stripped = 0;
        for entry in std::fs::read_dir(root.join("metrics")).expect("the copied metrics are readable") {
            let path = entry.expect("a directory entry reads").path();
            if path.file_name().is_some_and(|name| name == ANCHORED) {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a metric document reads");
            let mut inside = false;
            let kept: String = text
                .split_inclusive('\n')
                .filter(|line| {
                    inside = if line.starts_with("anchor:") {
                        true
                    } else {
                        inside && line.starts_with(' ')
                    };
                    !inside
                })
                .collect();
            if kept != text {
                stripped += 1;
                std::fs::write(&path, kept).expect("the metric document writes");
            }
        }
        assert!(
            stripped > 0,
            "no other metric of the example declares an anchor, so there was nothing to strip"
        );
        root
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy's directory is creatable");
    for entry in std::fs::read_dir(from).unwrap_or_else(|cause| panic!("{} is not readable: {cause}", from.display())) {
        let entry = entry.expect("a directory entry reads");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a catalog document copies");
        }
    }
}

impl Drop for TokenDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

#[test]
fn a_cube_catalog_token_is_held_out_of_every_reply_and_log_line_accepted_or_refused() {
    let case = "cube-catalog-no-echo";
    let token = TokenDir::holding(case);
    let leaked = bearer_echoed(case, CANARY, vec![meta(), meta()], |cube| {
        let (endpoint, file) = (cube.endpoint(), token.file());
        format!(
            "server:\n{LOOPBACK}security:\n{SINGLE_USER}  access_token: \"{TOKEN}\"\n\
             telemetry:\n  format: \"bunyan\"\n\
             catalogs:\n  - name: \"metrics\"\n    kind: \"cube\"\n    dir: \"/unused-for-cube\"\n    \
             data_dir: \"/unused-for-cube\"\n    version: \"{VERSION}\"\n    endpoint: \"{endpoint}\"\n    \
             token_file: \"{file}\"\n\
             sources:\n  metrics:\n    kind: \"cube\"\n    endpoint: \"{endpoint}\"\n    token_file: \"{file}\"\n    \
             posture: \"shared-service-user\"\n"
        )
    });
    assert!(leaked.is_empty(), "the cube token was sent back or logged: {leaked:?}");
}

/// Cube's answer to `recurring_revenue` in June: the number its anchor certifies.
fn june() -> Scripted {
    Scripted::ok(&serde_json::json!({ "data": [{
        "fct_subscription_monthly.month.month": "2026-06-01T00:00:00.000",
        "fct_subscription_monthly.recurring_revenue": "202121",
    }] }))
}

/// What Cube answers while the deployment boots, then `question`.
fn booted_then(question: Vec<Scripted>) -> Vec<Scripted> {
    let mut script = vec![meta(), june()];
    script.extend(question);
    script
}

/// The example's markdown catalog over a Cube source at `cube`, reading the token from `token`.
fn settings_over_cube(token: &TokenDir, cube: &FakeServer) -> String {
    format!(
        "server:\n{LOOPBACK}security:\n{SINGLE_USER}  access_token: \"{TOKEN}\"\n\
         telemetry:\n  format: \"bunyan\"\n\
         catalogs:\n  - name: \"model\"\n    kind: \"markdown\"\n    dir: \"{catalog}\"\n    data_dir: \"{data}\"\n    \
         version: \"{VERSION}\"\n\
         sources:\n  local:\n    kind: \"cube\"\n    endpoint: \"{endpoint}\"\n    token_file: \"{file}\"\n    \
         posture: \"shared-service-user\"\n",
        catalog = token.catalog_with_one_anchor().display(),
        data = example_root().join("data").display(),
        endpoint = cube.endpoint(),
        file = token.file(),
    )
}

/// One deployment booted over a Cube that then answers the question with `answer`: the reply, every
/// line the process wrote, and what Cube was sent.
fn asked(case: &str, answer: Scripted) -> (Reply, Vec<String>, CapturedAuthorizations) {
    let cube = FakeServer::start(booted_then(vec![meta(), answer]));
    let kept = TokenDir::holding(case);
    let mut served = start_configured_with_environment(case, &settings_over_cube(&kept, &cube), None, &TRACE, &[]);
    let reply = served.post(
        &v1(sutura_http::constants::base_paths::QUERY),
        Some(TOKEN),
        &recurring_revenue_june(),
    );
    served.terminate();
    (reply, served.log(), cube.finish())
}

#[test]
fn a_cube_warehouse_token_is_held_out_of_every_reply_and_log_line_when_a_question_is_answered_or_refused() {
    let naming = |status| Scripted::status(status, &format!(r#"{{"error":"{CANARY} was not accepted"}}"#));
    let (answered, mut lines, answering) = asked("cube-warehouse-answered", june());
    let (unauthorized, unauthorized_lines, rejecting) = asked("cube-warehouse-unauthorized", naming(401));
    let (failed, failed_lines, failing) = asked("cube-warehouse-failed", naming(500));
    lines.extend(unauthorized_lines.iter().cloned());
    lines.extend(failed_lines.iter().cloned());

    assert_eq!(answered.status, 200, "Cube's answer must be served: {}", answered.body);
    assert_eq!(
        unauthorized.status, 403,
        "a token Cube rejects is a refusal: {}",
        unauthorized.body
    );
    assert_eq!(
        failed.status, 503,
        "a Cube that fails is a data system that did not answer: {}",
        failed.body
    );
    assert!(
        failed_lines
            .iter()
            .any(|line| line.contains("the data system did not answer") && line.contains("Cube refused the request with 500")),
        "the question-time cause chain was never logged, so its absence of the token proves nothing:\n{}",
        failed_lines.join("\n")
    );
    assert!(
        unauthorized_lines.iter().any(|line| line.contains(r#""level":10"#)),
        "the 401 run captured no `trace` line, so its absence of the token proves nothing"
    );
    let bearer = format!("Bearer {CANARY}");
    for (fake, sent) in [("answering", answering), ("rejecting", rejecting), ("failing", failing)] {
        assert!(
            sent.iter().all(|request| request.authorization() == Some(bearer.as_str())),
            "the {fake} fake was not sent the token at boot and again for the question, so its absence proves nothing: {sent:?}"
        );
    }
    let leaked = echoed(&[("the cube token", CANARY)], &[&answered, &unauthorized, &failed], &lines);
    assert!(leaked.is_empty(), "the cube token was sent back or logged: {leaked:?}");
}

/// The `initialize` and the `ask_metric` call that put `/v1/query`'s question to `/mcp`.
#[cfg(feature = "agent")]
fn mcp_bodies() -> [String; 2] {
    let question: serde_json::Value = serde_json::from_str(&recurring_revenue_june()).expect("the question is JSON");
    [
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                       "clientInfo": {"name": "sutura-cube-token", "version": "0.0.0"}}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "ask_metric", "arguments": question}
        }),
    ]
    .map(|body| body.to_string())
}

/// The `initialize` reply and the `ask_metric` reply, every line the process wrote, and what Cube was sent.
#[cfg(feature = "agent")]
type Questioned = ([Reply; 2], Vec<String>, CapturedAuthorizations);

/// What `/mcp` answers when Cube fails the question with a body naming the token.
#[cfg(feature = "agent")]
fn questioned_over_mcp(case: &str) -> Questioned {
    let failure = Scripted::status(500, &format!(r#"{{"error":"{CANARY} was not accepted"}}"#));
    let cube = FakeServer::start(booted_then(vec![meta(), failure]));
    let kept = TokenDir::holding(case);
    let settings = settings_over_cube(&kept, &cube).replace(LOOPBACK, AGENT_LOOPBACK);
    let mut served = start_configured_with_environment(case, &settings, None, &TRACE, &[]);
    let [initialize, call] = mcp_bodies();
    let replies = [served.mcp(Some(TOKEN), &initialize), served.mcp(Some(TOKEN), &call)];
    served.terminate();
    (replies, served.log(), cube.finish())
}

#[cfg(feature = "agent")]
#[test]
fn a_cube_warehouse_token_is_held_out_of_every_mcp_reply_and_log_line_when_a_question_fails() {
    let (replies, lines, sent) = questioned_over_mcp("cube-warehouse-mcp-failed");
    let [initialized, called] = &replies;

    let result = &called.json()["result"];
    assert_eq!(
        (
            result["isError"].as_bool(),
            result["content"].as_array().map(Vec::len),
            result["content"][0]["text"].as_str(),
            result.get("structuredContent"),
        ),
        (Some(true), Some(1), Some("the data system did not answer"), None),
        "a failed question was not told as the fixed sentence alone: {}",
        called.body
    );
    assert!(
        lines.iter().any(|line| line.contains("the agent surface could not answer")
            && line.contains("Cube refused the request with 500")),
        "the question-time cause chain was never logged, so its absence of the token proves nothing:\n{}",
        lines.join("\n")
    );
    let bearer = format!("Bearer {CANARY}");
    assert!(
        sent.len() >= 4 && sent.iter().all(|request| request.authorization() == Some(bearer.as_str())),
        "Cube was not sent the token at boot and again for the question, so its absence proves nothing: {sent:?}"
    );
    let leaked = echoed(&[("the cube token", CANARY)], &[initialized, called], &lines);
    assert!(leaked.is_empty(), "the cube token was sent back or logged: {leaked:?}");
}

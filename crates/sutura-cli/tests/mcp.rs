//! The agent surface as an agent client gets it: **this binary, spawned, speaking the Model Context
//! Protocol on its own pipes.**
//!
//! The MCP half of `github.com/telekom/sutura#117`, whose third branch was blocked on #110 deciding
//! which composition root serves stdio. That decision is `sutura mcp`, so this is the shape #117
//! asks for pointed at it: list the tools, call one, and compare what came back against the
//! committed schema snapshots in `crates/sutura-mcp/src/snapshots/`.
//!
//! # Why it spawns the binary, when `src/mcp.rs` already has a handshake test
//!
//! `crate::mcp::tests::the_mcp_composition_serves_every_tool_the_surface_declares` joins the SDK's
//! own client and server over `tokio::io::duplex`. That proves `mcp_service` - the composition - and
//! it cannot prove the transport, because the pipe in it is a pair of in-memory buffers rather than
//! this process's standard input and output. #117's own rule for the HTTP half is the same rule
//! here: *a test that builds its own router proves the router*. So the two are not duplicates -
//! that one holds the composition, this one holds the PROCESS.
//!
//! What only a spawned process can answer:
//!
//! * **standard output carries the protocol and nothing else.** The startup notice that states the
//!   every-capability limit is an `eprintln!` precisely because stdout is the protocol channel, and
//!   a single stray `println!` anywhere below `serve_stdio` would corrupt the stream for every
//!   client in the world. [`Agent::reply`] fails by name on that: the first line it reads has to
//!   parse as a JSON-RPC message.
//! * **the process exits when its peer closes the pipe** - through the real `shutdown_timeout` in
//!   `crate::mcp`, with the engine released on the main thread.
//! * **the schemas a client generator consumes off the wire are the reviewed ones.** The snapshot
//!   test in `sutura-mcp` compares `input_schema(capability)` against the committed bytes in
//!   process; this reads the `inputSchema` a served `tools/list` actually put on the pipe and
//!   compares THAT against the same file.
//!
//! # The client is hand-written, deliberately
//!
//! No dev-dependency is added: the MCP stdio framing is one JSON-RPC object per line, so a client
//! is `writeln!` plus `read_line`. `crates/sutura-serve/tests/served.rs` takes the same decision
//! about HTTP for a cost reason; here the reason is evidence. An SDK client on both ends of the
//! pipe would be the SDK agreeing with itself, and it is also free to tolerate a line this suite
//! exists to forbid. A client that is not ours is what makes *stdout is the protocol* an assertion
//! rather than a hope.
//!
//! It is not a conformance suite and does not claim to be: it speaks the subset of the lifecycle
//! this surface serves - `initialize`, `notifications/initialized`, `tools/list`, `tools/call` -
//! and `sutura-mcp` owns interoperability by using the SDK rather than framing its own replies.
//!
//! # What this suite cannot claim
//!
//! Nothing about identity. A pipe has no header a token could arrive in, so this surface grants
//! every capability to whoever can launch the process, and every question below is answered under
//! the single shared identity `examples/single-player` declares.
//! `docs/where-identity-is-proven.md` carries that exclusion in the place a reader of a green run
//! will meet it, beside `just serve-e2e`'s, because a venue that cannot state its limit is how
//! *verified* drifts. `Permitted` narrowing this surface is a composition change nobody has made,
//! so there is no unpermitted-tool case here to write; the unknown-tool case below is the shape it
//! would take.
//!
//! # `unix` only
//!
//! Nothing here signals: the peer closing its end of the pipe IS the shutdown path for this
//! transport, which is why [`Agent::close`] drops the handle rather than sending anything. The
//! module is still `cfg(unix)`, because `CARGO_BIN_EXE_sutura` is the one binary shape this suite
//! reasons about and the platform the gates run on is the one it is measured on.

// `cfg(test)` for the reason `tests/example.rs` gives: clippy honours `allow-expect-in-tests` only
// inside a `#[cfg(test)]` item, and without it every `expect` in the harness below is a lint error.
// Two attributes rather than `cfg(all(test, unix))`, which is not a style choice - clippy looks for
// a literal `#[cfg(test)]` on an ancestor module and does not see through an `all(..)`.
#[cfg(unix)]
#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
    use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
    use std::time::{Duration, Instant};

    use sutura_app::Capability;

    /// The protocol version this suite asks for.
    ///
    /// A literal rather than a constant read out of `rmcp`, and that is the point of a hand-written
    /// client: what is pinned here is a version real clients in the field send, not the SDK's own
    /// idea of the latest. `rmcp` negotiates by echoing a version it supports, so this assertion
    /// goes red if the pinned SDK ever stops serving it - which is a fact about our deployed
    /// contract and worth a failing test rather than a silent fallback.
    const PROTOCOL: &str = "2025-06-18";

    /// The version the CLI stamps a bundle with: `crate::commands::DEFAULT_VERSION`.
    ///
    /// Spelled out rather than imported, because an integration test links the BINARY's crate as a
    /// library it has no access to. A drift here is a provenance change, which is exactly what an
    /// assertion on it should catch.
    const VERSION: &str = "local-working-tree";

    /// The source name the CLI opens a catalog under: `crate::commands::CATALOG_SOURCE`.
    const SOURCE: &str = "local";

    /// How long one request may take before the test gives up and prints the log it has.
    ///
    /// Generous on purpose, and it covers startup rather than a round trip: `mcp_service` loads the
    /// catalog and RE-EXECUTES every anchor against the engine before `serve_stdio` reads a byte, so
    /// the first reply pays for validating a bundle. A shared CI runner is slower than a laptop at
    /// both.
    const REPLY_BUDGET: Duration = Duration::from_secs(120);

    /// How long a closed pipe may take to become an exited process.
    ///
    /// Longer than the five-second `shutdown_timeout` in `crate::mcp`, because that bound is on
    /// letting blocking work settle and the process still has to unwind and exit after spending it.
    const STOP_BUDGET: Duration = Duration::from_secs(45);

    /// How long the startup notice may take to arrive on standard error.
    ///
    /// Its own budget, and shorter than [`REPLY_BUDGET`], because it is written before the catalog
    /// is opened: a notice that has not arrived by now is one that was not written, or was written
    /// to the wrong stream.
    const NOTICE_BUDGET: Duration = Duration::from_secs(30);

    /// The example deployment, which is also what a reader is told to run.
    ///
    /// Canonicalised because the two paths below are handed to a subprocess, and a path carrying
    /// `..` is harder to read in a failure than the real one.
    fn example_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/single-player")
            .canonicalize()
            .expect("the example directory is in this repository")
    }

    /// Reads one stream line by line into a channel of its own.
    ///
    /// **Two channels rather than one**, which is the opposite of what `served.rs` does and for the
    /// reason this whole file exists: there the two streams are one log, here one of them is the
    /// protocol and the other is not. Merging them would destroy the very distinction being
    /// asserted. Both are still drained, because a piped stream nobody reads fills its buffer and
    /// blocks the writer - and the writer is the process under test.
    fn forward<Stream>(stream: Stream, into: &Sender<String>)
    where
        Stream: std::io::Read,
    {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { return };
            if into.send(line).is_err() {
                return;
            }
        }
    }

    /// A running agent surface: the process, its two streams kept apart, and the request counter.
    struct Agent {
        child: Child,
        /// The peer's end of the pipe. Taken by [`Agent::close`], which is the shutdown path.
        stdin: Option<ChildStdin>,
        /// Standard output. Every line here has to be a JSON-RPC message.
        protocol: Receiver<String>,
        /// Standard error. The log, and the startup notice.
        log: Receiver<String>,
        next_id: u32,
        reaped: bool,
    }

    /// Starts the agent surface over the documented example.
    ///
    /// No `#[expect(clippy::zombie_processes)]`, unlike `crates/sutura-serve/tests/served.rs`'s
    /// `start`, and that is measured rather than assumed: the lint does not fire here, and an
    /// expectation that never fires is itself an error under `-D warnings`. The reaping is real
    /// either way - every path out of an [`Agent`], a clean [`Agent::close`] or a panicking
    /// assertion, waits on the child.
    fn spawn() -> Agent {
        let example = example_root();
        let mut child = Command::new(env!("CARGO_BIN_EXE_sutura"))
            .arg("mcp")
            .arg(example.join("catalog"))
            .arg(example.join("data"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the composed binary starts");

        let stdin = child.stdin.take().expect("standard input was piped");
        let stdout = child.stdout.take().expect("standard output was piped");
        let stderr = child.stderr.take().expect("standard error was piped");
        let (out_sender, protocol) = channel();
        let (err_sender, log) = channel();
        drop(std::thread::spawn(move || forward(stdout, &out_sender)));
        drop(std::thread::spawn(move || forward(stderr, &err_sender)));

        Agent {
            child,
            stdin: Some(stdin),
            protocol,
            log,
            next_id: 0,
            reaped: false,
        }
    }

    impl Agent {
        /// Writes one message as one line, which is the whole of the stdio framing.
        fn write(&mut self, message: &serde_json::Value) {
            let stdin = self.stdin.as_mut().expect("the peer has not closed its end of the pipe");
            writeln!(stdin, "{message}").expect("the request is writable");
            stdin.flush().expect("the request flushes");
        }

        /// One line off standard output, as JSON.
        ///
        /// **The assertion that stdout is the protocol channel and nothing else.** A banner, a
        /// `println!` left in a handler or a `tracing` subscriber writing to standard output all
        /// land here, and all fail by name rather than as a confusing protocol error three requests
        /// later.
        fn reply(&self) -> serde_json::Value {
            let line = self.protocol.recv_timeout(REPLY_BUDGET).unwrap_or_else(|cause| {
                panic!(
                    "nothing arrived on standard output within {}s ({cause}); standard error so far:\n{}",
                    REPLY_BUDGET.as_secs(),
                    self.drain_log().join("\n")
                )
            });
            serde_json::from_str(&line).unwrap_or_else(|cause| {
                panic!(
                    "standard output is the PROTOCOL channel and this line is not a JSON-RPC message \
                     ({cause}): {line}"
                )
            })
        }

        /// One request, and the whole reply object - error replies included.
        fn exchange(&mut self, method: &str, params: &serde_json::Value) -> serde_json::Value {
            self.next_id = self.next_id.saturating_add(1);
            let id = self.next_id;
            self.write(&serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
            let reply = self.reply();
            // The envelope, asserted once here rather than in every test: a reply that answered a
            // different request would otherwise be read as this one's.
            assert_eq!(reply["jsonrpc"], "2.0", "not a JSON-RPC 2.0 reply: {reply}");
            assert_eq!(
                reply["id"],
                serde_json::json!(id),
                "the reply answers another request: {reply}"
            );
            reply
        }

        /// One request whose reply has to be a result.
        fn request(&mut self, method: &str, params: &serde_json::Value) -> serde_json::Value {
            let reply = self.exchange(method, params);
            assert!(
                reply.get("error").is_none(),
                "`{method}` was refused at the protocol level: {reply}"
            );
            reply["result"].clone()
        }

        /// A notification: no id, so no reply to read.
        fn notify(&mut self, method: &str) {
            self.write(&serde_json::json!({ "jsonrpc": "2.0", "method": method }));
        }

        /// The handshake, and the result it produced.
        ///
        /// `notifications/initialized` is sent because a client owes it, not because this server
        /// waits for it - `rmcp` enters its service loop as soon as it has answered `initialize`.
        /// Sending it is what keeps this client honest against a server that does gate on it.
        fn initialize(&mut self) -> serde_json::Value {
            let result = self.request(
                "initialize",
                &serde_json::json!({
                    "protocolVersion": PROTOCOL,
                    "capabilities": {},
                    "clientInfo": { "name": "sutura-mcp-e2e", "version": "0" },
                }),
            );
            self.notify("notifications/initialized");
            result
        }

        /// One tool call's result content.
        fn call(&mut self, capability: Capability, arguments: &serde_json::Value) -> serde_json::Value {
            self.request(
                "tools/call",
                &serde_json::json!({ "name": capability.id(), "arguments": arguments }),
            )
        }

        /// Waits for a line on standard error containing `text`.
        fn expect_log(&self, text: &str) -> String {
            let deadline = Instant::now() + NOTICE_BUDGET;
            let mut seen = Vec::new();
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                let line = self
                    .log
                    .recv_timeout(left)
                    .unwrap_or_else(|cause| panic!("nothing on standard error said `{text}` ({cause}):\n{}", seen.join("\n")));
                if line.contains(text) {
                    return line;
                }
                seen.push(line);
            }
        }

        /// Whatever standard error has produced by now. Non-blocking: for a failure message.
        fn drain_log(&self) -> Vec<String> {
            let mut out = Vec::new();
            loop {
                match self.log.try_recv() {
                    Ok(line) => out.push(line),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
                }
            }
        }

        /// Closes the pipe the way a client that is finished does, and returns how the process
        /// exited.
        ///
        /// **This IS the shutdown path for this transport**, which is why there is no signal here
        /// and no `SIGTERM` counterpart to `served.rs`'s `terminate`: an agent client that goes away
        /// closes its end, `rmcp`'s stdio transport sees end-of-file, `waiting()` returns and the
        /// command's own `shutdown_timeout` and `drop` run.
        fn close(&mut self) -> ExitStatus {
            drop(self.stdin.take());
            let deadline = Instant::now() + STOP_BUDGET;
            loop {
                if let Some(status) = self.child.try_wait().expect("the child is waitable") {
                    self.reaped = true;
                    return status;
                }
                assert!(
                    Instant::now() < deadline,
                    "the process was still running {}s after its peer closed the pipe; standard error:\n{}",
                    STOP_BUDGET.as_secs(),
                    self.drain_log().join("\n")
                );
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }

    impl Drop for Agent {
        /// Never leaves a process behind, including after a panicking assertion.
        ///
        /// `SIGKILL` here and a closed pipe in [`Agent::close`]: this path runs when a test has
        /// already failed, so what it owes is cleanup rather than a drain.
        fn drop(&mut self) {
            if !self.reaped {
                drop(self.child.kill());
                drop(self.child.wait());
            }
        }
    }

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

    /// The grain every question in this file asks at.
    const GRAIN: &str = "month";

    /// A question as a tool call carries it, mirrored from the example's own fixture.
    ///
    /// **The fixture is READ rather than cited in a comment**, the way `served.rs` does it and for
    /// the same reason: `stem` names the file under `examples/single-player/questions/`, and the
    /// metric, the grain and both dates have to appear in it - so a fixture renamed, deleted or
    /// re-ranged goes red HERE instead of leaving this suite asking something the example no longer
    /// asks. That is the property #117 wants out of a file like this one: the example a reader is
    /// told to run is the example CI runs.
    ///
    /// A substring check and not a parse: `tests/example.rs` parses every question in that directory
    /// and pins what each one compiles to, and what is guarded here is a fixture that MOVED.
    fn question(stem: &str, metric: &str, start: &str, end: &str) -> serde_json::Value {
        let path = example_root().join("questions").join(format!("{stem}.yaml"));
        let fixture = std::fs::read_to_string(&path).unwrap_or_else(|cause| {
            panic!(
                "{} is the example question this call mirrors, and it is not readable: {cause}",
                path.display()
            )
        });
        for expected in [metric, GRAIN, start, end] {
            assert!(
                fixture.contains(expected),
                "{} no longer mentions `{expected}`, so this suite asks something the example does \
                 not:\n{fixture}",
                path.display()
            );
        }
        serde_json::json!({
            "metric": metric,
            "grain": GRAIN,
            "range": { "start": start, "end": end },
        })
    }

    /// The one question this file asserts numbers for.
    fn recurring_revenue_june() -> serde_json::Value {
        question("recurring-revenue-june", "recurring_revenue", "2026-06-01", "2026-07-01")
    }

    /// The example's own metric-unknown fixture, which exists to be refused.
    fn a_metric_this_catalog_does_not_define() -> serde_json::Value {
        question(
            "refused-metric-unknown",
            "customer_lifetime_value",
            "2026-06-01",
            "2026-07-01",
        )
    }

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
        assert!(
            result["instructions"].as_str().is_some_and(|text| !text.is_empty()),
            "the surface introduced itself with no instructions: {result}"
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
    fn the_schemas_the_binary_advertises_are_the_committed_ones() {
        // #117's own words for this branch: *list the tools, call one, and compare the reply against
        // the committed schema snapshots*. The comparison is what makes it a contract rather than a
        // smoke test - `sutura-mcp`'s snapshot test proves the GENERATOR, and this proves that what
        // a client generator reads off the wire is the same document a reviewer accepted.
        let mut agent = spawn();
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
        // `tests/example.rs` asserts over the libraries and `crates/sutura-serve/tests/served.rs`
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
        // And which identity produced the one leg. `shared-service-user` is the truth for this
        // surface: a pipe establishes no caller, and no source this binary opens executes as an
        // asking subject.
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

    #[test]
    fn a_refusal_arrives_as_a_tool_result_rather_than_a_protocol_error() {
        // *Refusal is a result, not an error*, on the wire and on the composed process. The bug this
        // prevents is a governance refusal reaching an agent as a JSON-RPC error, which a client
        // retries and a model reads as an outage rather than as an answer it may not have.
        let mut agent = spawn();
        drop(agent.initialize());
        // `request` already fails if the reply carried an `error`, which is half the claim.
        let result = agent.call(Capability::AskMetric, &a_metric_this_catalog_does_not_define());
        assert_ne!(
            result["isError"],
            serde_json::json!(true),
            "a refusal was reported as a failure: {result}"
        );

        let content = &result["structuredContent"];
        assert_eq!(content["outcome"], "refusal", "{result}");
        // The code, not the sentence: the code is the contract a client branches on and the sentence
        // is written for a person. `metric_unknown` is `RefusalReason::MetricUnknown` in snake_case,
        // which is the derivation `sutura_mcp::refusal`'s own test pins.
        assert_eq!(content["reason"]["code"], "metric_unknown", "{result}");
        assert!(
            content["reason"]["detail"].as_str().is_some_and(|detail| !detail.is_empty()),
            "a refusal arrived with no sentence for a person: {result}"
        );

        assert!(agent.close().success(), "the process did not exit cleanly");
    }

    #[test]
    fn a_tool_this_surface_does_not_have_is_a_protocol_error() {
        // The other side of the line above, and the reason both are here: a question this deployment
        // declines is a RESULT, and a tool that does not exist is a PROTOCOL fault. A surface that
        // answered the second one the way it answers the first would be inventing a refusal for a
        // call it never understood.
        //
        // The name is the one this surface may never have. `Query` declares no field for SQL and
        // `Capability` declares no such tool, so what is asserted here is that the composed process
        // says so in the protocol's own vocabulary rather than in prose.
        let mut agent = spawn();
        drop(agent.initialize());
        let reply = agent.exchange(
            "tools/call",
            &serde_json::json!({ "name": "run_sql", "arguments": { "sql": "select 1" } }),
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
        let listed = agent.request("tools/list", &serde_json::json!({}));
        assert_eq!(
            listed["tools"].as_array().map(Vec::len),
            Some(Capability::every().count()),
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
}

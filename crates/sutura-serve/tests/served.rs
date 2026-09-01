//! The served deployment, asked a question over HTTP.
//!
//! # What this is for
//!
//! The twenty-one named tests beside this file are startup refusals - a source with no declaration,
//! a kind this build cannot open, an anchor with no verification identity. They are the right tests
//! and not one of them starts the service and asks anything. Above them, `sutura_http::harness`
//! exercises the assembled router in-crate with a fake service, and `sutura-cli`'s example test
//! runs the example through the CLI, which is a different composition with no HTTP, no MCP and no
//! leg 1.
//!
//! **So the path a user actually takes - a settings file, a catalog directory, a listener, a
//! bearer token, one question, rows - was exercised by nothing in any tier.** This file is that
//! path. Every test here spawns the real `sutura-serve` binary through
//! `CARGO_BIN_EXE_sutura-serve` - the composition root this repository ships, not a re-assembled
//! one - points it at a settings file written over `examples/single-player`, reads the OS-assigned
//! port off its own startup log, and answers a real HTTP request against it. A test that builds its
//! own router proves the router; this is a proof of the deployment.
//!
//! The unit tests above never needed a process, because `open_engine` returns a value and a refusal
//! is a unit-testable value. Everything past `serve_as_configured` needs a listener, which needs a
//! runtime, a port and a temporary directory - which is exactly what this file supplies, and what
//! the lack of it made the gap invisible for so long: no gate could notice a test nobody wrote.
//!
//! # The port comes from the OS
//!
//! The settings file writes `server.port: 0`, so the kernel chooses. The bound address is read back
//! out of the serve process's own `listening` log line, which is the same line an operator would
//! see - this is also how the harness knows the deployment is ready before the first request. A
//! fixed port is deliberately never used: two checks share a sandbox, and a hard-coded port is how
//! that becomes a flake.
//!
//! # The shutdown is the real one
//!
//! Each test ends by sending `SIGTERM` into the listener and asserting the process drains and exits
//! with status zero - the same path an orchestrator takes it down with. The signal is sent by the
//! `kill` UTILITY rather than by `libc::kill`, because this workspace sets
//! `unsafe_code = "forbid"` and opening an FFI door for a test would be an architecture decision
//! the e2e suite is not. `kill` is coreutils (present in the nix sandbox `checks.nextest` runs in)
//! and `/bin/kill` on macOS.
//!
//! # Why this can be a gate
//!
//! A `files` source needs no network, so unlike the acceptance legs this is a real `checks.nextest`
//! gate rather than a `nix run` app: it runs in the sandbox, on loopback, against a port the OS
//! hands out. Run it locally with `just serve-e2e`.

#[cfg(test)]
mod tests {
    use std::io::BufRead as _;
    use std::net::SocketAddr;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
    use std::time::{Duration, Instant};

    /// A token that satisfies the configured floor: at least thirty-two RFC 6750 `b64token` chars.
    ///
    /// Thirty-two is the documented minimum, so the deployment below starts; the same constant
    /// configures the deployment and rides in the requests that authenticate to it.
    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// What the served bundle calls its snapshot, and what provenance therefore reports.
    ///
    /// Fixed rather than taken from the working tree, so the assertion on `definition_version` is
    /// about the served bundle rather than about whichever commit happened to build it.
    const VERSION: &str = "served-e2e";

    /// How long the served process may take to report its bound address.
    ///
    /// Startup loads a catalog, re-executes every anchor against the CSVs and registers the engine,
    /// so this is seconds rather than milliseconds; thirty is generous and finite, because an hung
    /// boot must fail the test rather than hang the suite.
    const READY_TIMEOUT: Duration = Duration::from_secs(30);

    /// The single-player example, which is what the quickstart and the CLI's own test run over.
    fn example_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
    }

    fn catalog_dir() -> PathBuf {
        example_root().join("catalog")
    }

    fn data_dir() -> PathBuf {
        example_root().join("data")
    }

    /// One serving deployment: the child process and the client that talks to it.
    struct Serving {
        child: Child,
        agent: ureq::Agent,
        address: SocketAddr,
    }

    impl Serving {
        /// The address the OS actually gave the listener, read off the startup log.
        fn address(&self) -> SocketAddr {
            self.address
        }

        /// One GET, with or without the deployment token, returning status and body.
        fn get(&self, path: &str, token: Option<&str>) -> (u16, String) {
            let mut request = self.agent.get(&self.url(path));
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            let response = request.call().unwrap_or_else(|cause| panic!("GET {path} failed: {cause}"));
            response_of(response)
        }

        /// One certified question as JSON, through the real `POST /v1/query` handler.
        fn ask(&self, token: Option<&str>, question: &str) -> (u16, String) {
            let mut request = self
                .agent
                .post(&self.url("/v1/query"))
                .header("content-type", "application/json");
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            let response = request
                .send(question)
                .unwrap_or_else(|cause| panic!("the question failed to send: {cause}"));
            response_of(response)
        }

        fn url(&self, path: &str) -> String {
            format!("http://{}{}", self.address, path)
        }

        /// The real shutdown path: a `SIGTERM` into the listener, and the process drains and exits.
        fn stop(&mut self) -> ExitStatus {
            let status = Command::new("kill")
                .arg("-TERM")
                .arg(self.child.id().to_string())
                .status()
                .unwrap_or_else(|cause| panic!("could not run the `kill` utility: {cause}"));
            assert!(status.success(), "`kill -TERM` did not reach the serve process: {status}");
            let exit = self
                .child
                .wait()
                .unwrap_or_else(|cause| panic!("could not wait on the serve process: {cause}"));
            assert_eq!(
                exit.code(),
                Some(0),
                "the serve process did not drain and exit cleanly on SIGTERM"
            );
            exit
        }
    }

    impl Drop for Serving {
        fn drop(&mut self) {
            // A backstop for a test that panicked before graceful stop: nothing is left serving.
            drop(self.child.kill());
            drop(self.child.wait());
        }
    }

    /// Reads the status and the whole body off a response.
    ///
    /// `http_status_as_error(false)` on the agent makes the client hand every status back on the
    /// response rather than through `Error::StatusCode`, which is what lets a refusal be asserted
    /// as a response here instead of as a result.
    fn response_of(mut response: ureq::http::Response<ureq::Body>) -> (u16, String) {
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .unwrap_or_else(|cause| panic!("could not read the response body: {cause}"));
        (status, text)
    }

    /// Writes the settings file a served deployment would hand this binary, over the example.
    ///
    /// Written into a per-process temp directory (the pid is unique to this nextest process, so two
    /// of these cannot collide), with `SUTURA_CONFIG_DIR` pointing at it when the binary is spawned.
    /// The paths are absolute, because a relative `catalog.dir` resolves against the working
    /// directory the supervisor chose, which for a nextest process is not this tree.
    fn write_config() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-serve-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir)
            .unwrap_or_else(|cause| panic!("could not create the config directory {}: {cause}", dir.display()));
        let config = format!(
            "server:\n  port: 0\nsecurity:\n  access_token: \"{TOKEN}\"\n  identity: \"single-user\"\n  \
             single_user_because: \"the served-e2e suite reads its own fixture files\"\ncatalogs:\n  \
             - name: \"model\"\n    kind: \"markdown\"\n    dir: \"{}\"\n    data_dir: \"{}\"\n    version: \
             \"{VERSION}\"\nsources:\n  local:\n    kind: \"files\"\n    data_dir: \"{}\"\n    posture: \
             \"shared-service-user\"\ntelemetry:\n  format: \"bunyan\"\n",
            catalog_dir().display(),
            data_dir().display(),
            data_dir().display()
        );
        std::fs::write(dir.join("base.yaml"), config).unwrap_or_else(|cause| panic!("could not write base.yaml: {cause}"));
        dir
    }

    /// Spawns the real binary, and returns it with its stdout piped to a reader thread.
    fn spawn(config_dir: &Path) -> (Child, Receiver<String>) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sutura-serve"))
            .env("SUTURA_CONFIG_DIR", config_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|cause| panic!("could not spawn the serve binary: {cause}"));
        let stdout = child.stdout.take().expect("the serve binary's stdout is piped");
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        (child, receiver)
    }

    /// Waits for the `listening` line and reads the OS-assigned address off it.
    ///
    /// Reading the log is the readiness signal as well as the port source: the deployment is not
    /// ready until it is bound, and the address it bound is the one the requests have to reach.
    fn await_ready(child: &mut Child, logs: &Receiver<String>) -> SocketAddr {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                timed_out(child, READY_TIMEOUT);
            }
            match logs.recv_timeout(remaining) {
                Ok(line) => {
                    if let Some(address) = bound(&line) {
                        return address;
                    }
                }
                Err(RecvTimeoutError::Timeout) => timed_out(child, READY_TIMEOUT),
                Err(RecvTimeoutError::Disconnected) => {
                    let exit = child
                        .wait()
                        .unwrap_or_else(|cause| panic!("could not wait on the serve process: {cause}"));
                    panic!("the serve process exited before it was listening: {exit}");
                }
            }
        }
    }

    /// The absent-completion arms of the readiness loop, so the loop reads as one decision.
    fn timed_out(child: &mut Child, timeout: Duration) -> ! {
        drop(child.kill());
        drop(child.wait());
        panic!("the serve process did not report a bound address within {timeout:?}");
    }

    /// The bound address a bunyan startup line carries, if it is the server's `listening` line.
    ///
    /// Matched on the field `bound` (the banner's own line says `bind`, spelling it differently) so
    /// the port a request must reach is read off the line that says the listener is actually up.
    fn bound(line: &str) -> Option<SocketAddr> {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        if value.get("msg").and_then(serde_json::Value::as_str) != Some("listening") {
            return None;
        }
        value.get("bound")?.as_str()?.parse().ok()
    }

    /// The whole harness in one call: config, process, port.
    fn boot() -> Serving {
        let config_dir = write_config();
        let (mut child, logs) = spawn(&config_dir);
        let address = await_ready(&mut child, &logs);
        drop(logs);
        let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder().http_status_as_error(false).build());
        Serving { child, agent, address }
    }

    /// The question the example README answers on its first page, and the CLI test pins as rows.
    ///
    /// A whole six months so the assertion is about a measure SET, not about one number; the figures
    /// are the same certified figures the README's table shows.
    const RECURRING_REVENUE_BY_MONTH: &str =
        r#"{"metric":"recurring_revenue","grain":"month","range":{"start":"2026-01-01","end":"2026-07-01"}}"#;

    // ------------------------------------------------------------------ the harness ---

    #[test]
    fn the_liveness_probe_answers_through_the_real_listener() {
        // The harness's own first test, deliberately trivial: everything else in this file stands
        // on the assertion that a spawned deployment on an OS-chosen port answers a request. If
        // this one fails, the failures below are about the vehicle, not about the service.
        let mut served = boot();
        let (status, body) = served.get("/health", None);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body, r#"{"status":"ok"}"#);
        // The settings asked for port zero; what the process bound is what the OS gave it, which is
        // how the deployment on an ephemeral port stays reachable by a test.
        assert_ne!(
            served.address().port(),
            0,
            "the OS-assigned port was not used: {}",
            served.address()
        );
        // And the process leaves through its own shutdown path, not through the harness killing it.
        // `stop` asserts a zero exit, which is what drains: a clean drain is a process that stopped
        // on its own terms rather than one this test killed.
        served.stop();
    }

    // ------------------------------------------------------------------ the token gate ---

    #[test]
    fn the_bearer_gate_refuses_without_a_token_and_answers_with_one() {
        let mut served = boot();
        let (status, body) = served.get("/v1/catalog", None);
        assert_eq!(status, 401, "{body}");
        assert!(body.contains(r#""code":"unauthorized""#), "{body}");

        let (status, _) = served.get("/v1/catalog", Some("wrong-but-long-enough"));
        assert_eq!(status, 401);

        let (status, body) = served.get("/v1/catalog", Some(TOKEN));
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("recurring_revenue"), "{body}");

        let (status, _) = served.ask(None, RECURRING_REVENUE_BY_MONTH);
        assert_eq!(status, 401);

        served.stop();
    }

    // ------------------------------------------------------------------ a certified question ---

    #[test]
    fn a_configured_deployment_answers_a_certified_question_over_http() {
        let mut served = boot();
        let (status, body) = served.ask(Some(TOKEN), RECURRING_REVENUE_BY_MONTH);
        assert_eq!(status, 200, "{body}");
        let document: serde_json::Value = serde_json::from_str(&body).expect("an answer is JSON");
        assert_eq!(document["outcome"], "answer", "{body}");
        assert_eq!(document["columns"], serde_json::json!(["period", "recurring_revenue"]));
        let rows = document["rows"].as_array().expect("an answer carries its rows");
        let period: Vec<&str> = rows
            .iter()
            .map(|row| row[0].as_str().expect("the period column is text"))
            .collect();
        let revenue: Vec<&str> = rows
            .iter()
            .map(|row| row[1].as_str().expect("the measure column is text"))
            .collect();
        // The same figures `crates/sutura-cli/tests/example.rs` pins and the README prints: one
        // certified number per month, over HTTP, through the real composition root.
        assert_eq!(
            period,
            [
                "2026-01-01",
                "2026-02-01",
                "2026-03-01",
                "2026-04-01",
                "2026-05-01",
                "2026-06-01"
            ]
        );
        assert_eq!(revenue, ["237320", "232822", "216700", "206160", "202994", "202121"]);
        // The answer says which leg produced it and under which bundle - the two things that make a
        // number attributable, carried over the wire because the domain requires them of an answer.
        assert_eq!(document["executed_as"][0]["source"], "local");
        assert_eq!(document["executed_as"][0]["posture"], "shared-service-user");
        assert_eq!(document["provenance"]["definition_version"], VERSION);
        assert!(
            document["provenance"]["definition_digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty())
        );
        served.stop();
    }

    // ------------------------------------------------------------------ a refusal, end to end ---

    #[test]
    fn a_refusal_reaches_the_caller_as_its_documented_status() {
        // The transport's exhaustive match over the refusal vocabulary, on the composed binary: the
        // question named by `examples/single-player/questions/refused-metric-unknown.yaml`. What a
        // call straight into `sutura-app` cannot prove is that the status survives the REAL router,
        // and what this adds over `sutura_http::harness` is that the whole stack is there - catalog
        // loaded, anchors reproduced, listener bound.
        let mut served = boot();
        let (status, body) = served.ask(
            Some(TOKEN),
            r#"{"metric":"customer_lifetime_value","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#,
        );
        assert_eq!(status, 404, "{body}");
        let document: serde_json::Value = serde_json::from_str(&body).expect("a refusal is JSON");
        assert_eq!(document["outcome"], "refusal", "{body}");
        assert_eq!(document["reason"]["code"], "metric_unknown");
        assert_eq!(document["reason"]["status"].as_u64(), Some(404));
        assert!(
            document["reason"]["detail"].as_str().is_some_and(|detail| !detail.is_empty()),
            "{body}"
        );
        served.stop();
    }

    // ------------------------------------------------------------------ the served document ---

    #[test]
    fn the_served_document_names_every_governed_route() {
        // The generated interface description AS SERVED by a running deployment, rather than as
        // generated in a unit test. It is behind the token, like the versioned API it describes; and
        // its route set is exactly `crate::capability::governed`'s - the liveness probe plus the two
        // operations, mounted under the version prefix. If the served document ever dropped a route,
        // both this and `RouteNotGoverned` at assembly would say so; this is the half a router test
        // built in-crate cannot see.
        let mut served = boot();
        let (status, _) = served.get("/openapi.json", None);
        assert_eq!(status, 401, "the interface description is served behind the token");

        let (status, body) = served.get("/openapi.json", Some(TOKEN));
        assert_eq!(status, 200, "{body}");
        let document: serde_json::Value = serde_json::from_str(&body).expect("the served document is JSON");
        let paths: Vec<&str> = document["paths"]
            .as_object()
            .expect("the document carries a paths map")
            .keys()
            .map(String::as_str)
            .collect();
        let mut sorted = paths;
        sorted.sort_unstable();
        assert_eq!(sorted, ["/health", "/v1/catalog", "/v1/query"]);
        served.stop();
    }
}

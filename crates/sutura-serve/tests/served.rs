//! The deployment a user actually gets: a settings file, a catalog directory, a listener, a bearer
//! token, one question, rows.
//!
//! **This is the only place in the repository where anything asks the SERVED deployment a
//! question.** `github.com/telekom/sutura#117` is the measurement that says why it had to exist:
//! `sutura-serve`'s twenty-one named tests are all startup refusals, `sutura_http::harness`
//! exercises the router in-crate over a fake service, and `crates/sutura-cli/tests/example.rs` runs
//! the example through a different binary with a different composition and no HTTP at all. So every
//! invariant that lives only on the composed path - the bearer gate, route governance, the generated
//! document as SERVED, the refusal statuses out of a real transport - had no end-to-end evidence on
//! any binary in any tier.
//!
//! # It spawns the binary, and that is not a shortcut
//!
//! `sutura-serve` is `[[bin]]` and nothing else: there is no library target, so an integration test
//! cannot call `run` and a test that assembled its own router would be proving the router rather
//! than the composition. `CARGO_BIN_EXE_sutura-serve` is the binary cargo just built from this
//! crate's own `main.rs`, so what is under test is the REAL composition root - the same environment
//! reading, the same layered settings, the same catalog load, the same anchor re-execution, the same
//! `ServiceState`, the same router, the same listener - reached the only way a deployment reaches it.
//!
//! # Why it can be a gate rather than a `nix run` app
//!
//! A files-backed source needs no network and no credential, so unlike `just bigquery-acceptance`
//! this runs inside `checks.nextest` - which is what makes it a real gate. The port comes from the
//! kernel (`server.port: 0`, read back off the socket and printed by `sutura_http::server::serve`),
//! because two checks share one sandbox and a fixed port is how that becomes a flake.
//!
//! # What the harness reads, and why the log is the channel
//!
//! `telemetry.format: bunyan` is written into the settings so the service's own log is one JSON
//! object per line, and the harness waits for the `listening` event and takes the address off its
//! `bound` field. Not a guess and not a scan for a free port: the kernel chose it, the process
//! reported it, and the two cannot disagree. The same stream is what the ordering assertion in
//! `the_liveness_probe_answers_only_once_the_catalog_has_loaded` reads.
//!
//! # `unix` only
//!
//! Stopping the process is `SIGTERM`, because that is the shutdown path a deployment takes and the
//! one `sutura_runtime::shutdown` installs a handler for. `unsafe_code` is `forbid` in this
//! workspace, so `libc::kill` is not available to a test, and `Child::kill` is `SIGKILL` - which
//! proves the process can be destroyed rather than that it drains. So the signal is sent through
//! `/bin/sh`, whose `kill` is a shell builtin present in every sandbox this suite runs in, and the
//! whole module is `cfg(unix)`.

// `cfg(test)` for the reason `crates/sutura-cli/tests/example.rs` gives: clippy honours
// `allow-expect-in-tests` only inside a `#[cfg(test)]` item, and without it every `expect` in the
// harness below is a lint error.
// `cfg(unix)` and `cfg(test)` as TWO attributes rather than `cfg(all(test, unix))`, which is not a
// style choice: clippy looks for a literal `#[cfg(test)]` on an ancestor module to decide whether
// `allow-expect-in-tests` applies and whether `tests_outside_test_module` fires, and it does not
// see through an `all(..)`. Written the other way this module was thirty-five lint errors.
#[cfg(unix)]
#[cfg(test)]
mod tests {
    use core::fmt::Write as _;
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::net::TcpStream;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
    use std::time::{Duration, Instant};

    /// The deployment's own bearer token, which authenticates the DEPLOYMENT and not a caller.
    ///
    /// Thirty-five characters, because `sutura_config::AccessToken::MIN_LENGTH` is thirty-two and a
    /// fixture shorter than that would be refused at startup rather than at the gate.
    const TOKEN: &str = "e2e-access-token-000000000000000000";

    /// The version label the served bundle is stamped with.
    ///
    /// Fixed rather than read from the working tree, for the reason the CLI example gives: the
    /// digest does not include it, and a version that moved between runs would churn every
    /// assertion that carries provenance.
    const VERSION: &str = "serve-e2e";

    /// How long a start may take before the test gives up and prints the log it has.
    ///
    /// Generous on purpose. Startup loads the catalog and RE-EXECUTES every anchor against the
    /// engine, so this is the cost of validating a bundle rather than of binding a socket, and a
    /// shared CI runner is slower than a laptop at both.
    const START_BUDGET: Duration = Duration::from_secs(120);

    /// How long a `SIGTERM` may take to become an exited process.
    ///
    /// Longer than the `shutdown_grace_seconds` the embedded defaults ship, because the bound is on
    /// the grace period and the process still has to unwind and exit after spending it.
    const STOP_BUDGET: Duration = Duration::from_secs(45);

    /// The example deployment, which is also what a reader is told to run.
    ///
    /// Canonicalised because `sutura_config` refuses a relative `data_dir` - a relative path
    /// resolves against whatever working directory the supervisor chose - and because a path
    /// carrying `..` in a settings file is harder to read in a failure than the real one.
    fn example_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/single-player")
            .canonicalize()
            .expect("the example directory is in this repository")
    }

    /// The settings file the deployment is started from.
    ///
    /// Everything not written here is the embedded default, which is the point: this is a
    /// deployment's own `base.yaml` and not a second copy of the configuration tree. Four things are
    /// said, and each of them is what makes one of the tests below possible.
    ///
    /// * `server.port: 0` - the kernel picks, so two checks in one sandbox cannot collide.
    /// * `security.access_token` - so the bearer gate is armed and its refusal is reachable.
    /// * `telemetry.format: bunyan` - so the harness can read the bound address out of the log.
    /// * `catalogs:` and `sources:` over the example directory - the one catalog and the one data
    ///   system a reader of the quickstart has.
    fn settings(example: &Path) -> String {
        let catalog = example.join("catalog").display().to_string();
        let data = example.join("data").display().to_string();
        format!(
            "server:\n  \
               host: \"127.0.0.1\"\n  \
               port: 0\n\
             security:\n  \
               identity: \"single-user\"\n  \
               single_user_because: \"an end-to-end test reads its own fixture files as one identity\"\n  \
               access_token: \"{TOKEN}\"\n\
             telemetry:\n  \
               format: \"bunyan\"\n\
             catalogs:\n  \
               - name: \"model\"\n    \
                 kind: \"markdown\"\n    \
                 dir: \"{catalog}\"\n    \
                 data_dir: \"{data}\"\n    \
                 version: \"{VERSION}\"\n\
             sources:\n  \
               local:\n    \
                 kind: \"files\"\n    \
                 data_dir: \"{data}\"\n    \
                 posture: \"shared-service-user\"\n"
        )
    }

    /// The command, with this shell's own `SUTURA_*` variables removed.
    ///
    /// **Not cosmetic.** `sutura_config` layers one environment variable per key on top of the
    /// files, so a developer with `SUTURA__SERVER__PORT` exported would be running a different
    /// deployment from CI and the failure would name a setting nobody wrote in this file.
    fn command(config_dir: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sutura-serve"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("SUTURA") {
                command.env_remove(key);
            }
        }
        command
            .env("SUTURA_ENVIRONMENT", "development")
            .env("SUTURA_CONFIG_DIR", config_dir)
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// One HTTP response, as much of it as an assertion needs.
    struct Reply {
        status: u16,
        body: String,
    }

    impl Reply {
        /// The body as JSON, or a failure naming what came back instead.
        fn json(&self) -> serde_json::Value {
            serde_json::from_str(&self.body)
                .unwrap_or_else(|cause| panic!("the {} body is not JSON ({cause}): {}", self.status, self.body))
        }
    }

    /// A running deployment: the process, the address it reported, and its log.
    struct Served {
        child: Child,
        address: String,
        /// Every line the process wrote up to and including `listening`.
        startup: Vec<String>,
        /// Everything it writes afterwards.
        lines: Receiver<String>,
        config_dir: PathBuf,
        reaped: bool,
    }

    /// Reads one stream line by line into the shared channel.
    ///
    /// Both streams go to ONE channel, and that is what stops the test deadlocking: a piped stream
    /// nobody drains fills its buffer and blocks the writer, and the writer here is the process
    /// under test. Relative order within a stream survives, which is all the ordering assertion
    /// below needs - both lines it compares are written by the subscriber, to standard output, from
    /// one thread.
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

    /// Starts the deployment and waits until it says what it bound.
    ///
    /// `case` names the temporary configuration directory, so a failure leaves a directory a reader
    /// can identify. `nextest` runs each test in its own process, so the process id in the name is
    /// what keeps two tests from sharing one.
    // `zombie_processes` cannot see the `Drop` impl below, which is where the wait for a failing test
    // lives: this function hands the child to a `Served`, and every path out of a `Served` - a clean
    // `terminate` or a panicking assertion - reaps it. Reaping here would mean waiting for the service
    // to exit before asking it anything.
    #[expect(clippy::zombie_processes, reason = "the returned `Served` waits on it in `Drop`")]
    fn start(case: &str) -> Served {
        let example = example_root();
        let config_dir = std::env::temp_dir().join(format!("sutura-serve-e2e-{case}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&config_dir));
        std::fs::create_dir_all(&config_dir).expect("the temporary configuration directory is creatable");
        std::fs::write(config_dir.join("base.yaml"), settings(&example)).expect("the settings file is writable");

        let mut child = command(&config_dir).spawn().expect("the composed binary starts");
        let stdout = child.stdout.take().expect("standard output was piped");
        let stderr = child.stderr.take().expect("standard error was piped");
        let (sender, lines) = channel();
        let second = sender.clone();
        drop(std::thread::spawn(move || forward(stdout, &sender)));
        drop(std::thread::spawn(move || forward(stderr, &second)));

        let mut startup = Vec::new();
        let deadline = Instant::now() + START_BUDGET;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = lines
                .recv_timeout(left)
                .unwrap_or_else(|cause| panic!("the service never reported a listener ({cause}):\n{}", startup.join("\n")));
            let bound = bound_address(&line);
            startup.push(line);
            if let Some(address) = bound {
                return Served {
                    child,
                    address,
                    startup,
                    lines,
                    config_dir,
                    reaped: false,
                };
            }
        }
    }

    /// The address a `listening` event reports, if this line is one.
    ///
    /// Reads the event's own `bound` field rather than matching text: that field is
    /// `TcpListener::local_addr` read back off the bound socket, which is the whole reason
    /// `server.port: 0` is usable here.
    fn bound_address(line: &str) -> Option<String> {
        let event: serde_json::Value = serde_json::from_str(line).ok()?;
        if event.get("msg").and_then(serde_json::Value::as_str)? != "listening" {
            return None;
        }
        Some(String::from(event.get("bound").and_then(serde_json::Value::as_str)?))
    }

    impl Served {
        /// A GET, with the deployment's token when one is given.
        fn get(&self, path: &str, token: Option<&str>) -> Reply {
            self.send("GET", path, token, None)
        }

        /// A POST of a JSON question.
        fn post(&self, path: &str, token: Option<&str>, body: &str) -> Reply {
            self.send("POST", path, token, Some(body))
        }

        /// One request over one connection.
        ///
        /// **Hand-written rather than a client crate, deliberately.** The alternative is `ureq`,
        /// which arrives with rustls and `ring`; `crane.buildDepsOnly` is unscoped so the four cross
        /// dependency derivations - two of them musl - would compile that closure for a binary that
        /// links none of it, which is the same cost `sutura-serve`'s `bigquery` feature is
        /// default-off to avoid. What is needed here is one plaintext loopback request with a fixed
        /// shape, so this is thirty lines and no dependency.
        ///
        /// `Connection: close` is what makes reading to end-of-file the whole response, and the
        /// chunked assertion in [`parse`] is what stops that quietly mis-parsing if a handler ever
        /// answers without a length.
        fn send(&self, method: &str, path: &str, token: Option<&str>, body: Option<&str>) -> Reply {
            let mut stream = TcpStream::connect(&self.address).expect("the listener accepts a connection");
            stream
                .set_read_timeout(Some(Duration::from_secs(60)))
                .expect("a read timeout is settable");
            let mut request = String::new();
            write!(request, "{method} {path} HTTP/1.1\r\n").expect("writing to a String cannot fail");
            write!(request, "Host: {}\r\n", self.address).expect("writing to a String cannot fail");
            request.push_str("Connection: close\r\n");
            if let Some(token) = token {
                write!(request, "Authorization: Bearer {token}\r\n").expect("writing to a String cannot fail");
            }
            if let Some(body) = body {
                request.push_str("Content-Type: application/json\r\n");
                write!(request, "Content-Length: {}\r\n", body.len()).expect("writing to a String cannot fail");
            }
            request.push_str("\r\n");
            if let Some(body) = body {
                request.push_str(body);
            }
            stream.write_all(request.as_bytes()).expect("the request is writable");
            stream.flush().expect("the request flushes");
            let mut raw = Vec::new();
            let read = stream.read_to_end(&mut raw).expect("the response is readable");
            assert!(read > 0, "the listener closed the connection without answering");
            parse(&String::from_utf8_lossy(&raw))
        }

        /// Stops the process the way an orchestrator does, and returns how it exited.
        fn terminate(&mut self) -> ExitStatus {
            let signalled = Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("kill -TERM {}", self.child.id()))
                .status()
                .expect("a shell is available to send the signal");
            assert!(signalled.success(), "the terminate signal was not delivered");
            let deadline = Instant::now() + STOP_BUDGET;
            loop {
                if let Some(status) = self.child.try_wait().expect("the child is waitable") {
                    self.reaped = true;
                    return status;
                }
                assert!(
                    Instant::now() < deadline,
                    "the process was still running {}s after SIGTERM",
                    STOP_BUDGET.as_secs()
                );
                std::thread::sleep(Duration::from_millis(25));
            }
        }

        /// The startup log, plus everything written since.
        ///
        /// Non-blocking: whatever has arrived by now is the answer. Called after
        /// [`Served::terminate`] has reaped the process, so the reader threads have already seen
        /// end-of-file.
        fn log(&self) -> Vec<String> {
            let mut out = self.startup.clone();
            loop {
                match self.lines.try_recv() {
                    Ok(line) => out.push(line),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
                }
            }
        }
    }

    impl Drop for Served {
        /// Never leaves a process or a directory behind, including after a panicking assertion.
        ///
        /// `SIGKILL` here and `SIGTERM` in [`Served::terminate`]: this path runs when a test has
        /// already failed, so what it owes is cleanup rather than a drain.
        fn drop(&mut self) {
            if !self.reaped {
                drop(self.child.kill());
                drop(self.child.wait());
            }
            drop(std::fs::remove_dir_all(&self.config_dir));
        }
    }

    /// Splits a response into its status and its body.
    fn parse(text: &str) -> Reply {
        let (head, body) = text
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("not an HTTP response: {text}"));
        assert!(
            !head.to_ascii_lowercase().contains("transfer-encoding: chunked"),
            "this harness reads a length-delimited response and got a chunked one:\n{head}"
        );
        let status_line = head.lines().next().unwrap_or_default();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or_else(|| panic!("no status in `{status_line}`"));
        Reply {
            status,
            body: String::from(body),
        }
    }

    /// Where an event with exactly this message first appears in the log.
    ///
    /// Matches the bunyan `msg` field WHOLE rather than as a substring, and that was MEASURED rather
    /// than preferred. The banner writes `listening on loopback only - reachable from this host and no
    /// other` before the catalog is loaded, so a substring search for `listening` found that line and
    /// the ordering assertion below went red against a service that was in fact ordered correctly. A
    /// search loose enough to match the wrong event reports the wrong thing in both directions.
    fn position(log: &[String], message: &str) -> usize {
        let field = format!("\"msg\":\"{message}\"");
        log.iter()
            .position(|line| line.contains(&field))
            .unwrap_or_else(|| panic!("no event says `{message}`:\n{}", log.join("\n")))
    }

    /// A question as it arrives on the wire.
    fn question(metric: &str, start: &str, end: &str) -> String {
        format!(r#"{{"metric":"{metric}","grain":"month","range":{{"start":"{start}","end":"{end}"}}}}"#)
    }

    /// The one question this file asserts numbers for.
    ///
    /// `examples/single-player/questions/recurring-revenue-june.yaml` as a body - which is the point:
    /// the corpus a reader is told to run is the corpus this asks over HTTP.
    fn recurring_revenue_june() -> String {
        question("recurring_revenue", "2026-06-01", "2026-07-01")
    }

    /// The versioned query route, composed the way the router composes it.
    fn query_path() -> String {
        format!(
            "{}{}",
            sutura_http::constants::API_V1_PREFIX,
            sutura_http::constants::base_paths::QUERY
        )
    }

    /// The versioned catalog route.
    fn catalog_path() -> String {
        format!(
            "{}{}",
            sutura_http::constants::API_V1_PREFIX,
            sutura_http::constants::base_paths::CATALOG
        )
    }

    // ------------------------------------------------------------------- the harness itself ---

    #[test]
    fn the_liveness_probe_answers_only_once_the_catalog_has_loaded() {
        // The harness's own first test, kept trivial on purpose: a harness whose first assertion is
        // complicated is a harness whose failures mean nothing. If this is red, nothing below it is
        // worth reading.
        //
        // The ORDERING half is #117's open question - "the liveness probe answers before the catalog
        // is loaded, or does not, and the test says which" - and this is the answer: it does not.
        // The listener is opened at step 7 of `main::run` and the catalog is loaded at step 6, so
        // there is no window in which this process accepts a connection with an unvalidated bundle
        // behind it. That is also why `sutura_http::routes::health` has no readiness route: a
        // process that is listening has already reproduced every anchor its author certified.
        let served = start("liveness");
        let probe = served.get(sutura_http::constants::HEALTH_PATH, None);
        assert_eq!(probe.status, 200, "{}", probe.body);
        // Byte for byte, which is `routes::health`'s own rule: the body carries no version, no
        // build and no catalog, and this is the assertion that fails when somebody adds one.
        assert_eq!(probe.body, r#"{"status":"ok"}"#);

        let log = served.log();
        assert!(
            position(&log, "catalog loaded and every anchor reproduced its number") < position(&log, "listening"),
            "the listener opened before the bundle was validated:\n{}",
            log.join("\n")
        );
    }

    #[test]
    fn the_real_shutdown_path_stops_the_process_on_a_terminate_signal() {
        // The other half of the harness, and the one that makes every test above it honest: the
        // process is stopped the way an orchestrator stops it, and it exits successfully rather than
        // being destroyed. `Child::kill` would prove neither.
        //
        // `stopped` is the last line `main::stop` writes, after `Runtime::shutdown_timeout` has
        // given the blocking pool what was left of the grace period - so its presence is the
        // evidence that the configured drain ran rather than that the process vanished.
        let mut served = start("shutdown");
        assert_eq!(served.get(sutura_http::constants::HEALTH_PATH, None).status, 200);
        let status = served.terminate();
        assert!(
            status.success(),
            "a terminate signal did not produce a clean exit: {status:?}"
        );
        let log = served.log();
        assert!(
            position(&log, "stopped") > position(&log, "listening"),
            "the process did not report stopping:\n{}",
            log.join("\n")
        );
    }

    // ---------------------------------------------------------------------------- the cases ---

    #[test]
    fn a_question_with_no_bearer_token_is_refused_by_the_gate() {
        // The bug this prevents: a deployment that configured a token and serves its whole surface
        // to anybody, because the layer was mounted on a subtree the versioned routes are not
        // under. Nothing else in the repository can see that - `sutura_http`'s harness builds its
        // own router, so it proves the layer and not the composition.
        //
        // All three token-gated paths, because the gate is one layer and a hole in it is a hole per
        // path. The probe is deliberately NOT here: it is the one path with no token, asserted by
        // the harness test above.
        let served = start("no-token");
        for reply in [
            served.post(&query_path(), None, &recurring_revenue_june()),
            served.get(&catalog_path(), None),
            served.get(sutura_http::constants::OPENAPI_JSON_PATH, None),
        ] {
            assert_eq!(reply.status, 401, "{}", reply.body);
            assert_eq!(reply.json()["code"], "unauthorized", "{}", reply.body);
        }
    }

    #[test]
    fn a_configured_deployment_answers_a_certified_question_over_http() {
        // The sentence roadmap #22 was waiting for, and the assertion
        // `crates/sutura-cli/tests/example.rs` already makes over the libraries - made here over
        // HTTP, on the composed binary, through the real router, the real service, the real static
        // broker and the real engine.
        //
        // The number is the example's own: `recurring-revenue-june__rows.snap` pins
        // `[["2026-06-01", "202121"]]`, and `recurring_revenue` declares an anchor, so that figure
        // was re-executed at startup before this process ever listened. Asserted by value rather
        // than snapshotted: it is one row of two cells, and a second copy of the CLI's snapshot
        // would be a file to keep in step rather than a claim.
        let served = start("answer");
        let reply = served.post(&query_path(), Some(TOKEN), &recurring_revenue_june());
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "answer", "{}", reply.body);
        assert_eq!(body["columns"], serde_json::json!(["period", "recurring_revenue"]));
        assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "202121"]]));
        // Provenance travels with the answer, and the version is the one this deployment declared -
        // so a bundle served from somewhere else would be visible here rather than inferred.
        assert_eq!(body["provenance"]["definition_version"], VERSION);
        assert!(
            body["provenance"]["definition_digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty()),
            "an answer arrived with no definition digest: {}",
            reply.body
        );
        // And which identity produced the one leg. `shared-service-user` is the truth for this
        // deployment: no source a shipped binary serves executes as the asking subject.
        assert_eq!(
            body["executed_as"],
            serde_json::json!([{ "source": "local", "posture": "shared-service-user" }])
        );
    }

    #[test]
    fn the_catalog_route_describes_the_bundle_this_deployment_serves() {
        // The second governed operation, over the same composition. It is here rather than folded
        // into the answer test because the two routes are two capabilities, and a route that stopped
        // being mounted is a different failure from one that stopped answering.
        let served = start("catalog");
        let reply = served.get(&catalog_path(), Some(TOKEN));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["provenance"]["definition_version"], VERSION);
        let metrics = body["metrics"].as_array().cloned().unwrap_or_default();
        assert!(!metrics.is_empty(), "the served catalog describes no metric: {}", reply.body);
        assert!(
            metrics.iter().any(|metric| metric["name"] == "recurring_revenue"),
            "the metric this suite asks about is not in the served catalog: {}",
            reply.body
        );
    }

    #[test]
    fn a_refusal_reaches_the_caller_as_its_documented_status() {
        // The bug this prevents: a governance refusal arriving as a 500. The exhaustive match in
        // `sutura_http::wire::refusal` decides the status, and its own tests assert the mapping over
        // the enum - what none of them can say is that the composed transport still says it once a
        // real service, a real serializer and a real listener are in the way.
        //
        // Two refusals rather than one, and they are chosen to be different SHAPES: one the compiler
        // refuses because the catalog does not define the metric, one because the question's own
        // bounds are exceeded. Both are questions `examples/single-player/questions/` already holds
        // as `refused-` fixtures.
        let served = start("refusal");
        for (body, status, code) in [
            (
                question("customer_lifetime_value", "2026-06-01", "2026-07-01"),
                404_u16,
                "metric_unknown",
            ),
            (
                question("recurring_revenue", "0001-01-01", "9999-12-31"),
                422_u16,
                "time_range_too_long",
            ),
        ] {
            let reply = served.post(&query_path(), Some(TOKEN), &body);
            assert_eq!(reply.status, status, "{}", reply.body);
            let json = reply.json();
            assert_eq!(json["outcome"], "refusal", "{}", reply.body);
            assert_eq!(json["reason"]["code"], code, "{}", reply.body);
            // The body repeats the status, and a client that logged only the body still has it -
            // so the two have to agree or one of the two readers is being told something else.
            assert_eq!(json["reason"]["status"], status, "{}", reply.body);
            assert!(
                json["reason"]["detail"].as_str().is_some_and(|detail| !detail.is_empty()),
                "a refusal arrived with no sentence: {}",
                reply.body
            );
        }
    }

    #[test]
    fn the_served_document_names_every_governed_route() {
        // The generated document as SERVED. `sutura_http::openapi`'s own tests read `document()`
        // in-process, which proves the generator; this reads the bytes a caller gets from the
        // composed binary, which is what a client generator would consume.
        //
        // Derived from `sutura_http::governed()` rather than from a list written here, so a third
        // capability is covered by this test the day it is added - and a route that is mounted,
        // governed and MISSING from the document fails here rather than at whoever generated a
        // client from it.
        let served = start("document");
        let reply = served.get(sutura_http::constants::OPENAPI_JSON_PATH, Some(TOKEN));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let document = reply.json();
        for route in sutura_http::governed() {
            let item = &document["paths"][route.route()];
            assert!(
                !item.is_null(),
                "{} is governed and absent from the served document: {}",
                route.route(),
                document["paths"]
            );
            assert!(
                item.as_object().is_some_and(|methods| !methods.is_empty()),
                "{} is in the served document with no operation on it",
                route.route()
            );
        }
        // Liveness is outside the version prefix and still described, which is what an orchestrator
        // reads the document for.
        assert!(
            !document["paths"][sutura_http::constants::HEALTH_PATH].is_null(),
            "the served document does not describe the liveness probe: {}",
            document["paths"]
        );
    }
}

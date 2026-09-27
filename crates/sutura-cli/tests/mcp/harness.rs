//! Everything `tests/mcp.rs`'s child modules need to spawn the agent surface, and no `#[test]`.

// No `#[cfg(test)]` of its own: the declaration in `mcp.rs` carries the literal one clippy needs for
// `allow-expect-in-tests`, and a second makes `just causality` read a new test module naming no test.

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
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
pub(super) const PROTOCOL: &str = "2025-06-18";

/// The version the CLI stamps a bundle with: `crate::commands::DEFAULT_VERSION`.
///
/// Spelled out rather than imported, because an integration test links the BINARY's crate as a
/// library it has no access to. A drift here is a provenance change, which is exactly what an
/// assertion on it should catch.
pub(super) const VERSION: &str = "local-working-tree";

/// The source name the CLI opens a catalog under: `crate::commands::CATALOG_SOURCE`.
pub(super) const SOURCE: &str = "local";

/// How long one request may take before the test gives up and prints the log it has.
///
/// Generous on purpose, and it covers startup rather than a round trip: `mcp_service` loads the
/// catalog and RE-EXECUTES every anchor against the engine before `serve_stdio` reads a byte, so
/// the first reply pays for validating a bundle. A shared CI runner is slower than a laptop at
/// both.
pub(super) const REPLY_BUDGET: Duration = Duration::from_secs(120);

/// How long a closed pipe may take to become an exited process.
///
/// Longer than the five-second `shutdown_timeout` in `crate::mcp`, because that bound is on
/// letting blocking work settle and the process still has to unwind and exit after spending it.
pub(super) const STOP_BUDGET: Duration = Duration::from_secs(45);

/// How long the startup notice may take to arrive on standard error.
///
/// Its own budget, and shorter than [`REPLY_BUDGET`], because it is written before the catalog
/// is opened: a notice that has not arrived by now is one that was not written, or was written
/// to the wrong stream.
pub(super) const NOTICE_BUDGET: Duration = Duration::from_secs(30);

/// The grain every question in this file asks at.
pub(super) const GRAIN: &str = "month";

/// The example deployment, which is also what a reader is told to run.
///
/// Canonicalised because the two paths below are handed to a subprocess, and a path carrying
/// `..` is harder to read in a failure than the real one.
pub(super) fn example_root() -> PathBuf {
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
pub(super) fn forward<Stream>(stream: Stream, into: &Sender<String>)
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
pub(super) struct Agent {
    child: Child,
    /// The peer's end of the pipe. Taken by [`Agent::close`], which is the shutdown path.
    pub(super) stdin: Option<ChildStdin>,
    /// Standard output. Every line here has to be a JSON-RPC message.
    pub(super) protocol: Receiver<String>,
    /// Standard error. The log, and the startup notice.
    pub(super) log: Receiver<String>,
    next_id: u32,
    reaped: bool,
}

/// A settings tree declaring the documented example as a deployment does: one `markdown`
/// catalog over `examples/single-player/catalog`, one `files` source over its `data`.
///
/// Issue #970 dropped `mcp`'s directory arguments - it reads `catalogs:`/`sources:` the way
/// `sutura serve` does - so every spawn here needs a settings tree that declares both, the way
/// `served.rs`'s `deployment` helper builds one. The directory paths are canonicalised (via
/// [`example_root`]) so the subprocess resolves them wherever it starts.
///
/// `overlay` is appended after the catalog and source declarations, for the tests that vary a
/// bound or a prompt setting: it arrives the way an operator's own `base.yaml` would layer
/// them.
pub(super) fn example_settings(case: &str, overlay: &str) -> PathBuf {
    let example = example_root();
    let catalog = example.join("catalog");
    let data = example.join("data");
    settings_tree(
        case,
        &format!(
            "security:\n  identity: \"single-user\"\n  single_user_because: \"an end-to-end \
             test reads its own fixture files as one identity\"\n\
             catalogs:\n  \
               - name: \"model\"\n    \
                 kind: \"markdown\"\n    \
                 dir: \"{}\"\n    \
                 data_dir: \"{}\"\n    \
                 version: \"{VERSION}\"\n\
             sources:\n  \
               {SOURCE}:\n    \
                 kind: \"files\"\n    \
                 data_dir: \"{}\"\n    \
                 posture: \"shared-service-user\"\n\
             {overlay}",
            catalog.display(),
            data.display(),
            data.display(),
        ),
    )
}

/// Starts the agent surface over the documented example, declared in a settings tree.
///
/// No `#[expect(clippy::zombie_processes)]`, unlike `crates/sutura-cli/tests/served.rs`'s
/// `start`, and that is measured rather than assumed: the lint does not fire here, and an
/// expectation that never fires is itself an error under `-D warnings`. The reaping is real
/// either way - every path out of an [`Agent`], a clean [`Agent::close`] or a panicking
/// assertion, waits on the child.
pub(super) fn spawn() -> Agent {
    spawn_configured(&example_settings("mcp-example", ""))
}

/// The same, over a deployment's own settings tree.
///
/// **`config_dir` is the only way to test a value this command READS rather than one a test
/// hands it.** `crate::sources::configured` resolves `SUTURA_CONFIG_DIR` inside the spawned
/// process, so a setting supplied here arrives the way an operator writes it - through
/// `base.yaml`, `Settings::load` and the wire - and a fix that moved the defect one frame out
/// would be red. That is `#266`'s `H1` reviewed: the first attempt passed the setting to the
/// function under test, which proves the argument.
///
/// The two directory arguments are GONE since issue #970: this command reads `catalogs:` and
/// `sources:` from the settings tree, so the spawned binary takes no catalogue/data path at
/// all - the deployment is the whole of what it serves.
pub(super) fn spawn_configured(config_dir: &Path) -> Agent {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sutura"));
    command
        .arg("mcp")
        // **REMOVED, not merely unset by convention**, and `SUTURA_CONFIG_DIR` below for the
        // same reason. This command reads the deployment's settings tree, so a developer's
        // exported `SUTURA_CONFIG_DIR` - the one an operator running `sutura serve` on the same
        // machine has - reached this child and turned passing tests red on "two answers to one
        // question", and `SUTURA_ENVIRONMENT=production` turned them red on an access token.
        // Found by review. `env_remove` because `std::env::set_var` is `unsafe` in this edition
        // and this crate's root forbids it: what a test can do is decide what the CHILD sees.
        .env_remove(sutura_config::ENVIRONMENT_VARIABLE)
        .env(sutura_config::CONFIG_DIR_VARIABLE, config_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("the composed binary starts");

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
    pub(super) fn write(&mut self, message: &serde_json::Value) {
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
    pub(super) fn reply(&self) -> serde_json::Value {
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
    pub(super) fn exchange(&mut self, method: &str, params: &serde_json::Value) -> serde_json::Value {
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
    pub(super) fn request(&mut self, method: &str, params: &serde_json::Value) -> serde_json::Value {
        let reply = self.exchange(method, params);
        assert!(
            reply.get("error").is_none(),
            "`{method}` was refused at the protocol level: {reply}"
        );
        reply["result"].clone()
    }

    /// A notification: no id, so no reply to read.
    pub(super) fn notify(&mut self, method: &str) {
        self.write(&serde_json::json!({ "jsonrpc": "2.0", "method": method }));
    }

    /// The handshake, and the result it produced.
    ///
    /// `notifications/initialized` is sent because a client owes it, not because this server
    /// waits for it - `rmcp` enters its service loop as soon as it has answered `initialize`.
    /// Sending it is what keeps this client honest against a server that does gate on it.
    pub(super) fn initialize(&mut self) -> serde_json::Value {
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
    pub(super) fn call(&mut self, capability: Capability, arguments: &serde_json::Value) -> serde_json::Value {
        self.request(
            "tools/call",
            &serde_json::json!({ "name": capability.id(), "arguments": arguments }),
        )
    }

    /// Waits for a line on standard error containing `text`.
    pub(super) fn expect_log(&self, text: &str) -> String {
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
    pub(super) fn drain_log(&self) -> Vec<String> {
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
    pub(super) fn close(&mut self) -> ExitStatus {
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

/// A settings tree with one `base.yaml` in it, the way a deployment supplies one.
///
/// `CARGO_TARGET_TMPDIR` is defined for an integration target and lives inside `target/`, which
/// is the same choice `tests/declared_source.rs` makes: the file a subprocess reads is under the
/// directory a build already owns rather than in a shared system temporary.
///
/// **One directory per CALL**: a path shared by case let one test's `fs::write` truncate
/// `base.yaml` under another's reading child, which fell back to the defaults' relative `catalog`
/// root and refused. The pid separates nextest's processes, the counter `cargo test`'s threads.
pub(super) fn settings_tree(case: &str, base_yaml: &str) -> PathBuf {
    static CALLS: AtomicU32 = AtomicU32::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{case}-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory under the target dir is creatable");
    std::fs::write(dir.join("base.yaml"), base_yaml).expect("the settings file is writable");
    dir
}

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
pub(super) fn question(stem: &str, metric: &str, start: &str, end: &str) -> serde_json::Value {
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
        "metrics": [metric],
        "grain": GRAIN,
        "range": { "start": start, "end": end },
    })
}

/// The one question this file asserts numbers for.
pub(super) fn recurring_revenue_june() -> serde_json::Value {
    question("recurring-revenue-june", "recurring_revenue", "2026-06-01", "2026-07-01")
}

/// The example's own metric-unknown fixture, which exists to be refused.
pub(super) fn a_metric_this_catalog_does_not_define() -> serde_json::Value {
    question(
        "refused-metric-unknown",
        "customer_lifetime_value",
        "2026-06-01",
        "2026-07-01",
    )
}

//! `impl Served`, its `Drop`, and the free helpers the served tests call; split out of `harness.rs`.

use core::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::process::{Command, ExitStatus};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use sutura_dev::issuer::{MockIssuer, Token};

use super::{RECORD_BUDGET, Reply, STOP_BUDGET, Served, derived_beside, example_root, joined};

impl Served {
    /// A GET, with the deployment's token when one is given.
    pub(crate) fn get(&self, path: &str, token: Option<&str>) -> Reply {
        self.send("GET", path, token, None, None)
    }

    /// A POST of a JSON question.
    pub(crate) fn post(&self, path: &str, token: Option<&str>, body: &str) -> Reply {
        self.send("POST", path, token, Some(body), None)
    }

    /// An MCP JSON-RPC POST to the agent surface, with the `Accept` header the streamable-HTTP transport requires. Reachable only when the `agent` feature is compiled in.
    #[cfg(feature = "agent")]
    pub(crate) fn mcp(&self, token: Option<&str>, body: &str) -> Reply {
        self.send(
            "POST",
            sutura_http::constants::AGENT_MOUNT_PATH,
            token,
            Some(body),
            Some("application/json, text/event-stream"),
        )
    }

    /// One request over one connection.
    ///
    /// **Hand-written rather than a client crate, deliberately.** The alternative is `ureq`,
    /// which arrives with rustls and `ring`; `crane.buildDepsOnly` is unscoped so the four cross
    /// dependency derivations - two of them musl - would compile that closure for a binary that
    /// links none of it, which is the same cost `sutura-cli`'s `bigquery` feature is
    /// default-off to avoid. What is needed here is one plaintext loopback request with a fixed
    /// shape, so this is thirty lines and no dependency.
    ///
    /// `Connection: close` is what makes reading to end-of-file the whole response, and the
    /// chunked assertion in [`parse`] is what stops that quietly mis-parsing if a handler ever
    /// answers without a length.
    fn send(&self, method: &str, path: &str, token: Option<&str>, body: Option<&str>, accept: Option<&str>) -> Reply {
        let mut stream = TcpStream::connect(&self.address).expect("the listener accepts a connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .expect("a read timeout is settable");
        let mut request = String::new();
        write!(request, "{method} {path} HTTP/1.1\r\n").expect("writing to a String cannot fail");
        write!(request, "Host: {}\r\n", self.address).expect("writing to a String cannot fail");
        request.push_str("Connection: close\r\n");
        if let Some(accept) = accept {
            write!(request, "Accept: {accept}\r\n").expect("writing to a String cannot fail");
        }
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
    pub(crate) fn terminate(&mut self) -> ExitStatus {
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
                // The reaped child's pipes are at end-of-file, so this returns as soon as the
                // readers have pushed the last lines in - which is what makes `log` below complete
                // rather than best-effort. Issue 387 is the same defect on the refusing path.
                joined(std::mem::take(&mut self.readers));
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
    /// **Complete after [`Served::terminate`] and best-effort before it, and the distinction is the
    /// correction `github.com/telekom/sutura#387` paid for.** This used to say the readers "have
    /// already seen end-of-file" because the process had been reaped, which reaping does not establish: a
    /// `try_recv` sweep stops at the first empty channel, so the lines a process writes as it dies
    /// can still be in flight. `terminate` JOINS the readers, so a call after it cannot miss one.
    ///
    /// Before a `terminate` it stays non-blocking on purpose - joining a running deployment's
    /// readers would never return - and the one caller that reads it that way asserts on two lines
    /// that were already read into `startup` before the deployment was handed over.
    pub(crate) fn log(&self) -> Vec<String> {
        let mut out = self.startup.clone();
        loop {
            match self.lines.try_recv() {
                Ok(line) => out.push(line),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
            }
        }
    }

    /// Reads until a line contains `needle`, and hands back every line read on the way to it.
    ///
    /// **Blocking, where [`Served::log`] is not, and the difference is not convenience.** The
    /// audit record for an answered question is written from the **blocking pool** - see
    /// `sutura_runtime::blocking` - so it is not ordered against the HTTP response the asking
    /// thread already has. A `try_recv` sweep straight after a `200` is a race that fails as *the
    /// record was never written*, which is the wrong diagnostic for a record that arrived a
    /// millisecond later.
    ///
    /// It is also why this assertion can only live here: `sutura_http`'s log-capture harness
    /// installs a **thread-scoped** subscriber, so the record from that pool is invisible to it
    /// (two of its own tests say so where they stand in `tower_http`'s response line instead).
    /// This suite reads the process's own streams, which is every thread.
    ///
    /// Panics on the deadline with everything it did read, because a failure here is about what
    /// the deployment wrote rather than about this helper.
    pub(crate) fn awaiting(&self, needle: &str) -> Vec<String> {
        let mut read = self.startup.clone();
        let deadline = Instant::now() + RECORD_BUDGET;
        loop {
            if read.iter().any(|line| line.contains(needle)) {
                return read;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(left).unwrap_or_else(|cause| {
                panic!(
                    "no line carrying `{needle}` inside {}s ({cause}):\n{}",
                    RECORD_BUDGET.as_secs(),
                    read.join("\n")
                )
            });
            read.push(line);
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
        // And the derived catalog a two-source case writes beside it. Unconditional: a case that
        // derived nothing has no such directory and the removal is a no-op, which is cheaper than a
        // flag saying which cases derive.
        drop(std::fs::remove_dir_all(derived_beside(&self.config_dir)));
    }
}

/// Splits a response into its status and its body.
pub(crate) fn parse(text: &str) -> Reply {
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
    // Field name matched case-insensitively, because HTTP field names are.
    let challenge = head
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|&(name, _)| name.eq_ignore_ascii_case("www-authenticate"))
        })
        .map(|(_, value)| String::from(value.trim()));
    Reply {
        status,
        body: String::from(body),
        challenge,
    }
}

/// Where an event with exactly this message first appears in the log.
///
/// Matches the bunyan `msg` field WHOLE rather than as a substring, and that was MEASURED rather
/// than preferred. The banner writes `listening on loopback only - reachable from this host and no
/// other` before the catalog is loaded, so a substring search for `listening` found that line and
/// the ordering assertion below went red against a service that was in fact ordered correctly. A
/// search loose enough to match the wrong event reports the wrong thing in both directions.
pub(crate) fn position(log: &[String], message: &str) -> usize {
    let field = format!("\"msg\":\"{message}\"");
    log.iter()
        .position(|line| line.contains(&field))
        .unwrap_or_else(|| panic!("no event says `{message}`:\n{}", log.join("\n")))
}

/// The grain every question in this file asks at.
///
/// One constant rather than a parameter because all three fixtures below ask at it, and it is
/// checked against each of them - so this is a fact about the corpus rather than a default.
pub(crate) const GRAIN: &str = "month";

/// A question as it arrives on the wire, mirrored from the example's own fixture.
///
/// **The fixture is READ rather than cited in a comment, and that is the difference between a
/// claim and a mechanism.** `stem` names the file under `examples/single-player/questions/`
/// that holds this question, and the metric, the grain and both dates have to appear in it - so
/// a fixture renamed, deleted or re-ranged goes red HERE instead of leaving this suite quietly
/// asserting a question the example no longer asks. That is the property #117 wants out of this
/// file: the example a reader is told to run is the example CI runs.
///
/// A substring check and not a parse, deliberately: parsing the YAML would mean a second
/// dev-dependency for four fields, and what is guarded here is a fixture that MOVED rather than
/// one that is subtly mis-shaped - `crates/sutura-cli/tests/example.rs` parses every question in
/// that directory and pins what each one compiles to.
pub(crate) fn question(stem: &str, metric: &str, start: &str, end: &str) -> String {
    let path = example_root().join("questions").join(format!("{stem}.yaml"));
    let fixture = std::fs::read_to_string(&path).unwrap_or_else(|cause| {
        panic!(
            "{} is the example question this body mirrors, and it is not readable: {cause}",
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
    format!(r#"{{"metrics":["{metric}"],"grain":"{GRAIN}","range":{{"start":"{start}","end":"{end}"}}}}"#)
}

/// The one question this file asserts numbers for.
pub(crate) fn recurring_revenue_june() -> String {
    question("recurring-revenue-june", "recurring_revenue", "2026-06-01", "2026-07-01")
}

/// A route inside the version prefix, composed the way the router composes it.
///
/// One helper over both routes rather than one per route: the composition is the part worth
/// having in a single place, and `base_paths` already owns each half.
pub(crate) fn v1(base: &str) -> String {
    format!("{}{base}", sutura_http::constants::API_V1_PREFIX)
}

/// What the mock issuer calls itself, and what its tokens are for.
///
/// The resource identifier is this deployment's own name for itself, so it is what a refused
/// caller's challenge has to carry - a challenge naming something else would send a client to the
/// wrong authorization server.
pub(crate) const ISSUER: &str = "https://issuer.example.com";
pub(crate) const RESOURCE: &str = "https://sutura.example.com";
pub(crate) const KEY_ID: &str = "the-current-key";

/// The bunyan `msg` of the audit record for an answered question.
///
/// `answered` is `sutura_runtime::audit`'s own word - the same one the per-outcome log line that
/// record replaced used, so a filter written against either finds this.
///
/// **The `[REQUEST - EVENT]` prefix is the request span's and is part of the match on purpose.**
/// The record is emitted inside the span the router opens, so that is the whole `msg` field as
/// written; matching only `answered` would be a substring search, which is the mistake
/// [`position`] carries its own warning about - it once matched a banner line and reported the
/// wrong thing in both directions. Written whole, this cannot match a different event.
pub(crate) const RECORD: &str = r#""msg":"[REQUEST - EVENT] answered""#;

/// The issuer this deployment is configured with, generating its own key pair.
pub(crate) fn an_issuer() -> MockIssuer {
    MockIssuer::generating(ISSUER, RESOURCE, KEY_ID).expect("a mock issuer generates a key pair")
}

/// A token this deployment accepts, granting every capability the surface has.
///
/// The scopes are read off `sutura_app::Capability` rather than written out, so a capability added
/// later widens what this token grants instead of leaving one governed route quietly unreachable
/// to it: leg 1 says who is asking and the capability gate decides what they may invoke, so a
/// token with no `scope` claim reaches a handler for nothing.
pub(crate) fn accepted_by(subject: &str) -> Token {
    let scopes = sutura_app::Capability::every()
        .map(sutura_app::Capability::scope)
        .collect::<Vec<&str>>()
        .join(" ");
    Token::for_subject(subject).granting(&scopes)
}

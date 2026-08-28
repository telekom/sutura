//! Reading the log back, for a test that has to assert on what was logged.
//!
//! It exists because everything this crate does is observable only as log output: a subscriber that
//! renders the wrong format, a panic hook that emits nothing, a shutdown that does not say why -
//! none of those has a return value to assert on. So the tests install a subscriber over a buffer
//! and assert on the bytes.
//!
//! Scoped and not global for this crate's own unit tests, on purpose:
//! `tracing::subscriber::set_global_default` succeeds once per process, and this crate's tests need
//! several different subscribers. `tests/blocking_span.rs` is the deliberate exception, and its
//! header says why a global one is the only thing that can prove what it proves.
//!
//! # Two visibilities, and the reason for the split
//!
//! `Capture` is `pub` under `cfg(test)` **or** the `test-capture` feature, because `sutura-http`
//! has the same problem and a second copy of a writer is a second thing to keep in step. The
//! helpers that build a subscriber around it stay `cfg(test)`: they use `expect`, which is denied
//! outside test code, so a feature that exposed them would make `--all-features` fail to lint.

use std::io;
use std::sync::Arc;

// Only the subscriber-building helpers below need these, and those are `cfg(test)` - see the header
// for why. Under `test-capture` alone the module is the writer and nothing else.
#[cfg(test)]
use sutura_config::{LogFilter, LogFormat, ServiceName, TelemetrySettings};

/// A writer that keeps what was written.
///
/// `std::sync::Mutex` is on the workspace's disallowed list, and the reason recorded there is that
/// an async task holding one across an `await` can deadlock the executor. Neither half applies
/// here: `MakeWriter` and `io::Write` are synchronous traits that cannot await, the guard is taken
/// and dropped inside one statement, and no runtime is involved in the tests that use this.
#[expect(
    clippy::disallowed_types,
    reason = "a synchronous test writer: io::Write cannot await, so the executor deadlock this ban is about is unreachable"
)]
#[derive(Clone)]
pub struct Capture {
    held: Arc<std::sync::Mutex<Vec<u8>>>,
    /// Also write everything through to standard output.
    ///
    /// Read once, at construction, from [`TEE_VARIABLE`]. A captured log is invisible while a test
    /// is green and is the first thing wanted when one is red, and the two cannot both be served by
    /// a default: printing always makes every green run noisy. So it is a switch, and it is an
    /// environment variable rather than a constant because the person who wants it is looking at a
    /// failure and does not want to edit a file to see it.
    tee: bool,
}

/// Set this to anything non-empty to have a captured log also reach standard output.
///
/// `cargo nextest` shows a failing test's output, so this is what makes the log of the request
/// under test readable without making every other run louder.
pub const TEE_VARIABLE: &str = "SUTURA_TEST_LOG";

impl Capture {
    /// An empty buffer, teeing to standard output if [`TEE_VARIABLE`] is set.
    #[must_use]
    pub fn new() -> Self {
        #[expect(
            clippy::disallowed_types,
            reason = "see the note on `Capture`: a synchronous test writer cannot deadlock an executor"
        )]
        let held = Arc::new(std::sync::Mutex::new(Vec::new()));
        Self {
            held,
            tee: std::env::var_os(TEE_VARIABLE).is_some_and(|value| !value.is_empty()),
        }
    }

    /// Everything written so far, as text.
    #[must_use]
    pub fn contents(&self) -> String {
        self.held
            .lock()
            .map_or_else(|_poisoned| String::new(), |held| String::from_utf8_lossy(&held).into_owned())
    }
}

impl Default for Capture {
    fn default() -> Self {
        Self::new()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.tee {
            // Best effort, and deliberately not propagated: a closed standard output must not turn
            // into the failure a test reports.
            drop(io::stdout().write_all(buf));
        }
        // A poisoned buffer means a test already failed; swallowing the write keeps the failure
        // that matters rather than replacing it with one about the writer.
        self.held.lock().map_or(Ok(buf.len()), |mut held| held.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Settings for a test subscriber.
#[cfg(test)]
pub(crate) fn settings(format: LogFormat, directive: &str) -> TelemetrySettings {
    TelemetrySettings::new(
        ServiceName::parse("sutura-test").expect("a test service name is a name"),
        LogFilter::parse(directive).expect("a test directive is a directive"),
        format,
        true,
    )
}

/// Runs `work` with a machine-readable subscriber and returns what it wrote.
#[cfg(test)]
pub(crate) fn capture(work: impl FnOnce()) -> String {
    capture_with(LogFormat::Bunyan, work)
}

/// Runs `work` with a subscriber in `format` and returns what it wrote.
#[cfg(test)]
pub(crate) fn capture_with(format: LogFormat, work: impl FnOnce()) -> String {
    let sink = Capture::new();
    let built =
        crate::telemetry::subscriber(&settings(format, "trace"), sink.clone()).expect("a valid directive builds a subscriber");
    tracing::subscriber::with_default(built, work);
    sink.contents()
}

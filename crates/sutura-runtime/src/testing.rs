//! Reading the log back, for the tests in this crate.
//!
//! Compiled only under `cfg(test)`. It exists because everything this crate does is observable
//! only as log output: a subscriber that renders the wrong format, a panic hook that emits nothing,
//! a shutdown that does not say why - none of those has a return value to assert on. So the tests
//! install a scoped subscriber over a buffer and assert on the bytes.
//!
//! Scoped and not global on purpose. `tracing::subscriber::set_global_default` succeeds once per
//! process, and this crate's tests need several different subscribers.

use std::io;
use std::sync::Arc;

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
pub(crate) struct Capture(Arc<std::sync::Mutex<Vec<u8>>>);

impl Capture {
    fn new() -> Self {
        #[expect(
            clippy::disallowed_types,
            reason = "see the note on `Capture`: a synchronous test writer cannot deadlock an executor"
        )]
        let held = Arc::new(std::sync::Mutex::new(Vec::new()));
        Self(held)
    }

    /// Everything written so far, as text.
    fn contents(&self) -> String {
        self.0
            .lock()
            .map_or_else(|_poisoned| String::new(), |held| String::from_utf8_lossy(&held).into_owned())
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // A poisoned buffer means a test already failed; swallowing the write keeps the failure
        // that matters rather than replacing it with one about the writer.
        self.0.lock().map_or(Ok(buf.len()), |mut held| held.write(buf))
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
pub(crate) fn settings(format: LogFormat, directive: &str) -> TelemetrySettings {
    TelemetrySettings::new(
        ServiceName::parse("sutura-test").expect("a test service name is a name"),
        LogFilter::parse(directive).expect("a test directive is a directive"),
        format,
        true,
    )
}

/// Runs `work` with a machine-readable subscriber and returns what it wrote.
pub(crate) fn capture(work: impl FnOnce()) -> String {
    capture_with(LogFormat::Bunyan, work)
}

/// Runs `work` with a subscriber in `format` and returns what it wrote.
pub(crate) fn capture_with(format: LogFormat, work: impl FnOnce()) -> String {
    let sink = Capture::new();
    let built =
        crate::telemetry::subscriber(&settings(format, "trace"), sink.clone()).expect("a valid directive builds a subscriber");
    tracing::subscriber::with_default(built, work);
    sink.contents()
}

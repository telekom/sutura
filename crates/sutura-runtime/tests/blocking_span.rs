//! Does work on the blocking pool still belong to the request that started it?
//!
//! # Why this is an integration test and not a `mod tests` in `src/blocking.rs`
//!
//! Because the property is only true when the span's own `Dispatch` and the pool thread's default
//! dispatcher are the **same** subscriber. `tracing`'s current span is a thread-local of the
//! subscriber: `Span::enter` registers the span on whatever thread enters it *inside the span's own
//! subscriber*, and the event that follows is dispatched to whatever that thread's default
//! subscriber is. In a deployment those are one and the same - the process-global one
//! `telemetry::install` sets - so the event finds the span. Under
//! `tracing::subscriber::with_default`, which is scoped to one thread, they are two different
//! things and the pool thread's line goes to the global default instead, which in a unit test is
//! nothing.
//!
//! So this installs a GLOBAL subscriber, which succeeds once per process - and a test binary is the
//! only unit of "once per process" a test suite has. That is the whole reason for the file.
//!
//! It is gated on the `test-capture` feature, which is what makes `Capture` reachable from outside
//! the crate. `cargo nextest run --all-features` builds it; a bare `cargo test -p sutura-runtime`
//! skips the target rather than failing to compile it.

// `cfg(test)` because clippy only honours `allow-expect-in-tests` and `allow-panic-in-tests` for
// code inside a `#[cfg(test)]` item, and `tests_outside_test_module` wants the `#[test]` functions
// there too. An integration test target is compiled with `--test`, so the gate is true here and
// nothing below is conditional in practice - `crates/sutura-sql/tests/adversarial_findings.rs`
// carries the same wrapper for the same reason.
#[cfg(test)]
mod tests {
    use sutura_config::{LogFilter, LogFormat, ServiceName, TelemetrySettings};
    use sutura_runtime::testing::Capture;

    /// The one field this test looks for, chosen so nothing else in the buffer can contain it.
    const CORRELATION: &str = "d1ffed0ffee5";

    #[tokio::test]
    async fn a_line_written_on_the_blocking_pool_carries_the_request_s_span() {
        let sink = Capture::new();
        let telemetry = TelemetrySettings::new(
            ServiceName::parse("sutura-test").expect("a test service name is a name"),
            LogFilter::parse("trace").expect("a test directive is a directive"),
            LogFormat::Bunyan,
            true,
        );
        let built =
            sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
        // Global, once, for this binary. See the header.
        tracing::subscriber::set_global_default(built).expect("this test binary installs one subscriber");

        let span = tracing::info_span!("a_request", correlation = CORRELATION);
        let entered = span.enter();
        let joined = sutura_runtime::spawn_carrying_span(|| {
            tracing::info!("work on the pool");
        })
        .await;
        // Dropped before the buffer is read, so the span's own END line is in it too.
        drop(entered);
        joined.expect("the blocking task ran");

        let rendered = sink.contents();
        let line = rendered
            .lines()
            .find(|line| line.contains("work on the pool"))
            .unwrap_or_else(|| panic!("the blocking task's line was never written: {rendered}"));
        assert!(
            line.contains(CORRELATION),
            "the pool's line is not attributable to the request that started it: {line}"
        );
    }
}

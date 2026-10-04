#![forbid(unsafe_code)]
#![cfg(feature = "test")]
//! [`sutura_http_client::test_support::FakeServer`] fails a cell whose code under test never
//! dials it, instead of hanging it.

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::mpsc::channel;
    use std::thread;
    use std::time::{Duration, Instant};

    use sutura_http_client::test_support::{FakeServer, Scripted};

    /// Longer than the bound `finish` holds, so a hang reads as a timeout here, not as a pass.
    const HANG: Duration = Duration::from_secs(30);

    /// The bound `finish` holds. `recv_timeout` never reports a timeout before its deadline, so
    /// the full bound is a floor that cannot flake.
    const BOUND: Duration = Duration::from_secs(10);

    #[test]
    fn finish_fails_within_its_bound_when_the_server_is_never_dialled() {
        let server = FakeServer::start(vec![Scripted::status(200, "")]);
        let started = Instant::now();
        let (sent, finished) = channel();
        drop(thread::spawn(move || {
            // `finish` is expected to panic; catch the unwind so its message is observable rather
            // than the unwind silently disconnecting `sent`.
            let panicked = catch_unwind(AssertUnwindSafe(move || server.finish()))
                .expect_err("finish must panic when the server is never dialled");
            drop(sent.send(panicked));
        }));
        // `finish` reports only by panicking once its own bound has elapsed. A server that
        // reported (or panicked) before accepting, or a `FINISH_TIMEOUT` of zero, makes this an
        // instant return and reddens the elapsed assertion.
        let panicked = finished
            .recv_timeout(HANG)
            .expect("finish must panic within its bound, neither hang nor return");
        let elapsed = started.elapsed();
        assert!(
            elapsed >= BOUND,
            "finish must wait out its bound rather than return early; elapsed {elapsed:?}"
        );
        let message = panicked
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panicked.downcast_ref::<&str>().copied())
            .expect("a panic payload is a message string");
        assert!(
            message.contains("Timeout"),
            "finish's panic must name the timeout; got: {message:?}"
        );
    }
}

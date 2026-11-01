#![forbid(unsafe_code)]
//! Does a `SIGTERM` that arrives the moment `shutdown::listen` returns start the drain?
//!
//! `github.com/telekom/sutura#1706`: `listen` was an `async fn`, so the handler was registered only
//! when the spawned task was first polled, and a signal in between met the default disposition. The
//! served binary's shutdown cell caught that once in CI as a process ended by signal 15 after its
//! listener had already answered.
//!
//! # Why the current-thread runtime is the whole test
//!
//! On it a spawned task is not polled until the test body awaits, so the signal is raised strictly
//! after `listen` returns and strictly before anything it spawned has run - the window the bug lived
//! in, made deterministic rather than left to a multi-thread scheduler. A `listen` that registers on
//! first poll has registered nothing when the signal lands, and the process ends with it.
//!
//! # Why this is its own test binary
//!
//! The signal is raised at this process, and on Unix a handler `tokio` installs stays installed
//! until the process exits. nextest runs every test in a process of its own; one test in its own
//! binary keeps that true under a plain `cargo test` too.
//!
//! `unix` only, for the reason `crates/sutura-cli/tests/served.rs` gives: `unsafe_code` is
//! forbidden, so the signal is sent by `/bin/sh`'s `kill` builtin rather than `libc::kill`.

// `cfg(unix)` and `cfg(test)` as two attributes, and `cfg(test)` at all, for the reasons
// `crates/sutura-cli/tests/served.rs` and `tests/blocking_span.rs` give: clippy honours
// `allow-expect-in-tests` only under a literal `#[cfg(test)]` ancestor.
#[cfg(unix)]
#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use sutura_runtime::{Shutdown, ShutdownReason, shutdown};

    #[tokio::test(flavor = "current_thread")]
    async fn a_terminate_signal_raised_as_soon_as_listen_returns_starts_the_drain() {
        let stopping = Shutdown::new();
        let listening = tokio::spawn(shutdown::listen(stopping.clone()));
        let raised = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("kill -TERM {}", std::process::id()))
            .status()
            .expect("a shell is available to send the signal");
        assert!(raised.success(), "the terminate signal was not sent");
        tokio::time::timeout(Duration::from_secs(10), listening)
            .await
            .expect("the listener resolved on the signal")
            .expect("the listener task ran to completion");
        assert_eq!(stopping.reason(), Some(ShutdownReason::Terminate));
    }
}

#![forbid(unsafe_code)]
#![cfg(feature = "test")]
//! [`sutura_http_client::test_support::FakeServer`] fails a cell whose code under test never
//! dials it, instead of hanging it.

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    use std::thread;
    use std::time::Duration;

    use sutura_http_client::test_support::{FakeServer, Scripted};

    /// Longer than the bound `finish` holds, so a hang reads as a timeout here, not as a pass.
    const HANG: Duration = Duration::from_secs(30);

    #[test]
    fn finish_fails_within_its_bound_when_the_server_is_never_dialled() {
        let server = FakeServer::start(vec![Scripted::status(200, "")]);
        let (sent, finished) = channel();
        drop(thread::spawn(move || sent.send(server.finish())));
        assert_eq!(
            finished.recv_timeout(HANG),
            Err(RecvTimeoutError::Disconnected),
            "finish must panic within its bound, neither hang nor return"
        );
    }
}

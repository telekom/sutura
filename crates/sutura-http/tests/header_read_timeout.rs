#![forbid(unsafe_code)]
//! Axum's own `serve` builds hyper's connection with `Builder::new(TokioExecutor::new())` and no
//! timer, so hyper 1.x's default header-read timeout never arms: a client that opens a connection
//! and never finishes a request head ties it up forever. `crate::server::run` instead drives each
//! accepted connection through `hyper_util::server::conn::auto::Builder` with a `TokioTimer` and a
//! bounded header-read timeout - this is the proof that bound is actually enforced on the wire,
//! `github.com/telekom/sutura#1680`.

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::time::Duration;

    use sutura_runtime::Shutdown;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpStream;

    /// A free loopback port, picked by the kernel and released before `sutura_http::serve` asks for
    /// the same address. `serve` binds its own listener and only reports the port it got through a
    /// trace log, so this is the only way an external test learns one ahead of calling it; the race
    /// between the release and `serve`'s own bind is the same one any "ask the kernel, then reuse
    /// the answer" test accepts.
    fn free_port() -> SocketAddr {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port is bindable");
        probe.local_addr().expect("a bound socket has an address")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_connection_that_never_finishes_its_request_head_is_closed_within_the_bound() {
        let address = free_port();
        let serving = tokio::spawn(sutura_http::serve(axum::Router::new(), address, Shutdown::new()));

        // The address is already chosen; only the listener itself still needs to come up.
        let mut stream = loop {
            match TcpStream::connect(address).await {
                Ok(stream) => break stream,
                Err(_) if !serving.is_finished() => tokio::time::sleep(Duration::from_millis(10)).await,
                Err(e) => panic!("the server never started listening: {e}"),
            }
        };

        stream
            .write_all(b"GET / HTTP/1.1\r\n")
            .await
            .expect("a partial request head is still just bytes on an open connection");
        // No blank line, ever: the head is deliberately left unfinished.

        let mut byte = [0_u8; 1];
        let outcome = tokio::time::timeout(Duration::from_secs(20), stream.read(&mut byte)).await;
        assert!(
            matches!(outcome, Ok(Ok(0))),
            "expected the connection to be closed (EOF) within 20s of an unfinished request head, got {outcome:?}"
        );

        drop(stream);
        serving.abort();
    }
}

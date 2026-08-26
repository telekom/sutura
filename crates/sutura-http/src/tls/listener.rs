//! Accepting a TLS connection, and why the handshake is not on the accept path.
//!
//! # The handshake is not on the accept path
//!
//! `axum::serve::Listener::accept` is the one place a TLS wrapper obviously goes, and putting the
//! handshake there is a denial of service: `accept` is called in a loop by one task, so a client
//! that opens a connection and never sends a `ClientHello` stalls **every** subsequent accept for as
//! long as it likes. One socket would be the outage.
//!
//! So the shape here is a task that owns the `TcpListener`, spawns each handshake, and sends the
//! ones that complete down a bounded channel. [`TlsListener::accept`] pops from that channel and
//! does no work. Handshakes are therefore concurrent, capped by a semaphore so a flood cannot spawn
//! without bound, and each one has its own deadline.
//!
//! A connection still mid-handshake when shutdown arrives is dropped rather than drained. That is
//! correct rather than a compromise: it carries no request yet, so there is no answer to lose.
//!
//! # `ConnectInfo`, and the reason for `tap_io`
//!
//! The limiter keys on the peer address, which reaches a handler as `ConnectInfo<SocketAddr>` - and
//! `crate::client_address` is where the consequence of losing it is written down: the limiter reports
//! that it cannot extract a key and bounds nothing.
//!
//! `axum` gives `SocketAddr` a `Connected` implementation for `TcpListener` specifically, and a
//! blanket one for `TapIo<L, F>` where `L::Addr` is the address type. It does *not* have one for an
//! arbitrary listener, and this crate cannot add it - `Connected` and `SocketAddr` are both foreign,
//! so the orphan rule forbids it. Wrapping in `tap_io` is therefore not decoration: it is what makes
//! the TLS path produce the same `ConnectInfo<SocketAddr>` the plaintext path produces, so the
//! limiter keys identically on both. The alternative was a local `Peer(SocketAddr)` newtype, which
//! would have compiled and left the limiter keying on nothing over TLS.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::{TlsAcceptor, server::TlsStream};

/// How long one handshake may take before it is abandoned.
///
/// A connection that has not finished a handshake is holding a semaphore permit and a task. Without
/// a deadline, a client that opens a socket and sends one byte holds both until the process exits,
/// which is the cheapest denial of service there is.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How many handshakes may be in flight at once.
///
/// The bound exists so a flood of connections cannot spawn tasks without limit. Above it, a new
/// connection waits in the kernel's accept queue - which is the right place for it to wait, because
/// that queue has a bound the operating system enforces.
const HANDSHAKES_IN_FLIGHT: usize = 256;

/// How many completed handshakes may wait for `axum` to pick them up.
///
/// Small on purpose. This queue holds connections that are *ready*, so a deep one would mean
/// accepting far more work than the server is getting through and discovering it late.
const ACCEPTED_QUEUE: usize = 64;

/// One connection that finished its handshake, and who it is from.
type Accepted = (TlsStream<TcpStream>, SocketAddr);

/// A TLS listener `axum::serve` can drive.
///
/// Holds no socket. The socket is owned by the task [`TlsListener::wrap`] spawns, and this is the
/// receiving end of the connections that task has finished handshaking - see the module
/// documentation for why the handshake is not done here.
pub struct TlsListener {
    local: SocketAddr,
    accepted: mpsc::Receiver<Accepted>,
}

impl core::fmt::Debug for TlsListener {
    /// Hand-written because a channel of TLS streams is not something a reader wants printed.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TlsListener")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

impl TlsListener {
    /// Takes over a bound socket and starts handshaking on it.
    ///
    /// The `TcpListener` is moved into the accept task, which is what makes it impossible to accept
    /// a plaintext connection on this socket by accident: after this call there is no other handle
    /// to it.
    pub fn wrap(tcp: TcpListener, config: Arc<ServerConfig>) -> std::io::Result<Self> {
        let local = tcp.local_addr()?;
        let (sender, accepted) = mpsc::channel(ACCEPTED_QUEUE);
        drop(tokio::spawn(handshake_until_closed(tcp, TlsAcceptor::from(config), sender)));
        Ok(Self { local, accepted })
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    /// The next connection that is ready to carry a request.
    ///
    /// The trait cannot report an error, so a closed channel - which means the accept task is gone,
    /// and it only goes when the socket is unusable - parks forever rather than returning a
    /// connection that does not exist. The graceful-shutdown future is then what ends the server,
    /// which is the same thing that ends it in the ordinary case.
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let Some(ready) = self.accepted.recv().await else {
            tracing::error!("the TLS accept task stopped; this listener will accept nothing further");
            return core::future::pending().await;
        };
        ready
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}

/// Accepts, handshakes each connection on its own task, and forwards the ones that finish.
///
/// Returns when the channel's receiving end is dropped, which is when the server has stopped.
async fn handshake_until_closed(tcp: TcpListener, acceptor: TlsAcceptor, ready: mpsc::Sender<Accepted>) {
    let permits = Arc::new(Semaphore::new(HANDSHAKES_IN_FLIGHT));
    loop {
        // Taken BEFORE the accept, so a flood waits in the kernel's accept queue rather than in a
        // task of ours. `Err` is a closed semaphore, which nothing here does.
        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
            return;
        };
        let (stream, peer) = match next_connection(&tcp, &ready).await {
            Incoming::Connection(stream, peer) => (stream, peer),
            Incoming::Retry => continue,
            Incoming::Stop => return,
        };
        let acceptor = acceptor.clone();
        let handing_over = ready.clone();
        drop(tokio::spawn(async move {
            let _permit = permit;
            handshake(acceptor, stream, peer, &handing_over).await;
        }));
    }
}

/// What the accept loop should do next.
enum Incoming {
    /// A connection to hand to a handshake.
    Connection(TcpStream, SocketAddr),
    /// This accept failed and the next one is worth trying.
    Retry,
    /// The server stopped taking connections.
    Stop,
}

/// One accept, raced against the server going away.
///
/// The race is what makes this task exit rather than park in `accept` for the life of the process:
/// `Sender::closed` resolves when `axum` has dropped the listener, which is the only signal this
/// task gets that serving is over.
#[expect(
    clippy::integer_division_remainder_used,
    reason = "the `select!` macro expands through remainder arithmetic to pick a poll order; nothing here does"
)]
async fn next_connection(tcp: &TcpListener, ready: &mpsc::Sender<Accepted>) -> Incoming {
    tokio::select! {
        () = ready.closed() => served_out(),
        accepted = tcp.accept() => arrived(accepted),
    }
}

/// The server let go of the listener.
fn served_out() -> Incoming {
    tracing::debug!("the server stopped taking connections; the TLS accept task is stopping");
    Incoming::Stop
}

/// What one `accept` produced.
///
/// A per-connection accept error - a file descriptor limit, a connection reset between the SYN and
/// the accept - is not a reason to stop listening. `axum`'s own `TcpListener` implementation does the
/// same thing, for the same reason.
fn arrived(accepted: std::io::Result<(TcpStream, SocketAddr)>) -> Incoming {
    match accepted {
        Ok((stream, peer)) => Incoming::Connection(stream, peer),
        Err(cause) => {
            tracing::warn!(error = %cause, "accepting a connection failed");
            Incoming::Retry
        }
    }
}

/// One handshake, with a deadline.
///
/// A failure here is a `debug` line and nothing else, on purpose. A handshake fails because a
/// scanner connected, because a client offered nothing in common, or because somebody pointed a
/// plaintext client at a TLS port - all of which are things a caller does, none of which are things
/// this deployment can fix, and any of which at `warn` is a log an operator learns to ignore. What
/// this module is loud about is its own material, which is a different thing entirely.
async fn handshake(acceptor: TlsAcceptor, stream: TcpStream, peer: SocketAddr, ready: &mpsc::Sender<Accepted>) {
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
        Ok(Ok(established)) => hand_over(established, peer, ready).await,
        Ok(Err(cause)) => declined(peer, &cause),
        Err(_elapsed) => expired(peer),
    }
}

/// A handshake the other end did not complete.
fn declined(peer: SocketAddr, cause: &std::io::Error) {
    tracing::debug!(%peer, error = %cause, "a TLS handshake failed");
}

/// A handshake that ran out of time.
fn expired(peer: SocketAddr) {
    tracing::debug!(
        %peer,
        timeout_seconds = HANDSHAKE_TIMEOUT.as_secs(),
        "a TLS handshake did not complete inside its deadline"
    );
}

/// Queues an established connection for `axum` to pick up.
async fn hand_over(established: TlsStream<TcpStream>, peer: SocketAddr, ready: &mpsc::Sender<Accepted>) {
    if ready.send((established, peer)).await.is_err() {
        tracing::debug!(%peer, "a handshake completed after the server stopped; dropping it");
    }
}

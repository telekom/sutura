//! Binding a socket, serving, and stopping without dropping an answer.
//!
//! # The bounded drain
//!
//! `axum` waits for every in-flight connection when it is asked to stop, which is what makes a
//! rolling deployment not drop answers - and which is also how one connection nothing is going to
//! close pins the process open past the deadline an orchestrator is running. So the drain is capped:
//! the deadline arms only *after* shutdown has been asked for, and when it expires the serve future
//! is dropped and the process goes on to exit.
//!
//! Two properties fall out of that shape and both are deliberate. Before shutdown the server runs
//! unbounded, so a long-lived connection is not a deadline. And after it, the worst case is the
//! grace period rather than forever - see `Shutdown::grace_period`.
//!
//! # Why the peer address is threaded through
//!
//! Threading it through is not optional: the rate limiter keys on the connection's peer address, and
//! without it there is no address to key on - the limiter would answer every request with "cannot
//! extract key" and limit nothing. That is the failure mode where a limiter appears configured and
//! is not.
//!
//! The same read says whether the client is a node. [`Peer`] is taken from each accepted connection
//! by [`PeerInfo`] and [`connected`] splits it into the `ConnectInfo<SocketAddr>` the limiter keys on
//! and, when the client presented a certificate the node authority signed, [`NodeCertified`]. Only
//! the TLS listener's stream can report one, and a request's extensions are never read off the wire,
//! so a client cannot claim it. It says that a node connected, not which one.
//!
//! # Serving connections by hand
//!
//! `axum::serve` builds its `hyper` connection as `hyper_util::server::conn::auto::Builder::new`
//! with no timer, so `hyper`'s own default header-read timeout never applies - a client that opens a
//! connection and trickles a request head one byte at a time, or never finishes it, holds the
//! connection (and, pre-shutdown, blocks nothing else, but accumulates) until something else closes
//! it. [`run`] serves each accepted connection itself instead, so it can set that timer and bound it.
//! `axum::serve::IncomingStream`, which `Connected` is normally keyed on, has no public constructor,
//! which is why [`Peer`] is read directly off the accepted `Io` through [`PeerInfo`] rather than
//! through `axum`'s own connect-info machinery.
//!
//! # Plaintext and TLS are one serve path
//!
//! [`serve`] and [`serve_tls`] differ in the listener they build and in nothing else. Both hand it
//! to `run`, which is where connection serving, the graceful-shutdown future and the bounded drain
//! above live - so the drain semantics are defined once and cannot drift between the two. That was
//! the deciding argument for wrapping the listener rather than taking `axum-server`, which brings its
//! own separately-implemented shutdown; `crate::tls` records the comparison in full.
//!
//! `serve_tls` exists only under the `tls` feature. With the feature off there is no TLS listener in
//! the build **and** `sutura_config::Settings::refusals` will not let a process start that was asked
//! to terminate TLS in-process - the two are wired to the same feature name so they cannot disagree.
//! Nothing here falls back to plaintext: material that will not load is an error returned before a
//! socket is bound.

use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::Request;
use axum::extract::connect_info::ConnectInfo;
use axum::serve::Listener;
use axum::{Extension, Router};
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder as ConnectionBuilder;
use hyper_util::service::TowerToHyperService;
use sutura_runtime::Shutdown;
use tokio::sync::watch;
use tower::{Layer as _, ServiceExt as _};

/// How long an accepted connection may take to send a complete request head before this process
/// closes it.
///
/// `axum::serve` builds its `hyper` connection with no timer, so this bound - `hyper`'s own default
/// header-read timeout - never applied; see the module documentation. Matches
/// `tls::listener::HANDSHAKE_TIMEOUT`: both cap how long one slow or silent client may hold a
/// connection open before it has sent anything this process can act on.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// How often an HTTP/2 connection is pinged to check it is still alive.
#[cfg(any(feature = "flight", feature = "worker"))]
const HTTP2_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(20);

/// How long a keep-alive ping may go unanswered before the connection is dropped.
#[cfg(any(feature = "flight", feature = "worker"))]
const HTTP2_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(20);

/// Why the server stopped, other than being asked to.
#[derive(Debug, thiserror::Error)]
pub enum ServeFailed {
    #[error("could not listen on {address}")]
    Bind {
        address: String,
        #[source]
        cause: std::io::Error,
    },
    #[error("the server stopped with an error")]
    Serve {
        #[source]
        cause: std::io::Error,
    },
    /// The configured certificate and key are not usable.
    ///
    /// Returned before the socket is bound, which is the property that matters: TLS was asked for
    /// and could not be established, so there is no listener at all rather than a plaintext one on
    /// a port somebody configured to be encrypted.
    #[cfg(feature = "tls")]
    #[error("in-process TLS was configured and the material is not usable")]
    Tls {
        #[source]
        cause: crate::tls::TlsNotUsable,
    },
}

/// Binds `address`, serves `router`, and returns when the shutdown has drained or the deadline
/// expired.
///
/// The bound address is read back from the socket rather than assumed, so a port of zero - a test
/// asking the kernel to choose - is reported as the port it actually got.
pub async fn serve(router: Router, address: SocketAddr, shutdown: Shutdown) -> Result<(), ServeFailed> {
    let listener = bind(address).await?;
    tracing::info!(bound = %local(&listener, address)?, tls = false, "listening");
    run(listener, router, shutdown).await
}

/// The same, with the connection terminated here.
///
/// The material is loaded and validated FIRST, before anything is bound. So a certificate that will
/// not parse, or a key that does not belong to it, is a process that does not start - not a port
/// that accepts connections and fails every handshake, and not a plaintext port. See `crate::tls`
/// for the rotation this also starts, which is what keeps a renewed certificate from needing a
/// restart.
#[cfg(feature = "tls")]
pub async fn serve_tls(
    router: Router,
    address: SocketAddr,
    shutdown: Shutdown,
    material: &sutura_config::TlsMaterial,
) -> Result<(), ServeFailed> {
    let termination = crate::tls::Termination::prepare(material).map_err(|cause| ServeFailed::Tls { cause })?;
    let (config, renewal) = termination.into_parts();
    let listener = bind(address).await?;
    let bound = local(&listener, address)?;
    let listener = crate::tls::TlsListener::wrap(listener, config).map_err(|cause| ServeFailed::Bind {
        address: address.to_string(),
        cause,
    })?;
    renewal.watch_until_shutdown(crate::tls::RENEWAL_INTERVAL, shutdown.clone());
    tracing::info!(%bound, tls = true, "listening");
    run(listener, router, shutdown).await
}

/// Binds the socket, naming the address in the failure.
async fn bind(address: SocketAddr) -> Result<tokio::net::TcpListener, ServeFailed> {
    tokio::net::TcpListener::bind(address)
        .await
        .map_err(|cause| ServeFailed::Bind {
            address: address.to_string(),
            cause,
        })
}

/// The address actually bound.
///
/// Read back from the socket rather than assumed, so a port of zero - a test asking the kernel to
/// choose - is reported as the port it got.
fn local(listener: &tokio::net::TcpListener, requested: SocketAddr) -> Result<SocketAddr, ServeFailed> {
    listener.local_addr().map_err(|cause| ServeFailed::Bind {
        address: requested.to_string(),
        cause,
    })
}

/// A connection whose client presented a certificate the node authority signed.
///
/// In a request's extensions only when its connection carried one, so a route only a node may call
/// requires it. See the module documentation for what it does not say.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NodeCertified;

/// An accepted connection's client: its address, and whether it is a node.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Peer {
    address: SocketAddr,
    node: Option<NodeCertified>,
}

/// Reads [`Peer`] directly off an accepted connection's `Io` and the address `accept` returned.
///
/// A replacement for `axum`'s own `Connected`, which is keyed on `axum::serve::IncomingStream` - a
/// type with no public constructor, so it cannot be built outside `axum` itself. [`run`] serves
/// connections by hand (see the module documentation for why) and so never has an `IncomingStream`
/// to key on; this trait is implemented once per listener this module serves instead, exactly where
/// `Connected` used to be.
pub(crate) trait PeerInfo: Listener<Addr = SocketAddr> {
    fn peer(io: &Self::Io, address: SocketAddr) -> Peer;
}

impl PeerInfo for tokio::net::TcpListener {
    fn peer(_io: &Self::Io, address: SocketAddr) -> Peer {
        Peer { address, node: None }
    }
}

#[cfg(feature = "tls")]
impl PeerInfo for crate::tls::TlsListener {
    /// Any client certificate here is one the node authority signed: its verifier fails the
    /// handshake for another, and without an authority none is asked for.
    fn peer(io: &Self::Io, address: SocketAddr) -> Peer {
        Peer {
            address,
            node: io.get_ref().1.peer_certificates().map(|_| NodeCertified),
        }
    }
}

/// `router`, with [`Peer`] split into each request's extensions as the module documentation
/// describes. Applied per connection in [`run`] rather than through
/// `into_make_service_with_connect_info`, which keys on `IncomingStream` - see [`PeerInfo`].
pub(crate) fn connected(router: Router) -> Router {
    router.layer(axum::middleware::map_request(split))
}

/// Replaces [`Peer`] in the request's extensions with its address and, for a node, the marker.
async fn split(mut request: Request) -> Request {
    if let Some(ConnectInfo(peer)) = request.extensions_mut().remove::<ConnectInfo<Peer>>() {
        let _previous = request.extensions_mut().insert(ConnectInfo(peer.address));
        if let Some(node) = peer.node {
            let _previous = request.extensions_mut().insert(node);
        }
    }
    request
}

/// Serves until the drain finishes or its deadline expires.
///
/// Generic over the listener so plaintext and TLS share this, which is the point - see the module
/// documentation. The [`PeerInfo`] bound is what guarantees the peer address reaches a handler: a
/// listener [`Peer`] cannot be read from does not compile, rather than producing a limiter with no
/// key.
pub(crate) async fn run<L>(listener: L, router: Router, shutdown: Shutdown) -> Result<(), ServeFailed>
where
    L: PeerInfo,
{
    let graceful = shutdown.clone();
    let outcome = drain(accept(listener, connected(router), graceful), &shutdown).await;
    report(outcome)
}

/// Accepts connections until `shutdown` is asked for, then waits for every connection already
/// accepted to finish - the same shape as `axum::serve`'s graceful shutdown, minus the parts that
/// need `IncomingStream` (see [`PeerInfo`] and the module documentation).
async fn accept<L>(mut listener: L, router: Router, shutdown: Shutdown) -> Result<(), std::io::Error>
where
    L: PeerInfo,
{
    // `signal_tx.closed()` resolves once every receiver - here, just `signal_rx` - is dropped, which
    // is what lets the accept loop below and every spawned connection learn shutdown was asked for
    // from one `watch` channel instead of polling `shutdown` directly in each place.
    let (signal_tx, signal_rx) = watch::channel(());
    drop(tokio::spawn(async move {
        let reason = shutdown.requested().await;
        tracing::debug!(%reason, "the server was asked to stop accepting connections");
        drop(signal_rx);
    }));

    // Dropped by each connection task when it finishes; held open here so `close_tx.closed()` below
    // does not resolve until every connection accepted before shutdown has actually finished.
    let (close_tx, close_rx) = watch::channel(());

    loop {
        let (io, address) = tokio::select! {
            accepted = listener.accept() => accepted,
            () = signal_tx.closed() => break,
        };

        tracing::trace!(%address, "connection accepted");
        let peer = L::peer(&io, address);
        let io = TokioIo::new(io);
        let service = Extension(ConnectInfo(peer))
            .layer(router.clone())
            .map_request(|request: axum::http::Request<Incoming>| request.map(axum::body::Body::new));
        let hyper_service = TowerToHyperService::new(service);
        let signal_tx = signal_tx.clone();
        let close_rx = close_rx.clone();

        drop(tokio::spawn(async move {
            let mut builder = ConnectionBuilder::new(TokioExecutor::new());
            builder
                .http1()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_READ_TIMEOUT);
            #[cfg(any(feature = "flight", feature = "worker"))]
            builder
                .http2()
                .timer(TokioTimer::new())
                .keep_alive_interval(Some(HTTP2_KEEP_ALIVE_INTERVAL))
                .keep_alive_timeout(HTTP2_KEEP_ALIVE_TIMEOUT);

            let mut connection = std::pin::pin!(builder.serve_connection_with_upgrades(io, hyper_service));
            let mut shutting_down = false;
            loop {
                tokio::select! {
                    result = connection.as_mut() => {
                        if let Err(cause) = result {
                            tracing::trace!(error = %cause, "failed to serve connection");
                        }
                        break;
                    }
                    () = signal_tx.closed(), if !shutting_down => {
                        connection.as_mut().graceful_shutdown();
                        shutting_down = true;
                    }
                }
            }
            drop(close_rx);
        }));
    }

    drop(close_rx);
    drop(listener);
    close_tx.closed().await;
    Ok(())
}

/// What the serve future did.
enum Outcome {
    /// It resolved on its own: a clean drain, or a fatal error.
    Completed(Result<(), std::io::Error>),
    /// Shutdown was asked for and the drain did not finish inside the grace period.
    ForcedAfterTimeout,
}

/// Drives `serving`, arming the deadline only once shutdown has been asked for.
///
/// The ordering is the whole point. A deadline armed at startup would be a bound on how long the
/// server may run; armed at shutdown it is a bound on how long the drain may take, which is the
/// thing an orchestrator's kill timer is racing.
async fn drain<F>(serving: F, shutdown: &Shutdown) -> Outcome
where
    F: Future<Output = Result<(), std::io::Error>>,
{
    let grace = shutdown.grace_period();
    tokio::pin!(serving);
    tokio::select! {
        result = &mut serving => Outcome::Completed(result),
        reason = shutdown.requested() => {
            tracing::info!(%reason, timeout_seconds = grace.as_secs(), "draining");
            match tokio::time::timeout(grace, &mut serving).await {
                Ok(result) => Outcome::Completed(result),
                Err(_elapsed) => Outcome::ForcedAfterTimeout,
            }
        }
    }
}

/// Turns an outcome into a result, and says which it was.
///
/// A drain that overran is `Ok`: the process was asked to stop and it stopped, later than it wanted
/// to. A serve error is `Err`, because the server fell over on its own.
fn report(outcome: Outcome) -> Result<(), ServeFailed> {
    match outcome {
        Outcome::Completed(Ok(())) => {
            tracing::info!("drained and stopped");
            Ok(())
        }
        Outcome::Completed(Err(cause)) => {
            tracing::error!(error = %cause, "the server stopped with an error");
            Err(ServeFailed::Serve { cause })
        }
        Outcome::ForcedAfterTimeout => {
            tracing::warn!(
                "the drain did not finish inside the grace period; exiting and dropping what was \
                 still in flight"
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sutura_runtime::{Shutdown, ShutdownReason};

    use super::{Outcome, drain};

    /// A serve future that never resolves, standing in for a connection nothing will close.
    async fn never() -> Result<(), std::io::Error> {
        core::future::pending().await
    }

    #[tokio::test]
    async fn the_deadline_is_not_armed_until_shutdown_is_asked_for() {
        // The property that keeps a long-lived connection from being a deadline: before shutdown
        // the server runs unbounded, so this must NOT complete inside the grace period.
        let shutdown = Shutdown::with_grace(Duration::from_millis(20));
        let raced = tokio::time::timeout(Duration::from_millis(120), drain(never(), &shutdown)).await;
        assert!(raced.is_err(), "the drain deadline armed before shutdown was requested");
    }

    #[tokio::test]
    async fn a_drain_that_overruns_the_grace_period_gives_up_rather_than_hanging() {
        // The bug this shape exists for. Without the cap, a connection nothing is going to close
        // keeps the process alive until the orchestrator kills it - so the process never gets to
        // exit on its own terms, and whatever it would have done on the way out does not happen.
        let shutdown = Shutdown::with_grace(Duration::from_millis(20));
        shutdown.trigger(ShutdownReason::Terminate);
        let outcome = tokio::time::timeout(Duration::from_secs(2), drain(never(), &shutdown))
            .await
            .expect("the capped drain returns rather than hanging");
        assert!(matches!(outcome, Outcome::ForcedAfterTimeout));
    }

    #[tokio::test]
    async fn a_drain_that_finishes_inside_the_window_reports_what_the_server_returned() {
        let shutdown = Shutdown::with_grace(Duration::from_secs(5));
        shutdown.trigger(ShutdownReason::Interrupt);
        let outcome = drain(async { Ok(()) }, &shutdown).await;
        assert!(matches!(outcome, Outcome::Completed(Ok(()))));
    }

    #[tokio::test]
    async fn a_server_that_falls_over_on_its_own_is_an_error_rather_than_a_clean_stop() {
        // Nobody asked it to stop, so this must not read as a shutdown in the log or in the result.
        let shutdown = Shutdown::new();
        let outcome = drain(async { Err(std::io::Error::other("the listener went away")) }, &shutdown).await;
        let Outcome::Completed(Err(cause)) = outcome else {
            panic!("a failing serve future is a completed failure");
        };
        assert!(super::report(Outcome::Completed(Err(cause))).is_err());
    }
}

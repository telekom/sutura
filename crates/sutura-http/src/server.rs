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
//! `into_make_service_with_connect_info` is not optional: the rate limiter keys on the connection's
//! peer address, and without the connect info there is no address to key on - the limiter would
//! answer every request with "cannot extract key" and limit nothing. That is the failure mode where
//! a limiter appears configured and is not.

use std::net::SocketAddr;

use axum::Router;
use sutura_runtime::Shutdown;

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
}

/// Binds `address`, serves `router`, and returns when the shutdown has drained or the deadline
/// expired.
///
/// The bound address is read back from the socket rather than assumed, so a port of zero - a test
/// asking the kernel to choose - is reported as the port it actually got.
pub async fn serve(router: Router, address: SocketAddr, shutdown: Shutdown) -> Result<(), ServeFailed> {
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|cause| ServeFailed::Bind {
            address: address.to_string(),
            cause,
        })?;
    let bound = listener.local_addr().map_err(|cause| ServeFailed::Bind {
        address: address.to_string(),
        cause,
    })?;
    tracing::info!(%bound, "listening");

    let graceful = shutdown.clone();
    let serving = axum::serve(
        listener,
        // See the module documentation: the limiter has no key without this.
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let reason = graceful.requested().await;
        tracing::debug!(%reason, "the server was asked to stop accepting connections");
    });

    // `WithGracefulShutdown` is `IntoFuture` rather than `Future`, so it is wrapped in an async
    // block to keep the deadline helper generic over a plain future.
    let outcome = drain(async move { serving.await }, &shutdown).await;
    report(outcome)
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
#[expect(
    clippy::integer_division_remainder_used,
    reason = "the `select!` macro expands through remainder arithmetic to pick a poll order; nothing here does"
)]
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
#[expect(
    clippy::cognitive_complexity,
    reason = "every arm is a tracing macro expanding into branches; the control flow is one match"
)]
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

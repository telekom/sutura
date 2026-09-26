//! Driving the outbound-material rotation poll a composition root owns.
//!
//! `sutura-tls`'s [`sutura_tls::Rotator`] is deliberately spawn-free - *starting* the poll is the
//! composition root's choice, so this crate can host the loop without dragging a requirement into the
//! leaf. This module is that loop, plus the one boot line per poll handle the deployment is promised.

/// Starts a rotation poll for the material a composition root built, if one exists and there is a
/// tokio runtime to run it on.
///
/// `rotator` is `None` when a consumer has no declaration to re-read (the compiled-in roots half of
/// `security.outbound`), in which case nothing is polled and `current()` keeps serving the one value
/// it was built with.
///
/// **No runtime means it does not rotate.** A one-shot command (`sutura query`, a `catalog read`)
/// completes one answer and exits; there is no tick to keep. The handle still serves the material it
/// was built with, which is the only material such a process ever had time to use. The boot line is
/// still logged so a deployment can see why a one-shot did not start a poll.
#[cfg(any(feature = "postgres", feature = "datahub", feature = "openmetadata"))]
pub(crate) fn drive_rotation<T, E>(source: &'static str, rotator: Option<sutura_tls::Rotator<T, E>>)
where
    T: Send + Sync + 'static,
    E: core::fmt::Display + Send + Sync + 'static,
{
    let Some(mut rotator) = rotator else {
        return;
    };
    tracing::info!(
        source,
        interval_seconds = sutura_tls::POLL_INTERVAL.as_secs(),
        "outbound trust material will be polled; new connections and requests adopt new material"
    );
    if tokio::runtime::Handle::try_current().is_err() {
        tracing::warn!(
            source,
            "no tokio runtime is running; the declared material will not rotate during this process"
        );
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(sutura_tls::POLL_INTERVAL).await;
            rotator.poll_once();
        }
    });
}

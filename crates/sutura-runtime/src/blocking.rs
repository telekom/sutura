//! Handing synchronous work to the blocking pool without losing the request it belongs to.
//!
//! # Why a helper rather than a convention
//!
//! `tokio::task::spawn_blocking` runs its closure on a thread that has no idea which request it is
//! serving. `tracing`'s current span is a thread-local, so every line the closure emits lands
//! outside the span the transport opened - which is exactly the context an operator filters on. The
//! fix is three lines and it was already written correctly at the one call site that existed.
//! Nothing made the *next* one write it, and a missing span is invisible: the code compiles, the
//! work runs, the answer is right, and one request's lines are simply not findable together.
//!
//! So the three lines live here and `tokio::task::spawn_blocking` is on the `disallowed-methods`
//! list in `clippy.toml`, with this module's own call carrying the one `#[expect]` for it. That
//! turns "remember to carry the span" into a lint, which is what the *Agent Operating Contract*
//! asks for: a rule with no mechanism is a wish.
//!
//! # Why it lives in this crate
//!
//! The same reason [`crate::admission`] does: **the resource is the process.** The blocking pool is
//! one pool per runtime, shared by every transport, and a second transport would need the same
//! span-carrying spawn over the same pool. A copy per transport is two conventions that can differ.
//!
//! # What it does not do, stated because the name invites the assumption
//!
//! It does not bound anything and it does not cancel anything. `tokio` documents that a started
//! blocking task cannot be aborted and that runtime shutdown waits for one, so a caller who has
//! given up does not stop the work. [`crate::Admission`] is what bounds how many of these run at
//! once; this only decides which span they are attributed to.
//!
//! # The one thing that has to be true of the deployment
//!
//! Carrying the span works because the span's own `Dispatch` and the pool thread's default
//! dispatcher are the *same* subscriber - the process-global one that
//! [`crate::telemetry::install`] sets. Entering the span registers it on the pool thread inside
//! that subscriber; the event then finds it there. Under a subscriber scoped to one thread with
//! `tracing::subscriber::with_default` the two are different, and the pool thread's line goes to
//! whatever global default exists instead. That is why the test for this is an integration test
//! that installs a global subscriber, and not a unit test in this file.

/// Spawns `work` on the blocking pool, entered in the caller's current span.
///
/// The span is captured *here*, on the caller's thread, and entered inside the closure - which is
/// the only order that works: reading the current span from the blocking thread would read that
/// thread's span, which is none.
///
/// Everything the closure does is inside the span, its own `Drop`s included. That matters where a
/// permit or a guard is released at the end of the closure: the release happens inside the span
/// too, so a diagnostic emitted while dropping is still attributable to the request.
///
/// # Example
///
/// ```
/// # async fn call() -> Result<u8, tokio::task::JoinError> {
/// // A synchronous port call. Anything traced inside belongs to the caller's request.
/// sutura_runtime::spawn_carrying_span(|| 7_u8).await
/// # }
/// ```
#[inline]
pub fn spawn_carrying_span<Work, Answer>(work: Work) -> tokio::task::JoinHandle<Answer>
where
    Work: FnOnce() -> Answer + Send + 'static,
    Answer: Send + 'static,
{
    let span = tracing::Span::current();
    #[expect(
        clippy::disallowed_methods,
        reason = "the one permitted call: this function is what the ban exists to route callers through"
    )]
    tokio::task::spawn_blocking(move || span.in_scope(work))
}

//! The reply deadline: what `server.request_timeout_seconds` bounds on this transport.
//!
//! **A file of its own for the reason the parent is one:** `server.rs` plus its whole suite crosses
//! the 1000-line cap `cargo xtask max-lines` enforces, and that cap is unexemptable under `crates/`.
//! What moved here is the group `telekom/sutura#339` added; the ADMISSION pair stayed in the parent
//! because it was there first, and taking assertions out of a file rather than a harness is how a
//! causality verdict reads green over tests that no longer exist where it looks.
//!
//! The fixtures and the seams are the parent's, reached through `super::` - one `served`, one
//! `admission`, one `reply`, so a document read here is read the way the other cells read one.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sutura_app::Permitted;
use sutura_app::prompt::CatalogProse;

use super::{a_certified_question, admission, ask, eventually, reply, served, text_of};
use crate::testing;

/// What one reply-deadline cell observed: how long the peer waited, and what it got.
///
/// A struct rather than a tuple because two of the four claims are about the process's state while
/// the question is still running, and a caller has to be able to read them apart.
struct Waited {
    /// Wall clock from the call going out to the tool result coming back.
    elapsed: Duration,
    /// The tool result the peer was answered with.
    answered: rmcp::model::CallToolResult,
    /// How many questions were inside the port when the peer was answered.
    inside: usize,
    /// How many slots were free at that moment, and the bound they are free of.
    free: usize,
    bound: usize,
}

/// One question, held inside the port, against the deadline the given document configures.
///
/// **Parameterised by the document, which is what makes the assertion about the CONFIGURED number.**
/// A single cell at the smallest value `RequestTimeout::parse` accepts cannot see a deadline that is
/// too LONG, and that was measured twice rather than argued: with `answer`'s `reply.duration()`
/// replaced by a hard-coded five seconds, the single-cell version passed the whole `sutura-mcp`
/// suite and that cell passed in **5.37 s**, because its ceiling was the fake's own twenty-second
/// cap. The same constant against these two cells fails at **5.003 s** on the one-second cell. So
/// the suite caught a deadline that was too short - the shed test does that - and nothing caught one
/// that was too long, which is the direction a bound is for.
///
/// Two cells at two numbers is the same two-documents argument the composition test makes one frame
/// up, and it is the only thing here that can catch a constant.
async fn waited_out(overlay: &str) -> (Waited, testing::Holding) {
    let deadline = reply(overlay);
    let admission = admission(overlay);
    let bound = admission.bound();
    let (surface, holding) = testing::surface_that_can_be_held();
    let client = Arc::new(
        served(
            surface,
            Permitted::every_capability(),
            CatalogProse::Quoted,
            admission.clone(),
            deadline,
        )
        .await,
    );
    holding.arm();

    let began = Instant::now();
    let asked = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.call_tool(ask(&a_certified_question())).await }
    });
    // Confirmed inside the port before anything is asserted: without this the test races the spawn
    // and a green run could mean the deadline fired before the question ever started.
    assert!(
        eventually(|| holding.inside() == 1).await,
        "the question never reached the port"
    );

    let answered = asked
        .await
        .expect("the calling task ran")
        .expect("a reply deadline is a tool result and not a protocol error");
    // Read BEFORE the fake is released, because three of the four claims are about the state the
    // process is in while the question is still running.
    let observed = Waited {
        elapsed: began.elapsed(),
        answered,
        inside: holding.inside(),
        free: admission.free(),
        bound,
    };
    (observed, holding)
}

/// **`telekom/sutura#339`, as a regression.** A peer's wait for a reply is bounded by the number an
/// operator configured, and the question it was waiting for is not stopped.
///
/// The finding's own shape: before this bound the only bounded wait on this surface was the
/// admission window, so a question that GOT a slot waited for as long as the data system took. This
/// asserts the four things that together make the fix a bound rather than a number:
///
/// 1. the peer is answered, on the failure channel, once `server.request_timeout_seconds` has
///    elapsed - **and against TWO documents, which is what makes it the configured number rather
///    than any number**: the short cell has to come back before the long one's deadline, so a
///    constant satisfies at most one of them;
/// 2. the answer carries no digit, because the deadline is the operator's configuration and not
///    something the asking model can act on - the same call `at_capacity` makes;
/// 3. the question is **still inside the port**, because `tokio` cannot abort a started blocking
///    task and this deadline does not pretend otherwise;
/// 4. its **slot is still taken**, which is the half a permit released by the waiting future would
///    fail: a deadline that handed the slot back would let a second question start on top of work
///    that is still running, and the bound would then count peers rather than questions.
///
/// A held fake and a real deadline rather than a settings assertion: what the root RESOLVES is
/// `sutura_cli::mcp`'s own test, and what a resolved number DOES is only observable here. Real time
/// and not a paused clock, because the work is a blocking-pool thread and the assertion is about the
/// order two real things happen in.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_question_that_outlives_its_reply_deadline_is_answered_rather_than_waited_on() {
    // One second is the smallest value `RequestTimeout::parse` accepts, and three is a number no
    // default carries. Read as durations from the same documents the surfaces were built from.
    let short = Duration::from_secs(1);
    let long = Duration::from_secs(3);

    let (quick, holding) = waited_out("server:\n  request_timeout_seconds: 1\n").await;
    quick.answers_on_the_failure_channel();
    assert!(
        quick.elapsed >= short,
        "answered before the configured deadline elapsed: {:?}",
        quick.elapsed
    );
    // **THE cell that catches a deadline that is too long.** A handler holding a constant of five
    // seconds - or of thirty, the shipped default - fails here rather than passing in five.
    assert!(
        quick.elapsed < long,
        "a one-second deadline took at least three seconds, so the number applied is not the one configured: {:?}",
        quick.elapsed
    );
    quick.kept_its_slot_and_its_worker(&holding, &short).await;

    let (slower, holding) = waited_out("server:\n  request_timeout_seconds: 3\n").await;
    slower.answers_on_the_failure_channel();
    // And the other direction: a constant of one second fails here.
    assert!(
        slower.elapsed >= long,
        "a three-second deadline answered in under three seconds, so the number applied is not the one configured: {:?}",
        slower.elapsed
    );
    assert!(
        slower.elapsed < long * 3,
        "the wait was not bounded by the deadline: {:?}",
        slower.elapsed
    );
    slower.kept_its_slot_and_its_worker(&holding, &long).await;
}

impl Waited {
    /// Claims 2 and 3 of the four: the third channel, and nothing of the operator's in it.
    fn answers_on_the_failure_channel(&self) {
        assert_eq!(self.answered.is_error, Some(true), "{:?}", self.answered);
        assert!(self.answered.structured_content.is_none(), "{:?}", self.answered);
        let text = text_of(&self.answered);
        assert!(
            !text.chars().any(|character| character.is_ascii_digit()),
            "the deadline is the operator's number and reached the model's context: {text}"
        );
        assert!(text.contains("may still be running"), "{text}");
    }

    /// Claims 3 and 4: the work was not stopped, and its slot was not handed back by the peer that
    /// stopped waiting - then handed back when the WORK finished, which is why the permit is the
    /// closure's.
    async fn kept_its_slot_and_its_worker(&self, holding: &testing::Holding, deadline: &Duration) {
        assert_eq!(
            self.inside, 1,
            "the worker left the port when its peer stopped waiting for it, at a deadline of {deadline:?}"
        );
        assert_eq!(
            self.free,
            self.bound - 1,
            "the timed-out call handed its slot back while the work it started was still running"
        );
        holding.release();
        assert!(
            eventually(|| holding.inside() == 0).await,
            "the worker never left the port after it was released"
        );
    }
}

/// **The reply deadline bounds the wait for a SLOT as well**, which is what makes it the same key it
/// is on the HTTP surface rather than a second meaning for one number.
///
/// The asymmetry a review found by reading both paths: HTTP installs `request_timeout_seconds` as an
/// OUTER `tower` layer, so the admission wait happens inside it and the key bounds a caller's total
/// wait. This transport applied it *after* `admit`, so a peer's worst case was
/// `admission_timeout_seconds + request_timeout_seconds` - one key, two meanings, which is the exact
/// divergence reusing the key was chosen to prevent.
///
/// So: a window LONGER than the deadline, and the only slot held. The shed answer cannot arrive
/// before the window expires, so a deadline that did not cover the admission wait would answer at
/// thirty seconds; covering it, the peer is answered at one, on the deadline's own channel rather
/// than at-capacity. That is also what `docs/serving.md` already says of the HTTP layer - *setting
/// the window above the request timeout is allowed and does nothing, the timeout answers first*.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_reply_deadline_bounds_the_wait_for_a_slot_too() {
    let overlay =
        "runtime:\n  max_concurrent_queries: 1\n  admission_timeout_seconds: 30\nserver:\n  request_timeout_seconds: 1\n";
    let deadline = reply(overlay);
    let admission = admission(overlay);
    assert_eq!(admission.bound(), 1, "the overlay's bound did not reach the surface");
    assert!(
        admission.wait() > deadline.duration(),
        "this test needs a window longer than the deadline: {:?} vs {:?}",
        admission.wait(),
        deadline.duration()
    );
    let (surface, holding) = testing::surface_that_can_be_held();
    let client = Arc::new(
        served(
            surface,
            Permitted::every_capability(),
            CatalogProse::Quoted,
            admission.clone(),
            deadline,
        )
        .await,
    );
    holding.arm();

    // The only slot, taken and held.
    let first = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.call_tool(ask(&a_certified_question())).await }
    });
    assert!(
        eventually(|| holding.inside() == 1).await,
        "the first question never reached the port"
    );
    assert_eq!(admission.free(), 0, "the first question took no slot");

    // The second cannot get one. It has to come back on the DEADLINE's channel, at the deadline,
    // rather than waiting out a thirty-second window for an at-capacity answer.
    let began = Instant::now();
    let second = client
        .call_tool(ask(&a_certified_question()))
        .await
        .expect("a reply deadline is a tool result and not a protocol error");
    let waited = began.elapsed();

    holding.release();
    drop(first.await.expect("the held question's task ran"));

    assert_eq!(second.is_error, Some(true), "{second:?}");
    let text = text_of(&second);
    assert!(
        text.contains("may still be running"),
        "answered at-capacity rather than on the deadline, so the deadline does not cover the admission wait: {text}"
    );
    assert!(waited >= deadline.duration(), "answered before the deadline: {waited:?}");
    // **A small multiple of the DEADLINE and not of the window, and that difference was measured.**
    // With the timeout applied after `admit` - the arrangement this test exists to refuse - the peer
    // waited until the held fake hit its own twenty-second cap and was then answered on the
    // deadline's channel anyway, so a ceiling of the thirty-second window passed at 21.3 s. The
    // claim is *answered at the deadline*, so the ceiling has to be the deadline's.
    assert!(
        waited < deadline.duration() * 5,
        "the peer waited far past its deadline for a slot, so the key means two different things on the two \
         transports - a window of {:?} was not inside a deadline of {:?}: {waited:?}",
        admission.wait(),
        deadline.duration()
    );
}

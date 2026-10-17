//! Where a key refresh runs, what bounds it, and what the revocation bound does not cover.
//!
//! The cells in `super` and in `super::review` assert the two triggers, the rate limit and the
//! one-read-per-window bound under concurrency. These assert the things none of them could see,
//! because all of them are about a look that is slow, that is abandoned, that fails, or that hands
//! back something this deployment will not adopt:
//!
//! | Cell | What it holds |
//! | --- | --- |
//! | [`a_held_source_read_does_not_block_the_executor`] | the read and the parse are off the async worker thread |
//! | [`a_read_lasting_beyond_its_window_does_not_start_a_second_one`] | one look in flight, whatever the windows say - which is also what makes two looks completing out of order unreachable |
//! | [`a_second_look_inside_the_window_is_refused_and_the_next_window_looks_again`] | the window compare in `reserve` itself, sequentially - the concurrency cell in `super::review` now holds the `InFlight` flag instead, see its header |
//! | [`a_caller_that_gives_up_mid_look_does_not_stop_refresh`] | the reservation follows the awaiter: a request that leaves mid-look frees it rather than holding it for the life of the process |
//! | [`a_panic_inside_the_source_read_frees_the_reservation_for_the_next_window`] | a look that never returns an answer is still a released reservation - the one exit path the other cells do not take |
//! | [`a_source_that_stays_unreadable_keeps_the_keys_and_says_how_stale_they_are`] | the availability half of the position, and the measurement the documented bound lacked |
//! | [`a_document_that_stays_unusable_is_never_a_successful_refresh`] | a rejected document does not become a freshness stamp by being read twice |
//! | [`a_refresh_that_recovers_confirms_the_keys_again`] | the position is not one-way: a source that comes back resets it |
//! | [`a_key_rotated_out_keeps_verifying_inside_the_lifetime_ceiling_and_not_after`] | behind a gateway, the set a rotation replaced verifies for the ceiling plus the leeway, and is refused after it |
//! | [`the_grace_follows_the_declared_ceiling_not_the_default`] | the grace is the deployment's own ceiling, not the default's |
//! | [`a_rotation_that_keeps_the_key_id_verifies_the_new_key_at_once`] | a set that keeps the `kid` and changes the key verifies the new key at the rotating look |
//! | [`a_refused_document_inside_the_grace_does_not_end_it`] | re-reading the set in use after a refused document is no rotation |
//! | [`a_different_set_after_a_refused_document_is_adopted_at_its_look`] | the set read after a refused document verifies its key at that look |
//! | [`a_set_adopted_after_a_refused_document_starts_the_grace_from_its_look`] | the replaced key verifies for the ceiling plus the leeway from the adopting look, and is refused after it |
//! | [`a_key_set_document_over_the_byte_bound_is_refused_rather_than_read`] | the cap and the edge on the work itself, in the one source that ships - not the pre-allocation read, see its header |
//!
//! **The instrument is the fake at the port**, `super::fixtures::StubSource`, scripted look by
//! look; the three timing cells hold one look inside it on a `std::sync::Barrier` released by an OS
//! thread that is **not** the runtime's. That is what makes the failing direction a failure rather
//! than a deadlock: on a build whose read runs inline on the executor, nothing scheduled on the
//! runtime could ever let the read go.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use sutura_config::{KeyFamily, ProofLifetime, RequiredTokenType};

use super::fixtures::{StubSource, cache_around, expiry_in, gateway_declaring, jwks, key_pair, signed, transit};
use super::variants::{certificate_pem, thumbprint_of};
use super::{ISSUER, KID, RESOURCE};
use crate::inbound::gate::InboundGate;
use crate::inbound::keys::{
    FileKeySet, KeyId, KeySetCache, KeySetSource as _, KeySetUnavailable, KeyUnavailable, MAX_KEY_SET_BYTES, Refreshed,
};
use crate::inbound::token::TokenRejected;

/// How long a held look stays held.
///
/// **A liveness grace and not a proof.** It is the window in which a free executor has to run a
/// one-millisecond ticker at least once; on a build whose read blocks the executor **no** grace
/// produces a single tick, so this number cannot turn a red into a green in either direction - only
/// a green into a red, and only on a machine that cannot schedule a task in a fifth of a second.
const GRACE: Duration = Duration::from_millis(200);

/// Both windows wide open, so every cell here decides when a look happens by the instant it passes.
const IMMEDIATELY: Duration = Duration::from_millis(1);

/// A document that parses as JSON and holds no key this deployment could verify with.
const UNUSABLE: &str = "{\"keys\":[]}";

/// A primed cache over a script, and the source to read afterwards.
///
/// Both windows are [`IMMEDIATELY`], so a look happens whenever an instant a cell passes is past
/// the last attempt - which is what lets every cell here be written without a sleep. The family is
/// the elliptic-curve one because every generated key here is.
fn scripted(script: &[Option<String>], hold_at: Option<usize>, now: Instant) -> (KeySetCache, Arc<StubSource>) {
    cache_around(
        StubSource::scripted(script.to_vec(), hold_at),
        KeyFamily::EllipticCurve,
        IMMEDIATELY,
        IMMEDIATELY,
        now,
    )
}

/// Lets the held look go, from a thread that is not the runtime's.
///
/// It sleeps first rather than waiting for the look to arrive: `Barrier::wait` already blocks until
/// the other waiter is there, so the sleep is only the grace the cell needs, and there is nothing
/// for this thread to observe.
fn releases_after_the_grace(source: &Arc<StubSource>) -> std::thread::JoinHandle<()> {
    let gate = Arc::clone(&source.gate);
    std::thread::spawn(move || {
        std::thread::sleep(GRACE);
        let _released = gate.wait();
    })
}

// ------------------------------------------------- where the read and the parse run ----

#[tokio::test]
async fn a_held_source_read_does_not_block_the_executor() {
    // **The first half of the finding.** `KeySetSource::read` is synchronous and the parse behind it
    // is CPU work over a foreign document, and both ran inline in an `async fn` - so a slow key set
    // read blocked the worker thread that was serving requests, from the timer as much as from a
    // caller.
    //
    // This runs on the default current-thread runtime ON PURPOSE: with one executor thread, "another
    // task made progress" and "the read is not on the executor" are the same statement. The ticker
    // is what makes it observable, and the OS thread that releases the read is what makes the
    // failing direction terminate - see this module's header.
    let pair = key_pair();
    let now = Instant::now();
    let (cache, source) = scripted(&[Some(jwks(KID, &pair)), Some(jwks(KID, &pair))], Some(1), now);
    let releaser = releases_after_the_grace(&source);

    let beats = Arc::new(AtomicUsize::new(0));
    let ticker = tokio::spawn({
        let beats = Arc::clone(&beats);
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(1)).await;
                let _previous = beats.fetch_add(1, Ordering::SeqCst);
            }
        }
    });

    let outcome = cache.poll_once(now + Duration::from_secs(1)).await;
    ticker.abort();
    releaser.join().expect("the releasing thread does not panic");

    assert_eq!(source.calls.load(Ordering::SeqCst), 2, "the held look happened");
    assert_eq!(outcome, Refreshed::Unchanged, "the same document came back: {outcome:?}");
    assert!(
        beats.load(Ordering::SeqCst) > 0,
        "no task made any progress while a source read was in flight, so the read ran on the executor"
    );
}

#[tokio::test]
async fn a_read_lasting_beyond_its_window_does_not_start_a_second_one() {
    // Moving the read off the executor is not by itself a bound: a started blocking task cannot be
    // aborted, so without a second gate a source slower than its own window accumulates one detached
    // read per window - and two of them completing out of order install the OLDER document, because
    // `adopt` compares against the last bytes examined and a late arrival therefore looks like a
    // change. `Cached::reading` makes that unreachable, which is why there is no separate ordering
    // cell: with one look in flight there is no ordering to get wrong.
    //
    // The third document is the assertion's teeth. If a second look happened it would rotate the key
    // set, so `describe` says whether one did even beyond the count.
    let first = key_pair();
    let second = key_pair();
    let now = Instant::now();
    let (cache, source) = scripted(
        &[
            Some(jwks(KID, &first)),
            Some(jwks(KID, &first)),
            Some(jwks("the-next-key", &second)),
        ],
        Some(1),
        now,
    );
    let releaser = releases_after_the_grace(&source);
    let cache = Arc::new(cache);

    let held = tokio::spawn({
        let cache = Arc::clone(&cache);
        async move { cache.poll_once(now + Duration::from_secs(1)).await }
    });
    // Let the held look start. One yield is enough - the reservation and the spawn both happen before
    // the task's first await - and the loop is bounded so that a build which never starts a look
    // fails on the assertions below rather than spinning here.
    for _ in 0..64_u16 {
        if source.calls.load(Ordering::SeqCst) == 2 {
            break;
        }
        tokio::task::yield_now().await;
    }

    // A second trigger a whole window later, while the first look is still inside the source.
    let outcome = cache.poll_once(now + Duration::from_secs(2)).await;
    assert_eq!(
        outcome,
        Refreshed::NotDue,
        "a look already in flight has to win the reservation: {outcome:?}"
    );
    let held = held.await.expect("the held task does not panic");
    assert_eq!(held, Refreshed::Unchanged, "{held:?}");
    releaser.join().expect("the releasing thread does not panic");

    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        2,
        "a read outlasting its own window must not start a second one"
    );
    let (count, ids) = cache.describe().await;
    assert_eq!(count, 1);
    assert_eq!(ids, vec![String::from(KID)], "nothing may rotate behind a held look");
}

#[tokio::test]
async fn a_second_look_inside_the_window_is_refused_and_the_next_window_looks_again() {
    // **Finding 1 + 2 from review, held sequentially.** Moving the read onto the blocking pool
    // parked `super::review::two_concurrent_callers_past_the_same_pre_state_perform_exactly_one_read`'s
    // first caller on its `JoinHandle`, so the second now loses on the `InFlight` flag in `reserve`
    // and never reaches the window compare that cell's own header used to describe. That cell still
    // holds a real bound - see its reworded header - but the compare itself needs a cell that reaches
    // it, and this one is deterministic and sequential: no barrier, no second task, no sleep. Every
    // instant is one this cell passes to `poll_once` itself, which is what `IMMEDIATELY` windows are
    // for.
    //
    // Three steps, and each kills a different mutation:
    // - the first look past the window happens, so the count below is not a fluke of priming.
    // - the SAME instant, immediately after, must be refused by the window compare - the flag is
    //   already clear (the first look's `InFlight` was dropped inside `record`, before this call even
    //   starts), so a `NotDue` here can only come from the compare in `reserve`. Killing the compare
    //   (mutation E: `window = Duration::ZERO`) turns this into a second read.
    // - a whole window later must look again. Reversing the two checks in `reserve` (mutation B: the
    //   flag swap before the window compare) makes the SECOND step above leak the flag forever - the
    //   window-loser sets it before failing the compare, and nothing clears a flag that was never
    //   handed out as an `InFlight` - so this third step is refused too, which is how it kills B.
    let pair = key_pair();
    let now = Instant::now();
    let (cache, source) = scripted(
        &[Some(jwks(KID, &pair)), Some(jwks(KID, &pair)), Some(jwks(KID, &pair))],
        None,
        now,
    );
    let t1 = now + Duration::from_millis(5);

    assert_eq!(
        cache.poll_once(t1).await,
        Refreshed::Unchanged,
        "the first look past the window happens"
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 2, "the priming read plus one look");

    assert_eq!(
        cache.poll_once(t1).await,
        Refreshed::NotDue,
        "the same instant is still inside the window the first look just stamped, and the flag is \
         already clear - only the compare can refuse this one"
    );
    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        2,
        "a look refused by the window compare must not read the source"
    );

    let t2 = t1 + Duration::from_secs(1);
    assert_eq!(
        cache.poll_once(t2).await,
        Refreshed::Unchanged,
        "a whole window later the compare must let a look through again"
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 3, "the window reopening must look");
}

#[tokio::test]
async fn a_caller_that_gives_up_mid_look_does_not_stop_refresh() {
    // The reservation was a `bool` set by `reserve` and cleared by `record`, and the look between
    // the two is awaited on the REQUEST task: `key_for` reaches `poll_once` with any token naming
    // an unknown `kid`, and the router puts a timeout around every request. A request that timed
    // out, or a client that disconnected, while the source was slow dropped that future between the
    // two - and the flag stayed set for the life of the process. No further look: no rotation and
    // no revocation, from one disconnect. The reservation is a value with a `Drop` now, so an
    // awaiter that leaves frees it on the way out.
    //
    // The timeout is the instrument and not a grace: nobody has released the barrier when it fires,
    // so the future is dropped mid-look whatever the number is. The third document is the teeth -
    // the look after the abandoned one has to actually happen, and it rotates.
    let first = key_pair();
    let second = key_pair();
    let now = Instant::now();
    let (cache, source) = scripted(
        &[
            Some(jwks(KID, &first)),
            Some(jwks(KID, &first)),
            Some(jwks("the-next-key", &second)),
        ],
        Some(1),
        now,
    );

    let abandoned = tokio::time::timeout(Duration::from_millis(10), cache.poll_once(now + Duration::from_secs(1))).await;
    // Released and joined BEFORE either assertion below - not after, which is `#692`'s hang.
    // `StubSource::read` only reaches the barrier AFTER its `fetch_add`, so a join here cannot
    // return before the read has genuinely started; that is what makes the count below true by
    // construction instead of a race against how fast the blocking pool got scheduled. Ordered the
    // other way, an assertion failing between the timeout and this release would unwind past it and
    // leave the pool thread waiting at the barrier for a partner that never arrives.
    releases_after_the_grace(&source)
        .join()
        .expect("the releasing thread does not panic");
    assert!(abandoned.is_err(), "the held look was still inside the source: {abandoned:?}");
    assert_eq!(source.calls.load(Ordering::SeqCst), 2, "the abandoned look had started");

    let outcome = cache.poll_once(now + Duration::from_secs(2)).await;
    assert_eq!(
        outcome,
        Refreshed::Rotated,
        "a reservation whose awaiter left must not hold for the life of the process: {outcome:?}"
    );
    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        3,
        "the look after the abandoned one happened"
    );
    let (count, ids) = cache.describe().await;
    assert_eq!(count, 1);
    assert_eq!(ids, vec![String::from("the-next-key")]);
}

#[tokio::test]
async fn a_panic_inside_the_source_read_frees_the_reservation_for_the_next_window() {
    // **NIT 4 from review.** `LookFailed::DidNotFinish` existed for a blocking task that never hands
    // an answer back - the task panicked, or the runtime is shutting down - and nothing provoked it:
    // `grep -rn DidNotFinish crates/` found only the declaration and the fallback text `announce`
    // prints for it. It is provokable, and provoking it also proves the thing the seven other cells
    // in this module do not reach: `InFlight` is held across the `.await` on the blocking task's
    // `JoinHandle`, so a panic that unwinds the closure on the POOL thread still drops the guard on
    // the AWAITER's stack when the `Err(JoinError)` comes back - there is no path from a panicking
    // `read` to a reservation that never clears.
    let pair = key_pair();
    let now = Instant::now();
    let (cache, source) = cache_around(
        StubSource::scripted(vec![Some(jwks(KID, &pair)), Some(jwks(KID, &pair))], None).panicking_at(1),
        KeyFamily::EllipticCurve,
        IMMEDIATELY,
        IMMEDIATELY,
        now,
    );

    let panicked = cache.poll_once(now + Duration::from_millis(5)).await;
    assert_eq!(
        panicked,
        Refreshed::Unavailable,
        "a look whose blocking task panicked did not finish, and did not finish is unavailable: {panicked:?}"
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 2, "the panicking look was attempted");

    // The next window: if the panic had leaked the flag, this would be `NotDue` and the count would
    // stay at two.
    let recovered = cache.poll_once(now + Duration::from_secs(1)).await;
    assert_eq!(
        recovered,
        Refreshed::Unchanged,
        "the reservation must be released across the panic, so the next window looks again: {recovered:?}"
    );
    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        3,
        "the next window's look actually happened"
    );
}

// ----------------------------------- what a failing refresh does, and does not, bound ----

#[tokio::test]
async fn a_source_that_stays_unreadable_keeps_the_keys_and_says_how_stale_they_are() {
    // **The second half of the finding, and the policy.** The documented bound is `MAX_KEY_SET_AGE`
    // plus one source read; a reservation stamps the ATTEMPT, so under a dead source that number
    // stays one window old for as long as the process lives while nothing has confirmed the keys
    // since boot. Both halves are asserted here on purpose:
    //
    // - the keys keep verifying, which is the availability position. Refusing instead would take a
    //   deployment's authentication down over a sidecar mid-rewrite, and no record has decided a
    //   deployment should degrade that way - so nothing here does, and this cell says so.
    // - the reported staleness grows, which is the correctness decision. A retained key set with no
    //   measurement is what let a finite bound be claimed for a case that has none.
    let pair = key_pair();
    let now = Instant::now();
    // One usable document and then nothing: the trailing failure repeats.
    let (cache, _source) = scripted(&[Some(jwks(KID, &pair)), None], None, now);
    let id = KeyId::parse(KID).expect("a test key id is a key id");

    for window in 1..=10_u64 {
        let later = now + Duration::from_secs(window);
        let outcome = cache.poll_once(later).await;
        assert_eq!(
            outcome,
            Refreshed::Unavailable,
            "a source that answered nothing is not an unchanged document: {outcome:?}"
        );
        drop(
            cache
                .key_for(&id, later)
                .await
                .expect("the keys in use keep verifying while refresh is unavailable"),
        );
        assert_eq!(
            cache.stale_for(later).await,
            Duration::from_secs(window),
            "staleness is measured from the last SUCCESS, and the boot read is the only one there was"
        );
    }
}

#[tokio::test]
async fn a_document_that_stays_unusable_is_never_a_successful_refresh() {
    // **The trap this cell exists for.** `adopt` records a rejected document as EXAMINED, so that
    // identical bytes on the next look are silent rather than an error once per interval forever -
    // and the look after a rejection therefore returns `Refreshed::Unchanged` over bytes this
    // deployment refused. A freshness stamp taken on that variant would be stamped by a document the
    // deployment would not use, which is the exact shape of the defect being fixed. It is decided by
    // the candidate's own parse instead.
    let pair = key_pair();
    let now = Instant::now();
    let (cache, _source) = scripted(
        &[
            Some(jwks(KID, &pair)),
            Some(String::from(UNUSABLE)),
            Some(String::from(UNUSABLE)),
            Some(String::from(UNUSABLE)),
        ],
        None,
        now,
    );
    let id = KeyId::parse(KID).expect("a test key id is a key id");

    let first = now + Duration::from_secs(1);
    assert_eq!(cache.poll_once(first).await, Refreshed::Rejected);
    assert_eq!(cache.stale_for(first).await, Duration::from_secs(1));
    // The two looks that matter: identical unusable bytes, reported as unchanged, and NOT a success.
    for window in 2..=3_u64 {
        let later = now + Duration::from_secs(window);
        assert_eq!(cache.poll_once(later).await, Refreshed::Unchanged);
        assert_eq!(
            cache.stale_for(later).await,
            Duration::from_secs(window),
            "a second look at a document this deployment rejected must not count as a refresh"
        );
    }
    drop(
        cache
            .key_for(&id, now + Duration::from_secs(3))
            .await
            .expect("the previous keys keep verifying when a candidate is refused"),
    );
}

#[tokio::test]
async fn a_pem_that_stops_loading_keeps_the_previous_pem_key_verifying() {
    // `tls.key` appended to `tls.crt` by mistake: the new document is refused, the set is not emptied,
    // and the key the last good PEM carried keeps verifying.
    let pair = key_pair();
    let now = Instant::now();
    let leaf = certificate_pem(&pair);
    let (cache, _source) = scripted(
        &[Some(leaf.clone()), Some(format!("{leaf}{}", pair.serialize_pem()))],
        None,
        now,
    );
    let later = now + Duration::from_secs(1);
    assert_eq!(cache.poll_once(later).await, Refreshed::Rejected);
    let id = KeyId::parse(thumbprint_of(&pair)).expect("a thumbprint is a key id");
    drop(
        cache
            .key_for(&id, later)
            .await
            .expect("the previous PEM key keeps verifying when its successor is refused"),
    );
}

#[tokio::test]
async fn a_refresh_that_recovers_confirms_the_keys_again() {
    // The policy is not one-way, which is the half an availability argument has to show: a source
    // that comes back rotates and resets the measurement, so the staleness is a live number rather
    // than a ratchet. Without this cell, "nothing refuses on it" and "nothing ever clears it" would
    // read the same.
    let old = key_pair();
    let new = key_pair();
    let now = Instant::now();
    let (cache, _source) = scripted(
        &[Some(jwks(KID, &old)), None, None, Some(jwks("the-next-key", &new))],
        None,
        now,
    );

    for window in 1..=2_u64 {
        let later = now + Duration::from_secs(window);
        assert_eq!(cache.poll_once(later).await, Refreshed::Unavailable);
        assert_eq!(cache.stale_for(later).await, Duration::from_secs(window));
    }
    let recovered = now + Duration::from_secs(3);
    assert_eq!(cache.poll_once(recovered).await, Refreshed::Rotated);
    assert_eq!(
        cache.stale_for(recovered).await,
        Duration::ZERO,
        "a successful refresh is the only thing that resets the measurement"
    );
    let (count, ids) = cache.describe().await;
    assert_eq!(count, 1);
    assert_eq!(ids, vec![String::from("the-next-key")]);
}

// ------------------------------------------------- a source that does not answer ----

#[tokio::test]
async fn a_failed_look_answers_source_unavailable_until_a_look_succeeds() {
    // Inside the refetch window after a failed look the miss is the same outage, not a rate limit;
    // the next look that answers turns a miss back into an unknown key id.
    let pair = key_pair();
    let now = Instant::now();
    let (cache, _source) = cache_around(
        StubSource::scripted(vec![Some(jwks(KID, &pair)), None, Some(jwks(KID, &pair))], None),
        KeyFamily::EllipticCurve,
        Duration::from_secs(30),
        Duration::from_secs(60),
        now,
    );
    let unknown = KeyId::parse("a-key-not-yet-read").expect("a test key id is a key id");
    let at = |seconds| now + Duration::from_secs(seconds);
    let looked = cache.key_for(&unknown, at(31)).await.expect_err("the look failed");
    assert!(matches!(looked, KeyUnavailable::SourceUnavailable { .. }), "{looked:?}");
    let limited = cache.key_for(&unknown, at(32)).await.expect_err("inside the window");
    assert!(matches!(limited, KeyUnavailable::SourceUnavailable { .. }), "{limited:?}");
    let id = KeyId::parse(KID).expect("a test key id is a key id");
    drop(cache.key_for(&id, at(32)).await.expect("the keys held keep verifying"));
    let recovered = cache.key_for(&unknown, at(62)).await.expect_err("the set does not hold it");
    assert!(matches!(recovered, KeyUnavailable::UnknownKeyId), "{recovered:?}");
}

// ------------------------------------------------------- the rotated-out grace ----

/// A transit assertion `pair` signed under `kid`, unexpired by the wall clock for the whole cell.
fn assertion_signed_by(pair: &rcgen::KeyPair, kid: &str) -> String {
    let issued = expiry_in(0);
    signed(
        pair,
        kid,
        &serde_json::json!({"sub": "someone@example.com", "aud": RESOURCE, "iss": ISSUER, "iat": issued, "exp": issued + 60}),
    )
}

/// Rotates a one-key file behind a gateway declaring `ceiling`, and brackets the outgoing key's grace
/// at `grace` seconds from the look that saw the rotation: it verifies one second before, and not at.
async fn brackets_the_grace(ceiling: ProofLifetime, grace: u64) {
    let outgoing = key_pair();
    let incoming = key_pair();
    let primed = Instant::now();
    let (cache, _source) = scripted(
        &[Some(jwks("outgoing", &outgoing)), Some(jwks("incoming", &incoming))],
        None,
        primed,
    );
    let gate = InboundGate::over(&gateway_declaring(RequiredTokenType::access_token(), ceiling), cache);
    let old = assertion_signed_by(&outgoing, "outgoing");

    let rotated = primed + Duration::from_secs(1);
    assert!(
        gate.establish(&transit(&old), rotated).await.is_ok(),
        "the look that rotates keeps the outgoing key verifying"
    );
    assert!(
        gate.establish(&transit(&assertion_signed_by(&incoming, "incoming")), rotated)
            .await
            .is_ok(),
        "the rotation happened"
    );
    assert!(
        gate.establish(&transit(&old), rotated + Duration::from_secs(grace - 1))
            .await
            .is_ok(),
        "inside the ceiling plus the leeway the outgoing key still verifies"
    );
    let refused = gate
        .establish(&transit(&old), rotated + Duration::from_secs(grace))
        .await
        .expect_err("at the end of the grace the outgoing key verifies nothing");
    assert!(
        matches!(
            refused,
            TokenRejected::NoKey {
                cause: KeyUnavailable::UnknownKeyId
            }
        ),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_key_rotated_out_keeps_verifying_inside_the_lifetime_ceiling_and_not_after() {
    // A one-key file behind a gateway: the rotation replaces the only key, so without a grace every
    // unexpired assertion the old key signed is refused. The default ceiling is 120 s and the leeway
    // 30 s, so the outgoing key verifies for 150 s from the look that saw the rotation.
    brackets_the_grace(ProofLifetime::default_lifetime(), 150).await;
}

#[tokio::test]
async fn the_grace_follows_the_declared_ceiling_not_the_default() {
    let ceiling = ProofLifetime::parse(300).expect("five minutes is a lifetime");
    brackets_the_grace(ceiling, 330).await;
}

#[tokio::test]
async fn a_rotation_that_keeps_the_key_id_verifies_the_new_key_at_once() {
    // A JWK set writer that keeps the `kid`: the current set is asked first, so the new key verifies
    // from the rotating look, and the old material under that id gets no grace.
    let outgoing = key_pair();
    let incoming = key_pair();
    let primed = Instant::now();
    let (cache, _source) = scripted(&[Some(jwks(KID, &outgoing)), Some(jwks(KID, &incoming))], None, primed);
    let gate = InboundGate::over(
        &gateway_declaring(RequiredTokenType::access_token(), ProofLifetime::default_lifetime()),
        cache,
    );
    let new = gate
        .establish(
            &transit(&assertion_signed_by(&incoming, KID)),
            primed + Duration::from_secs(1),
        )
        .await;
    assert!(new.is_ok(), "the new key under the kept id is refused: {new:?}");
}

#[tokio::test]
async fn a_refused_document_inside_the_grace_does_not_end_it() {
    // An in-place rewrite caught part-way, then the same new set again: re-reading the set in use is
    // no rotation, so the outgoing key keeps its grace.
    let outgoing = key_pair();
    let incoming = key_pair();
    let primed = Instant::now();
    let (cache, _source) = scripted(
        &[
            Some(jwks("outgoing", &outgoing)),
            Some(jwks("incoming", &incoming)),
            Some(String::from("not a key set")),
            Some(jwks("incoming", &incoming)),
        ],
        None,
        primed,
    );
    let gate = InboundGate::over(
        &gateway_declaring(RequiredTokenType::access_token(), ProofLifetime::default_lifetime()),
        cache,
    );
    let old = assertion_signed_by(&outgoing, "outgoing");
    for after in [1, 3, 5] {
        let verified = gate.establish(&transit(&old), primed + Duration::from_secs(after)).await;
        assert!(verified.is_ok(), "the outgoing key at +{after} s: {verified:?}");
    }
}

/// A gateway over `incoming` replacing `outgoing` at +1 s, a refused document at +3 s and the `next` set
/// at +5 s; each of those instants is a look once a cell asks for a key at it.
fn refused_then_replaced(incoming: &rcgen::KeyPair, next: &rcgen::KeyPair, primed: Instant) -> InboundGate {
    let (cache, _source) = scripted(
        &[
            Some(jwks("outgoing", &key_pair())),
            Some(jwks("incoming", incoming)),
            Some(String::from("not a key set")),
            Some(jwks("next", next)),
        ],
        None,
        primed,
    );
    InboundGate::over(
        &gateway_declaring(RequiredTokenType::access_token(), ProofLifetime::default_lifetime()),
        cache,
    )
}

#[tokio::test]
async fn a_different_set_after_a_refused_document_is_adopted_at_its_look() {
    let (incoming, next) = (key_pair(), key_pair());
    let primed = Instant::now();
    let gate = refused_then_replaced(&incoming, &next, primed);
    let current = assertion_signed_by(&incoming, "incoming");
    for after in [1, 3] {
        let verified = gate.establish(&transit(&current), primed + Duration::from_secs(after)).await;
        assert!(verified.is_ok(), "the incoming key at +{after} s: {verified:?}");
    }
    let adopted = gate
        .establish(&transit(&assertion_signed_by(&next, "next")), primed + Duration::from_secs(5))
        .await;
    assert!(adopted.is_ok(), "the set read after the refused document: {adopted:?}");
}

#[tokio::test]
async fn a_set_adopted_after_a_refused_document_starts_the_grace_from_its_look() {
    let (incoming, next) = (key_pair(), key_pair());
    let primed = Instant::now();
    let gate = refused_then_replaced(&incoming, &next, primed);
    let replaced = assertion_signed_by(&incoming, "incoming");
    for after in [1, 3, 5, 154] {
        let verified = gate.establish(&transit(&replaced), primed + Duration::from_secs(after)).await;
        assert!(verified.is_ok(), "the replaced key at +{after} s: {verified:?}");
    }
    let refused = gate
        .establish(&transit(&replaced), primed + Duration::from_secs(155))
        .await
        .expect_err("150 s after the adopting look the replaced key verifies nothing");
    assert!(
        matches!(
            refused,
            TokenRejected::NoKey {
                cause: KeyUnavailable::UnknownKeyId
            }
        ),
        "{refused:?}"
    );
}

// -------------------------------------------------- the bound on the work itself ----

#[test]
fn a_key_set_document_over_the_byte_bound_is_refused_rather_than_read() {
    // `std::fs::read_to_string` read whatever was at the path, so the read and the parse behind it
    // were work proportional to the file - a denial-of-service primitive whatever else it is, and one
    // a sidecar mistake reaches as easily as anything else does.
    //
    // **State the limit next to the claim (finding 3 from review).** This cell holds the CAP and the
    // EDGE - `TooLarge { limit }` at exactly `MAX_KEY_SET_BYTES + 1` and `MAX_KEY_SET_BYTES` below -
    // and both shapes of `FileKeySet::read` return the same refusal for the same bytes, so it does
    // NOT hold WHERE the check sits. "Refused rather than read" in this cell's own name is a
    // code-reading claim: the pre-allocation half - that `Read::take` at `source.rs:113` bounds the
    // allocation itself rather than a length check after `std::fs::read_to_string` - is held by
    // review, not by a cell here.
    //
    // The real source and not a fake, because the bound lives in the implementor: the port hands back
    // an owned `String`, so nothing above it could object once the allocation had happened. The
    // scratch file is named after the process and removed again, which is what the one other
    // filesystem cell in this suite does.
    let path = std::env::temp_dir().join(format!("sutura-oversized-jwks-{}.json", std::process::id()));
    let oversized = "x".repeat(MAX_KEY_SET_BYTES + 1);
    std::fs::write(&path, &oversized).expect("the scratch key set is writable");
    let refused = FileKeySet::at(&path)
        .read()
        .expect_err("a document over the bound is not read");
    let KeySetUnavailable::TooLarge { limit, .. } = refused else {
        panic!("expected a refusal about size, got {refused:?}");
    };
    assert_eq!(limit, MAX_KEY_SET_BYTES);

    // And a document exactly at the bound is accepted. What this half holds is the EDGE and not the
    // bound: a `>=` in the source fails it and a source with no bound passes it, so it is a control
    // on the number the refusal above proves exists.
    std::fs::write(&path, "x".repeat(MAX_KEY_SET_BYTES)).expect("the scratch key set is writable");
    let read = FileKeySet::at(&path).read().expect("a document at the bound is read");
    assert_eq!(read.len(), MAX_KEY_SET_BYTES);
    std::fs::remove_file(&path).expect("the scratch key set is removable");
}

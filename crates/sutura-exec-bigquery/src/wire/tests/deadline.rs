//! One deadline per answer, `docs/adr/0029`: tests for `crate::wire::remaining_of_the_ports_deadline`
//! and `crate::wire::configured_budget_seconds` - the two free functions that replaced
//! `QueryDeadline::within_request_timeout` and `CALLS_PER_ANSWER`. The claim that a shared `Deadline`
//! sees LESS of itself on a second, later read is `Deadline::remaining_at`'s own arithmetic and is
//! tested in `sutura-domain`; nothing here repeats it.
//!
//! **Split out of `wire/tests.rs` when this section took it past the 1000-line ceiling
//! `cargo xtask max-lines` enforces**, at the seam its own header comment already drew. Everything
//! below reaches the fixtures `super` (`wire::tests`) already builds - `pinned`, `bounds`, `project`,
//! `dataset`, `Fixed` - so nothing here is a second copy of them.

use core::time::Duration;
use std::time::Instant;

use sutura_domain::identity::{Expiry, Secret};
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use crate::transport::{JobDeadline, JobRequest, JobTransport as _};
use crate::wire::credential::Bearer;
use crate::wire::{
    BigQueryWire, CallDeadline, DryRun, EndpointMessage, QueryDeadline, ReasonCode, WireError, call_body,
    configured_budget_seconds, remaining_of_the_ports_deadline,
};

use super::{CannotFail, Fixed, bounds, dataset, pinned, project};

/// The port's deadline every test below opens, so each test states only what it changes: when it
/// was opened and how much it holds.
fn opened(started: Instant, seconds: u64) -> Deadline {
    Deadline::opened_at(
        started,
        Budget::parse(Duration::from_secs(seconds)).expect("a whole second is a budget"),
    )
}

#[test]
fn the_boot_path_always_opens_a_fresh_window_regardless_of_elapsed_time() {
    // **What `remaining_of_the_ports_deadline` decides that `Deadline::remaining_at` alone does
    // not**, and the reason this cell survives beside `sutura-domain`'s own arithmetic tests: the
    // `JobDeadline::Boot` arm ignores `now` entirely and always answers the configured bound, which
    // is the regression `CALLS_PER_ANSWER`'s replacement exists to rule out - a boot-path call never
    // "runs low" the way a port deadline would.
    let far_from_now = Instant::now() + Duration::from_secs(3600);
    assert_eq!(
        remaining_of_the_ports_deadline(JobDeadline::Boot, bounds(), far_from_now),
        Some(bounds().deadline().budget()),
        "the boot path must answer the same configured window no matter how much time has passed"
    );
}

#[test]
fn a_port_deadline_already_spent_is_named_by_its_own_configured_budget_and_not_this_adapters() {
    let opened_at = Instant::now()
        .checked_sub(Duration::from_secs(60))
        .expect("an instant a minute ago");
    let spent = opened(opened_at, 2);

    assert_eq!(
        remaining_of_the_ports_deadline(JobDeadline::Port(spent), bounds(), Instant::now()),
        None
    );
    assert_eq!(configured_budget_seconds(JobDeadline::Port(spent), bounds()), 2);
    // Bounds is thirty seconds - the number a `DeadlineSpent` must NOT name once a request carries
    // its own `Deadline`, because thirty is not the budget that ran out.
    assert_ne!(
        configured_budget_seconds(JobDeadline::Port(spent), bounds()),
        bounds().deadline().budget().as_secs()
    );
    // The boot path has no `Deadline` to be spent, so it always names this adapter's own bound.
    assert_eq!(
        configured_budget_seconds(JobDeadline::Boot, bounds()),
        bounds().deadline().budget().as_secs()
    );
}

#[test]
fn the_credential_exchange_is_handed_the_ports_deadline_and_not_this_adapters_configured_bound() {
    // **The wiring `submit` needed for the claim above to reach a real call.** `pinned()`'s own
    // `bounds()` is thirty seconds; a request carrying a two-second port `Deadline` must still hand
    // the EXCHANGE only two seconds; not the whole thirty. Reached with an expired bearer, exactly as
    // `an_expired_credential_is_refused_before_anything_is_sent` is, so this stops right after the
    // exchange and never reaches the network.
    let source = Fixed::holding(Bearer::of(Secret::new("t"), Expiry::At { unix_seconds: 1 }));
    let wire = BigQueryWire::new(pinned(), source);
    let project = project();
    let dataset = dataset();
    let tight = opened(Instant::now(), 2);
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None, JobDeadline::Port(tight));

    wire.run(&request)
        .expect_err("the fixture's token is expired, so the call refuses after the exchange");
    let handed = wire.credentials.handed.get().expect("the exchange was handed a budget");
    assert!(
        handed <= Duration::from_secs(2),
        "the exchange was handed more than the port's own two-second deadline - {handed:?}, against \
         a thirty-second configured bound it must not have read instead"
    );
}

#[test]
fn a_spent_port_deadline_refuses_before_the_exchange_is_ever_asked() {
    // **The other half of "checked BEFORE the credential exchange too".** A caller that waited past
    // its own budget before this adapter was even reached should not spend it on an exchange nobody
    // is still waiting for - asserted here as ZERO calls into the credential source, which
    // `Fixed::handed` staying `None` proves.
    let source = Fixed::holding(Bearer::of(Secret::new("t"), Expiry::NothingExpires));
    let wire = BigQueryWire::new(pinned(), source);
    let project = project();
    let dataset = dataset();
    let spent = opened(
        Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("an instant a minute ago"),
        1,
    );
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None, JobDeadline::Port(spent));

    match wire.run(&request) {
        Err(WireError::DeadlineSpent { budget_seconds }) => assert_eq!(budget_seconds, 1),
        other => panic!("a port deadline spent before the call started was mapped to {other:?}"),
    }
    assert!(
        wire.credentials.handed.get().is_none(),
        "the credential exchange was asked despite the port's own deadline already being spent"
    );
}

#[test]
fn the_bodys_two_timeouts_are_what_is_left_of_the_call_not_its_configured_ceiling() {
    // **The wiring `submit` needs, pinned with no socket.** `call_body` reads `call.remaining()` and
    // nothing else - never `Instant::now()` itself - so opening `call` at an instant already in the
    // past is a fixture rather than a race: what elapsed since `opened_at` is close to exactly known
    // (within the wall-clock time this test itself takes to run, hence the tolerance below rather
    // than an exact millisecond), and it is nowhere near the thirty-second ceiling. If `call_body`
    // fed `body` the configured ceiling instead of `call.remaining()`, this would read close to
    // 30 000 rather than close to 9 000.
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None, JobDeadline::Boot);
    let call = CallDeadline::opened_at(
        Instant::now().checked_sub(Duration::from_secs(21)).expect("21 seconds ago"),
        QueryDeadline::parse(30).expect("30 seconds is a deadline"),
    );

    let (built, left) = call_body::<CannotFail>(&request, DryRun::No, bounds(), call).expect("nine seconds is still left");
    let sent = serde_json::to_value(built).expect("the body serializes");
    let timeout_ms = sent["timeoutMs"].as_u64().expect("timeoutMs is a number");

    assert!(
        (8_000..=9_000).contains(&timeout_ms),
        "the body carried something other than what is LEFT of the call: {sent}"
    );
    assert_eq!(sent["jobTimeoutMs"], timeout_ms, "the two timeout fields must agree: {sent}");
    // The socket reads THIS `left`, returned alongside the body rather than re-derived - so it is
    // exactly the value the body was built from, in milliseconds, not a second independent read.
    let left_ms = u64::try_from(left.as_millis()).expect("nine seconds fits a u64 of milliseconds");
    assert_eq!(left_ms, timeout_ms, "the returned `left` must be what the body carries");
}

#[test]
fn call_body_refuses_a_spent_call_naming_the_configured_budget() {
    // The other half: `call_body` is where the post-exchange `DeadlineSpent` refusal actually lives
    // now, and it has to name the budget from `request.deadline()` - never from `call` alone, which
    // does not say whether it was opened from the port's own `Deadline` or from this transport's
    // configured bound.
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None, JobDeadline::Boot);
    let spent = CallDeadline::opened_at(
        Instant::now().checked_sub(Duration::from_secs(60)).expect("a minute ago"),
        QueryDeadline::parse(30).expect("30 seconds is a deadline"),
    );

    match call_body::<CannotFail>(&request, DryRun::No, bounds(), spent) {
        Err(WireError::DeadlineSpent { budget_seconds }) => assert_eq!(budget_seconds, 30),
        other => panic!("a spent call was mapped to {other:?}"),
    }
}

#[test]
fn a_job_that_did_not_finish_is_the_deadline_running_out_and_only_deadlinespent_joins_it() {
    // **The coded mapping had no cell at all before this** - `WireError::NotComplete`'s own doc
    // argues it is what a job stopped at `jobTimeoutMs` answers with, but an argument in a comment is
    // not a test. Any wire will do: `deadline_exceeded` reads the error and nothing else.
    let wire = BigQueryWire::new(
        pinned(),
        Fixed::holding(Bearer::of(Secret::new("t"), Expiry::At { unix_seconds: 1 })),
    );
    assert!(
        wire.deadline_exceeded(&WireError::<CannotFail>::NotComplete {
            named: ReasonCode::Absent,
        }),
        "a job that did not finish has to be recognised as the deadline running out"
    );
    assert!(
        wire.deadline_exceeded(&WireError::<CannotFail>::DeadlineSpent { budget_seconds: 1 }),
        "a budget found spent before the send is the deadline running out too"
    );
    // The control: an ordinary refusal or a shape defect is not the deadline, and answering `true`
    // for either would tell a caller to narrow a question that was never about time.
    for failure in [
        WireError::<CannotFail>::Refused {
            status: 403,
            named: ReasonCode::AccessDenied,
            detail: EndpointMessage::bounded(None),
        },
        WireError::NoSchema { rows: 1 },
        WireError::NotAScalar { row: 0, column: 0 },
    ] {
        assert!(
            !wire.deadline_exceeded(&failure),
            "{failure:?} was reported as the deadline running out"
        );
    }
}

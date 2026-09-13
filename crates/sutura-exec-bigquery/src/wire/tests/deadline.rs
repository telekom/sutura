//! One deadline per answer, `docs/adr/0029`: tests for `crate::wire::remaining_of_the_ports_deadline`
//! and `crate::wire::configured_budget_seconds` - the two free functions that replaced
//! `QueryDeadline::within_request_timeout` and `CALLS_PER_ANSWER`.
//!
//! **Split out of `wire/tests.rs` when this section took it past the 1000-line ceiling
//! `cargo xtask max-lines` enforces**, at the seam its own header comment already drew. Everything
//! below reaches the fixtures `super` (`wire::tests`) already builds - `pinned`, `bounds`, `project`,
//! `dataset`, `Fixed` - so nothing here is a second copy of them.

use core::time::Duration;
use std::time::Instant;

use sutura_domain::identity::{Expiry, Secret};
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use crate::transport::{JobRequest, JobTransport as _};
use crate::wire::credential::Bearer;
use crate::wire::{BigQueryWire, WireError, configured_budget_seconds, remaining_of_the_ports_deadline};

use super::{Fixed, bounds, dataset, pinned, project};

/// The port's deadline every test below opens, so each test states only what it changes: when it
/// was opened and how much it holds.
fn opened(started: Instant, seconds: u64) -> Deadline {
    Deadline::opened_at(
        started,
        Budget::parse(Duration::from_secs(seconds)).expect("a whole second is a budget"),
    )
}

#[test]
fn two_calls_that_share_one_deadline_see_the_same_instant_and_the_second_sees_what_the_first_left() {
    // **The claim `submit` now holds, pinned as a pure comparison rather than through a real sleep.**
    // `Deadline` will not hand out its own opening instant - by design - so this asks the free
    // function `submit` itself calls what is left at two DIFFERENT instants of the SAME `Deadline`,
    // which is exactly what two calls one answer makes do when they share one.
    let opened_at = Instant::now();
    let deadline = opened(opened_at, 10);

    let first = remaining_of_the_ports_deadline(Some(deadline), bounds(), opened_at).expect("ten seconds is not yet spent");
    let second = remaining_of_the_ports_deadline(Some(deadline), bounds(), opened_at + Duration::from_secs(4))
        .expect("six seconds is still left");

    assert_eq!(first, Duration::from_secs(10));
    assert_eq!(second, Duration::from_secs(6));
    assert!(
        second < first,
        "the second call did not see less of the SAME deadline the first call read: {first:?} then {second:?}"
    );

    // **The regression this guards, made concrete.** Re-opening from this adapter's own configured
    // bound instead of reading the port's `Deadline` - `None`, the boot path's own shape - answers
    // the same thirty seconds however much wall-clock time has passed, which is `CALLS_PER_ANSWER`'s
    // own arithmetic one level up: every call gets a full budget of its own regardless of what an
    // earlier one spent.
    let reopened_each_time = remaining_of_the_ports_deadline(None, bounds(), opened_at + Duration::from_secs(4))
        .expect("the boot path's own window is always fresh");
    assert_eq!(reopened_each_time, bounds().deadline().budget());
}

#[test]
fn a_port_deadline_already_spent_is_named_by_its_own_configured_budget_and_not_this_adapters() {
    let opened_at = Instant::now()
        .checked_sub(Duration::from_secs(60))
        .expect("an instant a minute ago");
    let spent = opened(opened_at, 2);

    assert_eq!(remaining_of_the_ports_deadline(Some(spent), bounds(), Instant::now()), None);
    assert_eq!(configured_budget_seconds(Some(spent), bounds()), 2);
    // Bounds is thirty seconds - the number a `DeadlineSpent` must NOT name once a request carries
    // its own `Deadline`, because thirty is not the budget that ran out.
    assert_ne!(
        configured_budget_seconds(Some(spent), bounds()),
        bounds().deadline().budget().as_secs()
    );
    // The boot path has no `Deadline` to be spent, so it always names this adapter's own bound.
    assert_eq!(
        configured_budget_seconds(None, bounds()),
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
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None, Some(tight));

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
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None, Some(spent));

    match wire.run(&request) {
        Err(WireError::DeadlineSpent { budget_seconds }) => assert_eq!(budget_seconds, 1),
        other => panic!("a port deadline spent before the call started was mapped to {other:?}"),
    }
    assert!(
        wire.credentials.handed.get().is_none(),
        "the credential exchange was asked despite the port's own deadline already being spent"
    );
}

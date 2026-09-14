//! One acceptance cell for `docs/adr/0029`'s `BigQuery` slice, split out when it took `acceptance.rs`
//! past the 1000-line ceiling `cargo xtask max-lines` enforces. `super::warehouse` and the fixture
//! helpers `acceptance.rs` already builds are reached from here exactly as every other cell in that
//! file reaches them.

use sutura_domain::plan::Executable;
use sutura_domain::warehouse::Warehouse as _;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_exec_bigquery::BigQueryError;
use sutura_exec_bigquery::wire::WireError;

use crate::fixture::{Fixture, plan};
use crate::support::presented;

#[test]
#[ignore = "needs a real BigQuery project, named in the developer's own environment"]
fn a_port_deadline_already_spent_refuses_through_the_real_wire_and_maps_to_deadline_exceeded() {
    // **Deliberately NOT a race against how long the statement takes.** A first version of this
    // cell asked for a two-second budget over a normal statement, betting that the round trip would
    // outrun it and reach the SERVICE's own `jobTimeoutMs` cancellation. `just bigquery-acceptance`
    // measured that bet wrong: this crate's corpus fixture is a handful of rows, and the whole
    // round trip - token exchange, job, one page read - finished in well under a second, so the
    // statement always answers before any deadline shorter than that could matter. A budget picked
    // to "usually" lose that race is a flake by construction, on a live network, in a gate that
    // bills a real project - exactly what `docs/adr/0029`'s own module header warns against for
    // BigQuery's reply margin.
    //
    // **So what THIS cell measures instead:** a port `Deadline` opened a minute in the past, with a
    // budget far shorter than that, is unambiguously spent before `submit` ever reads a clock -
    // `Deadline::remaining_at` is a pure comparison against `now`, not a timer, so there is no
    // window in which this could race and no dependence on how fast the endpoint answers.
    // `warehouse.execute` here goes through the REAL `BigQueryWire` and the REAL credential this
    // job's environment supplies - not the fake `AccessTokens`/`JobTransport` every other deadline
    // test in this crate uses - so the claim beyond those unit tests is narrow and stated exactly:
    // *the real composition refuses a spent port deadline before it sends anything, and
    // `deadline_exceeded` reports that refusal.* Zero network calls happen, so this cannot be flaky
    // and does not spend the project's quota.
    //
    // **What this does NOT measure, on purpose.** The service's own reply for a job it stopped at
    // `jobTimeoutMs` - the shape `WireError::NotComplete`'s doc comment names as documented rather
    // than measured - is not reached here, because the request is never sent. Proving that shape
    // needs a statement that reliably outruns a real budget without racing against fixture size or
    // network jitter, which this corpus does not yet provide; `docs/adr/0029` states this as an open
    // limit rather than claiming it closed.
    let fixture = Fixture::required();
    let table = fixture.unqualified();
    let warehouse = super::warehouse(fixture);
    let plan = plan(&table);
    let spent = Deadline::opened_at(
        std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(60))
            .expect("an instant a minute ago"),
        Budget::parse(std::time::Duration::from_secs(1)).expect("one second is a budget"),
    );

    let error = warehouse
        .execute(Executable::Query(&plan), &presented(), spent)
        .expect_err("a deadline opened a minute ago with a one-second budget is spent by construction");
    assert!(
        matches!(
            error,
            BigQueryError::Endpoint {
                cause: WireError::DeadlineSpent { .. }
            }
        ),
        "a spent port deadline reached the real wire and was not refused as DeadlineSpent: {error:?}"
    );
    assert!(
        warehouse.deadline_exceeded(&error),
        "DeadlineSpent through the real wire did not map to `deadline_exceeded`: {error:?}"
    );
}

#![forbid(unsafe_code)]
//! A result stream that outruns the request deadline, over the driver the tier mounts: the first
//! batch arrives, a later read is cut off by the driver's own client-side bound before the server's
//! `max_execution_time` fires, and the adapter reads that as the deadline - not as a source fault.
//! The driver reports the timeout across its C interface as error text only, so this cell is what
//! ties the text `adbc.rs` reads to the driver that is shipped.

#[cfg(test)]
mod adbc_stream_deadline {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::identity::Secret;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::ResultBudget;
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_exec_clickhouse::adbc::{AdbcClickHouseWarehouse, AdbcError, driver_location};

    /// Twenty one-row blocks, a fifth of a second apart: four seconds in all, with the first block
    /// at once. `max_block_size` sits in the subquery because the statement's own top level may carry
    /// no `SETTINGS`.
    const SLOW_STREAM: &str =
        "SELECT number, slept FROM (SELECT number, sleepEachRow(0.2) AS slept FROM numbers(20) SETTINGS max_block_size = 1)";

    fn variable(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_absent| panic!("{name} is unset, so the tier has no fixture credential"))
    }

    /// A 1.2 s budget is spent before the server's `max_execution_time` of 2 s (the budget rounded
    /// up), so it is the client that fires, and not the server's `Code: 159`.
    #[test]
    fn a_stream_that_outruns_the_deadline_is_a_deadline_after_its_first_batch() {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse") {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("adbc_stream_deadline: NOT RUN - {absent}");
                return;
            }
        };
        let driver = driver_location().unwrap_or_else(|e| panic!("the dev shell and every nix check mount the driver: {e}"));
        let warehouse = AdbcClickHouseWarehouse::new(
            corpus::source(),
            corpus::posture(),
            driver,
            format!("http://{}:{}", endpoint.host(), endpoint.port()),
            variable("SUTURA_CLICKHOUSE_TIER_USER"),
            Secret::new(variable("SUTURA_CLICKHOUSE_TIER_PASSWORD")),
            None,
            ResultBudget::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test budget is positive")),
        );
        let statement = RawStatement::parse(SLOW_STREAM).expect("bounded planned SQL");
        let tables = BTreeSet::new();
        let budget = Budget::parse(Duration::from_millis(1_200)).expect("a non-zero budget");

        let started = Instant::now();
        let error = warehouse
            .execute_planned_statement(
                &statement,
                &tables,
                &corpus::presented(),
                Deadline::opened_at(Instant::now(), budget),
            )
            .expect("planned SQL is supported")
            .expect_err("a four-second stream cannot finish inside 1.2 seconds");

        assert!(
            matches!(error, AdbcError::Batch(_)),
            "the first batch arrived, so the cut is in the stream and not in the request: {error}"
        );
        assert!(
            warehouse.deadline_exceeded(&error),
            "the adapter reads the driver's text as the deadline: {error}"
        );
        assert!(
            !error.to_string().contains("Code: 159"),
            "the server's own limit fired, so the client bound was not what cut the stream: {error}"
        );
        assert!(started.elapsed() < Duration::from_millis(1_900), "{:?}", started.elapsed());
    }
}

#![forbid(unsafe_code)]
//! A fixture table reloaded against the tier's server, which is the one that refuses the rows.

#[cfg(test)]
mod reload {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::plan::Executable;
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_exec_clickhouse::adbc::AdbcClickHouseWarehouse;
    use sutura_exec_clickhouse::fixture::{BatchError, FixtureError, credential_from_env};

    /// A reload the importer refuses leaves the previous rows answering. The blank line passes the
    /// type inference and is refused building the batch, before any statement is sent. **The limit:**
    /// that is a refusal ahead of the table swap, so this cell does not hold the swap itself - that a
    /// refusal AFTER rows were sent leaves the table as it was is held by the swap being one statement
    /// (see `fixture`), and by review.
    #[test]
    fn a_refused_reload_leaves_the_previous_rows_in_place() {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse") {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("reload: NOT RUN - {absent}");
                return;
            }
        };
        let warehouse = AdbcClickHouseWarehouse::connect_in_database(
            corpus::source(),
            corpus::posture(),
            endpoint.host(),
            endpoint.port(),
            &credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}")),
            &format!("reload_{}", std::process::id()),
            sutura_domain::warehouse::ResultBudget::of_bytes(
                core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test budget is positive"),
            ),
        )
        .unwrap_or_else(|e| panic!("clickhouse did not open at {endpoint}: {e}"));
        warehouse
            .load_conformance_csv(&corpus::table(), &corpus::on_disk())
            .unwrap_or_else(|e| panic!("clickhouse could not load the conformance corpus: {e}"));

        let text = std::fs::read_to_string(corpus::on_disk()).expect("the corpus is readable");
        let mut lines: Vec<&str> = text.lines().collect();
        lines.insert(2, "");
        let refused = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("reload-{}.csv", std::process::id()));
        std::fs::write(&refused, lines.join("\n")).expect("the target's scratch directory is writable");
        let reloaded = warehouse.load_conformance_csv(&corpus::table(), &refused);
        assert!(
            matches!(
                reloaded,
                Err(FixtureError::Rows {
                    cause: BatchError::BlankRow { line: 3 },
                    ..
                })
            ),
            "{reloaded:?}"
        );

        let case = corpus::cases()
            .into_iter()
            .find(|case| case.name() == "decimal-total-by-day")
            .expect("the corpus has the case");
        let answer = warehouse
            .execute(Executable::Query(case.plan()), &corpus::presented(), corpus::deadline())
            .unwrap_or_else(|e| panic!("the previous rows did not answer: {e}"));
        assert_eq!(answer.rows(), case.expected().rows().len());
    }
}

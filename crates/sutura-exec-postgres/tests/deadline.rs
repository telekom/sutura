#![forbid(unsafe_code)]
//! `docs/adr/0029`'s Postgres row, measured against a real server rather than asserted from the
//! driver's documentation - `SET LOCAL statement_timeout` actually stops a running statement AND a
//! blocked `PREPARE`, the certified path's per-request value is not the connect-time ceiling, and
//! the raw path now carries the same per-request `Deadline` too (`telekom/sutura#1144`), narrowing
//! the connect-time ceiling rather than only being stopped by it.
//!
//! Same tier, same absence handling as `tests/conformance.rs` and `tests/raw.rs`: every venue that
//! runs the suite provisions the tier, so these cells RUN; a developer machine with none writes
//! `NOT RUN` and the reason to stderr. See `tests/conformance.rs`'s header for the whole argument.
//!
//! # Why a VIEW cross-joined with `pg_sleep`, and not a `WHERE` guard or a projected column
//!
//! A view's projected columns that the outer certified query never selects can be pruned by the
//! planner even when they are volatile - the well-known reason a `random()` column nobody reads is
//! sometimes never called. A function named in the FROM clause is different: `pg_sleep(n)` there is
//! a required join input (one synthetic row this adapter's rows are cross-joined against), evaluated
//! exactly once to produce it regardless of how many rows the real table holds or which of its
//! columns the outer query projects - the standard idiom for making one statement take at least `n`
//! seconds independent of data size.

#[cfg(test)]
mod deadline {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_dev::tolerance::Tolerance;
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, TableName};
    use sutura_domain::plan::{
        Executable, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin,
        QueryPlan, ResultLabel, StatementTables,
    };
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_domain::warehouse::{ParamValue, Value, Warehouse as _};
    use sutura_exec_postgres::adbc::{AdbcPostgres, Conninfo, FixtureAdmin};
    use sutura_exec_postgres::fixture::FixtureCredential;

    const SERVICE: &str = "postgres";

    /// The tier's connection string into `schema`, or `None` (and a `NOT RUN` line) without a tier.
    fn conninfo(case: &str, schema: &str) -> Option<Conninfo> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("deadline::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        Some(
            credential
                .conninfo_in(&corpus::source(), endpoint.host(), endpoint.port(), schema)
                .unwrap_or_else(|e| panic!("no connection string for the tier at {endpoint}: {e}")),
        )
    }

    /// A fresh schema per test - the isolation `tests/conformance.rs` and `tests/raw.rs` use, so
    /// cells running in parallel against one server never see one another's views or tables.
    fn open(case: &str) -> Option<(AdbcPostgres, String)> {
        let schema = format!("deadline_{case}_{}", std::process::id());
        let warehouse = AdbcPostgres::new(
            corpus::source(),
            corpus::posture(),
            sutura_exec_postgres::adbc::PostgresDriver::from_host().expect("the tier is up, so a driver is named"),
            conninfo(case, &schema)?,
        )
        .expect("the default ceiling parses");
        warehouse
            .create_schema(&schema)
            .unwrap_or_else(|e| panic!("postgres could not create {schema}: {e}"));
        Some((warehouse, schema))
    }

    /// A second, PLAIN connection into the SAME schema `open` puts its warehouse in, running `sql` -
    /// what neither port method here can: `execute` only ever renders a `SELECT`, and `execute_raw`
    /// runs inside a `READ ONLY` transaction this adapter always rolls back. The connection is
    /// returned, so a transaction `sql` leaves open lives until the caller drops it.
    fn admin(case: &str, schema: &str, sql: &str) -> Option<FixtureAdmin> {
        let mut admin = FixtureAdmin::open(
            &sutura_exec_postgres::adbc::PostgresDriver::from_host().expect("the tier is up, so a driver is named"),
            &conninfo(case, schema)?,
        )
        .unwrap_or_else(|e| panic!("the admin connection did not open: {e}"));
        admin
            .run(sql)
            .unwrap_or_else(|e| panic!("the admin connection could not run `{sql}`: {e}"));
        Some(admin)
    }

    fn create_view(schema: &str, sql: &str) {
        drop(admin("view", schema, sql));
    }

    fn statement(sql: &str) -> RawStatement {
        RawStatement::parse(sql).expect("a test statement is a statement")
    }

    /// A second connection holding `table` locked `ACCESS EXCLUSIVE` in an open, uncommitted
    /// transaction - for as long as the returned value lives. Dropping it closes the connection,
    /// which terminates the backend and releases the lock.
    fn hold_exclusive_lock(case: &str, schema: &str, table: &TableName) -> Option<FixtureAdmin> {
        admin(
            case,
            schema,
            &format!("BEGIN; LOCK TABLE \"{table}\" IN ACCESS EXCLUSIVE MODE"),
        )
    }

    fn column(table: &TableName, name: &str) -> PlanColumn {
        PlanColumn::new(table.clone(), ColumnName::parse(name).expect("a test column name is a name"))
    }

    /// A range over `table`'s own `day` column, bound `[start, end)` - the plan's mandatory bucket
    /// and range machinery, built once here rather than at each call site.
    fn range_over(start: Date, end: Date, table: &TableName) -> (TimeRange, PlanBindings) {
        let range = TimeRange::new(start, end).expect("start precedes end");
        let day_column = column(table, "day");
        let filters = vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: day_column.clone(),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: day_column,
                    param: 1,
                },
            ),
        ];
        let bindings = PlanBindings::parse(filters, vec![ParamValue::Date(start), ParamValue::Date(end)])
            .expect("the two range bounds bind in placeholder order");
        (range, bindings)
    }

    /// A one-day-wide range over `table`'s own `day` column, covering exactly the fixed literal date
    /// a probe view projects.
    fn covers(day: Date, table: &TableName) -> (TimeRange, PlanBindings) {
        let day_after =
            Date::from_days_since_epoch(day.days_since_epoch() + 1).expect("a day plus one day is still representable");
        range_over(day, day_after, table)
    }

    /// **The cancellation proof.** A view that cross-joins the loaded corpus table with `pg_sleep(2)`
    /// (see this file's header for why a FROM-clause function and not a projected column) always
    /// takes at least two seconds to read, independent of how many corpus rows exist. A certified
    /// `execute` under a 300 ms budget must be stopped well inside it, not after the full two
    /// seconds and not after the connect-time ceiling (15 s by default, and this test does not touch
    /// it): before this change, the same call ran for the full 2 s and answered rows.
    #[test]
    fn a_certified_question_over_its_budget_is_stopped_at_the_data_system() {
        let Some((warehouse, schema)) = open("cancel") else { return };
        warehouse
            .load_fixture_csv(&corpus::table(), &corpus::on_disk())
            .expect("the corpus loads");
        create_view(
            &schema,
            &format!(
                "CREATE VIEW slow_events AS SELECT t.* FROM \"{table}\" AS t, pg_sleep(2)",
                table = corpus::table()
            ),
        );
        let table = TableName::parse("slow_events").expect("a view name is a table name");
        let metric = MetricName::parse("slow_probe").expect("a test metric name is a name");
        // A wide window rather than the corpus's own exact dates (private to `sutura-conformance`'s
        // `corpus` module) - it only has to include whatever the fixture's rows actually are, not
        // name them, and a nonempty result is not the point of this test: the cross-joined
        // `pg_sleep(2)` runs once regardless.
        let (range, bindings) = range_over(
            Date::new(2000, 1, 1).expect("year 2000 is in range"),
            Date::new(2099, 12, 31).expect("year 2099 is in range"),
            &table,
        );
        let plan = QueryPlan::new(
            corpus::source(),
            metric.clone(),
            StatementTables::only(table.clone()),
            PlanBucket::new(ResultLabel::bucket(), Grain::Day, column(&table, "day")),
            Vec::new(),
            PlanMeasure::Simple {
                term: PlanTerm::Aggregate {
                    aggregate: Aggregate::Sum,
                    column: column(&table, "amount_cents"),
                },
            },
            ResultLabel::measure(&metric),
            bindings,
            range,
        );
        let deadline = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_millis(300)).expect("300ms is a budget"),
        );

        let started = Instant::now();
        let outcome = warehouse.execute(Executable::Query(&plan), &corpus::presented(), deadline);
        let elapsed = started.elapsed();

        let error = outcome.expect_err("a question over its budget must not answer with rows");
        assert!(
            warehouse.deadline_exceeded(&error),
            "a statement stopped by SET LOCAL statement_timeout must classify as deadline_exceeded: {error}"
        );
        // CI's own margin, or a wider one on a shared machine - see `sutura_dev::tolerance`. Both
        // stay well under the 2s `pg_sleep` and the 15s connect-time ceiling this cell must land
        // inside of, not merely inside of *a* number.
        let ceiling = Tolerance::from_env().ceiling(Duration::from_secs(1), Duration::from_millis(1800));
        assert!(
            elapsed < ceiling,
            "stopped at ~300ms plus tolerance, not run to completion (2s) or to the 15s ceiling: {elapsed:?}"
        );
    }

    /// **The raw path's own ceiling variant.** A deadline WIDER than the connect-time ceiling
    /// (`telekom/sutura#1144`'s `SET LOCAL` only ever narrows, never widens it) proves the ceiling
    /// alone still stops an unbounded caller statement whose own budget would not have, which is
    /// what discharges `telekom/sutura#129`'s cancellation prerequisite for the raw tool: the source
    /// it runs over can cancel, even when the asker's own budget is generous.
    ///
    /// **Slow on purpose, rather than narrowing the ceiling.** `std::env::set_var` is `unsafe` in
    /// this edition and this crate's root forbids `unsafe` outright (`crates/sutura-cli/tests/declared_source.rs`'s
    /// own header states the same rule) - what a test can decide is what a CHILD process sees, and
    /// spawning one just to narrow this single value is not worth a second binary. So this cell
    /// leaves the default 15 s ceiling in place and sleeps past it instead of narrowing it.
    #[test]
    fn a_raw_statement_is_stopped_by_the_connect_time_ceiling() {
        let Some((warehouse, _schema)) = open("rawceiling") else {
            return;
        };
        // Wider than the 15s default ceiling on purpose: this cell proves the CEILING fires, not a
        // narrower per-request budget - `a_raw_statement_over_its_own_budget_is_stopped_before_the_ceiling`
        // below is the sibling that proves the budget narrows it.
        let generous = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_secs(30)).expect("30s is a budget"),
        );

        let started = Instant::now();
        let outcome = warehouse.execute_raw(&statement("select pg_sleep(20)"), &corpus::presented(), generous);
        let elapsed = started.elapsed();

        let error = outcome
            .expect("this adapter accepts a raw statement")
            .expect_err("a statement over the ceiling must not answer with rows");
        assert!(
            warehouse.deadline_exceeded(&error),
            "the ceiling firing must classify as deadline_exceeded too: {error:?}"
        );
        // Both bounds stay under the statement's own 20s `pg_sleep`, which is the ceiling this
        // cell has to prove happened BEFORE - answering past it is a different failure entirely.
        let ceiling = Tolerance::from_env().ceiling(Duration::from_secs(18), Duration::from_millis(19_500));
        assert!(
            elapsed < ceiling,
            "stopped at the ~15s default ceiling plus tolerance, not run to the statement's own 20s: {elapsed:?}"
        );
    }

    /// **The raw path's own per-request narrowing, `telekom/sutura#1144`'s own cell.** The sibling
    /// of `a_certified_question_over_its_budget_is_stopped_at_the_data_system`: a raw statement that
    /// would otherwise run for the full 2s `pg_sleep` is stopped at its own 300ms budget, well short
    /// of both the statement's own runtime and the 15s connect-time ceiling - proving `SET LOCAL
    /// statement_timeout` narrows the raw path's session the same way it narrows the certified one,
    /// rather than the raw path being bounded only by the ceiling as it was before this change.
    #[test]
    fn a_raw_statement_over_its_own_budget_is_stopped_before_the_ceiling() {
        let Some((warehouse, _schema)) = open("rawbudget") else {
            return;
        };
        let deadline = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_millis(300)).expect("300ms is a budget"),
        );

        let started = Instant::now();
        let outcome = warehouse.execute_raw(&statement("select pg_sleep(2)"), &corpus::presented(), deadline);
        let elapsed = started.elapsed();

        let error = outcome
            .expect("this adapter accepts a raw statement")
            .expect_err("a statement over its own budget must not answer with rows");
        assert!(
            warehouse.deadline_exceeded(&error),
            "a raw statement stopped by SET LOCAL statement_timeout must classify as deadline_exceeded: {error:?}"
        );
        let ceiling = Tolerance::from_env().ceiling(Duration::from_secs(1), Duration::from_millis(1800));
        assert!(
            elapsed < ceiling,
            "stopped at ~300ms plus tolerance, not run to completion (2s) or to the 15s ceiling: {elapsed:?}"
        );
    }

    /// **The raw path's own per-statement proof**, the sibling of
    /// `a_certified_call_sets_the_per_statement_value_not_the_ceiling`: a raw `SELECT` reading back
    /// `pg_settings.setting` for `statement_timeout` must see the smaller, per-request value `SET
    /// LOCAL` set from the deadline, not the connect-time ceiling this connection was opened with.
    #[test]
    fn a_raw_call_sets_the_per_statement_value_not_the_ceiling() {
        let Some((warehouse, _schema)) = open("rawshowtimeout") else {
            return;
        };
        // Comfortably inside the default 15s ceiling, and not a round number a Postgres display
        // format could coincide with - the same value the certified sibling cell uses.
        let deadline = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_millis(4321)).expect("4321ms is a budget"),
        );

        let outcome = warehouse
            .execute_raw(
                &statement("select (SELECT setting::bigint FROM pg_settings WHERE name = 'statement_timeout') AS timeout_ms"),
                &corpus::presented(),
                deadline,
            )
            .expect("this adapter accepts a raw statement")
            .expect("well inside every timeout, this call must answer");
        let seen_ms = match outcome.rows().first().and_then(|row| row.first()) {
            Some(Value::Integer(ms)) => *ms,
            other => panic!("expected exactly one integer cell, got {other:?}"),
        };
        assert!(
            seen_ms < 15_000,
            "the per-statement value must be smaller than the connect-time ceiling: saw {seen_ms}ms"
        );
        // Never above 4321: `SET LOCAL` cannot see a LARGER budget than the deadline was opened
        // with, the same exact arithmetic the certified sibling cell checks.
        assert!(
            seen_ms <= 4321,
            "the per-statement value must not exceed the budget it was opened with: saw {seen_ms}ms"
        );
        let slack_ms = Tolerance::from_env().ceiling(Duration::from_millis(200), Duration::from_millis(1000));
        let slack_ms = i64::try_from(slack_ms.as_millis()).expect("a millisecond slack of a few seconds fits an i64");
        assert!(
            seen_ms >= 4321 - slack_ms,
            "expected close to the 4321ms budget within {slack_ms}ms, saw {seen_ms}ms"
        );
    }

    /// **The per-statement proof.** `pg_settings.setting` for `statement_timeout` is the RAW stored
    /// millisecond count with no unit suffix - unlike `current_setting()`, which renders it in
    /// whichever unit is prettiest and would make this assertion depend on that formatting. A
    /// certified `execute` under a budget well inside the default 15 s connect-time ceiling must set
    /// the SMALLER, per-request value - not the ceiling this connection was opened with.
    #[test]
    fn a_certified_call_sets_the_per_statement_value_not_the_ceiling() {
        let Some((warehouse, schema)) = open("showtimeout") else {
            return;
        };
        create_view(
            &schema,
            "CREATE VIEW timeout_probe AS SELECT DATE '2005-06-15' AS day, \
             (SELECT setting::bigint FROM pg_settings WHERE name = 'statement_timeout') AS timeout_ms",
        );
        let table = TableName::parse("timeout_probe").expect("a view name is a table name");
        let metric = MetricName::parse("timeout_probe").expect("a test metric name is a name");
        let (range, bindings) = covers(Date::new(2005, 6, 15).expect("the probe's own fixed date"), &table);
        let plan = QueryPlan::new(
            corpus::source(),
            metric.clone(),
            StatementTables::only(table.clone()),
            PlanBucket::new(ResultLabel::bucket(), Grain::Day, column(&table, "day")),
            Vec::new(),
            PlanMeasure::Simple {
                term: PlanTerm::Aggregate {
                    aggregate: Aggregate::Max,
                    column: column(&table, "timeout_ms"),
                },
            },
            ResultLabel::measure(&metric),
            bindings,
            range,
        );
        // Comfortably inside the default 15s ceiling (`SUTURA_DEV_STATEMENT_TIMEOUT_MS` unset by
        // this test), and not a round number a Postgres display format could coincide with.
        let deadline = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_millis(4321)).expect("4321ms is a budget"),
        );

        let rows = warehouse
            .execute(Executable::Query(&plan), &corpus::presented(), deadline)
            .expect("well inside every timeout, this call must answer");
        // `[bucket, measure]` per row - no keys on this plan - so the `timeout_ms` aggregate is the
        // SECOND cell, not the first: that one is the `day` bucket every case in this corpus
        // projects ahead of its measure (`mean_by_day`'s own expected rows show the same order).
        let rows = rows.to_rows().expect("the probe's own two columns decode");
        let seen_ms = match rows.rows().first().and_then(|row| row.get(1)) {
            Some(Value::Integer(ms)) => *ms,
            other => panic!("expected exactly one integer cell at index 1, got {other:?}"),
        };
        assert!(
            seen_ms < 15_000,
            "the per-statement value must be smaller than the connect-time ceiling: saw {seen_ms}ms"
        );
        // Never above 4321: `SET LOCAL` cannot see a LARGER budget than the deadline was opened
        // with, which is exact arithmetic and not a margin - true at any load, so it needs no
        // venue to pick a number for it.
        assert!(
            seen_ms <= 4321,
            "the per-statement value must not exceed the budget it was opened with: saw {seen_ms}ms"
        );
        // How far BELOW 4321 is the actual margin, and it IS a margin: the round trip between
        // opening the deadline and the statement reaching the server takes real time, and a
        // 200ms allowance for it reddened under load (`telekom/sutura#140`'s comment thread: `saw
        // 4051ms` against a 4121ms floor). CI keeps the original 200ms; a shared machine gets
        // 1000ms - see `sutura_dev::tolerance`. The exact-value claim this slack used to be the
        // only proof of is held by `tests::deadline_statement_timeout` in `src/tests.rs`, hermetic
        // and load-independent, so this window is checking the WIRING reaches the real server
        // close to the budget, not re-proving the arithmetic.
        let slack_ms = Tolerance::from_env().ceiling(Duration::from_millis(200), Duration::from_millis(1000));
        let slack_ms = i64::try_from(slack_ms.as_millis()).expect("a millisecond slack of a few seconds fits an i64");
        assert!(
            seen_ms >= 4321 - slack_ms,
            "expected close to the 4321ms budget within {slack_ms}ms, saw {seen_ms}ms"
        );
    }

    /// **The `Prepare` arm.** `SET LOCAL statement_timeout` is sent before the `PREPARE`
    /// `dry_run` makes, so it must bound that round trip too, not only `execute`'s. A second
    /// connection holds `conformance_events` locked `ACCESS EXCLUSIVE` in an open transaction, so
    /// resolving the table's shape during `Parse` blocks on that lock rather than answering -
    /// `telekom/sutura#687`'s review, finding 3: before this cell, no test drove the `Prepare` arm
    /// of `deadline_exceeded` in either direction, and `SET LOCAL` could have been dropped from
    /// `dry_run` with nothing reddening.
    #[test]
    fn dry_run_blocked_on_a_table_lock_is_stopped_at_its_deadline() {
        let Some((warehouse, schema)) = open("lockedprepare") else {
            return;
        };
        warehouse
            .load_fixture_csv(&corpus::table(), &corpus::on_disk())
            .expect("the corpus loads");
        let table = corpus::table();
        let Some(lock_holder) = hold_exclusive_lock("lockedprepare", &schema, &table) else {
            return;
        };

        let metric = MetricName::parse("locked_probe").expect("a test metric name is a name");
        let (range, bindings) = range_over(
            Date::new(2000, 1, 1).expect("year 2000 is in range"),
            Date::new(2099, 12, 31).expect("year 2099 is in range"),
            &table,
        );
        let plan = QueryPlan::new(
            corpus::source(),
            metric.clone(),
            StatementTables::only(table.clone()),
            PlanBucket::new(ResultLabel::bucket(), Grain::Day, column(&table, "day")),
            Vec::new(),
            PlanMeasure::Simple {
                term: PlanTerm::Aggregate {
                    aggregate: Aggregate::Sum,
                    column: column(&table, "amount_cents"),
                },
            },
            ResultLabel::measure(&metric),
            bindings,
            range,
        );
        let deadline = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_millis(300)).expect("300ms is a budget"),
        );

        let started = Instant::now();
        let outcome = warehouse.dry_run(Executable::Query(&plan), &corpus::presented(), deadline);
        let elapsed = started.elapsed();
        drop(lock_holder);

        let error = outcome.expect_err("a PREPARE blocked past its budget must not silently accept");
        assert!(
            warehouse.deadline_exceeded(&error),
            "a PREPARE stopped by SET LOCAL statement_timeout must classify as deadline_exceeded: {error}"
        );
        // 3s stays far short of the ~15s a disabled per-statement narrowing measures here (this
        // file's own mutation table), so either number still tells a stopped `PREPARE` apart from
        // one left blocked on the lock.
        let ceiling = Tolerance::from_env().ceiling(Duration::from_secs(1), Duration::from_secs(3));
        assert!(
            elapsed < ceiling,
            "stopped at ~300ms plus tolerance, not left blocked on the lock: {elapsed:?}"
        );
    }
}

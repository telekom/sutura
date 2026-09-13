//! `docs/adr/0029`'s Postgres row, measured against a real server rather than asserted from the
//! driver's documentation - `SET LOCAL statement_timeout` actually stops a running statement AND a
//! blocked `PREPARE`, the certified path's per-request value is not the connect-time ceiling, and
//! the raw path (which carries no per-request `Deadline` at all - `telekom/sutura#129`'s limit,
//! stated in `raw.rs`) is still stopped by the connect-time ceiling that pre-dates this record.
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
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, TableName};
    use sutura_domain::plan::{
        Executable, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin,
        QueryPlan, ResultLabel, StatementTables,
    };
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_domain::warehouse::{ParamValue, Value, Warehouse as _};
    use sutura_exec_postgres::PostgresWarehouse;
    use sutura_exec_postgres::fixture::FixtureCredential;

    const SERVICE: &str = "postgres";

    /// A fresh schema per test - the isolation `tests/conformance.rs` and `tests/raw.rs` use, so
    /// cells running in parallel against one server never see one another's views or tables.
    fn open(case: &str) -> Option<(PostgresWarehouse, String)> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("deadline::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let config = PostgresWarehouse::local_config(endpoint.host(), endpoint.port(), &credential);
        let schema = format!("deadline_{case}_{}", std::process::id());
        let warehouse = PostgresWarehouse::connect_in_schema(corpus::source(), corpus::posture(), &config, &schema)
            .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}"));
        Some((warehouse, schema))
    }

    /// A second, PLAIN connection into the SAME schema `open` puts its warehouse in - used to issue
    /// the `CREATE VIEW` neither port method here can: `execute` only ever renders a `SELECT`, and
    /// `execute_raw` wraps every call in a transaction this adapter always rolls back.
    fn create_view(schema: &str, sql: &str) {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(_) => return,
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let config = PostgresWarehouse::local_config(endpoint.host(), endpoint.port(), &credential);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime builds");
        let (client, connection) = runtime
            .block_on(config.connect(tokio_postgres::NoTls))
            .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}"));
        runtime.spawn(async move {
            drop(connection.await);
        });
        runtime
            .block_on(client.batch_execute(&format!("SET search_path TO \"{schema}\"; {sql}")))
            .unwrap_or_else(|e| panic!("the admin connection could not create the view: {e}"));
    }

    fn statement(sql: &str) -> RawStatement {
        RawStatement::parse(sql).expect("a test statement is a statement")
    }

    /// A second, PLAIN connection holding `table` locked `ACCESS EXCLUSIVE` in an open, uncommitted
    /// transaction - for as long as this value lives. Dropping it closes the connection, which
    /// terminates the backend and releases the lock; there is no explicit `ROLLBACK` to run.
    struct LockHolder {
        _runtime: tokio::runtime::Runtime,
        _client: tokio_postgres::Client,
    }

    /// `None` when the tier is absent (same skip as `open`) - the caller must skip the test too, it
    /// cannot proceed without something to block on.
    fn hold_exclusive_lock(case: &str, schema: &str, table: &TableName) -> Option<LockHolder> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("deadline::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let config = PostgresWarehouse::local_config(endpoint.host(), endpoint.port(), &credential);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime builds");
        let (client, connection) = runtime
            .block_on(config.connect(tokio_postgres::NoTls))
            .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}"));
        runtime.spawn(async move {
            drop(connection.await);
        });
        runtime
            .block_on(client.batch_execute(&format!(
                "SET search_path TO \"{schema}\"; BEGIN; LOCK TABLE \"{table}\" IN ACCESS EXCLUSIVE MODE"
            )))
            .unwrap_or_else(|e| panic!("the admin connection could not lock {table}: {e}"));
        Some(LockHolder {
            _runtime: runtime,
            _client: client,
        })
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
        assert!(
            elapsed < Duration::from_secs(1),
            "stopped at ~300ms plus tolerance, not run to completion (2s) or to the 15s ceiling: {elapsed:?}"
        );
    }

    /// **The raw path variant.** `execute_raw` carries no per-request `Deadline` - `raw.rs`'s own
    /// doc names the limit - so this proves the connect-time ceiling alone still stops an unbounded
    /// caller statement, which is what discharges `telekom/sutura#129`'s cancellation prerequisite
    /// for the raw tool: the source it runs over can cancel, even though what it cancels WITH here is
    /// the ceiling and not the asker's own budget.
    ///
    /// **Slow on purpose, rather than narrowing the ceiling.** `std::env::set_var` is `unsafe` in
    /// this edition and the workspace forbids `unsafe` outright (`crates/sutura-cli/tests/declared_source.rs`'s
    /// own header states the same rule) - what a test can decide is what a CHILD process sees, and
    /// spawning one just to narrow this single value is not worth a second binary. So this cell
    /// leaves the default 15 s ceiling in place and sleeps past it instead of narrowing it.
    #[test]
    fn a_raw_statement_is_stopped_by_the_connect_time_ceiling() {
        let Some((warehouse, _schema)) = open("rawceiling") else {
            return;
        };

        let started = Instant::now();
        let outcome = warehouse.execute_raw(&statement("select pg_sleep(20)"), &corpus::presented());
        let elapsed = started.elapsed();

        let error = outcome
            .expect("this adapter accepts a raw statement")
            .expect_err("a statement over the ceiling must not answer with rows");
        assert!(
            warehouse.deadline_exceeded(&error),
            "the ceiling firing must classify as deadline_exceeded too: {error}"
        );
        assert!(
            elapsed < Duration::from_secs(18),
            "stopped at the ~15s default ceiling plus tolerance, not run to the statement's own 20s: {elapsed:?}"
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
        let seen_ms = match rows.rows().first().and_then(|row| row.get(1)) {
            Some(Value::Integer(ms)) => *ms,
            other => panic!("expected exactly one integer cell at index 1, got {other:?}"),
        };
        assert!(
            seen_ms < 15_000,
            "the per-statement value must be smaller than the connect-time ceiling: saw {seen_ms}ms"
        );
        // Some slack for the time between opening the deadline and this statement reaching the
        // server - never more, since `SET LOCAL` cannot see a LARGER budget than was opened with.
        assert!(
            (4321 - 200..=4321).contains(&seen_ms),
            "expected close to the 4321ms budget, saw {seen_ms}ms"
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
        assert!(
            elapsed < Duration::from_secs(1),
            "stopped at ~300ms plus tolerance, not left blocked on the lock: {elapsed:?}"
        );
    }
}

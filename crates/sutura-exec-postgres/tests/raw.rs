//! The raw SQL tool's execution path, against a real Postgres - `docs/adr/0013`'s amendment,
//! measured rather than asserted from the driver's documentation.
//!
//! Same tier, same absence handling as `tests/conformance.rs`: every venue that runs the suite
//! provisions the tier, so these cells RUN; a developer machine with none writes `NOT RUN` and the
//! reason to stderr. See that file's header for the whole argument - it is not restated here.
//!
//! **What these cells hold that no unit test can:** the SERVER's own refusal. `PostgresWarehouse`
//! wraps every raw call in `BEGIN READ ONLY` / `ROLLBACK` and reaches the statement through the
//! extended query protocol; both properties are about what Postgres does with bytes on the wire, not
//! about anything this adapter's own Rust can assert on itself.

#[cfg(test)]
mod raw {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_exec_postgres::fixture::FixtureCredential;
    use sutura_exec_postgres::{PostgresError, PostgresWarehouse};

    const SERVICE: &str = "postgres";

    /// A fresh schema per test, the same isolation `tests/conformance.rs` uses - so cells running in
    /// parallel against one server do not read or write one another's tables.
    fn open(case: &str) -> Option<PostgresWarehouse> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("raw::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let config = PostgresWarehouse::local_config(endpoint.host(), endpoint.port(), &credential);
        let schema = format!("raw_{case}_{}", std::process::id());
        Some(
            PostgresWarehouse::connect_in_schema(corpus::source(), corpus::posture(), &config, &schema)
                .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}")),
        )
    }

    /// A second, PLAIN connection into the SAME schema `open(case)` puts its warehouse in - used
    /// only to seed and read back a fixture table `execute_raw` itself could never persist to:
    /// every raw call is wrapped in `BEGIN READ ONLY` / `ROLLBACK` by design, so nothing run
    /// through it can be the thing that proves what did or did not persist.
    struct AdminConnection {
        runtime: tokio::runtime::Runtime,
        client: tokio_postgres::Client,
        schema: String,
    }

    fn admin(case: &str) -> Option<AdminConnection> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(_) => return None,
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let config = PostgresWarehouse::local_config(endpoint.host(), endpoint.port(), &credential);
        // Deliberately the same expression `open` uses: the two connections must land in the one
        // schema for the admin connection to see what the warehouse under test wrote.
        let schema = format!("raw_{case}_{}", std::process::id());
        let runtime = tokio::runtime::Runtime::new().expect("a runtime builds");
        let (client, connection) = runtime
            .block_on(config.connect(tokio_postgres::NoTls))
            .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}"));
        runtime.spawn(async move {
            drop(connection.await);
        });
        Some(AdminConnection { runtime, client, schema })
    }

    fn statement(sql: &str) -> RawStatement {
        RawStatement::parse(sql).expect("a test statement is a statement")
    }

    /// The property `docs/adr/0013`'s amendment states as the transaction boundary's whole job: a
    /// write is refused by the SERVER, `25006 read_only_sql_transaction`, independent of what the
    /// connecting role's grants would otherwise allow - measured, not asserted from documentation.
    /// `CREATE TABLE` rather than an `INSERT` into a fixture table, so this cell needs no corpus
    /// schema: a `READ ONLY` transaction refuses DDL exactly as it refuses a row write.
    #[test]
    fn a_write_inside_the_call_is_refused_by_the_server_as_read_only() {
        let Some(warehouse) = open("write") else { return };
        let outcome = warehouse.execute_raw(&statement("create table raw_write_attempt (x int)"), &corpus::presented());
        let error = outcome
            .expect("this adapter accepts a raw statement")
            .expect_err("a write inside a read-only transaction does not succeed");
        assert!(
            warehouse.source_refused(&error),
            "a read-only violation must classify as `source_refused`, so the raw path reports \
             `RawRefusalReason::SourceRefused` rather than a generic failure: {error}"
        );
        let (PostgresError::Prepare { ref cause } | PostgresError::Execute { ref cause }) = error else {
            panic!("expected a prepare or execute failure, got {error}");
        };
        assert_eq!(
            cause.code(),
            Some(&tokio_postgres::error::SqlState::READ_ONLY_SQL_TRANSACTION),
            "expected 25006 read_only_sql_transaction, got {error}"
        );
    }

    /// The property the extended protocol gives for free: Postgres refuses more than one command in
    /// one `Parse` message, so `SELECT 1; SELECT 2` is refused before either half runs - never a
    /// generic-driver-error guess, the actual server behaviour this adapter depends on.
    #[test]
    fn a_multi_statement_string_is_refused_before_either_half_runs() {
        let Some(warehouse) = open("multi") else { return };
        let outcome = warehouse.execute_raw(&statement("select 1; select 2"), &corpus::presented());
        let error = outcome
            .expect("this adapter accepts a raw statement")
            .expect_err("a multi-statement string is refused");
        assert!(
            matches!(error, PostgresError::Prepare { .. }),
            "a multi-statement string is refused while PREPARING, not while running: {error}"
        );
        // Not a read-only violation: this is a syntax-shaped refusal, and the two must stay
        // distinguishable so a caller is told the right thing.
        assert!(!warehouse.source_refused(&error), "{error}");
    }

    /// The ordinary path still works: one statement, over the extended protocol, inside the
    /// transaction this adapter wraps every call in - and the wrapping is invisible to a statement
    /// that never tried to write.
    #[test]
    fn an_ordinary_select_runs_and_returns_its_rows() {
        let Some(warehouse) = open("select") else { return };
        let rows = warehouse
            .execute_raw(&statement("select 1 as n, 'hi' as label"), &corpus::presented())
            .expect("this adapter accepts a raw statement")
            .expect("an ordinary select succeeds");
        assert_eq!(rows.columns(), ["n", "label"]);
        assert_eq!(rows.rows().len(), 1);
    }

    /// And the wrapper's own rollback is not a special case a second call has to work around: the
    /// same connection answers a second raw call normally, which is what "always rolled back, never
    /// left half-open" has to mean in practice.
    #[test]
    fn the_same_connection_answers_a_second_raw_call_after_a_refused_one() {
        let Some(warehouse) = open("sequence") else { return };
        let refused = warehouse.execute_raw(&statement("create table raw_sequence_attempt (x int)"), &corpus::presented());
        drop(refused.expect("accepts raw").unwrap_err());
        let rows = warehouse
            .execute_raw(&statement("select 2 as n"), &corpus::presented())
            .expect("this adapter accepts a raw statement")
            .expect("the connection still answers after a refused call");
        assert_eq!(rows.rows().len(), 1);
    }

    /// **The review's row-0 falsifier, landed as the regression it demands.** Two callers sharing
    /// ONE `PostgresWarehouse` used to interleave their `BEGIN` / statement / `ROLLBACK` triples on
    /// the wire - `Client` pipelines, so nothing about a multi-thread `Runtime` put one caller's
    /// bytes on the wire before another's started. The measured consequence: a concurrent write's
    /// refusal aborted the transaction another caller was inside, so that caller's own well-formed
    /// `SELECT` failed with `25P02`, and the write itself ran OUTSIDE any transaction once the
    /// other caller's `ROLLBACK` had already closed it - and PERSISTED, contradicting "nothing the
    /// call did persists". `PostgresWarehouse::execution_lock` is the fix: single-flighting every
    /// exchange on the shared client by construction rather than by scheduling luck.
    ///
    /// **Timing-dependent, honestly.** This cell has been green every time it has run while
    /// landing this fix and its review - `execution_lock` makes the two loops fully serialize
    /// rather than merely narrowing the window the review measured, so nothing here is expected to
    /// flake. But no number of green runs is a proof of absence for a race: each is evidence that
    /// run's scheduler did not find a gap the lock was supposed to close, and the lock is what
    /// closes it, not the count of green runs. (`#666` round 2, row 2a-c: 3/3 RED across
    /// independent runs with `run_raw`'s own guard removed - `select_failures` 168, 167, 131 of
    /// 400 - which is the evidence this is a regression guard rather than a coincidence.)
    #[test]
    fn concurrent_raw_calls_on_one_connection_never_let_a_write_escape_the_read_only_transaction() {
        let Some(warehouse) = open("concurrency") else { return };
        let Some(AdminConnection {
            runtime: admin_runtime,
            client: admin_client,
            schema,
        }) = admin("concurrency")
        else {
            return;
        };
        admin_runtime
            .block_on(admin_client.batch_execute(&format!(
                "create table \"{schema}\".race_t (x int); insert into \"{schema}\".race_t values (1)"
            )))
            .expect("the admin connection seeds the fixture table");

        let mut select_failures = 0usize;
        let mut inserts_accepted = 0usize;
        std::thread::scope(|scope| {
            let selects = scope.spawn(|| {
                let mut failures = 0usize;
                for _ in 0..400 {
                    match warehouse.execute_raw(&statement("select 1"), &corpus::presented()) {
                        Some(Ok(_)) => {}
                        _ => failures += 1,
                    }
                }
                failures
            });
            let inserts = scope.spawn(|| {
                let mut accepted = 0usize;
                for _ in 0..400 {
                    if matches!(
                        warehouse.execute_raw(&statement("insert into race_t values (2)"), &corpus::presented()),
                        Some(Ok(_))
                    ) {
                        accepted += 1;
                    }
                }
                accepted
            });
            select_failures = selects.join().expect("the select thread does not panic");
            inserts_accepted = inserts.join().expect("the insert thread does not panic");
        });

        let persisted_count: i64 = admin_runtime
            .block_on(admin_client.query_one(&format!("select count(*) from \"{schema}\".race_t"), &[]))
            .expect("the admin connection reads the fixture table back")
            .get(0);

        // One `assert!` naming all three counts, `persisted_count` first: it is the security
        // property (did a write escape the read-only transaction), and `select_failures` is the
        // collateral (did a concurrent caller's transaction get aborted from under it) - a red run
        // must report both rather than stopping at whichever assertion came first.
        assert!(
            persisted_count == 1 && select_failures == 0 && inserts_accepted == 0,
            "persisted_count={persisted_count} (want 1 - a write must never escape the read-only \
             transaction) select_failures={select_failures} (want 0 - a concurrent write's refusal \
             must never abort another caller's transaction, 25P02) inserts_accepted={inserts_accepted} \
             (want 0 - a write inside the read-only transaction must never be accepted)"
        );
    }

    /// **`#666` round 2, BLOCKING 1's certified half.** The landed cell above races raw-vs-raw;
    /// `execution_lock` is taken in `run` too (the certified path), and nothing proved that half
    /// before this cell - round 2's own falsifier measured `certified_failures=194` of 400 with
    /// `run`'s guard removed, all `25P02` inside a raw caller's aborted transaction. A concurrent
    /// certified `execute` and a raw insert on ONE adapter must never interleave their exchanges
    /// on the wire, the same property the raw-vs-raw cell holds for the other pairing.
    #[test]
    fn certified_execute_racing_a_raw_insert_never_fails_with_25p02() {
        use sutura_domain::plan::Executable;
        let Some(warehouse) = open("certrace") else { return };
        let Some(AdminConnection {
            runtime: admin_runtime,
            client: admin_client,
            schema,
        }) = admin("certrace")
        else {
            return;
        };
        warehouse
            .load_fixture_csv(&corpus::table(), &corpus::on_disk())
            .expect("the corpus loads");
        admin_runtime
            .block_on(admin_client.batch_execute(&format!(
                "create table \"{schema}\".race_t (x int); insert into \"{schema}\".race_t values (1)"
            )))
            .expect("the admin connection seeds the fixture table");
        let cases = corpus::cases();
        let case = cases.first().expect("the corpus has a case");

        let mut certified_failures: Vec<String> = Vec::new();
        let mut inserts_accepted = 0usize;
        std::thread::scope(|scope| {
            let certified = scope.spawn(|| {
                let mut failures = Vec::new();
                for _ in 0..400 {
                    if let Err(cause) =
                        warehouse.execute(Executable::Query(case.plan()), &corpus::presented(), corpus::deadline())
                    {
                        failures.push(format!("{cause:?}"));
                    }
                }
                failures
            });
            let inserts = scope.spawn(|| {
                let mut accepted = 0usize;
                for _ in 0..400 {
                    if matches!(
                        warehouse.execute_raw(&statement("insert into race_t values (2)"), &corpus::presented()),
                        Some(Ok(_))
                    ) {
                        accepted += 1;
                    }
                }
                accepted
            });
            certified_failures = certified.join().expect("the certified thread does not panic");
            inserts_accepted = inserts.join().expect("the insert thread does not panic");
        });

        let persisted_count: i64 = admin_runtime
            .block_on(admin_client.query_one(&format!("select count(*) from \"{schema}\".race_t"), &[]))
            .expect("the admin connection reads the fixture table back")
            .get(0);

        assert!(
            persisted_count == 1 && inserts_accepted == 0 && certified_failures.is_empty(),
            "persisted_count={persisted_count} (want 1) inserts_accepted={inserts_accepted} (want 0) \
             certified_failures={} (want 0), first={:?}",
            certified_failures.len(),
            certified_failures.first()
        );
    }

    /// **`#666` round 2, finding 2.** `run_raw_statement` streams via `Client::query_raw` and stops
    /// COLLECTING one row past `MAX_ROWS` - proved here by a statement that would return far more,
    /// asserting the adapter handed back exactly `MAX_ROWS + 1` rows rather than every row the
    /// statement could produce.
    ///
    /// **What this does NOT prove**, stated because the doc comment above `run_raw_statement` used
    /// to overstate it: the stop bounds this process's own heap, not the wire. The server still
    /// computes and ships the whole result, and the driver's connection task drains every `DataRow`
    /// off the socket - only after that does `ROLLBACK` get to run - so a statement's wall time and
    /// the server's own work stay proportional to what the STATEMENT would return, not to what the
    /// cap kept. `docs/adr/0013`'s `statement_timeout` is what bounds that, not this stop.
    #[test]
    fn a_statement_over_the_row_cap_returns_exactly_the_capped_count() {
        let Some(warehouse) = open("rowcap") else { return };
        let over_cap = sutura_domain::plan::MAX_ROWS + 2;
        let rows = warehouse
            .execute_raw(
                &statement(&format!("select generate_series(1, {over_cap})")),
                &corpus::presented(),
            )
            .expect("this adapter accepts a raw statement")
            .expect("a statement over the cap is not itself a server-side error");
        assert_eq!(
            rows.rows().len(),
            usize::try_from(sutura_domain::plan::MAX_ROWS + 1).expect("the cap plus one fits a usize"),
            "the adapter must stop one row past the cap, neither at it nor at the statement's own count"
        );
    }
}

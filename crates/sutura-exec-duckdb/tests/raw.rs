#![forbid(unsafe_code)]
//! The raw statement path (`docs/adr/0013`) against the pinned driver, over a database file opened
//! the way `kind: duckdb` opens one - [`DuckDbWarehouse::open`], its `READ_ONLY` options and its
//! `THEN_LOCKED` statements.
//!
//! **Every statement here runs on a connection opened after `open` returned**: `execute_raw` opens
//! its own per call, so a refusal holds that `THEN_LOCKED`'s settings are the database's, not those
//! of the session that ran them.
//!
//! **Each refusal asserts the EFFECT is absent, not only that an error came back**: the table still
//! holds its rows, the file was not written, the setting did not move. An error alone would also be
//! what a statement that half ran and then failed returns.

#[cfg(test)]
mod raw {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use sutura_conformance::corpus;
    use sutura_domain::identity::{Presented, PrincipalName};
    use sutura_domain::model::TableName;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_domain::warehouse::{RawRows, ResultBudget, Value, Warehouse as _};
    use sutura_exec_duckdb::{DuckDbError, DuckDbWarehouse, write_database};

    /// A directory of this test's own, holding a database with one table `t` of two rows and a CSV
    /// beside it that is NOT in the database.
    struct Scratch {
        dir: PathBuf,
    }

    impl Scratch {
        fn new(case: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("sutura-duckdb-raw-{}-{case}", std::process::id()));
            drop(std::fs::remove_dir_all(&dir));
            std::fs::create_dir_all(&dir).expect("the scratch directory is created");
            let csv = dir.join("t.csv");
            std::fs::write(&csv, "id,amount\n1,10\n2,20\n").expect("the table's CSV is written");
            std::fs::write(dir.join("side.csv"), "secret\nside-file\n").expect("the side file is written");
            write_database(
                &dir.join("db.duckdb"),
                &[(TableName::parse("t").expect("t is a table name"), csv)],
            )
            .expect("the fixture database is written");
            Self { dir }
        }

        fn at(&self, file: &str) -> PathBuf {
            self.dir.join(file)
        }

        fn open(&self) -> DuckDbWarehouse {
            self.open_with(budget(1 << 20))
        }

        fn open_with(&self, budget: ResultBudget) -> DuckDbWarehouse {
            DuckDbWarehouse::open(corpus::source(), corpus::posture(), &self.at("db.duckdb"), budget)
                .expect("the fixture database opens read-only")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.dir));
        }
    }

    fn budget(bytes: usize) -> ResultBudget {
        ResultBudget::of_bytes(core::num::NonZeroUsize::new(bytes).expect("a test budget is positive"))
    }

    /// What `sql` answered as `presented`: `None` only on a database `in_memory` opened.
    fn executed(warehouse: &DuckDbWarehouse, sql: &str, presented: &Presented) -> Option<Result<RawRows, DuckDbError>> {
        let statement = RawStatement::parse(sql).expect("a test statement is a statement");
        warehouse.execute_raw(&statement, presented, corpus::deadline())
    }

    fn answer(warehouse: &DuckDbWarehouse, sql: &str) -> RawRows {
        match executed(warehouse, sql, &corpus::presented()) {
            Some(Ok(rows)) => rows,
            other => panic!("`{sql}` must have answered, got {other:?}"),
        }
    }

    fn refused_as(warehouse: &DuckDbWarehouse, sql: &str, presented: &Presented) -> DuckDbError {
        match executed(warehouse, sql, presented) {
            Some(Err(error)) => error,
            other => panic!("`{sql}` must have been refused, got {other:?}"),
        }
    }

    /// The driver's own message, which is what says WHICH barrier refused.
    fn refusal(warehouse: &DuckDbWarehouse, sql: &str) -> String {
        format!("{:?}", refused_as(warehouse, sql, &corpus::presented()))
    }

    /// What `t` holds, read in a fresh connection - so a write a refused statement made would show.
    fn table(warehouse: &DuckDbWarehouse) -> Vec<Vec<Value>> {
        answer(warehouse, "SELECT id, amount FROM t ORDER BY id").into_parts().1
    }

    fn one(warehouse: &DuckDbWarehouse, sql: &str) -> Value {
        let (_, rows) = answer(warehouse, sql).into_parts();
        rows.into_iter()
            .next()
            .and_then(|row| row.into_iter().next())
            .expect("the probe answers one cell")
    }

    /// **The watchdog lets go of a call when its answer arrives**, not when the budget runs out: the
    /// call runs on a thread of its own, and an answer later than a third of its 30s budget fails
    /// this cell's own bound.
    #[test]
    fn a_raw_select_answers_the_declared_database_rows() {
        let scratch = Scratch::new("select");
        let (answered, answers) = mpsc::channel();
        let warehouse = scratch.open();
        std::thread::spawn(move || {
            let rows = answer(&warehouse, "SELECT id, amount FROM t ORDER BY id");
            drop(answered.send((rows, warehouse)));
        });
        let (rows, warehouse) = answers
            .recv_timeout(Duration::from_secs(10))
            .expect("a two-row SELECT is answered well inside its 30s budget");
        let (columns, rows) = rows.into_parts();
        assert_eq!(columns, ["id", "amount"]);
        assert_eq!(rows, table(&warehouse));
        assert_eq!(rows.len(), 2, "{rows:?}");
    }

    /// `access_mode`: a read-only open creates nothing, so a path with no database behind it is
    /// refused rather than becoming an empty database the deployment then serves.
    #[test]
    fn a_read_only_open_of_a_missing_file_is_refused_and_creates_none() {
        let scratch = Scratch::new("missing");
        let missing = scratch.at("absent.duckdb");
        let opened = DuckDbWarehouse::open(corpus::source(), corpus::posture(), &missing, budget(1 << 20));
        assert!(matches!(opened, Err(DuckDbError::Open { .. })), "{opened:?}");
        assert!(!missing.exists(), "a read-only open must not create {}", missing.display());
    }

    /// `access_mode`: no write and no DDL takes effect - asserted on the table, read afterwards.
    /// `ATTACH ':memory:'` is refused by this option alone: without it a writable catalog would
    /// join the database, for every later call.
    #[test]
    fn a_write_or_ddl_is_refused_and_leaves_the_database_unchanged() {
        let scratch = Scratch::new("write");
        let warehouse = scratch.open();
        let before = table(&warehouse);
        for sql in [
            "INSERT INTO t VALUES (3, 30)",
            "UPDATE t SET amount = 0",
            "DELETE FROM t",
            "DROP TABLE t",
            "CREATE TABLE u AS SELECT 1 AS x",
            "CREATE VIEW v AS SELECT 1 AS x",
            "ALTER TABLE t ADD COLUMN extra INTEGER",
            "ATTACH ':memory:' AS m",
        ] {
            let refused = refusal(&warehouse, sql);
            assert!(
                refused.contains("read-only"),
                "`{sql}` was refused for another reason: {refused}"
            );
        }
        assert_eq!(table(&warehouse), before);
        let tables = answer(&warehouse, "SELECT table_name FROM duckdb_tables() ORDER BY table_name")
            .into_parts()
            .1;
        assert_eq!(tables, [[Value::Text(String::from("t"))]], "{tables:?}");
        assert_eq!(
            one(
                &warehouse,
                "SELECT count(*) FROM duckdb_databases() WHERE database_name = 'm'"
            ),
            Value::Integer(0)
        );
    }

    /// Nothing outside the declared database is read or written.
    ///
    /// **Two barriers refuse every statement here, and no cell tells them apart**: on the pinned
    /// driver `disabled_filesystems` refuses each of these first ("File system `LocalFileSystem` has
    /// been disabled by configuration"), so the effect holds with `enable_external_access` dropped.
    /// What ties that option to the open is the setting asserted last, on a connection opened after
    /// `open` returned; the file system it alone would refuse - a network one a driver build links -
    /// the pinned driver does not have.
    #[test]
    fn a_file_outside_the_database_is_neither_read_nor_written() {
        let scratch = Scratch::new("external");
        let warehouse = scratch.open();
        let side = scratch.at("side.csv").display().to_string();
        let attached = scratch.at("other.duckdb");
        let copied = scratch.at("out.csv");
        let exported = scratch.at("exported");
        for sql in [
            format!("SELECT * FROM read_csv('{side}')"),
            format!("SELECT * FROM read_csv_auto('{side}')"),
            format!("SELECT * FROM '{side}'"),
            format!("SELECT * FROM read_text('{side}')"),
            format!("SELECT * FROM read_parquet('{side}')"),
            format!("SELECT * FROM glob('{}')", scratch.at("*").display()),
            format!("ATTACH '{}' AS other", attached.display()),
            format!("COPY t TO '{}'", copied.display()),
            format!("EXPORT DATABASE '{}'", exported.display()),
        ] {
            let refused = refusal(&warehouse, &sql);
            assert!(
                refused.contains("disabled by configuration"),
                "`{sql}` was refused for another reason: {refused}"
            );
        }
        for written in [&attached, &copied, &exported] {
            assert!(!written.exists(), "{} was written", written.display());
        }
        assert_eq!(
            one(&warehouse, "SELECT current_setting('enable_external_access')::VARCHAR"),
            Value::Text(String::from("false"))
        );
    }

    /// No extension is fetched or loaded. On the pinned driver `INSTALL` is refused by the disabled
    /// local file system and `LOAD` by external access - and, with external access dropped, by the
    /// file system instead - so `disabled` matches either barrier's message.
    #[test]
    fn an_extension_is_neither_installed_nor_loaded() {
        let scratch = Scratch::new("extension");
        let warehouse = scratch.open();
        let fake = scratch.at("fake.duckdb_extension");
        std::fs::write(&fake, "not an extension").expect("the fake extension is written");
        for sql in [
            String::from("INSTALL httpfs"),
            String::from("LOAD httpfs"),
            format!("LOAD '{}'", fake.display()),
        ] {
            let refused = refusal(&warehouse, &sql);
            assert!(
                refused.contains("disabled"),
                "`{sql}` was refused for another reason: {refused}"
            );
        }
    }

    /// `lock_configuration`: a setting is instance-wide in `DuckDB`, so one raw statement could
    /// otherwise move it for every later call - the spill directory, the thread count and the
    /// disabled file system included, the last of which would hand the file's bytes back.
    #[test]
    fn a_global_setting_cannot_be_moved_by_a_raw_statement() {
        let scratch = Scratch::new("lock");
        let warehouse = scratch.open();
        let threads = one(&warehouse, "SELECT current_setting('threads')");
        let spill = one(&warehouse, "SELECT current_setting('temp_directory')");
        for sql in [
            String::from("SET threads = 1"),
            String::from("SET memory_limit = '1MB'"),
            format!("SET temp_directory = '{}'", scratch.at("spill").display()),
            String::from("SET enable_external_access = true"),
            String::from("SET disabled_filesystems = ''"),
            String::from("RESET disabled_filesystems"),
            String::from("RESET lock_configuration"),
        ] {
            let refused = refusal(&warehouse, &sql);
            assert!(
                refused.contains("locked"),
                "`{sql}` was refused for another reason: {refused}"
            );
        }
        assert_eq!(one(&warehouse, "SELECT current_setting('threads')"), threads);
        assert_eq!(one(&warehouse, "SELECT current_setting('temp_directory')"), spill);
        let own = scratch.at("db.duckdb").display().to_string();
        let blob = refusal(&warehouse, &format!("SELECT size FROM read_blob('{own}')"));
        assert!(
            blob.contains("disabled by configuration"),
            "the file system came back: {blob}"
        );
    }

    /// **The driver runs every statement but the last while PREPARING**, so the options, not a parse
    /// of the text, are what a statement placed before a harmless `SELECT` runs under.
    #[test]
    fn a_statement_hidden_before_a_select_takes_no_effect() {
        let scratch = Scratch::new("multi");
        let warehouse = scratch.open();
        let before = table(&warehouse);
        let threads = one(&warehouse, "SELECT current_setting('threads')");
        let copied = scratch.at("out.csv");
        let attached = scratch.at("other.duckdb");
        for sql in [
            String::from("INSERT INTO t VALUES (3, 30); SELECT 1"),
            String::from("DROP TABLE t; SELECT 1"),
            format!("COPY t TO '{}'; SELECT 1", copied.display()),
            format!("ATTACH '{}' AS other; SELECT 1", attached.display()),
            String::from("INSTALL httpfs; SELECT 1"),
            String::from("SET threads = 1; SELECT 1"),
        ] {
            drop(refusal(&warehouse, &sql));
        }
        assert_eq!(table(&warehouse), before);
        assert_eq!(one(&warehouse, "SELECT current_setting('threads')"), threads);
        for written in [&copied, &attached] {
            assert!(!written.exists(), "{} was written", written.display());
        }
    }

    /// `disabled_filesystems`: `DuckDB` keeps the declared database's own file readable when external
    /// access is off, so without it a raw statement could read the file's bytes - pages a `SELECT` no
    /// longer shows included. The tables stay readable, through the handle the open already holds.
    #[test]
    fn the_declared_database_file_is_not_readable_as_bytes() {
        let scratch = Scratch::new("own-file");
        let warehouse = scratch.open();
        for file in ["db.duckdb", "db.duckdb.wal"] {
            let own = scratch.at(file).display().to_string();
            for sql in [
                format!("SELECT size FROM read_blob('{own}')"),
                format!("SELECT content FROM read_text('{own}')"),
            ] {
                let refused = refusal(&warehouse, &sql);
                assert!(
                    refused.contains("disabled by configuration"),
                    "`{sql}` was refused for another reason: {refused}"
                );
            }
        }
        assert_eq!(table(&warehouse).len(), 2);
    }

    /// The row cap: the stream is read to one past `MAX_ROWS` and no further, so the caller's own
    /// check refuses rather than receiving a truncated result that looks complete. **Under a budget
    /// the whole result would cross**, so a read that ran past the cap and truncated afterwards is
    /// refused here instead of answering the same rows.
    #[test]
    fn a_raw_result_is_read_to_one_past_the_row_cap() {
        let scratch = Scratch::new("rows");
        let warehouse = scratch.open_with(budget(4 << 20));
        let cap = usize::try_from(sutura_domain::plan::MAX_ROWS).expect("the cap fits a usize");
        let rows = answer(&warehouse, &format!("SELECT range AS n FROM range({})", cap * 10))
            .into_parts()
            .1;
        assert_eq!(rows.len(), cap + 1);
    }

    /// The byte cap: a result over the materialisation budget is refused while it is read, as the
    /// port's `result_did_not_fit` - which the application layer answers `result_too_large`.
    #[test]
    fn a_raw_result_over_the_byte_budget_is_refused_as_did_not_fit() {
        let scratch = Scratch::new("bytes");
        let warehouse = scratch.open_with(budget(64 * 1024));
        let error = refused_as(
            &warehouse,
            "SELECT repeat('x', 1000) AS wide FROM range(5000)",
            &corpus::presented(),
        );
        assert!(warehouse.result_did_not_fit(&error), "{error:?}");
    }

    /// A budget already spent is refused by the adapter itself, on a statement that would answer at
    /// once - so the refusal is the check before the statement starts, not a stop while it runs.
    #[test]
    fn a_raw_statement_whose_budget_is_already_spent_is_refused_by_name() {
        let scratch = Scratch::new("spent");
        let warehouse = scratch.open();
        let statement = RawStatement::parse("SELECT 1").expect("a test statement is a statement");
        let spent = Deadline::opened_at(
            Instant::now()
                .checked_sub(Duration::from_secs(10))
                .expect("ten seconds before now does not underflow the monotonic clock"),
            Budget::parse(Duration::from_secs(1)).expect("a second is a budget"),
        );
        let executed = warehouse.execute_raw(&statement, &corpus::presented(), spent);
        assert!(
            matches!(&executed, Some(Err(error)) if warehouse.deadline_exceeded(error)),
            "a spent budget must be refused as deadline_exceeded, got {executed:?}"
        );
    }

    /// The deadline: the watchdog `execute` runs under cancels the connection on the raw path too,
    /// and the port reads the interrupt as `deadline_exceeded`. **Armed before the driver prepares**,
    /// so a long statement placed before a `SELECT` - which the driver runs while preparing - is
    /// stopped as well. The calls run on a thread of their own, so a statement nothing stops fails
    /// this cell's own bound rather than waiting for the runner's.
    #[test]
    fn a_raw_statement_still_running_at_its_deadline_is_stopped_and_refused_by_name() {
        let scratch = Scratch::new("deadline");
        let warehouse = scratch.open();
        let long = "SELECT count(*) FROM range(100000) a, range(100000) b, range(1000) c \
                    WHERE (a.range * b.range + c.range) % 7 = 3";
        let inputs = [String::from(long), format!("{long}; SELECT 1")];
        let (answered, answers) = mpsc::channel();
        let calls = inputs.clone();
        std::thread::spawn(move || {
            for sql in calls {
                let statement = RawStatement::parse(&sql).expect("a test statement is a statement");
                let second = Budget::parse(Duration::from_secs(1)).expect("a second is a budget");
                let executed =
                    warehouse.execute_raw(&statement, &corpus::presented(), Deadline::opened_at(Instant::now(), second));
                let by_name = matches!(&executed, Some(Err(error)) if warehouse.deadline_exceeded(error));
                drop(answered.send((by_name, format!("{executed:?}"))));
            }
        });
        for sql in inputs {
            let (by_name, executed) = answers
                .recv_timeout(Duration::from_secs(30))
                .unwrap_or_else(|_| panic!("`{sql}` was not stopped within 30s of a one-second deadline"));
            assert!(by_name, "`{sql}` must have been refused as deadline_exceeded, got {executed}");
        }
    }

    /// A subject's own credential has nowhere to go on this adapter, on the raw path as on the
    /// certified one - refused before the statement is prepared.
    #[test]
    fn a_subject_credential_is_refused_on_the_raw_path() {
        let scratch = Scratch::new("subject");
        let warehouse = scratch.open();
        let presented = Presented::SubjectPrincipal {
            name: PrincipalName::parse("analyst_role").expect("a test name is a name"),
        };
        let refused = refused_as(&warehouse, "SELECT 1", &presented);
        assert!(matches!(refused, DuckDbError::NoPlaceForASubject { .. }), "{refused:?}");
    }

    /// A database `in_memory` opened is writable, for fixtures, so it accepts no raw statement at all.
    #[test]
    fn an_in_memory_database_accepts_no_raw_statement() {
        let warehouse = DuckDbWarehouse::in_memory(corpus::source(), corpus::posture(), budget(1 << 20))
            .expect("an in-memory database opens");
        assert!(executed(&warehouse, "SELECT 1", &corpus::presented()).is_none());
    }
}

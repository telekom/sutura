#![forbid(unsafe_code)]
//! The raw statement path (`docs/adr/0013`) against the pinned driver, over a database file opened
//! the way `kind: duckdb` opens one - [`DuckDbWarehouse::open`], its `READ_ONLY` options and its
//! `THEN_LOCKED` statements.
//!
//! **Every statement here runs on a connection opened after `open` returned**: `execute_raw` opens
//! its own per call, so a refusal holds that `THEN_LOCKED`'s settings are the database's, not those
//! of the session that ran them.
//!
//! **Two layers, each held on its own.** `execute_raw` screens a text before any of it runs
//! (`RAW_TABLE_FUNCTIONS`); the screen's cells go through it. The barrier cells go through the
//! fixtures-only `execute_unscreened`, so each of `READ_ONLY` and `THEN_LOCKED` is red without it
//! rather than answered by the screen first.
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
    use sutura_exec_duckdb::{DuckDbError, DuckDbWarehouse, MAX_NESTING, MAX_QUERIES, NotARead, write_database};

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

    /// What `sql` answered with the screen left out, so a cell reads a barrier's own refusal.
    fn unscreened(warehouse: &DuckDbWarehouse, sql: &str) -> Result<RawRows, DuckDbError> {
        warehouse.execute_unscreened(&statement(sql), corpus::deadline())
    }

    fn statement(sql: &str) -> RawStatement {
        RawStatement::parse(sql).expect("a test statement is a statement")
    }

    /// The driver's own message, which is what says WHICH barrier refused.
    fn refusal(warehouse: &DuckDbWarehouse, sql: &str) -> String {
        match unscreened(warehouse, sql) {
            Err(error) => format!("{error:?}"),
            Ok(rows) => panic!("`{sql}` must have been refused, got {rows:?}"),
        }
    }

    /// What `sql` answers behind the screen, on a connection of its own.
    fn read(warehouse: &DuckDbWarehouse, sql: &str) -> RawRows {
        match unscreened(warehouse, sql) {
            Ok(rows) => rows,
            Err(error) => panic!("`{sql}` must have answered, got {error:?}"),
        }
    }

    /// What the screen refused `sql` as.
    fn screened_out(warehouse: &DuckDbWarehouse, sql: &str) -> NotARead {
        match refused_as(warehouse, sql, &corpus::presented()) {
            DuckDbError::NotARead { cause } => cause,
            other => panic!("`{sql}` must have been refused by the screen, got {other:?}"),
        }
    }

    /// An instance-wide setting a table function can move, read on a connection of its own.
    fn logging(warehouse: &DuckDbWarehouse) -> Value {
        one(warehouse, "SELECT current_setting('enable_logging')::VARCHAR")
    }

    /// What `t` holds, read in a fresh connection - so a write a refused statement made would show.
    fn table(warehouse: &DuckDbWarehouse) -> Vec<Vec<Value>> {
        read(warehouse, "SELECT id, amount FROM t ORDER BY id").into_parts().1
    }

    fn one(warehouse: &DuckDbWarehouse, sql: &str) -> Value {
        let (_, rows) = read(warehouse, sql).into_parts();
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
        let tables = read(&warehouse, "SELECT table_name FROM duckdb_tables() ORDER BY table_name")
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

    /// **The driver runs every statement but the last while PREPARING**, so behind the screen the
    /// options are what a statement placed before a harmless `SELECT` runs under.
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

    /// The screen's first class: a statement `DuckDB`'s parser does not read as a `SELECT`, or a
    /// text it cannot parse, is refused before any statement in it runs - one placed before a
    /// `SELECT`, which the driver would run while preparing, included.
    #[test]
    fn a_statement_that_is_not_a_select_is_refused_before_any_of_it_runs() {
        let scratch = Scratch::new("screen-statement");
        let warehouse = scratch.open();
        let before = logging(&warehouse);
        for sql in [
            "CALL enable_logging('QueryLog')",
            "CALL enable_logging('QueryLog'); SELECT 1",
            "SELECT 1; CALL enable_logging('QueryLog')",
            "PRAGMA database_list",
            "SET search_path = 'main'",
            "EXPLAIN SELECT 1",
            "CREATE TEMP TABLE scratch AS SELECT 1 AS x",
            "PIVOT t ON id USING sum(amount)",
            "SELECT 1 FROM",
        ] {
            let refused = screened_out(&warehouse, sql);
            assert!(matches!(refused, NotARead::Statement(_)), "`{sql}`: {refused:?}");
        }
        assert_eq!(logging(&warehouse), before);
    }

    /// The screen's second class: a table function [`sutura_exec_duckdb::RAW_TABLE_FUNCTIONS`] does
    /// not list is refused wherever the text calls it - a subquery, a CTE, a set operation, a join
    /// and a `DESCRIBE` of a query included - and so is one that runs SQL from a string.
    #[test]
    fn a_table_function_not_listed_is_refused_wherever_the_text_calls_it() {
        let scratch = Scratch::new("screen-function");
        let warehouse = scratch.open();
        let before = logging(&warehouse);
        for (sql, function) in [
            ("SELECT * FROM enable_logging('QueryLog')", "enable_logging"),
            ("FROM enable_logging('QueryLog')", "enable_logging"),
            ("SELECT * FROM \"enable_logging\"('QueryLog')", "enable_logging"),
            (
                "WITH x AS (SELECT * FROM enable_logging('QueryLog')) SELECT * FROM x",
                "enable_logging",
            ),
            (
                "SELECT 1 UNION ALL SELECT 1 FROM enable_logging('QueryLog')",
                "enable_logging",
            ),
            ("SELECT (SELECT count(*) FROM enable_logging('QueryLog'))", "enable_logging"),
            (
                "SELECT * FROM t WHERE id IN (SELECT 1 FROM enable_logging('QueryLog'))",
                "enable_logging",
            ),
            ("SELECT * FROM t, LATERAL (FROM enable_logging('QueryLog'))", "enable_logging"),
            ("SELECT * FROM t JOIN enable_logging('QueryLog') ON true", "enable_logging"),
            ("DESCRIBE SELECT * FROM enable_logging('QueryLog')", "enable_logging"),
            ("SELECT * FROM query('SELECT * FROM enable_logging(''QueryLog'')')", "query"),
            ("SELECT * FROM query_table('t')", "query_table"),
        ] {
            let refused = screened_out(&warehouse, sql);
            assert!(
                matches!(refused, NotARead::TableFunction(ref name) if name == function),
                "`{sql}`: {refused:?}"
            );
        }
        assert_eq!(logging(&warehouse), before);
    }

    /// The screen's third class: a listed name is refused when the text qualifies it, so a catalog
    /// or schema of the database's own cannot answer for it.
    #[test]
    fn a_qualified_table_function_is_refused_though_its_name_is_listed() {
        let scratch = Scratch::new("screen-qualified");
        let warehouse = scratch.open();
        for sql in ["SELECT * FROM system.main.range(3)", "SELECT * FROM main.range(3)"] {
            let refused = screened_out(&warehouse, sql);
            assert!(
                matches!(refused, NotARead::TableFunction(ref name) if name == "range"),
                "`{sql}`: {refused:?}"
            );
        }
    }

    /// The screen's fourth class: a tree nested deeper than `serde_json` reads is refused rather
    /// than passed unread, so nesting cannot carry a table function past the walk.
    #[test]
    fn a_text_nested_past_what_the_screen_reads_is_refused() {
        let scratch = Scratch::new("screen-deep");
        let warehouse = scratch.open();
        let before = logging(&warehouse);
        let depth = 70;
        let sql = format!(
            "SELECT {}(SELECT count(*) FROM enable_logging('QueryLog')){}",
            "abs(".repeat(depth),
            ")".repeat(depth)
        );
        let refused = screened_out(&warehouse, &sql);
        assert!(matches!(refused, NotARead::Unreadable { .. }), "{refused:?}");
        assert_eq!(logging(&warehouse), before);
    }

    /// The nesting bound: queries nested [`MAX_NESTING`] deep answer and one level deeper is
    /// refused by name. A reference to a CTE counts as the query it names, however its case is
    /// spelled, so a chain of CTEs nests as deep as the subqueries it stands for.
    #[test]
    fn a_text_nested_past_the_nesting_bound_is_refused_by_name() {
        let scratch = Scratch::new("screen-nesting");
        let warehouse = scratch.open();
        let deepest = usize::try_from(MAX_NESTING).expect("the bound is a count");
        let subqueries = |inner: usize| format!("SELECT {}1{}", "(SELECT ".repeat(inner), ")".repeat(inner));
        let chained = |ctes: usize| {
            let chain: Vec<String> = (1..=ctes)
                .map(|cte| format!("c{cte} AS (SELECT x FROM C{})", cte - 1))
                .collect();
            format!("WITH c0 AS (SELECT 1 AS x), {} SELECT x FROM C{ctes}", chain.join(", "))
        };
        for sql in [subqueries(deepest - 1), chained(deepest - 2)] {
            drop(answer(&warehouse, &sql));
        }
        for sql in [subqueries(deepest), chained(deepest - 1)] {
            let refused = screened_out(&warehouse, &sql);
            assert!(
                matches!(refused, NotARead::TooDeep { depth } if depth == MAX_NESTING + 1),
                "`{sql}`: {refused:?}"
            );
        }
    }

    /// A CTE counts only inside the query that defines it: a later statement reading the table of
    /// the same name reads the table, and is counted as the table, so it still answers at
    /// [`MAX_NESTING`] deep.
    #[test]
    fn a_cte_counts_only_inside_the_query_that_defines_it() {
        let scratch = Scratch::new("screen-scope");
        let warehouse = scratch.open();
        let deepest = usize::try_from(MAX_NESTING).expect("the bound is a count");
        let sql = format!(
            "WITH t AS (SELECT 1 AS id) SELECT id FROM t; SELECT {}count(*) FROM t{}",
            "(SELECT ".repeat(deepest - 1),
            ")".repeat(deepest - 1)
        );
        let (_, rows) = answer(&warehouse, &sql).into_parts();
        assert_eq!(rows, [[Value::Integer(2)]], "`{sql}`");
    }

    /// The query bound: a text of [`MAX_QUERIES`] queries answers and one more is refused by name,
    /// counted across its statements. A CTE counts again at every reference to it, however its case
    /// is spelled, so a few lines that refer to each CTE several times count every query they stand
    /// for.
    #[test]
    fn a_text_holding_more_queries_than_the_bound_is_refused_by_name() {
        let scratch = Scratch::new("screen-queries");
        let warehouse = scratch.open();
        let most = usize::try_from(MAX_QUERIES).expect("the bound is a count");
        let statements = |count: usize| vec!["SELECT 1"; count].join("; ");
        drop(answer(&warehouse, &statements(most)));
        let refused = screened_out(&warehouse, &statements(most + 1));
        assert!(
            matches!(refused, NotARead::TooMany { queries } if queries == MAX_QUERIES + 1),
            "{refused:?}"
        );
        let fanned: Vec<String> = (1..=5)
            .map(|cte| format!("c{cte} AS (SELECT a.x FROM C{p} a, C{p} b, C{p} c, C{p} d)", p = cte - 1))
            .collect();
        let sql = format!("WITH c0 AS (SELECT 1 AS x), {} SELECT count(*) FROM c5", fanned.join(", "));
        let refused = screened_out(&warehouse, &sql);
        assert!(
            matches!(refused, NotARead::TooMany { queries } if queries > MAX_QUERIES),
            "`{sql}`: {refused:?}"
        );
    }

    /// What the screen lets through answers: every listed table function, the read shapes
    /// `DuckDB` parses as a `SELECT`, and a text of several of them.
    #[test]
    fn a_read_the_screen_lists_answers_through_it() {
        let scratch = Scratch::new("screen-read");
        let warehouse = scratch.open();
        for sql in [
            "SELECT * FROM range(3)",
            "SELECT * FROM generate_series(1, 3)",
            "SELECT * FROM unnest([1, 2, 3])",
            "SELECT table_name FROM duckdb_tables()",
            "SELECT column_name FROM duckdb_columns()",
            "SELECT view_name FROM duckdb_views()",
            "SELECT schema_name FROM duckdb_schemas()",
            "SELECT type_name FROM duckdb_types()",
            "SELECT constraint_type FROM duckdb_constraints()",
            "SELECT index_name FROM duckdb_indexes()",
            "DESCRIBE t",
            "FROM t",
            "WITH x AS (SELECT id FROM t) SELECT * FROM x",
            "SELECT id FROM t; SELECT amount FROM t",
        ] {
            drop(answer(&warehouse, sql));
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

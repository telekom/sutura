#![forbid(unsafe_code)]
//! A leg read as several `ctid` partitions under one exported snapshot, against a real Postgres
//! through the real driver - `github.com/telekom/sutura#828`.
//!
//! Same tier, same absence handling as `tests/conformance.rs`: every venue that runs the suite
//! provisions the tier, so these cells RUN; a developer machine with none writes `NOT RUN` and the
//! reason to stderr.
//!
//! **What these cells hold that the fake cannot:** that the server accepts the import before the
//! partition's statement, that the ranges together are exactly the leg read whole, and that a row
//! committed after the export reaches no partition. Each table is created with autovacuum off and
//! analysed BEFORE more rows arrive, so `relpages` is stale on purpose and the last, open range is
//! what reads past it. **The limit**: whether a range is answered by a TID range scan is not read by
//! any cell here; the rows are what is compared.

#[cfg(test)]
mod split {
    use core::num::NonZeroUsize;
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::definitions::DefinitionDigest;
    use sutura_domain::model::{ColumnName, DimensionName, MetricName, TableName};
    use sutura_domain::pinned::MetricDigest;
    use sutura_domain::plan::{Executable, LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel};
    use sutura_domain::warehouse::{ResultBatches, UnannouncedBatch, Warehouse as _};
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::adbc::{AdbcError, AdbcPostgres, Conninfo, FixtureAdmin, PostgresDriver, SplitLeg};
    use sutura_exec_postgres::fixture::FixtureCredential;

    const SERVICE: &str = "postgres";

    /// The table's narrow rows: about 226 to a page, so a thousand are a handful of pages.
    const CREATE: &str = r#"CREATE TABLE "paged" ("k" bigint NOT NULL) WITH (autovacuum_enabled = false)"#;

    /// A fresh schema per test, the isolation `tests/raw.rs` uses, and a plain second connection
    /// into it for what no port method may do - or `None`, and a `NOT RUN` line, with no tier.
    fn open(case: &str) -> Option<(AdbcPostgres, FixtureAdmin)> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("split::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let schema = format!("split_{case}_{}", std::process::id());
        let conninfo: Conninfo = credential
            .conninfo_in(&corpus::source(), endpoint.host(), endpoint.port(), &schema)
            .unwrap_or_else(|e| panic!("no connection string for the tier at {endpoint}: {e}"));
        let driver = PostgresDriver::from_host().expect("the tier is up, so a driver is named");
        let admin = FixtureAdmin::open(&driver, &conninfo).unwrap_or_else(|e| panic!("the admin connection did not open: {e}"));
        let warehouse = AdbcPostgres::new(corpus::source(), corpus::posture(), driver, conninfo)
            .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}"));
        warehouse
            .create_schema(&schema)
            .unwrap_or_else(|e| panic!("postgres could not create {schema}: {e}"));
        Some((warehouse, admin))
    }

    fn run(admin: &mut FixtureAdmin, sql: &str) {
        admin
            .run(sql)
            .unwrap_or_else(|e| panic!("the admin connection could not run `{sql}`: {e}"));
    }

    fn split_into<'warehouse>(warehouse: &'warehouse AdbcPostgres, leg: &LegPlan, partitions: usize) -> SplitLeg<'warehouse> {
        let partitions = NonZeroUsize::new(partitions).expect("a cell asks for at least one partition");
        warehouse
            .split_leg(leg, partitions, &corpus::presented(), corpus::deadline())
            .unwrap_or_else(|e| panic!("the leg did not split: {e}"))
    }

    /// The leg's own statement, read whole through the port.
    fn read_whole(warehouse: &AdbcPostgres, leg: &LegPlan) -> Vec<String> {
        rendered(
            &warehouse
                .execute(Executable::Leg(leg), &corpus::presented(), corpus::deadline())
                .unwrap_or_else(|e| panic!("the whole leg did not read: {e}")),
        )
    }

    /// Every partition's rows together.
    fn together(split: &SplitLeg<'_>) -> Vec<String> {
        let mut rows: Vec<String> = split
            .partitions()
            .flat_map(|partition| {
                rendered(
                    &partition
                        .read(corpus::deadline())
                        .unwrap_or_else(|e| panic!("a partition did not read: {e}")),
                )
            })
            .collect();
        rows.sort_unstable();
        rows
    }

    /// Each row as its rendered cells, sorted, so two reads compare as multisets.
    fn rendered(batches: &ResultBatches) -> Vec<String> {
        let mut rows: Vec<String> = batches
            .to_rows()
            .expect("the leg's key column decodes")
            .rows()
            .iter()
            .map(|row| {
                row.iter()
                    .map(sutura_domain::warehouse::Value::render)
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect();
        rows.sort_unstable();
        rows
    }

    /// The distinct `k` of `table`, as a lookup leg reads it.
    fn lookup(table: &str) -> LegPlan {
        let table_name = TableName::parse(table).expect("a test name is a name");
        LegPlan::Lookup {
            source: corpus::source(),
            metric: MetricDigest::new(
                MetricName::parse("amount_total").expect("a test name is a name"),
                DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
            ),
            table: table_name.clone().into(),
            keys: vec![PlanKey::new(
                ResultLabel::dimension(&DimensionName::parse("k").expect("a test name is a name")),
                PlanColumn::new(table_name, ColumnName::parse("k").expect("a test name is a name")),
            )],
            bindings: PlanBindings::none(),
        }
    }

    #[test]
    fn the_partitions_together_are_exactly_the_leg_read_whole() {
        let Some((warehouse, mut admin)) = open("differential") else {
            return;
        };
        run(
            &mut admin,
            &format!(
                "{CREATE}; INSERT INTO \"paged\" SELECT g FROM generate_series(1, 2000) AS g; ANALYZE \"paged\"; \
             INSERT INTO \"paged\" SELECT g FROM generate_series(2001, 2600) AS g"
            ),
        );
        let leg = lookup("paged");
        let whole = read_whole(&warehouse, &leg);
        assert_eq!(whole.len(), 2600, "the whole read is the baseline every split is held to");
        for (asked, expected) in [(1, Some(1)), (3, Some(3)), (100_000, None)] {
            let split = split_into(&warehouse, &leg, asked);
            let cut = split.partitions().len();
            match expected {
                Some(count) => assert_eq!(cut, count, "{asked} partitions asked for"),
                None => assert!(1 < cut && cut < asked, "more partitions than pages are one per page: {cut}"),
            }
            assert_eq!(together(&split), whole, "{asked} partitions asked for, {cut} cut");
        }
    }

    #[test]
    fn a_row_committed_after_the_export_reaches_no_partition() {
        let Some((warehouse, mut admin)) = open("snapshot") else {
            return;
        };
        run(
            &mut admin,
            &format!("{CREATE}; INSERT INTO \"paged\" SELECT g FROM generate_series(1, 2000) AS g; ANALYZE \"paged\""),
        );
        let leg = lookup("paged");
        let before = read_whole(&warehouse, &leg);
        let split = split_into(&warehouse, &leg, 4);
        assert_eq!(split.partitions().len(), 4);
        run(&mut admin, "INSERT INTO \"paged\" VALUES (999999)");
        let after = read_whole(&warehouse, &leg);
        assert!(
            after.contains(&String::from("999999")),
            "the row committed and a fresh read sees it"
        );
        assert_eq!(
            together(&split),
            before,
            "every partition reads the exported snapshot, not the table now"
        );
    }

    #[test]
    fn a_partition_past_the_stream_bound_is_refused_by_its_type() {
        let Some((warehouse, mut admin)) = open("bound") else {
            return;
        };
        run(
            &mut admin,
            &format!(
                "{CREATE}; INSERT INTO \"paged\" SELECT g FROM generate_series(1, 1000) AS g; ANALYZE \"paged\"; \
             INSERT INTO \"paged\" SELECT g FROM generate_series(1001, 1010000) AS g"
            ),
        );
        let split = split_into(&warehouse, &lookup("paged"), 2);
        let outcomes: Vec<_> = split
            .partitions()
            .map(|partition| partition.read(corpus::deadline()))
            .collect();
        let [ref first, ref last] = *outcomes.as_slice() else {
            panic!("two partitions asked for over a table of several pages: {}", outcomes.len());
        };
        assert!(first.is_ok(), "the range inside the analysed pages fits: {first:?}");
        let Err(ref refused) = *last else {
            panic!("the open range holds over a million keys and must be refused");
        };
        assert!(
            matches!(
                *refused,
                PostgresError::Adbc {
                    cause: AdbcError::Unannounced(UnannouncedBatch::OverBound { most: 1_000_000 })
                }
            ),
            "{refused:?}"
        );
        assert!(
            warehouse.result_did_not_fit(refused),
            "the port reads it as a result that did not fit"
        );
    }

    #[test]
    fn a_view_is_read_whole_as_one_partition() {
        let Some((warehouse, mut admin)) = open("view") else {
            return;
        };
        run(
            &mut admin,
            &format!(
                "{CREATE}; INSERT INTO \"paged\" SELECT g FROM generate_series(1, 2000) AS g; ANALYZE \"paged\"; \
             CREATE VIEW \"paged_view\" AS SELECT \"k\" FROM \"paged\""
            ),
        );
        let leg = lookup("paged_view");
        let split = split_into(&warehouse, &leg, 4);
        assert_eq!(split.partitions().len(), 1, "a view has no pages of its own to cut");
        assert_eq!(together(&split), read_whole(&warehouse, &leg));
    }
}

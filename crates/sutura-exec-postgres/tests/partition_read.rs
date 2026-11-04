#![forbid(unsafe_code)]
//! One partition read from its descriptor's bytes alone, on a connection that never saw the split,
//! against a real Postgres through the real driver - `github.com/telekom/sutura#828`.
//!
//! Same tier, same absence handling as `tests/split.rs`: every venue that runs the suite provisions
//! the tier, so these cells RUN; a developer machine with none writes `NOT RUN` and the reason to
//! stderr. One cell needs no tier and runs everywhere: malformed bytes, against a warehouse whose
//! driver does not exist, so a connection attempted before the parse would answer with the
//! driver's failure rather than the parse's.
//!
//! **What these cells hold that the unit cells cannot:** that the server imports a snapshot named by
//! descriptor bytes alone, that a descriptor's read is its partition's and all of them together are
//! the leg read whole, that an ended exporting transaction fails the read, and that the per-stream
//! bound holds on this path too. **The limit**: which SQLSTATE the server answers a snapshot that no
//! longer exists with differs between server versions and is not asserted - the cell holds that the
//! same bytes read while the split lived and fail once it is gone.

#[cfg(test)]
mod partition_read {
    use core::num::NonZeroUsize;
    use core::time::Duration;
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::definitions::DefinitionDigest;
    use sutura_domain::identity::Secret;
    use sutura_domain::model::{ColumnName, DimensionName, MetricName, TableName};
    use sutura_domain::pinned::MetricDigest;
    use sutura_domain::plan::{Executable, LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel};
    use sutura_domain::warehouse::{ResultBatches, UnannouncedBatch, Warehouse as _};
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::adbc::{
        AdbcError, AdbcPostgres, Channel, Conninfo, FixtureAdmin, InvalidDescriptor, PostgresDriver, SplitLeg,
    };
    use sutura_exec_postgres::connection::ConnectionTarget;
    use sutura_exec_postgres::fixture::FixtureCredential;

    const SERVICE: &str = "postgres";

    /// The table's narrow rows: about 226 to a page, so a thousand are a handful of pages.
    const CREATE: &str = r#"CREATE TABLE "paged" ("k" bigint NOT NULL) WITH (autovacuum_enabled = false)"#;

    /// A fresh schema per test and a plain second connection into it - or `None`, and a `NOT RUN`
    /// line, with no tier.
    fn open(case: &str) -> Option<(AdbcPostgres, FixtureAdmin)> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("partition_read::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let schema = format!("partition_read_{case}_{}", std::process::id());
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

    /// The method `clippy.toml` bans, called from this one place.
    #[expect(
        clippy::disallowed_methods,
        reason = "a test reads descriptor bytes the way a reading worker's grant handler will; no grant exists here"
    )]
    fn read_descriptor(warehouse: &AdbcPostgres, leg: &LegPlan, descriptor: &[u8]) -> Result<ResultBatches, PostgresError> {
        warehouse.read_partition(leg, descriptor, corpus::deadline())
    }

    /// Every partition's descriptor bytes, in page order - empty where a partition hands out none.
    fn minted(split: &SplitLeg<'_>) -> Vec<Vec<u8>> {
        split
            .partitions()
            .filter_map(|partition| partition.descriptor())
            .map(|descriptor| descriptor.to_bytes())
            .collect()
    }

    /// The leg's own statement, read whole through the port.
    fn read_whole(warehouse: &AdbcPostgres, leg: &LegPlan) -> Vec<String> {
        keys(
            &warehouse
                .execute(Executable::Leg(leg), &corpus::presented(), corpus::deadline())
                .unwrap_or_else(|e| panic!("the whole leg did not read: {e}")),
        )
    }

    /// The lookup leg's one key column, rendered and sorted, so two reads compare as multisets.
    fn keys(batches: &ResultBatches) -> Vec<String> {
        let rows = batches.to_rows().expect("the leg's key column decodes");
        let mut keys: Vec<String> = rows
            .rows()
            .iter()
            .flatten()
            .map(sutura_domain::warehouse::Value::render)
            .collect();
        keys.sort_unstable();
        keys
    }

    /// The distinct `k` of `table`, as a lookup leg reads it.
    fn lookup(table: &str) -> LegPlan {
        let table = TableName::parse(table).expect("a test name is a name");
        let key = PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("k").expect("a test name is a name")),
            PlanColumn::new(table.clone(), ColumnName::parse("k").expect("a test name is a name")),
        );
        let metric = MetricDigest::new(
            MetricName::parse("amount_total").expect("a test name is a name"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        );
        LegPlan::Lookup {
            source: corpus::source(),
            metric,
            table: table.into(),
            keys: vec![key],
            bindings: PlanBindings::none(),
        }
    }

    /// A table of more pages than `relpages` says, so the last, open range reads past the estimate.
    fn paged(admin: &mut FixtureAdmin) {
        run(
            admin,
            &format!(
                "{CREATE}; INSERT INTO \"paged\" SELECT g FROM generate_series(1, 2000) AS g; ANALYZE \"paged\"; \
                 INSERT INTO \"paged\" SELECT g FROM generate_series(2001, 2600) AS g"
            ),
        );
    }

    #[test]
    fn a_descriptor_read_is_its_partition_read() {
        let Some((warehouse, mut admin)) = open("same") else {
            return;
        };
        paged(&mut admin);
        let leg = lookup("paged");
        let split = split_into(&warehouse, &leg, 3);
        assert_eq!(split.partitions().len(), 3);
        for (index, partition) in split.partitions().enumerate() {
            let descriptor = partition
                .descriptor()
                .expect("a split of a plain table hands out descriptors");
            let read = read_descriptor(&warehouse, &leg, &descriptor.to_bytes())
                .unwrap_or_else(|e| panic!("partition {index} did not read from its descriptor: {e}"));
            let direct = partition
                .read(corpus::deadline())
                .unwrap_or_else(|e| panic!("partition {index} did not read: {e}"));
            assert_eq!(keys(&read), keys(&direct), "partition {index}, {descriptor:?}");
        }
    }

    #[test]
    fn every_descriptor_read_together_is_the_leg_read_whole() {
        let Some((warehouse, mut admin)) = open("union") else {
            return;
        };
        paged(&mut admin);
        let leg = lookup("paged");
        let whole = read_whole(&warehouse, &leg);
        assert_eq!(
            whole.len(),
            2600,
            "the whole read is the baseline the descriptors are held to"
        );
        let split = split_into(&warehouse, &leg, 4);
        let mut together: Vec<String> = minted(&split)
            .iter()
            .flat_map(|bytes| {
                keys(&read_descriptor(&warehouse, &leg, bytes).unwrap_or_else(|e| panic!("a descriptor did not read: {e}")))
            })
            .collect();
        together.sort_unstable();
        assert_eq!(together, whole, "{} partitions cut", split.partitions().len());
    }

    #[test]
    fn a_descriptor_outliving_its_exporting_transaction_does_not_read() {
        let Some((warehouse, mut admin)) = open("ended") else {
            return;
        };
        paged(&mut admin);
        let leg = lookup("paged");
        let split = split_into(&warehouse, &leg, 2);
        let bytes = split
            .partitions()
            .next()
            .and_then(|partition| partition.descriptor())
            .expect("a split of a plain table hands out descriptors")
            .to_bytes();
        let live = read_descriptor(&warehouse, &leg, &bytes);
        assert!(live.is_ok(), "the bytes read while the split lives: {:?}", live.err());
        drop(split);
        let ended = (0..50).find_map(|_| match read_descriptor(&warehouse, &leg, &bytes) {
            Ok(_) => {
                std::thread::sleep(Duration::from_millis(100));
                None
            }
            Err(refused) => Some(refused),
        });
        assert!(
            matches!(
                ended,
                Some(PostgresError::Adbc {
                    cause: AdbcError::Adbc(_)
                })
            ),
            "the server ends the exporting transaction once its connection closes, and the import fails: {ended:?}"
        );
    }

    #[test]
    fn a_descriptor_range_past_the_stream_bound_is_refused_by_its_type() {
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
        let leg = lookup("paged");
        let split = split_into(&warehouse, &leg, 2);
        let outcomes: Vec<_> = minted(&split)
            .iter()
            .map(|bytes| read_descriptor(&warehouse, &leg, bytes))
            .collect();
        let [ref first, ref last] = *outcomes.as_slice() else {
            panic!("two partitions asked for over a table of several pages: {}", outcomes.len());
        };
        assert!(
            first.is_ok(),
            "the range inside the analysed pages fits: {:?}",
            first.as_ref().err()
        );
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
    fn a_split_with_no_snapshot_hands_out_no_descriptor() {
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
        let split = split_into(&warehouse, &lookup("paged_view"), 4);
        assert_eq!(split.partitions().len(), 1, "a view has no pages of its own to cut");
        assert!(
            split.partitions().all(|partition| partition.descriptor().is_none()),
            "a read of the view as it is now is nothing another connection can repeat"
        );
    }

    #[test]
    fn malformed_descriptor_bytes_are_refused_before_any_connection() {
        let driver = PostgresDriver::parse("/nonexistent/libadbc_driver_postgresql.so").expect("an absolute path parses");
        let conninfo = Conninfo::new(
            &corpus::source(),
            ConnectionTarget::Host("127.0.0.1"),
            1,
            "sutura",
            "sutura",
            &Secret::new("unused"),
            Channel::Plaintext,
        )
        .expect("plaintext builds");
        let warehouse =
            AdbcPostgres::new(corpus::source(), corpus::posture(), driver, conninfo).expect("the default ceiling parses");
        let version_two = [2, 1, b'0', 0, 0, 0, 0, 0];
        let refused = read_descriptor(&warehouse, &lookup("paged"), &version_two);
        assert!(
            matches!(
                refused,
                Err(PostgresError::MalformedDescriptor {
                    cause: InvalidDescriptor::Version { found: 2 }
                })
            ),
            "{:?}",
            refused.err()
        );
    }
}

#![cfg(feature = "fixtures")]
#![forbid(unsafe_code)]
#![expect(
    clippy::disallowed_methods,
    reason = "a test dials the driver directly to set up and change its fixture; test code does not ship"
)]

//! A lookup leg split by flashback query, against the compose-only Oracle venue. `just
//! oracle-acceptance` and `nix run .#oracle-acceptance` (the `oracle-tier` CI job) start the tier
//! and run these ignored cells; the nix checks `just validate` runs execute no Oracle statement.
//!
//! Each cell owns its table, so the cells can run at once in the fixture user's schema. The
//! expectations are written from the fixture rows, never read off the adapter's own split.

#[cfg(test)]
mod acceptance {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use sutura_conformance::corpus;
    use sutura_dev::provisioned;
    use sutura_domain::definitions::DefinitionDigest;
    use sutura_domain::model::{ColumnName, DimensionName, MetricName, QualifiedTable, TableName};
    use sutura_domain::pinned::MetricDigest;
    use sutura_domain::plan::{Executable, InternalLabel, LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel};
    use sutura_domain::source::SourcePosture;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_domain::warehouse::{ResultBudget, Value, Warehouse as _};
    use sutura_exec_oracle::{Channel, Dial, OracleError, OracleWarehouse, Planned, SplitLeg};

    /// The tier as this process reaches it: its address, the fixture credential and its authority.
    struct Tier {
        host: String,
        port: u16,
        user: String,
        password: String,
        anchors: String,
    }

    fn tier() -> Tier {
        let endpoint = provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle")
            .endpoint()
            .cloned()
            .expect("the named acceptance task requires an Oracle endpoint");
        Tier {
            host: String::from(endpoint.host()),
            port: endpoint.port(),
            user: std::env::var("SUTURA_DEV_USER").expect("the acceptance task sets SUTURA_DEV_USER"),
            password: std::env::var("SUTURA_DEV_PASSWORD").expect("the acceptance task sets SUTURA_DEV_PASSWORD"),
            anchors: std::fs::read_to_string(
                std::env::var("SUTURA_ORACLE_TIER_CA").expect("the acceptance task sets SUTURA_ORACLE_TIER_CA"),
            )
            .expect("the tier's certificate authority is readable"),
        }
    }

    impl Tier {
        /// A driver session as `user`, to set up and change the fixture.
        fn session(&self, user: &str, password: &str) -> oracledb::Connection {
            let config = oracledb::Config::default()
                .set_connect_string(&format!("tcps://{}:{}/FREEPDB1", self.host, self.port))
                .expect("the discovered endpoint is an Oracle connect string")
                .set_trust_anchors_pem(&self.anchors)
                .expect("the tier's certificate authority is an anchor")
                .set_credentials(user, password);
            oracledb::connect(config).expect("the provisioned Oracle accepts the login")
        }

        /// The adapter, as `user`.
        fn adapter(&self, user: &str, password: &str) -> OracleWarehouse {
            self.adapter_as(corpus::posture(), user, password)
        }

        /// The adapter, as `user`, declared with `posture`.
        fn adapter_as(&self, posture: SourcePosture, user: &str, password: &str) -> OracleWarehouse {
            OracleWarehouse::connect(
                corpus::source(),
                posture,
                Dial::new(
                    &self.host,
                    self.port,
                    "FREEPDB1",
                    Channel::Verified {
                        anchors_pem: &self.anchors,
                    },
                ),
                user,
                password,
                ResultBudget::of_bytes(core::num::NonZeroUsize::new(1024 * 1024).expect("a mebibyte is positive")),
            )
            .expect("the adapter opens the provisioned Oracle")
        }

        /// The fixture user's schema, as the dictionary stores it.
        fn schema(&self) -> String {
            self.user.to_uppercase()
        }
    }

    fn deadline(seconds: u64) -> Deadline {
        Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(seconds)).expect("a budget"))
    }

    fn execute(session: &oracledb::Connection, statement: &str) {
        session
            .execute(statement, &[])
            .unwrap_or_else(|cause| panic!("{statement} failed: {cause}"));
    }

    /// Drops `table` where it exists.
    fn drop_table(session: &oracledb::Connection, table: &str) {
        if let Err(cause) = session.execute(&format!("DROP TABLE \"{table}\" PURGE"), &[]) {
            assert!(cause.to_string().contains("ORA-00942"), "{table} did not drop: {cause}");
        }
    }

    /// `table` with 36 keyed customers in three regions and two with no key, its statistics
    /// gathered, then a wait past `TIMESTAMP_TO_SCN`'s precision so a snapshot taken now sees it.
    fn customers(session: &oracledb::Connection, table: &str) {
        drop_table(session, table);
        execute(
            session,
            &format!("CREATE TABLE \"{table}\" (\"customer_key\" NUMBER(10,0), \"region\" VARCHAR2(16))"),
        );
        for (key, region) in (1..=36_i64).zip(["north", "south", "east"].into_iter().cycle()) {
            session
                .execute(
                    &format!("INSERT INTO \"{table}\" (\"customer_key\", \"region\") VALUES (:1, :2)"),
                    &[&key, &region],
                )
                .expect("a keyed customer inserts");
        }
        for region in ["west", "east"] {
            session
                .execute(
                    &format!("INSERT INTO \"{table}\" (\"customer_key\", \"region\") VALUES (NULL, :1)"),
                    &[&region],
                )
                .expect("a customer with no key inserts");
        }
        session.commit().expect("the fixture commits");
        execute(
            session,
            &format!("BEGIN DBMS_STATS.GATHER_TABLE_STATS(USER, '\"{table}\"'); END;"),
        );
        std::thread::sleep(Duration::from_secs(8));
    }

    /// The customer half of a federated question: the integer link key, then the region.
    fn lookup(table: &str) -> LegPlan {
        let column = |name: &str| {
            PlanColumn::new(
                TableName::parse(table).expect("a test table is a table"),
                ColumnName::parse(name).expect("a test column is a column"),
            )
        };
        LegPlan::Lookup {
            source: corpus::source(),
            metric: MetricDigest::new(
                MetricName::parse("revenue").expect("a test metric is a metric"),
                DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
            ),
            table: TableName::parse(table).expect("a test table is a table").into(),
            keys: vec![
                PlanKey::new(ResultLabel::internal(InternalLabel::link(0)), column("customer_key")),
                PlanKey::new(
                    ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension is a dimension")),
                    column("region"),
                ),
            ],
            bindings: PlanBindings::none(),
        }
    }

    /// `leg` split into three buckets: 38 rows over a 13-row stream bound, three readers.
    fn split(warehouse: &OracleWarehouse, leg: &LegPlan) -> SplitLeg {
        let planned = warehouse
            .split_by_estimate(
                leg,
                &corpus::presented(),
                deadline(60),
                core::num::NonZeroU64::MIN,
                core::num::NonZeroUsize::new(13).expect("thirteen"),
                core::num::NonZeroUsize::new(3).expect("three"),
            )
            .expect("the split reads the dictionary and a snapshot");
        let Planned::Split(split) = planned else {
            panic!("38 analysed rows over a 13-row bound split three ways: {planned:?}");
        };
        assert_eq!(split.partitions().len(), 3, "{split:?}");
        split
    }

    fn rows(read: &sutura_domain::warehouse::ResultBatches) -> Vec<Vec<Value>> {
        read.to_rows().expect("Oracle's answer decodes to rows").rows().to_vec()
    }

    /// **The measurement.** Every row the leg reads whole at the split comes back from exactly one
    /// bucket, the two with no key from bucket 0 alone - and after the table changes, every bucket
    /// still reads the table as it was at the snapshot.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_split_leg_reads_every_row_once_at_its_snapshot_and_a_null_key_in_bucket_zero_alone() {
        const TABLE: &str = "flashback_buckets";
        let tier = tier();
        let session = tier.session(&tier.user, &tier.password);
        customers(&session, TABLE);
        let tables = BTreeSet::from([QualifiedTable::new(None, TableName::parse(TABLE).expect("a table"))]);
        let warehouse = tier
            .adapter(&tier.user, &tier.password)
            .with_flashback(&tables)
            .expect("the fixture user owns the table, so it reads it at a snapshot");
        let leg = lookup(TABLE);
        let whole = rows(
            &warehouse
                .execute(Executable::Leg(&leg), &corpus::presented(), deadline(60))
                .expect("the leg reads whole"),
        );
        assert_eq!(whole.len(), 38, "36 keyed customers and two with no key: {whole:?}");
        let split = split(&warehouse, &leg);

        execute(&session, &format!("DELETE FROM \"{TABLE}\" WHERE \"customer_key\" <= 12"));
        execute(
            &session,
            &format!("INSERT INTO \"{TABLE}\" (\"customer_key\", \"region\") VALUES (99, 'later')"),
        );
        session.commit().expect("the change commits");

        let buckets: Vec<Vec<Vec<Value>>> = split
            .partitions()
            .iter()
            .map(|partition| {
                rows(
                    &warehouse
                        .read_partition(&leg, &partition.to_bytes(), deadline(60))
                        .expect("a fresh partition reads"),
                )
            })
            .collect();
        for row in &whole {
            let holding: Vec<usize> = buckets
                .iter()
                .enumerate()
                .filter(|&(_, bucket)| bucket.contains(row))
                .map(|(index, _)| index)
                .collect();
            if row.first() == Some(&Value::Null) {
                assert_eq!(holding, vec![0], "a row with no key is bucket 0's alone: {row:?}");
            } else {
                assert_eq!(holding.len(), 1, "{row:?} is in buckets {holding:?}");
            }
        }
        assert_eq!(
            buckets.iter().map(Vec::len).sum::<usize>(),
            38,
            "the buckets read a row the snapshot does not hold: {buckets:?}"
        );
        assert!(
            buckets.iter().filter(|bucket| !bucket.is_empty()).count() >= 2,
            "every row landed in one bucket: {buckets:?}"
        );
        drop_table(&session, TABLE);
    }

    /// **D4's refusal, at the database.** A partition whose snapshot is older than the reading
    /// request's budget is refused naming the budget; the same partition under a longer budget reads.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_partition_whose_snapshot_is_older_than_the_request_budget_is_refused() {
        const TABLE: &str = "flashback_stale";
        let tier = tier();
        let session = tier.session(&tier.user, &tier.password);
        customers(&session, TABLE);
        let tables = BTreeSet::from([QualifiedTable::new(None, TableName::parse(TABLE).expect("a table"))]);
        let warehouse = tier
            .adapter(&tier.user, &tier.password)
            .with_flashback(&tables)
            .expect("the probe passes");
        let leg = lookup(TABLE);
        let first = split(&warehouse, &leg).partitions()[0].to_bytes();
        std::thread::sleep(Duration::from_secs(12));
        let refused = warehouse.read_partition(&leg, &first, deadline(3));
        assert!(
            matches!(refused, Err(OracleError::SnapshotStale { most_seconds: 3 })),
            "a twelve-second-old snapshot under a three-second budget: {refused:?}"
        );
        warehouse
            .read_partition(&leg, &first, deadline(120))
            .expect("the same snapshot under a two-minute budget reads");
        drop_table(&session, TABLE);
    }

    /// A snapshot the database no longer maps to a time is refused as gone, never read fresh.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_partition_at_a_snapshot_the_database_cannot_read_is_refused_as_gone() {
        const TABLE: &str = "flashback_gone";
        let tier = tier();
        let session = tier.session(&tier.user, &tier.password);
        customers(&session, TABLE);
        let tables = BTreeSet::from([QualifiedTable::new(None, TableName::parse(TABLE).expect("a table"))]);
        let warehouse = tier
            .adapter(&tier.user, &tier.password)
            .with_flashback(&tables)
            .expect("the probe passes");
        // Version 1, SCN 1, the key at position 0, bucket 0 of 2.
        let ancient = [1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
        let refused = warehouse.read_partition(&lookup(TABLE), &ancient, deadline(60));
        assert!(
            matches!(refused, Err(OracleError::SnapshotGone { .. })),
            "the first system change number is past every retention: {refused:?}"
        );
        drop_table(&session, TABLE);
    }

    /// A new user named `prefix` and this process, that may sign in and nothing else, and its
    /// password. Dropped first where an earlier run left it.
    fn signing_in(administrator: &oracledb::Connection, prefix: &str) -> (String, String) {
        let reader = format!("{prefix}_{}", std::process::id());
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is past 1970")
            .subsec_nanos();
        let password = format!("P{:x}x{nanos:x}", std::process::id());
        if let Err(cause) = administrator.execute(&format!("DROP USER \"{reader}\" CASCADE"), &[]) {
            assert!(cause.to_string().contains("ORA-01918"), "{reader} did not drop: {cause}");
        }
        execute(
            administrator,
            &format!("CREATE USER \"{reader}\" IDENTIFIED BY \"{password}\""),
        );
        execute(administrator, &format!("GRANT CREATE SESSION TO \"{reader}\""));
        (reader, password)
    }

    /// Drops `role` where it exists.
    fn drop_role(administrator: &oracledb::Connection, role: &str) {
        if let Err(cause) = administrator.execute(&format!("DROP ROLE \"{role}\""), &[]) {
            assert!(cause.to_string().contains("ORA-01919"), "{role} did not drop: {cause}");
        }
    }

    /// **D2's probe**: a service user that may read a table but holds no `FLASHBACK` on it is
    /// refused naming the table; granted `FLASHBACK` on it, it passes; granted `FLASHBACK ANY TABLE`
    /// as well, it is refused again.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn the_flashback_probe_refuses_a_user_without_flashback_on_a_table_or_with_any_table() {
        const TABLE: &str = "flashback_probe";
        let tier = tier();
        let owner = tier.session(&tier.user, &tier.password);
        customers(&owner, TABLE);
        let administrator = tier.session("SYSTEM", &tier.password);
        let (reader, password) = signing_in(&administrator, "FLASHBACK_PROBE");
        execute(&owner, &format!("GRANT SELECT ON \"{TABLE}\" TO \"{reader}\""));
        let table = format!("{}.{TABLE}", tier.schema());
        let tables = BTreeSet::from([QualifiedTable::parse(&table).expect("a two-part path")]);

        let refused = tier.adapter(&reader, &password).with_flashback(&tables);
        assert!(
            matches!(refused, Err(OracleError::FlashbackRefused { table: ref named, .. }) if *named == table),
            "SELECT alone does not read a snapshot: {refused:?}"
        );

        execute(&owner, &format!("GRANT FLASHBACK ON \"{TABLE}\" TO \"{reader}\""));
        drop(
            tier.adapter(&reader, &password)
                .with_flashback(&tables)
                .expect("FLASHBACK on the table passes the probe"),
        );

        execute(&administrator, &format!("GRANT FLASHBACK ANY TABLE TO \"{reader}\""));
        let any = tier.adapter(&reader, &password).with_flashback(&tables);
        assert!(
            matches!(any, Err(OracleError::FlashbackAnyTable)),
            "the system privilege is refused: {any:?}"
        );

        execute(&administrator, &format!("DROP USER \"{reader}\" CASCADE"));
        drop_table(&owner, TABLE);
    }

    /// **The probe refuses `FLASHBACK ANY TABLE` through roles granted to the service user**: a
    /// default role carrying it, the same role as a role the user does not enable at sign-in, and
    /// that role nested in another; and a role granted to the service user that it does not enable
    /// at sign-in, whatever the role carries.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn the_flashback_probe_refuses_the_privilege_through_any_role() {
        const TABLE: &str = "flashback_roles";
        let tier = tier();
        let owner = tier.session(&tier.user, &tier.password);
        customers(&owner, TABLE);
        let administrator = tier.session("SYSTEM", &tier.password);
        let (reader, password) = signing_in(&administrator, "FLASHBACK_ROLES");
        execute(&owner, &format!("GRANT SELECT, FLASHBACK ON \"{TABLE}\" TO \"{reader}\""));
        let tables = BTreeSet::from([QualifiedTable::parse(format!("{}.{TABLE}", tier.schema())).expect("a two-part path")]);
        let probe = || tier.adapter(&reader, &password).with_flashback(&tables);
        drop(probe().expect("FLASHBACK on the table alone passes the probe"));

        let [any, nesting, plain] = ["ANY", "NEST", "PLAIN"].map(|role| format!("FLASHBACK_ROLES_{role}_{}", std::process::id()));
        for role in [&any, &nesting, &plain] {
            drop_role(&administrator, role);
            execute(&administrator, &format!("CREATE ROLE \"{role}\""));
        }
        execute(&administrator, &format!("GRANT FLASHBACK ANY TABLE TO \"{any}\""));
        execute(&administrator, &format!("GRANT \"{any}\" TO \"{nesting}\""));
        execute(&administrator, &format!("GRANT CREATE TABLE TO \"{plain}\""));
        let not_by_default = || execute(&administrator, &format!("ALTER USER \"{reader}\" DEFAULT ROLE NONE"));

        execute(&administrator, &format!("GRANT \"{any}\" TO \"{reader}\""));
        execute(&administrator, &format!("ALTER USER \"{reader}\" DEFAULT ROLE ALL"));
        let default = probe();
        assert!(
            matches!(default, Err(OracleError::FlashbackAnyTable)),
            "a default role carrying the privilege: {default:?}"
        );
        not_by_default();
        let later = probe();
        assert!(
            matches!(later, Err(OracleError::FlashbackAnyTable)),
            "the same role, not enabled at sign-in, read through ROLE_SYS_PRIVS: {later:?}"
        );

        execute(&administrator, &format!("REVOKE \"{any}\" FROM \"{reader}\""));
        execute(&administrator, &format!("GRANT \"{nesting}\" TO \"{reader}\""));
        not_by_default();
        let nested = probe();
        assert!(
            matches!(nested, Err(OracleError::FlashbackAnyTable)),
            "the role nested in another, not enabled at sign-in, read through ROLE_SYS_PRIVS: {nested:?}"
        );

        execute(&administrator, &format!("REVOKE \"{nesting}\" FROM \"{reader}\""));
        execute(&administrator, &format!("GRANT \"{plain}\" TO \"{reader}\""));
        not_by_default();
        let plain_refused = probe();
        assert!(
            matches!(plain_refused, Err(OracleError::FlashbackRoleNotEnabled)),
            "any role not enabled at sign-in: {plain_refused:?}"
        );

        execute(&administrator, &format!("DROP USER \"{reader}\" CASCADE"));
        for role in [&any, &nesting, &plain] {
            drop_role(&administrator, role);
        }
        drop_table(&owner, TABLE);
    }

    /// **The probe refuses a service user that can grant itself privileges or roles**: `GRANT ANY
    /// PRIVILEGE` and `GRANT ANY ROLE`, each refused naming it. Read through the refusal's text,
    /// which names the privilege.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn the_flashback_probe_refuses_grant_any_privilege_and_grant_any_role() {
        const TABLE: &str = "flashback_grants";
        let tier = tier();
        let owner = tier.session(&tier.user, &tier.password);
        customers(&owner, TABLE);
        let administrator = tier.session("SYSTEM", &tier.password);
        let (reader, password) = signing_in(&administrator, "FLASHBACK_GRANTS");
        execute(&owner, &format!("GRANT SELECT, FLASHBACK ON \"{TABLE}\" TO \"{reader}\""));
        let tables = BTreeSet::from([QualifiedTable::parse(format!("{}.{TABLE}", tier.schema())).expect("a two-part path")]);
        for privilege in ["GRANT ANY PRIVILEGE", "GRANT ANY ROLE"] {
            execute(&administrator, &format!("GRANT {privilege} TO \"{reader}\""));
            let refused = tier.adapter(&reader, &password).with_flashback(&tables);
            assert!(
                matches!(refused, Err(ref cause) if cause.to_string().contains(&format!("`{privilege}`"))),
                "{privilege} is refused naming it: {refused:?}"
            );
            execute(&administrator, &format!("REVOKE {privilege} FROM \"{reader}\""));
        }
        drop(
            tier.adapter(&reader, &password)
                .with_flashback(&tables)
                .expect("with both revoked the probe passes"),
        );
        execute(&administrator, &format!("DROP USER \"{reader}\" CASCADE"));
        drop_table(&owner, TABLE);
    }

    /// **The adapter itself refuses a flashback split at a source that reads as each caller**, before
    /// any probe statement, whatever its user may read.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn an_impersonating_source_is_refused_its_flashback_probe() {
        let tier = tier();
        let refused = tier
            .adapter_as(SourcePosture::ImpersonationAtSource, &tier.user, &tier.password)
            .with_flashback(&BTreeSet::new());
        assert!(
            matches!(refused, Err(OracleError::FlashbackAtImpersonatingSource { .. })),
            "{refused:?}"
        );
    }

    /// **Only a lookup leg of a probed source splits**: the same leg over a source that was not
    /// probed, and a fact leg over one that was, are read whole where the probed lookup leg splits.
    /// The fact leg groups by the same integer key of the same table, so only its being a fact leg
    /// keeps it whole; it is never rendered, because it is read whole before any statement.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_leg_is_read_whole_unless_it_is_a_lookup_leg_of_a_probed_source() {
        const TABLE: &str = "flashback_whole";
        let tier = tier();
        let session = tier.session(&tier.user, &tier.password);
        customers(&session, TABLE);
        let tables = BTreeSet::from([QualifiedTable::new(None, TableName::parse(TABLE).expect("a table"))]);
        let leg = lookup(TABLE);
        let ask = |warehouse: &OracleWarehouse, leg: &LegPlan| {
            warehouse
                .split_by_estimate(
                    leg,
                    &corpus::presented(),
                    deadline(60),
                    core::num::NonZeroU64::MIN,
                    core::num::NonZeroUsize::new(13).expect("thirteen"),
                    core::num::NonZeroUsize::new(3).expect("three"),
                )
                .expect("the split is asked")
        };
        let unprobed = ask(&tier.adapter(&tier.user, &tier.password), &leg);
        assert!(matches!(unprobed, Planned::Whole), "an unprobed source: {unprobed:?}");
        let probed = tier
            .adapter(&tier.user, &tier.password)
            .with_flashback(&tables)
            .expect("the probe passes");
        let LegPlan::Fact {
            source,
            metric,
            bucket,
            terms,
            bindings,
            range,
            ..
        } = corpus::leg_case().leg().clone()
        else {
            panic!("the corpus leg is a fact leg");
        };
        let LegPlan::Lookup { table, keys, .. } = leg.clone() else {
            panic!("`lookup` builds a lookup leg");
        };
        let fact = LegPlan::Fact {
            source,
            metric,
            tables: table,
            bucket,
            keys,
            terms,
            bindings,
            range,
        };
        let read = ask(&probed, &fact);
        assert!(
            matches!(read, Planned::Whole),
            "a fact leg keyed by the integer link: {read:?}"
        );
        drop(split(&probed, &leg));
        drop_table(&session, TABLE);
    }

    /// **A granted split's one schema.** An integer key and a text column are declared fixed, so
    /// the split describes `Int64` and `Utf8`, and every bucket answers exactly that schema - an
    /// empty or all-`NULL` column among them included, which its cells would type otherwise.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_split_of_an_integer_key_and_a_text_column_is_described_and_every_bucket_answers_it() {
        const TABLE: &str = "flashback_described";
        let tier = tier();
        let session = tier.session(&tier.user, &tier.password);
        customers(&session, TABLE);
        let tables = BTreeSet::from([QualifiedTable::new(None, TableName::parse(TABLE).expect("a table"))]);
        let warehouse = tier
            .adapter(&tier.user, &tier.password)
            .with_flashback(&tables)
            .expect("the probe passes");
        let leg = lookup(TABLE);
        let split = split(&warehouse, &leg);
        let described = warehouse
            .describe_split(&leg, &split, deadline(60))
            .expect("the first partition is described")
            .expect("an integer key and a text column are fixed");
        let kinds: Vec<arrow_schema::DataType> = described.fields().iter().map(|field| field.data_type().clone()).collect();
        assert_eq!(
            kinds,
            vec![arrow_schema::DataType::Int64, arrow_schema::DataType::Utf8],
            "{described:?}"
        );
        for partition in split.partitions() {
            let read = warehouse
                .read_partition(&leg, &partition.to_bytes(), deadline(60))
                .expect("a fresh partition reads");
            assert_eq!(read.schema(), &described, "bucket {partition:?} answered another schema");
        }
        drop_table(&session, TABLE);
    }

    /// A column whose cells decide its type - a `NUMBER` with a scale - leaves the split with no one
    /// schema, so it is not granted.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_split_with_a_column_of_scale_two_describes_no_schema() {
        const TABLE: &str = "flashback_undescribed";
        let tier = tier();
        let session = tier.session(&tier.user, &tier.password);
        drop_table(&session, TABLE);
        execute(
            &session,
            &format!("CREATE TABLE \"{TABLE}\" (\"customer_key\" NUMBER(10,0), \"region\" NUMBER(10,2))"),
        );
        execute(
            &session,
            &format!(
                "INSERT INTO \"{TABLE}\" (\"customer_key\", \"region\") SELECT LEVEL, LEVEL + 0.5 FROM DUAL CONNECT BY LEVEL <= 38"
            ),
        );
        session.commit().expect("the fixture commits");
        execute(
            &session,
            &format!("BEGIN DBMS_STATS.GATHER_TABLE_STATS(USER, '\"{TABLE}\"'); END;"),
        );
        std::thread::sleep(Duration::from_secs(8));
        let tables = BTreeSet::from([QualifiedTable::new(None, TableName::parse(TABLE).expect("a table"))]);
        let warehouse = tier
            .adapter(&tier.user, &tier.password)
            .with_flashback(&tables)
            .expect("the probe passes");
        let leg = lookup(TABLE);
        let split = split(&warehouse, &leg);
        let described = warehouse
            .describe_split(&leg, &split, deadline(60))
            .expect("the first partition is described");
        assert_eq!(described, None, "a NUMBER(10,2) column is not fixed");
        drop_table(&session, TABLE);
    }
}

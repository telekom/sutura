//! The real tier's bucket reads use the same caller and settings as the whole lookup.

use core::num::{NonZeroU64, NonZeroUsize};

use sutura_domain::source::{AcknowledgementReason, HashBucketsDeclared};
use sutura_exec_clickhouse::adbc::{BucketDescriptor, PartitionIdentity, Planned};

use super::*;

/// Base has the public split API but no opt-in builder. Its no-op fixture fallback keeps this
/// cell executable there: the base must fail at the split assertion, not at compilation. On head,
/// Rust selects the adapter's inherent method, which retains the declaration.
trait BaseOptIn {
    fn with_hash_buckets(self, _declared: &HashBucketsDeclared) -> Self;
}

impl BaseOptIn for AdbcClickHouseWarehouse {
    fn with_hash_buckets(self, _declared: &HashBucketsDeclared) -> Self {
        self
    }
}

fn declared() -> HashBucketsDeclared {
    HashBucketsDeclared::of(
        AcknowledgementReason::written_under("hash_buckets", "the fixture does not change during its bucket reads")
            .expect("a reason"),
    )
}

fn table() -> TableName {
    TableName::parse("bucket_lookup").expect("a table")
}

fn lookup() -> LegPlan {
    LegPlan::Lookup {
        source: corpus::source(),
        metric: MetricDigest::new(
            MetricName::parse("amount_total").expect("a metric"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: StatementTables::only(table()),
        keys: vec![
            keyed("id", column(&table(), "id")),
            keyed("region", column(&table(), "region")),
        ],
        bindings: PlanBindings::parse(Vec::new(), Vec::new()).expect("no bindings"),
    }
}

fn load(tier: &Tier) {
    let path = std::env::temp_dir().join(format!("sutura-ch-buckets-{}.csv", std::process::id()));
    let mut csv = String::from("id,region\n");
    for id in 0..32 {
        use core::fmt::Write as _;
        writeln!(csv, "{id},row_{id}").expect("a string writes");
    }
    csv.push_str(",null_key\n0,zero_key\n");
    std::fs::write(&path, csv).expect("the fixture writes");
    let credential = credential_from_env().expect("a tier credential");
    let loader = AdbcClickHouseWarehouse::connect_in_database(
        corpus::source(),
        corpus::posture(),
        &tier.host,
        tier.port,
        &credential,
        LEGS,
        budget(),
    )
    .expect("the fixture database opens");
    loader.load_conformance_csv(&table(), &path).expect("the lookup loads");
    std::fs::remove_file(path).expect("the scratch CSV is removed");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "this fixture calls the granted-read boundary directly to measure each bucket under the split's caller"
)]
fn a_clickhouse_lookup_split_reads_every_group_once_under_the_callers_grants_and_settings() {
    let Some(tier) = Tier::here("buckets") else {
        return;
    };
    load(&tier);
    let _base_compatibility: fn(AdbcClickHouseWarehouse, &HashBucketsDeclared) -> AdbcClickHouseWarehouse =
        <AdbcClickHouseWarehouse as BaseOptIn>::with_hash_buckets;
    let warehouse = tier
        .at(LEGS, SourcePosture::ImpersonationAtSource)
        .with_hash_buckets(&declared());
    let leg = lookup();
    let caller = switched_to("sutura_switched");
    let whole = warehouse
        .execute(Executable::Leg(&leg), &caller, corpus::deadline())
        .expect("the whole lookup answers");
    let whole_rows = whole.to_rows().expect("the lookup decodes");
    assert_eq!(
        whole_rows.rows().len(),
        34,
        "including a null key and two groups sharing key zero"
    );
    let planned = warehouse
        .split_by_estimate(
            &leg,
            &caller,
            corpus::deadline(),
            NonZeroU64::MIN,
            NonZeroUsize::new(17).expect("a stream target"),
            NonZeroUsize::new(2).expect("two readers"),
        )
        .expect("the lookup estimates and describes under the caller");
    let Planned::Split(split) = planned else {
        panic!("the 34-row lookup must split across two 17-row targets");
    };
    assert_eq!(split.queries().len(), 2);
    assert!(matches!(split.reads(), PartitionIdentity::Switched(name) if name.as_str() == "sutura_switched"));
    let mut all = Vec::new();
    for (bucket, _) in split.queries() {
        let index = u8::try_from(bucket.index()).expect("one of two buckets");
        let descriptor = BucketDescriptor::try_from([1, 0, 0, 0, 2, 0, 0, 0, index].as_slice())
            .expect("one of two bucket descriptors")
            .to_bytes();
        let rows = warehouse
            .read_partition(
                &leg,
                &descriptor,
                split.described(),
                PartitionIdentity::Switched(PrincipalName::parse("sutura_switched").expect("a caller")),
                corpus::deadline(),
            )
            .expect("the caller reads its bucket")
            .to_rows()
            .expect("the bucket decodes");
        assert!(!rows.rows().is_empty(), "both buckets contain real groups");
        all.extend_from_slice(rows.rows());
        let refused = warehouse
            .read_partition(
                &leg,
                &descriptor,
                split.described(),
                PartitionIdentity::Switched(PrincipalName::parse("sutura_switched_ungranted").expect("a caller")),
                corpus::deadline(),
            )
            .expect_err("no IMPERSONATE means no bucket");
        assert!(warehouse.source_refused(&refused), "{refused}");
        let refused = warehouse
            .read_partition(
                &leg,
                &descriptor,
                split.described(),
                PartitionIdentity::Switched(PrincipalName::parse("sutura_switched_readonly").expect("a caller")),
                corpus::deadline(),
            )
            .expect_err("readonly must not drop required settings");
        assert!(matches!(refused, AdbcError::SettingRefused(_)), "{refused}");
        assert!(
            warehouse
                .read_partition(
                    &leg,
                    &descriptor,
                    split.described(),
                    PartitionIdentity::ServiceUser,
                    corpus::deadline()
                )
                .is_err(),
            "a missing principal never falls back to the deployment"
        );
        let shared = tier.at(LEGS, corpus::posture()).with_hash_buckets(&declared());
        assert!(
            shared
                .read_partition(
                    &leg,
                    &descriptor,
                    split.described(),
                    PartitionIdentity::Switched(PrincipalName::parse("sutura_switched").expect("a caller")),
                    corpus::deadline()
                )
                .is_err(),
            "a shared source does not silently drop a switched principal"
        );
        assert!(
            tier.at(LEGS, SourcePosture::ImpersonationAtSource)
                .read_partition(
                    &leg,
                    &descriptor,
                    split.described(),
                    PartitionIdentity::Switched(PrincipalName::parse("sutura_switched").expect("a caller")),
                    corpus::deadline()
                )
                .is_err(),
            "an undeclared source reads no bucket"
        );
        let changed = std::sync::Arc::new(arrow_schema::Schema::empty());
        assert!(
            warehouse
                .read_partition(
                    &leg,
                    &descriptor,
                    &changed,
                    PartitionIdentity::Switched(PrincipalName::parse("sutura_switched").expect("a caller")),
                    corpus::deadline()
                )
                .is_err(),
            "a mismatched grant schema reads no bucket"
        );
    }
    let split_rows = RowSet::new(whole_rows.columns().to_vec(), all).expect("the same columns");
    agreeing(&whole_rows, &split_rows);
}

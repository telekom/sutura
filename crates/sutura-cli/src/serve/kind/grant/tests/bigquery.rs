//! The `BigQuery` arm of the split dispatch: which lookup legs the planning node splits - only one
//! at a pool source offered to a grantee - and a granted partition a reading worker is asked to
//! read with no token. No driver is loaded here, so a leg the arm splits fails at the adapter, not
//! at a stream.

use core::num::{NonZeroU64, NonZeroUsize};
use core::time::Duration;
use std::collections::BTreeMap;
use std::time::{Instant, SystemTime};

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::identity::{
    Agreed, Expiry, LegCredentials, Minted, Presented, PrincipalChain, RequestContext, Secret, SourceSet, Subject, WorkloadPool,
};
use sutura_domain::model::{ColumnName, DimensionName, MetricName, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{Grantee, LegPlan, Partitioned, PlanBindings, PlanColumn, PlanKey, ResultLabel};
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::labels::JobLabels;
use sutura_exec_bigquery::adbc::{BytesBilledCeiling, DelegatingAccount, DriverLocation, Impersonation};
use sutura_exec_bigquery::{BigQueryWarehouse, JobBilling};
use sutura_exec_datafusion::Unread;

use super::super::super::super::BigQuerySource;
use super::super::super::split::SplitKind as _;

fn ledger() -> SourceName {
    SourceName::parse("ledger").expect("a test source is a source")
}

fn opened_as(impersonation: Impersonation) -> BigQuerySource {
    BigQueryWarehouse::over_adbc(
        ledger(),
        SourcePosture::ImpersonationAtSource,
        JobBilling::CallerRun { caller_may_name: false },
        DriverLocation::parse("/nonexistent/libadbc_driver_bigquery.so").expect("an absolute path"),
        impersonation,
        BytesBilledCeiling::parse(1 << 30).expect("a ceiling"),
    )
}

fn at_a_pool() -> Impersonation {
    Impersonation::ThroughPool(
        WorkloadPool::parse("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/q")
            .expect("a test pool is a pool"),
    )
}

fn ledger_lookup() -> LegPlan {
    let table = TableName::parse("accounts").expect("a test name is a name");
    LegPlan::Lookup {
        source: ledger(),
        metric: MetricDigest::new(
            MetricName::parse("amount_total").expect("a test name is a name"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: table.clone().into(),
        keys: vec![PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("k").expect("a test name is a name")),
            PlanColumn::new(table, ColumnName::parse("k").expect("a test name is a name")),
        )],
        bindings: PlanBindings::none(),
    }
}

fn callers_token() -> Presented {
    Presented::SubjectToken {
        material: Secret::new("the-callers-assertion"),
    }
}

fn offered_to_the_caller() -> Grantee {
    let subject = Subject::verified("user@example.com").expect("a test caller is a subject");
    let sources = SourceSet::of(ledger());
    let credentials = LegCredentials::minted(
        subject.clone(),
        Expiry::NothingExpires,
        &sources,
        BTreeMap::from([(ledger(), callers_token())]),
    )
    .expect("one leg covers its one source");
    let Ok(Agreed::Granted { credentials }) = (Minted::Granted { credentials }).agreeing_with(&subject, &sources, 0) else {
        panic!("a fixture grant agrees with its own request");
    };
    Grantee::new(&ledger_lookup(), &credentials, SystemTime::now() + Duration::from_secs(60))
        .expect("a verified caller's lookup leg is offered")
}

/// What the arm answered: `whole`, or the split it made or failed at.
fn split_by(bigquery: &BigQuerySource, grantee: Option<Grantee>) -> String {
    let labels = JobLabels::of(&RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself)));
    let deadline = Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(30)).expect("a budget"));
    match bigquery.split(
        &ledger_lookup(),
        &callers_token(),
        deadline,
        NonZeroU64::MIN,
        NonZeroUsize::new(4).expect("a literal is not zero"),
        grantee,
        &labels,
    ) {
        Ok(Partitioned::Whole) => String::from("whole"),
        Ok(_) => String::from("answered a split"),
        Err(cause) => cause.to_string(),
    }
}

/// A leg at a shared, Google-login or delegating source is read whole, offered to a grantee or
/// not, and never reaches the adapter's split. At a pool source a leg offered to no grantee is read
/// whole, and one offered to a grantee goes to the adapter's split - here it fails there, with no
/// driver.
#[test]
fn a_bigquery_lookup_leg_is_split_only_at_a_pool_source_and_only_when_offered_to_a_grantee() {
    let delegating = Impersonation::DomainDelegation(
        DelegatingAccount::parse("dwd@acme-analytics.iam.gserviceaccount.com").expect("a service-account address"),
    );
    for impersonation in [Impersonation::Disabled, Impersonation::GoogleLogin, delegating] {
        let bigquery = opened_as(impersonation.clone());
        assert_eq!(
            split_by(&bigquery, Some(offered_to_the_caller())),
            "whole",
            "{impersonation:?} read the leg whole"
        );
    }
    let bigquery = opened_as(at_a_pool());
    assert_eq!(
        split_by(&bigquery, None),
        "whole",
        "a leg offered to no grantee is read whole"
    );
    let split = split_by(&bigquery, Some(offered_to_the_caller()));
    assert_ne!(split, "whole", "a leg offered to a grantee is split at its source");
}

/// A granted `BigQuery` partition whose task carries no token is refused naming the source, never
/// read as this node's own identity: the backstop to a grant the planning node made with none.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the cell asks the kind's own read of a partition that carries no token, which answers without reading anything"
)]
fn a_granted_bigquery_partition_with_no_token_is_refused_naming_the_source() {
    let bigquery = opened_as(at_a_pool());
    let deadline = Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(30)).expect("a budget"));
    let read = bigquery.read_granted(&ledger_lookup(), b"", deadline);
    assert!(
        matches!(&read, Err(Unread::Failed(cause)) if cause.to_string().contains("a partition of `ledger` is read only with its caller's token")),
        "a granted partition with no token was not refused as one: {read:?}"
    );
}

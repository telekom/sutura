//! Granted identities through the concrete reader, ending at identity or descriptor guards.
//! No caller admission, live source read or driver load is exercised here.

use core::num::NonZeroUsize;
use core::time::Duration;
use std::collections::BTreeSet;
use std::sync::Arc;

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{
    Audience, Definitions, Description, Dimension, JoinKey, JoinKeys, Metric, Model, Relationship, ViaChain,
};
use sutura_domain::identity::{PrincipalName, VerifierId};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::remote::PartitionLeg;
use sutura_domain::plan::{InternalLabel, LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel, StatementTables};
use sutura_domain::warehouse::deadline::Budget;
use sutura_exec_clickhouse::adbc::{AdbcError, InvalidBucketDescriptor};
use sutura_exec_datafusion::{PartitionReader as _, SchemaRef, SendableRecordBatchStream, SwitchedPrincipal, WorkingSet};

const SERVICE_USER: &str = "sutura_worker_service";

fn source() -> SourceName {
    SourceName::parse("ledger").expect("a source")
}

fn column(name: &str) -> ColumnName {
    ColumnName::parse(name).expect("a column")
}

fn pinned() -> PinnedDefinitions {
    let relation = RelationshipName::parse("fact_lookup").expect("a relationship");
    let definitions = Definitions::assemble(
        vec![
            Model::new(
                ModelName::parse("facts").unwrap(),
                SourceName::parse("facts").unwrap(),
                TableName::parse("facts").unwrap(),
                ["id", "amount", "day"].map(column),
                Description::default(),
            ),
            Model::new(
                ModelName::parse("lookup").unwrap(),
                source(),
                TableName::parse("accounts").unwrap(),
                [column("id")],
                Description::default(),
            ),
        ],
        vec![Relationship::new(
            relation.clone(),
            ModelName::parse("facts").unwrap(),
            ModelName::parse("lookup").unwrap(),
            JoinType::ManyToOne,
            JoinKeys::of(vec![JoinKey::Equal {
                origin: column("id"),
                target: column("id"),
            }])
            .unwrap(),
        )],
        vec![
            Metric::new(
                MetricName::parse("amount_total").unwrap(),
                ModelName::parse("facts").unwrap(),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount")))),
                vec![],
                column("day"),
                BTreeSet::from([Grain::Month]),
                vec![Dimension::new(
                    DimensionName::parse("id").unwrap(),
                    column("id"),
                    Some(ViaChain::of(vec![relation]).unwrap()),
                    None,
                    Description::default(),
                )],
                None,
                Description::default(),
                Audience::Open,
            )
            .unwrap(),
        ],
    )
    .expect("a real cross-source definition");
    let produced = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    PinnedDefinitions::pin(
        DefinitionVersion::parse("switched-reader").unwrap(),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(source(), Contribution::of(produced)),
    )
    .expect("the fixture pins")
}

async fn reading(posture: &'static str) -> (super::super::Reader, Vec<u8>, SchemaRef) {
    sutura_runtime::spawn_carrying_span(move || {
        let file = std::env::temp_dir().join(format!("sutura-switched-reader-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::write(&file, "unused").expect("a fixture credential writes");
        let registry = crate::serve::tests::direct_registry(&format!(
            "  ledger:\n    kind: clickhouse\n    host: 127.0.0.1\n    port: 1\n    user: {SERVICE_USER}\n    password_file: {}\n    transport_mode: plaintext\n    posture: {posture}\n    hash_buckets_acknowledged_because: the fixture is immutable\n", file.display()
        ));
        let configured = registry.get(&source()).expect("the source is declared");
        let warehouse = crate::clickhouse::build(&source(), configured, WorkingSet::of_bytes(NonZeroUsize::MIN));
        std::fs::remove_file(file).expect("the credential is removed after composition");
        let sources = sutura_app::Warehouses::of(warehouse.expect("the declared source composes without loading its driver"));
        let pinned = pinned();
        let table = TableName::parse("accounts").unwrap();
        let leg = LegPlan::Lookup {
            source: source(), metric: pinned.metric_digest(&MetricName::parse("amount_total").unwrap()).unwrap(),
            table: StatementTables::only(table.clone()),
            keys: vec![
                PlanKey::new(ResultLabel::internal(InternalLabel::link(0)), PlanColumn::new(table.clone(), column("id"))),
                PlanKey::new(ResultLabel::dimension(&DimensionName::parse("id").unwrap()), PlanColumn::new(table, column("id"))),
            ], bindings: PlanBindings::none(),
        };
        let wire = PartitionLeg::encode(&leg).expect("the real lookup encodes");
        sutura_app::read_recertified(&pinned, &sources, &source(), &wire, |_, _| ()).expect("the real fixture leg re-certifies");
        let reader = super::super::Reader::new(
            pinned, crate::serve::OpenedSources::ClickHouse(sources),
            Budget::parse(Duration::from_secs(30)).unwrap(),
            sutura_config::credentials::SourceAdmissions::from_registry(&registry),
        );
        (reader, wire, Arc::new(arrow_schema::Schema::empty()))
    }).await.expect("the fixture composes on a blocking task")
}

fn principal(name: &str) -> SwitchedPrincipal {
    SwitchedPrincipal::new(PrincipalName::parse(name).expect("a principal"))
}

fn is_descriptor(error: &AdbcError) -> bool {
    core::error::Error::source(error).is_some_and(<dyn core::error::Error + 'static>::is::<InvalidBucketDescriptor>)
}

fn is_disagreement(error: &AdbcError) -> bool {
    error.to_string() == "the ClickHouse partition identity disagrees with this source's posture"
}

async fn failed_with(mut stream: SendableRecordBatchStream, is: impl FnOnce(&AdbcError) -> bool) -> (bool, String) {
    let Some(Err(error)) = std::future::poll_fn(|context| stream.as_mut().poll_next(context)).await else {
        return (false, String::from("the read did not return an error"));
    };
    let matched = core::error::Error::source(&error)
        .and_then(|cause| cause.downcast_ref::<AdbcError>())
        .is_some_and(is);
    (matched, error.to_string())
}

#[tokio::test]
async fn a_switched_grant_is_read_as_the_principal_it_names_at_an_impersonating_source() {
    let (reader, wire, schema) = reading("impersonation-at-source").await;
    let read = reader
        .read_switched(
            &VerifierId::primary(),
            &wire,
            &source(),
            b"",
            &schema,
            principal("alice"),
            None,
        )
        .expect("the worker dispatches the switched grant instead of refusing every switched read");
    let (matched, cause) = failed_with(read, is_descriptor).await;
    assert!(
        matched,
        "the identity guard passed and the read stopped at the descriptor: {cause}"
    );
}

#[tokio::test]
async fn a_switched_grant_at_a_shared_service_source_is_refused_as_disagreeing_with_its_posture() {
    let (reader, wire, schema) = reading("shared-service-user").await;
    let read = reader
        .read_switched(
            &VerifierId::primary(),
            &wire,
            &source(),
            b"",
            &schema,
            principal("alice"),
            None,
        )
        .expect("the admitted grant reaches the source's own identity guard");
    let (matched, cause) = failed_with(read, is_disagreement).await;
    assert!(
        matched,
        "the shared source must refuse the switched grant as an identity disagreement: {cause}"
    );
}

#[tokio::test]
async fn a_service_user_grant_at_an_impersonating_source_is_refused_as_disagreeing_with_its_posture() {
    let (reader, wire, schema) = reading("impersonation-at-source").await;
    let read = reader
        .read(&VerifierId::primary(), &wire, &source(), b"", &schema, None)
        .expect("the admitted service-user grant reaches the source");
    let (matched, cause) = failed_with(read, is_disagreement).await;
    assert!(
        matched,
        "the impersonating source must refuse the service-user grant as an identity disagreement: {cause}"
    );
}

#[tokio::test]
async fn the_principal_a_worker_is_told_to_switch_to_reaches_the_adapter_and_is_never_dropped() {
    for (posture, name, descriptor) in [
        ("shared-service-user", "alice", false),
        ("impersonation-at-source", "alice", true),
        ("impersonation-at-source", SERVICE_USER, false),
    ] {
        let (reader, wire, schema) = reading(posture).await;
        let read = reader
            .read_switched(&VerifierId::primary(), &wire, &source(), b"", &schema, principal(name), None)
            .expect("the worker dispatches the granted principal to the adapter");
        let (matched, cause) = failed_with(read, if descriptor { is_descriptor } else { is_disagreement }).await;
        assert!(
            matched,
            "the granted principal and source posture must both reach the adapter: {posture}/{name}: {cause}"
        );
    }
}

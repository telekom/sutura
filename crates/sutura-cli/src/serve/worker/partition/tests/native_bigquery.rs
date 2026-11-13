//! A real forwarded-token native read through the CLI reader and worker receipt barrier.
//! No HTTP caller or delegation hop participates; assertions are the existing live pool fixture's.
use core::convert::Infallible;
use core::num::{NonZeroU64, NonZeroUsize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use sha2::Digest as _;
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{
    Audience, Definitions, Description, Dimension, JoinKey, JoinKeys, Metric, Model, Relationship, ViaChain,
};
use sutura_domain::federation::Federation;
use sutura_domain::identity::{
    Agreed, BoundToTheRequest, Expiry, LegCredentials, Minted, Presented, PrincipalChain, RequestContext, Secret, SourceSet,
    Subject,
};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, QualifiedTable, RelationshipName, SourceName,
    TableName,
};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::{
    AnswerKey, Executable, FederatedPlan, FederationCombiner as _, Granted, Grantee, InternalLabel, LegPlan, LegRunner, LegTerm,
    LinkCast, Looked, LookupLegs, Partitioned, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel,
    StatementTables, range_filters,
};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::execution::{Execution, PlanStage};
use sutura_domain::warehouse::labels::JobLabels;
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{ResultBatches, Warehouse as _};
use sutura_exec_bigquery::adbc::{AdbcBigQuery, BytesBilledCeiling, DriverLocation, Impersonation, WorkloadPool};
use sutura_exec_bigquery::{BigQueryWarehouse, JobBilling};
use sutura_exec_datafusion::{DataFusionCombiner, DataFusionWarehouse, NodeIdentity, Nodes, TaskGrant};
use tokio::sync::Mutex;

fn live(name: &str) -> String {
    assert!(
        std::env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true"),
        "native BigQuery fixture runs only in CI"
    );
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| panic!("{name} is required"))
}

fn source(name: &str) -> SourceName {
    SourceName::parse(name).unwrap()
}
fn column(name: &str) -> ColumnName {
    ColumnName::parse(name).unwrap()
}
fn deadline() -> Deadline {
    Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(600)).unwrap())
}

/// Row 1 of the checked-in test-infra/pulumi/google/split-shared.sql seed.
fn fixture_id() -> String {
    let first = base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(b"0:1"));
    let second = base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(b"1:1"));
    format!("000001{first}{}", second.chars().take(64).collect::<String>())
}

fn opened() -> BigQueryWarehouse<AdbcBigQuery> {
    use sutura_http_client::delegation::{GOOGLE_STS, GoogleSts, TokenEndpoint};
    let pool = WorkloadPool::parse(&live("SUTURA_BQ_AUDIENCE")).unwrap_or_else(|_| panic!("fixture pool is invalid"));
    let driver = DriverLocation::parse(&live("SUTURA_BIGQUERY_ADBC_DRIVER"))
        .unwrap_or_else(|_| panic!("fixture driver location is invalid"));
    let bounds = sutura_http_client::ReadBounds::parse(10, 64 * 1024).unwrap();
    let exchange = GoogleSts::new(
        TokenEndpoint::parse(GOOGLE_STS).unwrap(),
        sutura_http_client::fixed(bounds, None),
        bounds,
    );
    BigQueryWarehouse::new(
        source("warehouse"),
        SourcePosture::ImpersonationAtSource,
        JobBilling::CallerRun { caller_may_name: false },
        AdbcBigQuery::new(
            driver,
            Impersonation::ThroughPool(pool),
            BytesBilledCeiling::parse(1 << 30).unwrap(),
        )
        .exchanging_through(Box::new(exchange)),
    )
}

fn pin(table: &QualifiedTable, metric: &MetricName, measure: &Measure, dimension: &DimensionName) -> PinnedDefinitions {
    let relation = RelationshipName::parse("fact_lookup").unwrap();
    let definitions = Definitions::assemble(
        vec![
            Model::new(
                ModelName::parse("facts").unwrap(),
                source("facts"),
                TableName::parse("facts").unwrap(),
                ["id", "amount", "day"].map(column),
                Description::default(),
            ),
            Model::new(
                ModelName::parse("lookup").unwrap(),
                source("warehouse"),
                table.clone(),
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
                metric.clone(),
                ModelName::parse("facts").unwrap(),
                measure.clone(),
                vec![],
                column("day"),
                BTreeSet::from([Grain::Month]),
                vec![Dimension::new(
                    dimension.clone(),
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
    .unwrap();
    let produced = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    PinnedDefinitions::pin(
        DefinitionVersion::parse("native-fixture").unwrap(),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(source("facts"), Contribution::of(produced)),
    )
    .unwrap()
}

/// One lookup over a declared fixture table, certified by its own metadata pin.
fn plan_for(table_env: &str) -> (PinnedDefinitions, FederatedPlan) {
    let table = QualifiedTable::parse(live(table_env)).unwrap_or_else(|_| panic!("fixture table declaration is invalid"));
    let metric = MetricName::parse("native_reads").unwrap();
    let measure = Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount"))));
    let dimension = DimensionName::parse("id").unwrap();
    let pinned = pin(&table, &metric, &measure, &dimension);
    let digest = pinned.metric_digest(&metric).unwrap();
    let fact_table = TableName::parse("facts").unwrap();
    let bucket = PlanBucket::new(
        ResultLabel::bucket(),
        Grain::Month,
        PlanColumn::new(fact_table.clone(), column("day")),
    );
    let range = TimeRange::new(Date::parse("2026-06-01").unwrap(), Date::parse("2026-07-01").unwrap()).unwrap();
    let mut values = Vec::new();
    let filters = range_filters(range, bucket.column(), &mut values);
    let fact = LegPlan::Fact {
        source: source("facts"),
        metric: digest.clone(),
        tables: StatementTables::only(fact_table.clone()),
        bucket: bucket.clone(),
        keys: vec![PlanKey::new(
            ResultLabel::internal(InternalLabel::link(0)),
            PlanColumn::new(fact_table.clone(), column("id")),
        )],
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: PlanColumn::new(fact_table, column("amount")),
            },
            ResultLabel::internal(InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::parse(filters.into(), values).unwrap(),
        range,
    };
    let lookup = LegPlan::Lookup {
        source: source("warehouse"),
        metric: digest,
        table: StatementTables::only(table.clone()),
        keys: vec![
            PlanKey::new(
                ResultLabel::internal(InternalLabel::link(0)),
                PlanColumn::new(table.name().clone(), column("id")),
            ),
            PlanKey::new(
                ResultLabel::dimension(&dimension),
                PlanColumn::new(table.name().clone(), column("id")),
            ),
        ],
        bindings: PlanBindings::none(),
    };
    let plan = FederatedPlan::new(
        metric.clone(),
        ResultLabel::measure(&metric),
        bucket,
        fact,
        None,
        LookupLegs::one(lookup, true),
        Federation::of(&measure),
        vec![AnswerKey::lookup(0, ResultLabel::dimension(&dimension))],
    )
    .unwrap();
    (pinned, plan)
}

struct Actual<'g> {
    fact: Mutex<Option<ResultBatches>>,
    grant: Mutex<Option<Granted<'g>>>,
}

impl LegRunner for Actual<'_> {
    type Error = Infallible;
    #[expect(
        clippy::panic_in_result_fn,
        clippy::unwrap_in_result,
        reason = "an infallible fixture refuses a misrouted or repeated leg"
    )]
    fn run(&self, leg: &LegPlan) -> Result<ResultBatches, Infallible> {
        assert_eq!(leg.source(), &source("facts"));
        let mut fact = self.fact.try_lock().expect("the fixture fact is uncontended");
        Ok(fact.take().expect("one real files fact"))
    }
    #[expect(
        clippy::panic_in_result_fn,
        clippy::unwrap_in_result,
        reason = "an infallible fixture refuses a misrouted or repeated leg"
    )]
    fn lookup(&self, leg: &LegPlan, _: LinkCast) -> Result<Looked<'_>, Infallible> {
        assert_eq!(leg.source(), &source("warehouse"));
        let mut grant = self.grant.try_lock().expect("the fixture grant is uncontended");
        Ok(Looked::Granted(grant.take().expect("one real BigQuery split")))
    }
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

fn listening(
    settings: &sutura_config::Settings,
    pinned: PinnedDefinitions,
    room: &Scratch,
) -> (tokio::runtime::Runtime, sutura_runtime::Shutdown, Nodes) {
    let working_set = crate::sources::working_set(settings.runtime());
    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let authority = rcgen::CertifiedIssuer::self_signed(ca_params, rcgen::KeyPair::generate().unwrap()).unwrap();
    let signing_key = rcgen::KeyPair::generate().unwrap();
    let issued = rcgen::CertificateParams::new(vec![String::from("127.0.0.1")])
        .unwrap()
        .signed_by(&signing_key, &authority)
        .unwrap();
    let certificate = room.0.join("node.pem");
    let key = room.0.join("node.key");
    let trust = room.0.join("authority.pem");
    std::fs::write(&trust, authority.pem()).unwrap();
    std::fs::write(&certificate, issued.pem()).unwrap();
    std::fs::write(&key, signing_key.serialize_pem()).unwrap();
    let identity = || {
        NodeIdentity::new(
            vec![issued.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der())),
            [authority.der().clone()],
        )
        .unwrap()
    };
    let tls = sutura_config::TlsMaterial::parse(
        Some(certificate.to_str().unwrap()),
        Some(key.to_str().unwrap()),
        Some(trust.to_str().unwrap()),
    )
    .unwrap()
    .unwrap();
    let own = Nodes::worker(working_set, identity()).unwrap();
    let reader = super::super::Reader::new(
        pinned,
        crate::serve::OpenedSources::BigQuery(sutura_app::Warehouses::of(opened())),
        settings.server().request_timeout().budget(),
        sutura_config::credentials::SourceAdmissions::default().and(
            source("warehouse"),
            Some(BTreeSet::from([sutura_domain::identity::VerifierId::primary()])),
        ),
    );
    let reading = Nodes::worker_reading(working_set, identity(), Some(Arc::new(reader))).unwrap();
    let servers = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let stopping = sutura_runtime::Shutdown::new();
    let listeners = [(); 2].map(|()| std::net::TcpListener::bind("127.0.0.1:0").unwrap());
    let [own_url, reader_url] = listeners
        .each_ref()
        .map(|listener| format!("https://{}", listener.local_addr().unwrap()));
    let nodes = Nodes::declared(own, own_url.parse().unwrap(), [reader_url.parse().unwrap()], identity())
        .reading(source("warehouse"), [reader_url.parse().unwrap()])
        .across_above(NonZeroU64::new(1));
    for (worker, listener) in [nodes.served(), reading.with_call_authorizer(TaskGrant::default())]
        .into_iter()
        .zip(listeners)
    {
        let mount = sutura_http::WorkerMount::new(worker.into_worker_server().max_decoding_message_size(working_set.bytes()));
        let router = servers
            .block_on(async { sutura_http::worker_router(settings, &mount) })
            .unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let tls = tls.clone();
        let shutdown = stopping.clone();
        drop(servers.spawn(async move { sutura_http::serve_tls(router, address, shutdown, &tls).await }));
        assert!(
            (0..250).any(|_| {
                std::thread::sleep(Duration::from_millis(20));
                std::net::TcpStream::connect(address).is_ok()
            }),
            "fixture worker never listened"
        );
    }
    (servers, stopping, nodes)
}

#[expect(clippy::create_dir, reason = "the fixture requires a fresh temporary directory")]
fn local_fact(plan: &FederatedPlan) -> (sutura_config::Settings, Scratch, String, ResultBatches) {
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development)
            .with_overlay("runtime:\n  working_set_max_bytes: 1073741824\n"),
    )
    .unwrap();
    let working_set = crate::sources::working_set(settings.runtime());
    let room = Scratch(std::env::temp_dir().join(format!(
        "sutura-native-forwarded-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
    )));
    std::fs::create_dir(&room.0).unwrap();
    let data = room.0.join("facts.csv");
    let fact_id = fixture_id();
    std::fs::write(&data, format!("id,amount,day\n{fact_id},7,2026-06-01\n")).unwrap();
    let declared = SharedIdentityDeclared::of(AcknowledgementReason::parse("the fixture reads its one local file").unwrap());
    let files = DataFusionWarehouse::new(
        source("facts"),
        SourcePosture::SharedServiceUser {
            declared: declared.clone(),
        },
        working_set,
    )
    .unwrap();
    files.attach_csv(&TableName::parse("facts").unwrap(), &data).unwrap();
    let fact = files
        .execute(
            Executable::Leg(plan.fact()),
            &Presented::SharedServiceUser { declared },
            deadline(),
        )
        .unwrap();
    (settings, room, fact_id, fact)
}

fn native_credentials(subject: &Subject) -> BoundToTheRequest {
    let assertion = Secret::new(live("SUTURA_BQ_ASSERTION_A"));
    let presented = Presented::SubjectToken { material: assertion };
    let sources = SourceSet::of(source("warehouse"));
    let minted = LegCredentials::minted(
        subject.clone(),
        Expiry::NothingExpires,
        &sources,
        BTreeMap::from([(source("warehouse"), presented)]),
    )
    .unwrap();
    let Ok(Agreed::Granted { credentials }) = (Minted::Granted { credentials: minted }).agreeing_with(subject, &sources, 0)
    else {
        panic!("fixture credentials disagree with their source");
    };
    credentials
}

fn native_split<'w>(
    warehouse: &'w crate::serve::kind::AnyWarehouse,
    lookup: &LegPlan,
    credentials: &BoundToTheRequest,
    until: SystemTime,
    labels: &JobLabels,
) -> Partitioned<'w, crate::serve::kind::AnyWarehouseError> {
    warehouse
        .split_lookup(
            lookup,
            credentials.presented_for(lookup.source()).unwrap(),
            deadline(),
            NonZeroU64::new(1).unwrap(),
            NonZeroUsize::new(4).unwrap(),
            Some(Grantee::new(lookup, credentials, until).unwrap()),
            labels,
        )
        .unwrap_or_else(|_| panic!("the native BigQuery split dispatch did not run"))
}

fn sql_receipts(records: &[Execution]) -> Vec<&Execution> {
    records
        .iter()
        .filter(|record| matches!(record, Execution::Sql { .. }))
        .collect()
}

#[test]
#[ignore = "needs the existing CI pool assertions, native BigQuery driver and split table"]
fn forwarded_bigquery_native_read_keeps_its_descriptor_receipt() {
    let (pinned, plan) = plan_for("SUTURA_BQ_SPLIT_TABLE");
    let (settings, room, fact_id, fact) = local_fact(&plan);
    assert_eq!(fact.rows(), 1);

    let subject = Subject::verified("user@example.com").unwrap();
    let lookup = plan.lookups().legs().first().unwrap().leg();
    let labels = JobLabels::of(&RequestContext::of(PrincipalChain::of(subject.clone())));
    let until = SystemTime::now() + Duration::from_secs(600);
    let credentials = native_credentials(&subject);
    let warehouse = crate::serve::kind::AnyWarehouse::BigQuery(Box::new(opened()));
    let split = native_split(&warehouse, lookup, &credentials, until, &labels);
    let Partitioned::Granted(grant) = split else {
        panic!("the native fixture must actually take the multiple-stream Granted branch");
    };
    assert!(
        grant.partitions().len() >= 2,
        "fixture did not produce multiple real native streams"
    );
    let mut expected: Vec<[u8; 32]> = grant
        .partitions()
        .iter()
        .map(|descriptor| sha2::Sha256::digest(descriptor).into())
        .collect();
    let expected_sql = sutura_sql::generate_leg(lookup, sutura_sql::Dialect::BigQuery).unwrap();

    let (servers, stopping, nodes) = listening(&settings, pinned, &room);
    let combiner = DataFusionCombiner::across(nodes).unwrap();
    let actual = Actual {
        fact: Mutex::new(Some(fact)),
        grant: Mutex::new(Some(grant)),
    };
    let mut answer = combiner
        .execute(combiner.plan(&plan), &actual, 1 << 30)
        .unwrap_or_else(|cause| {
            assert!(
                matches!(
                    cause,
                    sutura_domain::plan::ExecuteError::Combine(sutura_exec_datafusion::CombineError::Execute { cause })
                        if cause.to_string().contains("successful source read supplied no submission facts")
                ),
                "forwarded native CLI fixture failed without its native-success witness"
            );
            panic!("forwarded native CLI receipt barrier: successful source read supplied no submission facts");
        });
    assert_eq!(answer.rows(), 1);
    let rows = answer.to_rows().unwrap();
    let id = rows.columns().iter().position(|label| label == "id").unwrap();
    let measured = rows.columns().iter().position(|label| label == "native_reads").unwrap();
    assert_eq!(rows.cell(0, id), Some(&sutura_domain::warehouse::Value::Text(fact_id)));
    assert_eq!(rows.cell(0, measured).unwrap().render(), "7");
    let records = answer.take_executions();
    let sqls = sql_receipts(&records);
    assert_eq!(
        sqls.len(),
        1,
        "the forwarded answer lost the BigQuery split creation SQL receipt"
    );
    let Execution::Sql {
        source: at,
        dialect,
        sql,
        parameters,
    } = sqls[0]
    else {
        panic!("the SQL-only receipt census contains a SQL receipt")
    };
    assert_eq!(at, &source("warehouse"));
    assert_eq!(*dialect, TypeDialect::BigQuery);
    assert_eq!(
        sql,
        expected_sql.sql(),
        "BigQuery split creation receipt differs from the submitted lookup SQL"
    );
    assert!(parameters.is_empty(), "the unfiltered lookup has no bindings");
    assert_eq!(
        records.len(),
        expected.len() + 3,
        "the complete native answer receipt census differs"
    );
    let mut hashes = Vec::new();
    let (mut files_count, mut combine_count) = (0, 0);
    for record in &records {
        match record {
            Execution::BigQueryRead {
                source: at,
                descriptor_sha256,
            } => {
                assert_eq!(at, &source("warehouse"));
                hashes.push(*descriptor_sha256);
            }
            Execution::Sql { .. } => {}
            Execution::DataFusion {
                source: Some(at),
                stage: PlanStage::Files,
                ..
            } => {
                assert_eq!(at, &source("facts"));
                files_count += 1;
            }
            Execution::DataFusion {
                source: None,
                stage: PlanStage::Combine,
                ..
            } => combine_count += 1,
            _ => panic!("the native answer contains an unclassified receipt"),
        }
    }
    assert_eq!(
        (files_count, combine_count),
        (1, 1),
        "files and final combine are accounted for once"
    );
    hashes.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        hashes, expected,
        "native CLI receipts did not match the actual split descriptors"
    );
    stopping.trigger(sutura_runtime::ShutdownReason::Requested);
    servers.shutdown_timeout(Duration::from_secs(5));

    // The existing one-row fixture takes the local branch without unsupported bound parameters.
    let (_, local_plan) = plan_for("SUTURA_BQ_A_ONLY_TABLE");
    let local_lookup = local_plan.lookups().legs().first().unwrap().leg();
    let local_sql = sutura_sql::generate_leg(local_lookup, sutura_sql::Dialect::BigQuery).unwrap();
    let local = native_split(&warehouse, local_lookup, &credentials, until, &labels);
    let Partitioned::Split(reads) = local else {
        panic!("no <=1-stream credit: the one-row native query did not actually take Partitioned::Split");
    };
    let mut local_records = Vec::new();
    let mut local_rows = 0;
    for read in reads {
        let mut read = read.unwrap_or_else(|_| panic!("the local native stream did not read"));
        local_rows += read.rows();
        local_records.extend(read.take_executions());
    }
    assert_eq!(local_rows, 1, "the local fixture must return its one real row");
    let creation = sql_receipts(&local_records);
    assert_eq!(
        creation.len(),
        1,
        "the <=1-stream branch lost its BigQuery creation SQL receipt"
    );
    assert_eq!(
        creation[0],
        &Execution::Sql {
            source: source("warehouse"),
            dialect: TypeDialect::BigQuery,
            sql: String::from(local_sql.sql()),
            parameters: vec![],
        },
        "the <=1-stream creation SQL or empty bindings differ"
    );
    let native_reads: Vec<_> = local_records
        .iter()
        .filter(|record| {
            matches!(record,
        Execution::BigQueryRead { source: at, .. } if *at == source("warehouse"))
        })
        .collect();
    assert_eq!(native_reads.len(), 1, "the local branch must read its one real native stream");
    assert_eq!(
        local_records.len(),
        1 + native_reads.len(),
        "no local-branch receipt is unclassified"
    );
}

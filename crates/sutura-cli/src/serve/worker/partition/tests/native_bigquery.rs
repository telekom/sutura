//! A real forwarded-token native read through the CLI reader and worker receipt barrier.
//! No HTTP caller or delegation hop participates; assertions are the existing live pool fixture's.
use core::convert::Infallible;
use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
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
    Agreed, Expiry, LegCredentials, Minted, Presented, PrincipalChain, RequestContext, Secret, SourceSet, Subject,
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
    LinkCast, Looked, LookupLegs, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel, StatementTables,
    range_filters,
};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::execution::Execution;
use sutura_domain::warehouse::labels::JobLabels;
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

/// One lookup over the existing split table, certified by the same pin the actual reader holds.
fn plan() -> (PinnedDefinitions, FederatedPlan) {
    let table =
        QualifiedTable::parse(live("SUTURA_BQ_SPLIT_TABLE")).unwrap_or_else(|_| panic!("fixture table declaration is invalid"));
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

struct Actual {
    fact: Mutex<Option<ResultBatches>>,
    grant: Mutex<Option<Granted<'static>>>,
}

impl LegRunner for Actual {
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
    );
    let reading = Nodes::worker_reading(working_set, identity(), Some(Arc::new(reader))).unwrap();
    let servers = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let stopping = sutura_runtime::Shutdown::new();
    let mut urls = Vec::new();
    for worker in [own.clone(), reading] {
        let mount = sutura_http::WorkerMount::new(
            worker
                .with_call_authorizer(TaskGrant::default())
                .into_worker_server()
                .max_decoding_message_size(working_set.bytes()),
        );
        let router = servers
            .block_on(async { sutura_http::worker_router(settings, &mount) })
            .unwrap();
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .unwrap();
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
        urls.push(format!("https://{address}"));
    }
    let [own_url, reader_url]: [_; 2] = urls.try_into().unwrap();
    let nodes = Nodes::declared(own, own_url.parse().unwrap(), [reader_url.parse().unwrap()], identity())
        .reading(source("warehouse"), [reader_url.parse().unwrap()])
        .across_above(NonZeroU64::new(1));
    (servers, stopping, nodes)
}

#[test]
#[expect(clippy::create_dir, reason = "the fixture requires a fresh temporary directory")]
#[ignore = "needs the existing CI pool assertions, native BigQuery driver and split table"]
fn forwarded_bigquery_native_read_keeps_its_descriptor_receipt() {
    let (pinned, plan) = plan();
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
    assert_eq!(fact.rows(), 1);

    let subject = Subject::verified("user@example.com").unwrap();
    let assertion = Secret::new(live("SUTURA_BQ_ASSERTION_A"));
    let presented = Presented::SubjectToken { material: assertion };
    let lookup = plan.lookups().legs().first().unwrap().leg();
    let labels = JobLabels::of(&RequestContext::of(PrincipalChain::of(subject.clone())));
    let until = SystemTime::now() + Duration::from_secs(600);
    let split = opened()
        .split_leg(lookup, &presented, deadline(), NonZeroUsize::new(4).unwrap(), until, &labels)
        .unwrap_or_else(|_| panic!("the real BigQuery split did not run"));
    let schema = Arc::clone(split.schema());
    let ends = split.until();
    let (descriptors, token) = split.into_parts();
    assert!(descriptors.len() >= 2, "fixture did not produce multiple real native streams");
    let mut expected: Vec<[u8; 32]> = descriptors
        .iter()
        .map(|descriptor| sha2::Sha256::digest(descriptor).into())
        .collect();
    let sources = SourceSet::of(source("warehouse"));
    let minted = LegCredentials::minted(
        subject.clone(),
        Expiry::NothingExpires,
        &sources,
        BTreeMap::from([(source("warehouse"), presented)]),
    )
    .unwrap();
    let Ok(Agreed::Granted { credentials }) = (Minted::Granted { credentials: minted }).agreeing_with(&subject, &sources, 0)
    else {
        panic!("fixture credentials disagree with their source");
    };
    let grant = Grantee::new(lookup, &credentials, ends)
        .unwrap()
        .forwarding(token)
        .unwrap()
        .grant(
            schema,
            descriptors.into_iter().map(Some),
            NonZeroU32::new(1_000_000).unwrap(),
            EstimatedBytes::parse(32 << 20),
            (),
            SystemTime::now(),
        )
        .unwrap();

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
    let mut hashes = Vec::new();
    for execution in answer.take_executions() {
        if let Execution::BigQueryRead {
            source: actual_source,
            descriptor_sha256,
        } = execution
        {
            assert_eq!(actual_source, source("warehouse"));
            hashes.push(descriptor_sha256);
        }
    }
    hashes.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        hashes, expected,
        "native CLI receipts did not match the actual split descriptors"
    );
    stopping.trigger(sutura_runtime::ShutdownReason::Requested);
    servers.shutdown_timeout(Duration::from_secs(5));
}

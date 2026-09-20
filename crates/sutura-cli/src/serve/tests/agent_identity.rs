//! The agent-route half of the byte join: a `/mcp` tool call, mounted by the composition root's
//! own [`crate::serve::agent::mount`] behind the REAL `sutura_http::capability::establish_asked`
//! and leg 1, answered by the SHIPPED exchanging broker - and the assertion that the
//! `subject_token` that broker offered to an exchange is, byte for byte, the compact JWT leg 1
//! verified.
//!
//! This is the sibling of `sutura_http`'s own
//! `the_shipped_exchanging_broker_exchanges_the_document_leg_one_verified`, but reached from the
//! agent route rather than `/v1/query`: it rides the streamable-HTTP transport's `Asking::PerRequest`
//! path, where each call's `Asked` comes out of `sutura_http::capability::establish_asked` inserting
//! it into the request's own extensions. The two halves each stay green against their own fixture -
//! `crate::inbound` retains what verified, `WorkloadIdentityBroker` offers an asker's token to an
//! exchange - and the assertion here is the identity of those bytes on the surface a pipe cannot
//! carry. It has to live in this crate, and not in `sutura-http` next to its sibling, because only
//! a composition root links both transports: `sutura-http`'s `agent` feature is deliberately empty
//! (a transport never links another transport), so the mount that combines `establish_asked` behind
//! leg 1 with `sutura_mcp::http::service` exists here and only here (`crate::serve::agent::mount`).
//!
//! Gated on BOTH `agent` and `bigquery`, because that is what the two halves need: the `agent`
//! feature links `sutura_mcp::http` (and `sutura-http`'s `agent`), and the `bigquery` feature links
//! the shipped `WorkloadIdentityBroker` a `bigquery` deployment serves under. `just test` runs
//! `--all-features`, so this cell runs there; a default build has no `/mcp` and no exchanging
//! broker to join.

use std::sync::Arc;

use sutura_config::{Environment, Settings, Sources};
use sutura_dev::issuer::{MockIssuer, PublishedKeySet};
use sutura_domain::identity::{Expiry, Presented, Secret};
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{AnchorRows, PreFlight, RowSet, Value, Warehouse};
use sutura_exec_bigquery::{StsCredential, StsExchange, WorkloadIdentity, WorkloadIdentityBroker};

use super::super::agent::mount as mount_agent_surface;

// ------------------------------------------------------------------- fixtures ----

const ISSUER: &str = "https://issuer.example.com";
const RESOURCE: &str = "https://sutura.example.com";
const KID: &str = "the-current-key";

/// The workload-identity pool an impersonating `bigquery` source declares. Not a real one.
const POOL: &str = "//iam.googleapis.com/projects/000000000000/locations/global/workloadIdentityPools/example/providers/example";

/// The scope that declaration asks for. A published Google scope string, which is public.
const SCOPE: &str = "https://www.googleapis.com/auth/bigquery.readonly";

/// The number the anchor certifies, and the number the answering fake reproduces.
const ANCHORED_VALUE: &str = "197122";

/// The one data system name every fixture here uses - the same `local` the served suite uses.
fn source() -> sutura_domain::model::SourceName {
    sutura_domain::model::SourceName::parse("local").expect("a test source is a source")
}

fn an_issuer() -> MockIssuer {
    MockIssuer::generating(ISSUER, RESOURCE, KID).expect("a mock issuer generates a key pair")
}

/// Every scope this surface has, space-delimited per RFC 6749.
fn every_scope() -> String {
    sutura_app::Capability::every()
        .map(sutura_app::Capability::scope)
        .collect::<Vec<&str>>()
        .join(" ")
}

/// A token this deployment would accept, granting every capability the surface has.
fn accepted_by(subject: &str) -> sutura_dev::issuer::Token {
    sutura_dev::issuer::Token::for_subject(subject).granting(&every_scope())
}

/// A `direct` inbound declaration for `issuer`, reading its key set at `key_set_path`.
fn direct_overlay(issuer: &MockIssuer, key_set_path: &str) -> String {
    format!(
        "security:\n  inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
         authorization_server: \"{}\"\n    key_set_file: \"{key_set_path}\"\n    algorithms: [\"ES256\"]\n",
        issuer.audience(),
        issuer.issuer(),
    )
}

/// A catalog port that hands back a bundle somebody else built - the pass-through
/// `crate::serve::catalog` composes deployments over, so `LocalService::start` validates it.
struct FixedCatalog {
    bundle: sutura_domain::pinned::PinnedDefinitions,
}

/// Never returned by [`FixedCatalog`]; the port requires an error type.
#[derive(Debug, thiserror::Error)]
#[error("a fixed catalog cannot fail")]
struct Infallible;

impl sutura_domain::pinned::SemanticCatalog for FixedCatalog {
    type Error = Infallible;

    const KIND: sutura_domain::pinned::CatalogKind = sutura_domain::pinned::CatalogKind::Golden;

    fn capabilities() -> sutura_domain::capabilities::MetadataCapabilities {
        sutura_domain::capabilities::MetadataCapabilities::everything()
    }

    fn load(&self) -> Result<sutura_domain::pinned::PinnedDefinitions, Self::Error> {
        Ok(self.bundle.clone())
    }
}

fn catalog_of(bundle: sutura_domain::pinned::PinnedDefinitions) -> FixedCatalog {
    FixedCatalog { bundle }
}

/// One model, one anchored metric, one filterable dimension - the smallest bundle that `start`
/// will re-validate an anchor against, so the answer really ran rather than skipped a boot check.
fn bundle() -> sutura_domain::pinned::PinnedDefinitions {
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::catalog::{
        Anchor, AnchorValue, Audience, Definitions, Description, Dimension, DimensionValue, Metric, Model,
    };
    use sutura_domain::knowledge::Knowledge;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, ModelName, TableName};
    use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let description = |raw: &str| Description::parse(raw).expect("a test description is a description");
    let declared_value = |raw: &str| DimensionValue::parse(raw).expect("a test value is a value");
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        std::collections::BTreeSet::from([column("amount_cents"), column("order_date"), column("region")]),
        description("Orders, one row per order."),
    );
    let region = Dimension::new(
        DimensionName::parse("region").expect("a test dimension is a dimension"),
        column("region"),
        None,
        Some(std::collections::BTreeSet::from([
            declared_value("north"),
            declared_value("south"),
        ])),
        description("Sales region."),
    );
    let anchor = Anchor::new(
        TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range"),
        AnchorValue::parse(ANCHORED_VALUE).expect("a test anchor value is a value"),
    );
    let revenue = Metric::new(
        MetricName::parse("revenue").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        std::collections::BTreeSet::from([Grain::Day, Grain::Month]),
        vec![region],
        Some(anchor),
        description("Revenue, in minor units."),
        Audience::Open,
    )
    .expect("one dimension cannot duplicate another");
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            sutura_domain::model::SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(sutura_domain::capabilities::MetadataCapabilities::produced(
                &definitions,
                &Knowledge::none(),
            )),
        ),
    )
    .expect("the test definitions hash")
}

/// The fixture exchange's own defect, which nothing here provokes.
#[derive(Debug, thiserror::Error)]
#[error("the fixture exchange in `serve::tests::agent_identity` cannot fail, and did")]
struct NoFixtureExchangeFailure;

/// One call the broker made to the exchange, recorded as it was made.
///
/// The `subject_token` field is the seam this module exists to close, so it is named rather than
/// positional.
struct Exchanged {
    audience: String,
    scope: String,
    subject_token: String,
}

/// An RFC 8693 exchange that records what it was asked, and answers with a credential naming it.
///
/// A real implementor of `sutura_exec_bigquery::StsExchange`, so the shipped broker's real code
/// path runs - a fixture broker of this crate's own would be asserting on itself. Records through
/// an unbounded channel rather than a lock: `std::sync::Mutex` is banned here and the port method
/// is synchronous, the same argument `sutura_http`'s own fake gives.
struct EchoesWhatItWasAskedToExchange {
    asked: tokio::sync::mpsc::UnboundedSender<Exchanged>,
}

impl StsExchange for EchoesWhatItWasAskedToExchange {
    type Error = NoFixtureExchangeFailure;

    #[expect(
        clippy::disallowed_methods,
        reason = "reading the subject token IS the assertion: that the document leg 1 verified is what \
                  the shipped broker offered to an exchange on the agent route"
    )]
    fn exchange(&self, audience: &str, scope: &str, subject_token: &Secret) -> Result<StsCredential, Self::Error> {
        let offered = String::from(subject_token.expose_secret());
        drop(self.asked.send(Exchanged {
            audience: String::from(audience),
            scope: String::from(scope),
            subject_token: offered.clone(),
        }));
        Ok(StsCredential::of(
            Secret::new(format!("sts-token-for/{offered}")),
            Expiry::At {
                unix_seconds: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs())
                    .saturating_add(3_600),
            },
        ))
    }
}

fn an_exchange() -> (
    EchoesWhatItWasAskedToExchange,
    tokio::sync::mpsc::UnboundedReceiver<Exchanged>,
) {
    let (asked, received) = tokio::sync::mpsc::unbounded_channel();
    (EchoesWhatItWasAskedToExchange { asked }, received)
}

/// A data system that can carry a per-subject credential, and answers every question it is asked.
///
/// `PerSubjectCredential` and `ImpersonationAtSource`, which is what makes it the right fake here:
/// it is the only fixture in this crate that agrees with a `SubjectToken` the exchanging broker
/// mints, so the byte join can observe that credential travelling. `verify_anchor` reproduces the
/// number the bundle's anchor certifies, so `LocalService::start` validates.
struct PersonaWarehouse {
    source: sutura_domain::model::SourceName,
    posture: SourcePosture,
    result: RowSet,
}

/// The one way this fake fails: it was handed a credential that disagrees with its own posture.
#[derive(Debug, thiserror::Error)]
#[error("this fake was handed a credential that does not agree with its posture")]
struct Disagreed {
    #[source]
    cause: sutura_domain::identity::PresentedDisagreesWithPosture,
}

impl Warehouse for PersonaWarehouse {
    type Error = Disagreed;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;

    fn source(&self) -> &sutura_domain::model::SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }

    fn execute(&self, _executable: Executable<'_>, presented: &Presented, _deadline: Deadline) -> Result<RowSet, Self::Error> {
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| Disagreed { cause })?;
        Ok(self.result.clone())
    }
}

// --------------------------------------------------------------------- the cell ----

/// The exchange the settled plan asks for, as the mounted `/mcp` transport frames it.
fn ask_metric_call() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "ask_metric",
            "arguments": {
                "metric": "revenue",
                "grain": "month",
                "range": { "start": "2026-06-01", "end": "2026-07-01" }
            }
        }
    })
}

fn initialize(id: i64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "sutura-serve-agent-identity-test", "version": "0.0.0"}
        }
    })
}

/// One JSON-RPC POST to `/mcp`, carrying `token`, over the composed router.
async fn post(app: axum::Router, token: &str, body: serde_json::Value) -> serde_json::Value {
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt as _;

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(axum::http::header::HOST, "localhost")
        .header(axum::http::header::ACCEPT, "application/json, text/event-stream")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            serde_json::to_vec(&body).expect("a test JSON-RPC body serializes"),
        ))
        .expect("a well-formed request builds");
    let response = app.oneshot(request).await.expect("a tower service's Error is Infallible");
    assert_eq!(response.status(), axum::http::StatusCode::OK, "the agent route answers 200");
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the response body reads");
    serde_json::from_slice(&bytes).unwrap_or_else(|cause| panic!("the response body is JSON: {cause}"))
}

#[tokio::test]
async fn the_shipped_exchanging_broker_exchanges_the_document_the_agent_route_verified() {
    // **The agent-route join, and the assertion neither half could make alone.** `establish_asked`
    // retains the token leg 1 verified and `WorkloadIdentityBroker` offers an asker's token to an
    // exchange - each against its own fixture, each green whatever the other did with the bytes.
    // What is asserted here is the identity of those bytes on the mounted `/mcp` surface, through
    // the composition root's own `mount` and the real `Asking::PerRequest` transport: an
    // `establish_asked` that retained a MANGLED assertion (a scheme still on it, or a caller's
    // NAME, or the process's own identity) passes every other agent-surface cell and fails here.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "agent-byte-join").expect("the key set publishes");
    let overlay = direct_overlay(&issuer, &published.path().to_string_lossy());
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay))
        .expect("the agent-route overlay loads");

    let (exchange, mut asked) = an_exchange();
    let broker = WorkloadIdentityBroker::empty(exchange)
        .impersonating(source(), WorkloadIdentity::of(String::from(POOL), String::from(SCOPE)));
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    let warehouses = sutura_app::Warehouses::of(PersonaWarehouse {
        source: source(),
        posture: SourcePosture::ImpersonationAtSource,
        result,
    });

    let service: Arc<dyn sutura_app::surface::Surface> = Arc::new(
        sutura_app::surface::LocalService::start(
            &catalog_of(bundle()),
            warehouses,
            sutura_runtime::TracingAuditSink::new(),
            broker,
            1 << 30,
        )
        .expect("the test bundle validates: the anchor path takes no credential"),
    );

    let admission = sutura_runtime::Admission::from_settings(settings.runtime());
    // The composition root's own mount: `sutura_mcp::http::service` (always `Asking::PerRequest`)
    // wrapped in the erased-surface `Serving` that `sutura serve` serves over.
    // No `governance.per_replica_spend_ceiling` in this overlay, so there is no
    // `sutura_spend_headroom_bytes` series to push onto and the mount says so.
    let mount = mount_agent_surface(
        Arc::clone(&service),
        &settings,
        admission.clone(),
        sutura_http::SpendHeadroomPush::NoCeilingConfigured,
    )
    .expect("the agent mount builds");
    let gate = sutura_http::InboundGate::from_declaration(
        &settings
            .security()
            .inbound()
            .expect("this overlay declares an inbound identity")
            .clone(),
    )
    .expect("a published key set builds a gate");
    let state = sutura_http::ServiceState::new(service, Arc::new(settings), admission)
        .with_inbound_identity(Arc::new(gate))
        .with_agent_surface(mount);
    let app = sutura_http::router(&state).expect("the test router assembles");

    let token = issuer
        .mint(&accepted_by("ada@example.com"))
        .expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);
    let answered = post(app, &token, ask_metric_call()).await;
    assert!(
        answered.get("error").is_none(),
        "the ask_metric call must be answered, not refused: {answered}"
    );

    let call = asked
        .try_recv()
        .expect("the agent surface performed an exchange through the shipped broker");
    // The seam, and it is on the AGENT route this time: a transport that retained the header's whole
    // value, or trimmed the token, or handed over the subject's NAME, passes every other agent-surface
    // cell and fails here.
    assert_eq!(
        call.subject_token, token,
        "the document exchanged on the agent route is not the one leg 1 verified"
    );
    assert_eq!(call.audience, POOL, "the pool the exchange was asked for");
    assert_eq!(call.scope, SCOPE, "the scope the exchange was asked for");
    assert!(asked.try_recv().is_err(), "one question over one source is one exchange");
}

/// Claim-Cell: `two_callers_over_the_agent_route_offer_two_distinct_subject_tokens_to_the_exchange`.
///
/// Pins behaviour the base tree already provides - the broker already offers each caller's own
/// token, unchanged by this cell - so it needs the declared trailer and its killing mutation at
/// `devco/claim-mutations/two_callers_over_the_agent_route_offer_two_distinct_subject_tokens_to_the_exchange.patch`.
///
/// The gap this closes: `two_callers_over_one_connection_are_two_different_askers`
/// (`crates/sutura-mcp/src/server/tests/asking.rs`) proves asker distinctness at the PORT - it
/// records the subject each context the port receives. It says nothing about the SUBJECT TOKENS
/// offered to the exchange: a broker that resolved each caller correctly and then offered a cached
/// or process-owned token would still pass it. This cell reaches the exchange itself, over the SAME
/// mounted `/mcp` surface the one-caller cell above joins, with two different bearer tokens.
#[tokio::test]
async fn two_callers_over_the_agent_route_offer_two_distinct_subject_tokens_to_the_exchange() {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "agent-byte-join-two-callers").expect("the key set publishes");
    let overlay = direct_overlay(&issuer, &published.path().to_string_lossy());
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay))
        .expect("the agent-route overlay loads");

    let (exchange, mut asked) = an_exchange();
    let broker = WorkloadIdentityBroker::empty(exchange)
        .impersonating(source(), WorkloadIdentity::of(String::from(POOL), String::from(SCOPE)));
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    let warehouses = sutura_app::Warehouses::of(PersonaWarehouse {
        source: source(),
        posture: SourcePosture::ImpersonationAtSource,
        result,
    });

    let service: Arc<dyn sutura_app::surface::Surface> = Arc::new(
        sutura_app::surface::LocalService::start(
            &catalog_of(bundle()),
            warehouses,
            sutura_runtime::TracingAuditSink::new(),
            broker,
            1 << 30,
        )
        .expect("the test bundle validates: the anchor path takes no credential"),
    );

    let admission = sutura_runtime::Admission::from_settings(settings.runtime());
    // No `governance.per_replica_spend_ceiling` in this overlay, so there is no
    // `sutura_spend_headroom_bytes` series to push onto and the mount says so.
    let mount = mount_agent_surface(
        Arc::clone(&service),
        &settings,
        admission.clone(),
        sutura_http::SpendHeadroomPush::NoCeilingConfigured,
    )
    .expect("the agent mount builds");
    let gate = sutura_http::InboundGate::from_declaration(
        &settings
            .security()
            .inbound()
            .expect("this overlay declares an inbound identity")
            .clone(),
    )
    .expect("a published key set builds a gate");
    let state = sutura_http::ServiceState::new(service, Arc::new(settings), admission)
        .with_inbound_identity(Arc::new(gate))
        .with_agent_surface(mount);
    let app = sutura_http::router(&state).expect("the test router assembles");

    // Two callers, two tokens the same issuer signs for two different subjects - the only thing
    // that varies between them.
    let ada = issuer
        .mint(&accepted_by("ada@example.com"))
        .expect("the issuer signs ada's token");
    let grace = issuer
        .mint(&accepted_by("grace@example.com"))
        .expect("the issuer signs grace's token");

    drop(post(app.clone(), &ada, initialize(1)).await);
    let ada_answered = post(app.clone(), &ada, ask_metric_call()).await;
    assert!(
        ada_answered.get("error").is_none(),
        "ada's ask_metric call must be answered, not refused: {ada_answered}"
    );

    drop(post(app.clone(), &grace, initialize(1)).await);
    let grace_answered = post(app, &grace, ask_metric_call()).await;
    assert!(
        grace_answered.get("error").is_none(),
        "grace's ask_metric call must be answered, not refused: {grace_answered}"
    );

    let first = asked
        .try_recv()
        .expect("ada's ask_metric call performed an exchange through the shipped broker");
    let second = asked
        .try_recv()
        .expect("grace's ask_metric call performed an exchange through the shipped broker");
    assert!(
        asked.try_recv().is_err(),
        "two questions over one source is two exchanges, not three"
    );

    // The seam this cell exists to close: two callers over the agent route must offer two
    // DISTINCT subject tokens - a broker that resolved each caller and then offered a cached or
    // process-owned token would still pass the port-level distinctness cell, and fail only here.
    assert_ne!(
        first.subject_token, second.subject_token,
        "two callers over the agent route must not offer the same subject token to the exchange"
    );
    assert_eq!(
        first.subject_token, ada,
        "ada's own token must be the one offered on her behalf"
    );
    assert_eq!(
        second.subject_token, grace,
        "grace's own token must be the one offered on her behalf"
    );
}

// ----------------------------------------------------------------- spend headroom over /mcp ----

/// What every dry run a [`PricedWarehouse`] answers is priced at - so a spend ceiling can be
/// drained by real answered calls, the same fixture `sutura-http`'s own spend cell uses.
const PRICE_BYTES: u64 = 500;

/// A data system whose dry run reports a real byte price. The served surface never reaches a real
/// priced adapter (`bigquery` needs a real project), so this fake lets
/// `governance.per_replica_spend_ceiling` move the ledger through the REAL `/mcp` transport.
struct PricedWarehouse {
    source: sutura_domain::model::SourceName,
    posture: SourcePosture,
    result: RowSet,
}

impl Warehouse for PricedWarehouse {
    type Error = std::convert::Infallible;
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;
    const PRICES_DRY_RUN: bool = true;

    fn source(&self) -> &sutura_domain::model::SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::Accepted {
            estimated_bytes: Some(EstimatedBytes::parse(PRICE_BYTES)),
        })
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented, _deadline: Deadline) -> Result<RowSet, Self::Error> {
        Ok(self.result.clone())
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

/// `direct_overlay`'s twin that also configures a per-replica spend ceiling, so the served surface
/// reports a real headroom and `ServiceState` registers the spend gauge. Also turns on
/// `server.agent_surface.enabled`, so the composition root's own `agent_mount(&state)` builds the
/// mount rather than the test hand-wiring one - the cell exercises the real seam.
fn priced_overlay(issuer: &MockIssuer, key_set_path: &str) -> String {
    format!(
        "{}\nserver:\n  agent_surface:\n    enabled: true\ngovernance:\n  per_replica_spend_ceiling:\n    bytes: 1000\n    window_seconds: 3600\n",
        direct_overlay(issuer, key_set_path)
    )
}

// The gauge must move when a served AGENT surface answers - `telekom/sutura#892`. `/mcp` answers
// through the same `Surface` and charges the same ledger as the HTTP query route, so an
// agent-only deployment reading the full ceiling while the ledger drains is the stale-gauge lie
// this closes. Mirrors `sutura_http::harness::metrics`'s HTTP-path cell, but reached from the
// composition root's own `agent_mount(&state)` helper - NOT a hand-wired `agent::mount` call - so
// the cell exercises the real seam: `agent_mount` reads `state.spend_headroom_gauge()` and hands
// it into the `Serving` wrapper, which is what makes both surfaces drive one series.

/// A priced, agent-enabled served surface over the REAL `/mcp` transport, built through the
/// composition root's own `agent_mount(&state)` helper. Both spend-headroom cells share this
/// setup: it builds the state, hands the state's own gauge into the agent mount, and returns the
/// router, the gauge handle, and the issuer (for minting a caller's token).
///
/// `key_set_id` distinguishes each cell's `PublishedKeySet` so two cells writing to the same
/// temporary directory do not collide.
struct PricedAgentSurface {
    app: axum::Router,
    gauge: sutura_runtime::Gauge,
    issuer: MockIssuer,
}

/// The priced, agent-enabled state and its leg-1 gate, before any mount is attached.
///
/// Split out of [`priced_agent_surface`] so a cell can attach a mount this state's own
/// `SpendHeadroomPush::of` did NOT produce - the mistake `AgentMount::new`'s required declaration
/// narrows to one nameable case and `agent_subtree` then refuses.
struct PricedState {
    state: sutura_http::ServiceState,
    gate: sutura_http::InboundGate,
    issuer: MockIssuer,
}

/// Builds the priced agent surface and returns the router, gauge, and issuer.
fn priced_agent_surface(key_set_id: &str) -> PricedAgentSurface {
    let PricedState { state, gate, issuer } = priced_state(key_set_id);
    // The state owns the gauge; the composition root's own `agent_mount(&state)` hands a handle to
    // the SAME gauge into the agent mount, so both surfaces drive one
    // `sutura_spend_headroom_bytes` series. Going through `agent_mount` rather than hand-wiring
    // `agent::mount` is the seam both cells below exist to exercise: a mutation that declared
    // `NoCeilingConfigured` instead of `SpendHeadroomPush::of(&state)` would build a mount whose
    // `Serving` wrapper never pushes - and `an_agent_mount_declaring_no_ceiling_on_a_priced_state_\
    // does_not_assemble` refuses it outright, while these two catch a push that stopped.
    let gauge = sutura_http::SpendHeadroomPush::of(&state)
        .gauge()
        .expect("the priced surface reports headroom at boot")
        .clone();
    let mount = super::super::agent_mount(&state)
        .expect("the agent mount builds")
        .expect("the priced overlay enabled the agent surface");
    let state = state.with_inbound_identity(Arc::new(gate)).with_agent_surface(mount);
    let app = sutura_http::router(&state).expect("the test router assembles");
    PricedAgentSurface { app, gauge, issuer }
}

/// A mount declaring there is no ceiling, attached to a state that registered the gauge, is refused
/// at assembly - `AgentMount`'s required declaration plus the parity check behind it.
///
/// **`SpendHeadroomPush` alone cannot hold this half.** It makes the handle unforgeable and makes
/// the absence a name a caller has to write, but `NoCeilingConfigured` still typechecks on a priced
/// deployment - and that deployment would serve `/mcp` with `sutura_spend_headroom_bytes` frozen at
/// its boot reading while the agent surface drained the ledger. Built through the composition root's
/// own `agent::mount`, so what is refused is the real wiring mistake rather than a hand-made state.
#[test]
fn an_agent_mount_declaring_no_ceiling_on_a_priced_state_does_not_assemble() {
    let PricedState { state, gate, .. } = priced_state("agent-spend-mismatch");
    let mount = super::super::agent::mount(
        state.surface(),
        state.settings(),
        state.admission().clone(),
        sutura_http::SpendHeadroomPush::NoCeilingConfigured,
    )
    .expect("the agent mount builds");
    let state = state.with_inbound_identity(Arc::new(gate)).with_agent_surface(mount);

    let refused = sutura_http::router(&state).expect_err("a ceiling-configured state refuses a mount that declares none");
    assert!(
        matches!(
            refused,
            sutura_http::RouterNotBuilt::AgentSurfaceSpendPushMismatched {
                mount_pushes: false,
                state_registered: true
            }
        ),
        "expected the spend-push mismatch refusal, got {refused:?}"
    );
}

/// Everything up to the state: the priced overlay, the ledger, the gate and the issuer.
fn priced_state(key_set_id: &str) -> PricedState {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, key_set_id).expect("the key set publishes");
    let overlay = priced_overlay(&issuer, &published.path().to_string_lossy());
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay))
        .expect("the priced agent-route overlay loads");
    let budget = settings.spend_budget().expect("the priced overlay configured a ceiling");

    let (exchange, _asked) = an_exchange();
    let broker = WorkloadIdentityBroker::empty(exchange)
        .impersonating(source(), WorkloadIdentity::of(String::from(POOL), String::from(SCOPE)));
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    let posture = SourcePosture::ImpersonationAtSource;
    let warehouses = sutura_app::Warehouses::of(PricedWarehouse {
        source: source(),
        posture,
        result,
    });

    let service: Arc<dyn sutura_app::surface::Surface> = Arc::new(
        sutura_app::surface::LocalService::start(
            &catalog_of(bundle()),
            warehouses,
            sutura_runtime::TracingAuditSink::new(),
            broker,
            1 << 30,
        )
        .expect("the priced test bundle validates")
        .with_spend_ledger(sutura_app::SpendLedger::new(Some(sutura_app::SpendBudget::new(
            budget.ceiling_bytes(),
            budget.window(),
        )))),
    );

    let admission = sutura_runtime::Admission::from_settings(settings.runtime());
    let gate = sutura_http::InboundGate::from_declaration(
        &settings
            .security()
            .inbound()
            .expect("this overlay declares an inbound identity")
            .clone(),
    )
    .expect("a published key set builds a gate");
    PricedState {
        state: sutura_http::ServiceState::new(service, Arc::new(settings), admission),
        gate,
        issuer,
    }
}

#[tokio::test]
async fn a_served_agent_surface_pushes_spend_headroom_after_an_answer() {
    let PricedAgentSurface { app, gauge, issuer } = priced_agent_surface("agent-spend");

    let untouched = gauge.value();
    assert_eq!(untouched, 1_000, "the boot reading is the full ceiling");

    let token = issuer
        .mint(&accepted_by("ada@example.com"))
        .expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);
    let answered = post(app, &token, ask_metric_call()).await;
    assert!(
        answered.get("error").is_none(),
        "the ask_metric call must be answered, not refused: {answered}"
    );

    // One priced call drained 500 bytes of headroom; the gauge must read the post-call value, not
    // the boot-time full ceiling. A `Serving` whose answer stopped pushing leaves it at 1000 and
    // reddens this cell.
    assert_eq!(
        gauge.value(),
        500,
        "the /mcp answer pushed the post-call headroom onto the gauge"
    );
}

/// One JSON-RPC `tools/call` for `run_sql`, carrying `token`, over the composed router.
fn run_sql_call(statement: &str, id: i64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "run_sql",
            "arguments": { "statement": statement }
        }
    })
}

/// The `run_sql` push is its own arm of `Serving`: a separate push call after `Surface::run_sql`
/// returns, and a mutation that wrapped it in `if false` left the whole suite green because no cell
/// reached it. This cell does: it drains the ledger with an `ask_metric` call (which the `answer`
/// push catches), tampers the gauge to a stale reading, then makes a `run_sql` call. The `run_sql`
/// call does not charge the ledger, so headroom is unchanged - but the `run_sql` push must still
/// write the CURRENT headroom onto the gauge, correcting the stale reading. A `Serving` whose
/// `run_sql` stopped pushing leaves the tampered value and reddens this cell.
#[tokio::test]
async fn a_served_agent_surface_pushes_spend_headroom_after_a_run_sql_call() {
    let PricedAgentSurface { app, gauge, issuer } = priced_agent_surface("agent-spend-run-sql");

    let token = issuer
        .mint(&accepted_by("ada@example.com"))
        .expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);
    // One priced `ask_metric` call drains 500 bytes; the `answer` push sets the gauge to 500.
    let answered = post(app.clone(), &token, ask_metric_call()).await;
    assert!(
        answered.get("error").is_none(),
        "the ask_metric call must be answered, not refused: {answered}"
    );
    assert_eq!(gauge.value(), 500, "the answer push set the post-call headroom");

    // Tamper the gauge to a stale reading, simulating a gauge that was never pushed after `run_sql`.
    // `run_sql` does not charge the ledger, so the current headroom is still 500 - but the push
    // must still write it. A `Serving` whose `run_sql` arm stopped pushing leaves this stale value.
    gauge.set(999);

    // The `run_sql` call fails - `PricedWarehouse` does not accept raw statements - but the push
    // runs regardless of the outcome, the same way `Serving::answer` pushes whether the call
    // answered, refused or failed.
    // The call is an error (no accepting source), and that is fine: the push is what this cell
    // asserts on, not the outcome.
    let _: serde_json::Value = post(app, &token, run_sql_call("select 1", 2)).await;

    assert_eq!(
        gauge.value(),
        500,
        "the /mcp run_sql push corrected the stale gauge to the current headroom"
    );
}

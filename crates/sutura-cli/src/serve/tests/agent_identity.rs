//! The composed agent route, mounted by the composition root's own `crate::serve::agent_mount`,
//! behind the REAL `sutura_http::capability::establish_asked` and leg 1 - and answered by the broker
//! a served `bigquery` deployment attaches.
//!
//! It rides the streamable-HTTP transport's `Asking::PerRequest` path, where each call's `Asked`
//! comes out of `sutura_http::capability::establish_asked` inserting it into the request's own
//! extensions. It has to live in this crate, and not in `sutura-http`, because only a composition
//! root links both transports: `sutura-http`'s `agent` feature is deliberately empty (a transport
//! never links another transport), so the mount that combines `establish_asked` behind leg 1 with
//! `sutura_mcp::http::service` exists here and only here (`crate::serve::agent`).
//!
//! Gated on BOTH `agent` and `bigquery`: the `agent` feature links `sutura_mcp::http` (and
//! `sutura-http`'s `agent`), and the `bigquery` feature links `DeclaredPrincipalBroker`. `just test`
//! runs `--all-features`, so these cells run there; a default build has no `/mcp` and no
//! per-subject broker.
//!
//! # Two cells lived here that are deleted, and why
//!
//! They were the agent-route half of a BYTE JOIN: that the `subject_token`
//! `sts::WorkloadIdentityBroker` offered to an `StsExchange` was, byte for byte, the compact JWT
//! leg 1 verified. That broker had no implementor a composition root could reach - every
//! `StsExchange` in the tree was a test fake - so the join was between leg 1's real bytes and a
//! fixture, and `docs/adr/0018`'s eighth amendment deleted the broker with its tree. The same
//! property on the path that SHIPS is held inside the adapter, where the asker's assertion is
//! served to the ADBC driver verbatim over a loopback source
//! (`crates/sutura-exec-bigquery/src/adbc/subject.rs`).
//!
//! **What survives here is narrower and says so:** `governance.per_replica_spend_ceiling` moving
//! through the real `/mcp` transport for a verified caller, over the broker `crate::serve` really
//! attaches.

use std::sync::Arc;

use sutura_config::{DeclaredPrincipalBroker, Environment, Settings, Sources};
use sutura_dev::issuer::{MockIssuer, PublishedKeySet};
use sutura_domain::identity::Presented;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, RowSet, Value, Warehouse};

use super::support::{accepted_by, catalog_of, direct_overlay};

// ------------------------------------------------------------------- fixtures ----

const ISSUER: &str = "https://issuer.example.com";
const RESOURCE: &str = "https://sutura.example.com";
const KID: &str = "the-current-key";

/// The one subject these cells ask as.
///
/// A constant rather than two literals: the broker DECLARES this subject and the issuer MINTS for
/// it, and a drift between the two would be refused as `credential_unavailable` rather than
/// answered - which would read as a spend defect.
const ASKING_SUBJECT: &str = "ada@example.com";

/// The number the anchor certifies, and the number the answering fake reproduces.
const ANCHORED_VALUE: &str = "197122";

/// The one data system name every fixture here uses - the same `local` the served suite uses.
fn source() -> sutura_domain::model::SourceName {
    sutura_domain::model::SourceName::parse("local").expect("a test source is a source")
}

fn an_issuer() -> MockIssuer {
    MockIssuer::generating(ISSUER, RESOURCE, KID).expect("a mock issuer generates a key pair")
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
                "metrics": ["revenue"],
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

// ----------------------------------------------------------------- spend headroom over /mcp ----

/// What every dry run a [`PricedWarehouse`] answers is priced at - so a spend ceiling can be
/// drained by real answered calls, the same fixture `sutura-http`'s own spend cell uses.
const PRICE_BYTES: u64 = 500;

/// A fake's canned [`RowSet`] as the port's own Arrow currency.
///
/// `arrow::of_row_set`'s only failure is a row set whose width invariant is broken, and
/// `RowSet::new` refuses one before it exists - so a fake built from a literal cannot reach it.
/// Outside the `execute` that calls it, because `clippy::unwrap_in_result` is denied and an
/// unreachable arm threaded through a canned answer says less than this sentence does.
fn canned(rows: &RowSet) -> ResultBatches {
    sutura_domain::warehouse::arrow::of_row_set(rows).expect("a row set's width invariant is the only failure this has")
}

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

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Ok(canned(&self.result))
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

/// `direct_overlay`'s twin that also configures a per-replica spend ceiling, so the served surface
/// reports a real headroom and `ServiceState` registers the spend gauge. Also turns on
/// `server.agent_surface.enabled`, so the composition root's own `agent_mount(&state)` builds the
/// mount rather than the test hand-wiring one - the cell exercises the real seam. And turns on
/// `tools.run_sql.enabled`, so a `run_sql` call reaches `Serving::run_sql` rather than the switch.
fn priced_overlay(issuer: &MockIssuer, key_set_path: &str) -> String {
    format!(
        "{}\nserver:\n  agent_surface:\n    enabled: true\ngovernance:\n  per_replica_spend_ceiling:\n    bytes: 1000\n    window_seconds: 3600\ntools:\n  run_sql:\n    enabled: true\n",
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
    /// The state's own metrics registry - the same one the router's `/metrics` route renders, and
    /// what `sutura_spend_bytes_total` is read off: the gauge handle alone cannot see the counter,
    /// because the declaration carries only the gauge out and the state is consumed in here.
    registry: Arc<sutura_runtime::metrics::Registry>,
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
    let registry = Arc::clone(&state.registry());
    let state = state.with_inbound_identity(Arc::new(gate)).with_agent_surface(mount);
    let app = sutura_http::router(&state).expect("the test router assembles");
    PricedAgentSurface {
        app,
        gauge,
        registry,
        issuer,
    }
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

    // **The broker `crate::serve` really attaches**, declaring the one subject these cells ask as -
    // so the spend the cells below measure is spend a broker admitted for that subject.
    //
    let broker = DeclaredPrincipalBroker::empty().federating(source(), None);
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
            sutura_domain::plan::RefusingCombiner,
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
    let PricedAgentSurface { app, gauge, issuer, .. } = priced_agent_surface("agent-spend");

    let untouched = gauge.value();
    assert_eq!(untouched, 1_000, "the boot reading is the full ceiling");

    let token = issuer.mint(&accepted_by(ASKING_SUBJECT)).expect("the issuer signs a token");
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
    let PricedAgentSurface { app, gauge, issuer, .. } = priced_agent_surface("agent-spend-run-sql");

    let token = issuer.mint(&accepted_by(ASKING_SUBJECT)).expect("the issuer signs a token");
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

/// The running `sutura_spend_bytes_total` counter must move when a served AGENT surface answers -
/// the counter half of the seam the gauge cells above exercise. The agent surface answers through
/// the same `Surface` and charges the same ledger as `POST /v1/query`, so `Serving`'s post-answer
/// push has to raise the counter to the ledger's fresh reading too, or it stays frozen at its boot
/// zero while the ledger drains. Reached through the composition root's own `agent_mount(&state)`
/// helper, the same real seam the headroom cells ride, and asserted on the RENDERED exposition the
/// `/metrics` route serves, not on a registry-internal handle - a push onto a differently-named
/// series no scrape would show reddens this cell like a stopped one.
///
/// Red against a tree with no series and no push. It makes TWO priced calls on the same fixture
/// and asserts 500 then 1000, which is what holds `raise_to` at this site: with only ONE priced
/// call from the zero boot reading, `0 + 500 == max(0, 500)` and an `add`-shaped push would read
/// the same 500 this cell asserts, indistinguishable from `raise_to`. Two calls make an
/// `add`-shaped double-counting push read 1500 and redden it, because the reading the push
/// carries already sums every byte admitted so far.
#[tokio::test]
async fn a_served_agent_surface_raises_spend_bytes_total_after_an_answer() {
    fn counter(exposition: &str) -> u64 {
        exposition
            .lines()
            .find_map(|line| line.strip_prefix("sutura_spend_bytes_total "))
            .unwrap_or_else(|| panic!("the exposition has no sutura_spend_bytes_total sample: {exposition}"))
            .parse()
            .expect("the spend total is an integer sample")
    }

    let PricedAgentSurface {
        app, registry, issuer, ..
    } = priced_agent_surface("agent-spend-total");

    let token = issuer.mint(&accepted_by(ASKING_SUBJECT)).expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);

    // Boot reading: zero - the untouched total genuinely is zero, unlike the headroom gauge, whose
    // boot reading is the ceiling.
    assert_eq!(counter(&registry.render()), 0, "{}", registry.render());

    // One priced `ask_metric` call admits `PRICE_BYTES`; the push must raise the counter to that
    // reading. A `Serving` whose answer stopped pushing leaves it at zero and reddens this cell.
    let answered = post(app.clone(), &token, ask_metric_call()).await;
    assert!(
        answered.get("error").is_none(),
        "the ask_metric call must be answered, not refused: {answered}"
    );
    assert_eq!(
        counter(&registry.render()),
        500,
        "the /mcp answer raised the spend total to the post-call reading"
    );

    // A SECOND priced call, admitted exactly at the 1000 ceiling, is what makes this cell hold
    // `raise_to` rather than merely "a push happened": one call from the zero boot reading is
    // indistinguishable between `raise_to` and `add` (`0 + 500 == max(0, 500)`). Two calls pin
    // the sequence 500 then 1000 - an `add`-shaped push reads 1500 here and reddens this cell,
    // as does a push that stopped (which would freeze at 500).
    let answered = post(app, &token, ask_metric_call()).await;
    assert!(
        answered.get("error").is_none(),
        "the second ask_metric call must be answered, not refused: {answered}"
    );
    assert_eq!(
        counter(&registry.render()),
        1_000,
        "the second /mcp answer raised the spend total to 1000, not 1500: the push is raise_to, not add"
    );
}

// ------------------------------------------------- the question over /mcp, as over /v1 ----

/// One sample off a rendered exposition, by its series name with labels.
fn sample(exposition: &str, series: &str) -> u64 {
    exposition
        .lines()
        .find_map(|line| line.strip_prefix(series)?.strip_prefix(' '))
        .unwrap_or_else(|| panic!("the exposition has no `{series}` sample: {exposition}"))
        .parse()
        .unwrap_or_else(|cause| panic!("`{series}` is not an integer sample: {cause}"))
}

#[tokio::test]
async fn a_served_agent_question_writes_the_question_line_the_query_route_writes() {
    let PricedAgentSurface { app, issuer, .. } = priced_agent_surface("agent-question-line");
    let token = issuer.mint(&accepted_by(ASKING_SUBJECT)).expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);

    let sink = sutura_runtime::testing::Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let subscriber =
        sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
    let answered = {
        let _logging = tracing::subscriber::set_default(subscriber);
        post(app, &token, ask_metric_call()).await
    };
    assert!(
        answered.get("error").is_none(),
        "the ask_metric call must be answered, not refused: {answered}"
    );

    let rendered = sink.contents();
    let line = rendered
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|line| line["msg"].as_str().is_some_and(|msg| msg.contains("question received")))
        .unwrap_or_else(|| panic!("no `question received` line was written for the /mcp question: {rendered}"));
    // The fields `/v1` writes: which question (on the span in a log, so every line of the request
    // carries it), who asked as a label, and how many parts it had - never a value.
    assert_eq!(line["metric"], "revenue", "{line}");
    assert_eq!(line["grain"], "month", "{line}");
    assert_eq!(line["asker"], "verified", "{line}");
    assert_eq!(line["dimensions"], 0, "{line}");
    assert_eq!(line["filters"], 0, "{line}");
    assert!(
        !line.to_string().contains(ASKING_SUBJECT),
        "the line names the subject rather than the label: {line}"
    );
}

#[tokio::test]
async fn a_served_agent_question_is_counted_in_the_question_series() {
    let PricedAgentSurface {
        app, registry, issuer, ..
    } = priced_agent_surface("agent-question-series");
    let token = issuer.mint(&accepted_by(ASKING_SUBJECT)).expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);

    let answered = post(app, &token, ask_metric_call()).await;
    assert!(
        answered.get("error").is_none(),
        "the ask_metric call must be answered, not refused: {answered}"
    );

    // One answered question, on the series `/v1` moves for one answered question: the count by
    // outcome, the duration, the time it waited for a slot and the rows it returned.
    let exposition = registry.render();
    for series in [
        r#"sutura_questions_total{code="answer"}"#,
        "sutura_question_duration_seconds_count",
        "sutura_admission_wait_seconds_count",
        "sutura_answer_rows_count",
    ] {
        assert_eq!(
            sample(&exposition, series),
            1,
            "{series} after one /mcp question: {exposition}"
        );
    }
}

/// One execution slot, a one-second wait for it, and `run_sql` enabled.
const ONE_SLOT: &str =
    "runtime:\n  max_concurrent_queries: 1\n  admission_timeout_seconds: 1\ntools:\n  run_sql:\n    enabled: true\n";

/// One execution slot, a five-second wait for it, and a two-second reply deadline: the deadline
/// fires first, so a question that cannot get the slot outruns it rather than being shed.
const TIMES_OUT: &str =
    "runtime:\n  max_concurrent_queries: 1\n  admission_timeout_seconds: 5\nserver:\n  request_timeout_seconds: 2\n";

/// [`priced_agent_surface`] with its mount and its permit set built from `under`, an overlay of
/// runtime, server and tool keys, and the permit set both surfaces share: a cell that holds its slot
/// makes the next `/mcp` question wait.
///
/// The router keeps the priced state's own settings, so a bound `under` shortens is the mount's
/// alone. That is what lets the call's own deadline fire before the router's request timeout, which
/// answers `408` for the same key and starts first.
fn agent_surface_under(key_set_id: &str, under: &str) -> (PricedAgentSurface, sutura_runtime::Admission) {
    let PricedState { state, gate, issuer } = priced_state(key_set_id);
    let under = Settings::load(&Sources::defaults(Environment::Development).with_overlay(under)).expect("the overlay loads");
    let admission = sutura_runtime::Admission::from_settings(under.runtime());
    let state = sutura_http::ServiceState::new(state.surface(), Arc::new(state.settings().clone()), admission.clone());
    let push = sutura_http::SpendHeadroomPush::of(&state);
    let gauge = push.gauge().expect("the priced surface reports headroom at boot").clone();
    let mount = super::super::agent::mount(state.surface(), &under, admission.clone(), push).expect("the agent mount builds");
    let registry = Arc::clone(&state.registry());
    let state = state.with_inbound_identity(Arc::new(gate)).with_agent_surface(mount);
    let app = sutura_http::router(&state).expect("the test router assembles");
    let surface = PricedAgentSurface {
        app,
        gauge,
        registry,
        issuer,
    };
    (surface, admission)
}

#[tokio::test]
async fn a_served_agent_question_shed_for_want_of_a_slot_is_counted() {
    let (
        PricedAgentSurface {
            app, registry, issuer, ..
        },
        admission,
    ) = agent_surface_under("agent-question-shed", ONE_SLOT);
    let token = issuer.mint(&accepted_by(ASKING_SUBJECT)).expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);

    // The one slot is taken by something else, so the question waits the whole window and is shed.
    let held = admission.admit().await.expect("the only slot is free");
    let shed = post(app, &token, ask_metric_call()).await;
    drop(held);
    assert_eq!(
        shed["result"]["isError"], true,
        "the question was shed as a tool error: {shed}"
    );

    let exposition = registry.render();
    for series in [
        "sutura_admission_shed_total",
        r#"sutura_questions_total{code="at_capacity"}"#,
        "sutura_question_duration_seconds_count",
        "sutura_admission_wait_seconds_count",
    ] {
        assert_eq!(
            sample(&exposition, series),
            1,
            "{series} after one shed /mcp question: {exposition}"
        );
    }
}

#[tokio::test]
async fn a_served_agent_question_refused_while_its_arguments_are_read_is_counted_as_that_refusal() {
    let PricedAgentSurface {
        app, registry, issuer, ..
    } = priced_agent_surface("agent-question-refused");
    let token = issuer.mint(&accepted_by(ASKING_SUBJECT)).expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);

    // One metric more than a question may name: refused before a slot is waited for or the port asked.
    let mut call = ask_metric_call();
    call["params"]["arguments"]["metrics"] = serde_json::json!(vec!["revenue"; sutura_domain::query::MAX_METRICS + 1]);
    let refused = post(app, &token, call).await;
    assert!(
        refused.get("error").is_none() && refused.to_string().contains("too_many_metrics"),
        "the question was refused as a tool result naming its reason: {refused}"
    );

    // Counted as `/v1` counts the same refusal: under the generic code and under its own reason, with
    // its duration, and not as an answer.
    let exposition = registry.render();
    for series in [
        r#"sutura_questions_total{code="refused"}"#,
        r#"sutura_refusals_total{reason="too_many_metrics"}"#,
        "sutura_question_duration_seconds_count",
    ] {
        assert_eq!(
            sample(&exposition, series),
            1,
            "{series} after one refused /mcp question: {exposition}"
        );
    }
    assert_eq!(
        sample(&exposition, r#"sutura_questions_total{code="answer"}"#),
        0,
        "a refusal is not an answer: {exposition}"
    );
}

#[tokio::test]
async fn a_served_agent_question_that_outruns_its_deadline_is_counted_as_a_timeout() {
    let (
        PricedAgentSurface {
            app, registry, issuer, ..
        },
        admission,
    ) = agent_surface_under("agent-question-timeout", TIMES_OUT);
    let token = issuer.mint(&accepted_by(ASKING_SUBJECT)).expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);

    // The one slot is taken by something else, and the wait for it is longer than the deadline.
    let held = admission.admit().await.expect("the only slot is free");
    let outran = post(app, &token, ask_metric_call()).await;
    drop(held);
    assert_eq!(
        outran["result"]["isError"], true,
        "the question outran its deadline as a tool error: {outran}"
    );

    let exposition = registry.render();
    for series in [
        r#"sutura_questions_total{code="timeout"}"#,
        "sutura_question_duration_seconds_count",
        "sutura_admission_wait_seconds_count",
    ] {
        assert_eq!(
            sample(&exposition, series),
            1,
            "{series} after one /mcp question that outran its deadline: {exposition}"
        );
    }
    assert_eq!(
        sample(&exposition, "sutura_admission_shed_total"),
        0,
        "the deadline fired inside the admission window, so nothing was shed: {exposition}"
    );
}

#[tokio::test]
async fn a_served_agent_raw_statement_shed_for_want_of_a_slot_is_counted() {
    let (
        PricedAgentSurface {
            app, registry, issuer, ..
        },
        admission,
    ) = agent_surface_under("agent-raw-shed", ONE_SLOT);
    let token = issuer.mint(&accepted_by(ASKING_SUBJECT)).expect("the issuer signs a token");
    drop(post(app.clone(), &token, initialize(1)).await);

    let held = admission.admit().await.expect("the only slot is free");
    let shed = post(app, &token, run_sql_call("select 1", 2)).await;
    drop(held);
    assert_eq!(
        shed["result"]["isError"], true,
        "the raw statement was shed as a tool error: {shed}"
    );

    let exposition = registry.render();
    for series in ["sutura_admission_shed_total", "sutura_admission_wait_seconds_count"] {
        assert_eq!(
            sample(&exposition, series),
            1,
            "{series} after one shed /mcp raw statement: {exposition}"
        );
    }
}

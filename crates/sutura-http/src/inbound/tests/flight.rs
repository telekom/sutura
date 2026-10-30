//! The Flight surface through the assembled router: leg 1 runs before `RemoteLegService` does, and
//! the `Asked` it establishes is the one the service's capability check reads.
//!
//! The client is `arrow_flight::FlightClient` over the router itself, so each call is the
//! tonic-generated request a peer node sends, with no listener in between.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use arrow_flight::FlightClient;
use arrow_flight::error::FlightError;
use futures_util::TryStreamExt as _;
use sutura_app::surface::{Adopted, NotAdopted, SurfaceFailure};
use sutura_dev::issuer::{MockIssuer, Token};
use sutura_domain::identity::RequestContext;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::remote::{LegWire, RemoteLegOutcome};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{RowSet, Value};
use tonic::Code;

use super::gate_over;
use crate::surface::Surface;
use crate::testing::{an_issuer, canned, declared_inbound, direct_overlay, every_scope, settings_with, source};

const FLIGHT: &str = "server:\n  flight_surface:\n    enabled: true\n";

/// Counts the legs that reached it and refuses each one, so a cell reads the count and the code.
#[derive(Default)]
struct CountingSurface {
    legs: AtomicUsize,
}

impl Surface for CountingSurface {
    fn definitions(&self) -> Arc<PinnedDefinitions> {
        Arc::new(crate::testing::bundle())
    }

    #[expect(clippy::panic_in_result_fn, reason = "never called: a remote leg never adopts a bundle")]
    fn adopt(&self, _next: PinnedDefinitions) -> Result<Adopted, NotAdopted> {
        panic!("a remote leg never adopts a bundle")
    }

    #[expect(clippy::panic_in_result_fn, reason = "never called: a remote leg never asks a question")]
    fn answer(
        &self,
        _context: &RequestContext,
        _query: &sutura_domain::query::Query,
        _deadline: Deadline,
    ) -> Result<sutura_domain::query::ToolOutcome, SurfaceFailure> {
        panic!("a remote leg never asks a question")
    }

    #[expect(clippy::panic_in_result_fn, reason = "never called: a remote leg never runs raw SQL")]
    fn run_sql(
        &self,
        _context: &RequestContext,
        _statement: &sutura_domain::raw::RawStatement,
        _deadline: Deadline,
    ) -> Result<sutura_domain::raw::RawOutcome, SurfaceFailure> {
        panic!("a remote leg never runs raw SQL")
    }

    fn run_remote_leg(
        &self,
        _context: &RequestContext,
        _leg: LegWire,
        _params: Vec<sutura_domain::warehouse::ParamValue>,
        _deadline: Deadline,
    ) -> Result<RemoteLegOutcome, SurfaceFailure> {
        let _before = self.legs.fetch_add(1, Ordering::SeqCst);
        Ok(RemoteLegOutcome::Refusal {
            reason: RefusalReason::RemoteLegNotCertified { source: source() },
        })
    }

    fn spend_headroom_bytes(&self) -> Option<u64> {
        None
    }

    fn spent_bytes_total(&self) -> Option<u64> {
        None
    }
}

/// A router verifying `issuer`, with the Flight surface mounted over `surface`.
fn with_flight_mount(issuer: &MockIssuer, surface: &Arc<CountingSurface>) -> axum::Router {
    let settings = settings_with(&format!("{FLIGHT}{}", direct_overlay(issuer, "/unread.json")));
    let gate = gate_over(&declared_inbound(&settings), &issuer.key_set());
    let state =
        crate::testing::state_over(Arc::clone(surface) as Arc<dyn Surface>, settings).with_inbound_identity(Arc::new(gate));
    let mount = crate::flight::mount(&state);
    let peer: std::net::SocketAddr = "203.0.113.7:44444".parse().expect("a test peer address is an address");
    crate::router(&state.with_flight_surface(mount))
        .expect("a direct deployment with a Flight mount assembles")
        .layer(axum::Extension(axum::extract::ConnectInfo(peer)))
}

/// A lookup leg the counting surface never reads; built by deserializing, as a peer's bytes are.
fn a_lookup_leg() -> LegWire {
    serde_json::from_value(serde_json::json!({
        "leg": "lookup",
        "source": "local",
        "metric": {"metric": "revenue", "digest": "a".repeat(64)},
        "table": "orders",
        "joins": [],
        "keys": [],
        "filters": [],
    }))
    .expect("a well-formed lookup leg")
}

/// The status one `DoExchange` through `router` is refused with, carrying `bearer` if there is one.
///
/// A refusal from the service is a trailers-only response and fails the call itself; one from leg 1
/// is a plain HTTP status, which the client reports on the first read of the response stream.
async fn refused(router: axum::Router, bearer: Option<&str>) -> tonic::Status {
    let rows =
        RowSet::new(vec![String::from("key")], vec![vec![Value::Text(String::from("berlin"))]]).expect("a one-column row set");
    let batches = canned(&rows).into_batches().into_iter().map(Ok::<_, FlightError>);
    let request = sutura_flight::request_stream(&a_lookup_leg(), futures_util::stream::iter(batches)).expect("an encodable leg");
    let mut client = FlightClient::new(router);
    if let Some(bearer) = bearer {
        client
            .add_header("authorization", &format!("Bearer {bearer}"))
            .expect("a bearer is a header value");
    }
    let error = match client.do_exchange(request).await {
        Ok(mut stream) => stream.try_next().await.expect_err("the leg was expected to be refused"),
        Err(error) => error,
    };
    let FlightError::Tonic(status) = error else {
        panic!("expected a tonic status, got {error:?}");
    };
    *status
}

fn granting(issuer: &MockIssuer, scope: &str) -> String {
    issuer
        .mint(&Token::for_subject("someone@example.com").granting(scope))
        .expect("the issuer signs a token")
}

#[tokio::test]
async fn a_leg_with_no_bearer_is_refused_by_leg_one_before_the_service_runs() {
    let issuer = an_issuer();
    let surface = Arc::new(CountingSurface::default());
    let status = refused(with_flight_mount(&issuer, &surface), None).await;
    assert_eq!(status.code(), Code::Unauthenticated, "{status:?}");
    assert_eq!(surface.legs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_verified_caller_without_the_leg_scope_is_permission_denied() {
    let issuer = an_issuer();
    let surface = Arc::new(CountingSurface::default());
    let status = refused(
        with_flight_mount(&issuer, &surface),
        Some(&granting(&issuer, "sutura:metrics.ask")),
    )
    .await;
    assert_eq!(status.code(), Code::PermissionDenied, "{status:?}");
    assert_eq!(surface.legs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_verified_caller_holding_the_leg_scope_reaches_the_surface() {
    let issuer = an_issuer();
    let surface = Arc::new(CountingSurface::default());
    let status = refused(with_flight_mount(&issuer, &surface), Some(&granting(&issuer, &every_scope()))).await;
    assert_eq!(status.code(), Code::FailedPrecondition, "{status:?}");
    assert_eq!(surface.legs.load(Ordering::SeqCst), 1);
}

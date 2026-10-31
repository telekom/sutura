//! The mounted Flight transport: [`sutura_flight::RemoteLegService`] behind tonic's generated server.

use arrow_flight::flight_service_server::FlightServiceServer;
use sutura_flight::{DescriptorLimit, FrameBudget, RemoteLegService};

use crate::ServiceState;
use crate::state::FlightMount;

/// Builds the mounted Flight transport: [`RemoteLegService`] over `state`'s own surface, behind
/// tonic's generated server.
///
/// A leg shares everything a `POST /v1/query` has: the admission permits, the request budget, and
/// `server.max_body_bytes` as both the descriptor limit and the per-frame budget.
#[must_use]
pub fn mount(state: &ServiceState) -> FlightMount {
    let server = state.settings().server();
    let limit = server.max_body().limit();
    let service = RemoteLegService::new(
        state.surface(),
        state.admission().clone(),
        server.request_timeout().budget(),
        DescriptorLimit::new(limit),
        FrameBudget::new(limit),
    );
    FlightMount::new(FlightServiceServer::new(service))
}

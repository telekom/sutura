//! Building the agent-surface transport, under the `agent` feature.
//!
//! One function, gated at the call site: turn the erased serving surface and the settings into an
//! [`sutura_http::AgentMount`] for the composition root to attach. Nothing here can be reached in a
//! build that did not link `sutura_mcp` - the whole file is `#[cfg(feature = "agent")]`, and the
//! refusal for "enabled but this build has no transport" lives in `main.rs` beside the other boot
//! refusals rather than here.

use std::sync::Arc;

use sutura_app::surface::{Surface, SurfaceFailure};
use sutura_domain::identity::RequestContext;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_http::SpendHeadroomPush;

/// The erased serving service behind a [`Surface`].
///
/// `sutura_mcp::http::service` is generic over a *sized* `S: Surface`, but this composition root
/// erases the adapter to `Arc<dyn Surface>` for the HTTP surface (`type Serving` in `main.rs`) -
/// PR3's "a root nests the service itself" cannot run over an erased object. This wrapper forwards
/// every method to the `dyn`, giving the transport a sized `Surface` to hold without un-erasing.
///
/// **It also pushes this replica's spend headroom after every answered call.** `/mcp` answers
/// through the same [`Surface`] and charges the same ledger as the HTTP query route, but the agent
/// transport cannot link `sutura-http` (a transport never links another transport), so the gauge
/// it must keep honest lives here instead - the composition root hands this wrapper the same
/// `sutura_runtime::Gauge` the HTTP route writes, so both surfaces drive one
/// `sutura_spend_headroom_bytes` series.
///
/// The handle arrives as a [`SpendHeadroomPush`] rather than an `Option<Gauge>`, so this wrapper
/// cannot be built over a gauge that is not the served state's own, and a build over no gauge at
/// all had to say `NoCeilingConfigured` out loud.
struct Serving {
    surface: Arc<dyn Surface>,
    spend_headroom: SpendHeadroomPush,
}

impl Surface for Serving {
    fn definitions(&self) -> &PinnedDefinitions {
        self.surface.definitions()
    }
    fn answer(&self, context: &RequestContext, query: &Query, deadline: Deadline) -> Result<ToolOutcome, SurfaceFailure> {
        let answered = self.surface.answer(context, query, deadline);
        // Pushed regardless of whether this answered, refused or failed: a charge is a reservation
        // never released, so the ledger can have moved even where the call went on to fail - the
        // same reading the HTTP query route takes right after `Surface::answer`.
        self.push_headroom();
        answered
    }
    fn run_sql(
        &self,
        context: &RequestContext,
        statement: &sutura_domain::raw::RawStatement,
    ) -> Result<sutura_domain::raw::RawOutcome, SurfaceFailure> {
        let answered = self.surface.run_sql(context, statement);
        self.push_headroom();
        answered
    }
    fn spend_headroom_bytes(&self) -> Option<u64> {
        self.surface.spend_headroom_bytes()
    }

    fn spent_bytes_total(&self) -> Option<u64> {
        self.surface.spent_bytes_total()
    }
}

impl Serving {
    /// Pushes this replica's current spend headroom and running spend total, if this deployment has
    /// a spend ceiling at all. Mirrors `ServiceState::record_spend_headroom`: a `None` reading
    /// leaves the gauge and the counter untouched rather than fabricating zero. The counter is
    /// raised to the cumulative total - `add` would double-count a reading that already carries
    /// every byte admitted so far.
    fn push_headroom(&self) {
        if let (Some(gauge), Some(bytes)) = (self.spend_headroom.gauge(), self.spend_headroom_bytes()) {
            gauge.set(bytes);
        }
        if let Some(total) = self.spent_bytes_total() {
            self.spend_headroom.push_spend_total(total);
        }
    }
}

/// The mounted streamable-HTTP transport over the serving surface, ready for
/// [`sutura_http::ServiceState::with_agent_surface`].
///
/// `sutura_mcp::http::service` is always `Asking::PerRequest` and needs the server's execution
/// bound and per-request deadline; the bound is the process's own `Admission`, shared with the HTTP
/// surface, so both transports answer under one permit set and one deadline.
///
/// **Fallible since `telekom/sutura#776`**, for the reason [`crate::commands::agent_instructions`]
/// already states: an operator-configured `prompt.instructions_file` that cannot be read is a
/// startup refusal naming the path, not a served surface that silently omitted the operator's own
/// section. Reads the bundle off `service` before erasing it, so what this renders the prompt over
/// is the exact bundle `Surface::answer` computes against - never a second catalog load that could
/// drift from it.
pub(crate) fn mount(
    service: Arc<dyn Surface>,
    settings: &sutura_config::Settings,
    admission: sutura_runtime::Admission,
    spend_headroom: SpendHeadroomPush,
) -> Result<sutura_http::AgentMount, String> {
    let instructions = crate::commands::agent_instructions(service.definitions(), settings)?;
    let serving = Arc::new(Serving {
        surface: service,
        spend_headroom: spend_headroom.clone(),
    });
    Ok(sutura_http::AgentMount::new(
        sutura_mcp::http::service(
            serving,
            crate::commands::catalog_prose(settings.prompt().catalog_prose()),
            admission,
            settings.server().request_timeout(),
            Arc::from(instructions),
        ),
        spend_headroom,
    ))
}

//! Building the agent-surface transport, under the `agent` feature.
//!
//! One function, gated at the call site: turn the erased serving surface and the settings into an
//! [`sutura_http::AgentMount`] for the composition root to attach. Nothing here can be reached in a
//! build that did not link `sutura_mcp` - the whole file is `#[cfg(feature = "agent")]`, and the
//! refusal for "enabled but this build has no transport" lives in `main.rs` beside the other boot
//! refusals rather than here.

use std::sync::Arc;

use sutura_app::prompt::CatalogProse;
use sutura_app::surface::{Surface, SurfaceFailure};
use sutura_domain::identity::RequestContext;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::warehouse::deadline::Deadline;

/// The erased serving service behind a [`Surface`].
///
/// `sutura_mcp::http::service` is generic over a *sized* `S: Surface`, but this composition root
/// erases the adapter to `Arc<dyn Surface>` for the HTTP surface (`type Serving` in `main.rs`) -
/// PR3's "a root nests the service itself" cannot run over an erased object. This wrapper forwards
/// every method to the `dyn`, giving the transport a sized `Surface` to hold without un-erasing.
struct Serving(Arc<dyn Surface>);

impl Surface for Serving {
    fn definitions(&self) -> &PinnedDefinitions {
        self.0.definitions()
    }
    fn answer(&self, context: &RequestContext, query: &Query, deadline: Deadline) -> Result<ToolOutcome, SurfaceFailure> {
        self.0.answer(context, query, deadline)
    }
    fn run_sql(
        &self,
        context: &RequestContext,
        statement: &sutura_domain::raw::RawStatement,
    ) -> Result<sutura_domain::raw::RawOutcome, SurfaceFailure> {
        self.0.run_sql(context, statement)
    }
}

/// The agent-surface prose, from the deployment's prompt declaration.
///
/// The same two-way mapping `sutura_app::prompt::CatalogProse` and
/// `sutura_config::prompt::CatalogProse` hold equal in `sutura-cli`'s own composition root; copied
/// here rather than shared because the two roots are different crates and `sutura-cli`'s conversion
/// is `pub(crate)` there.
const fn catalog_prose(configured: sutura_config::prompt::CatalogProse) -> CatalogProse {
    match configured {
        sutura_config::prompt::CatalogProse::Quoted => CatalogProse::Quoted,
        sutura_config::prompt::CatalogProse::Omitted => CatalogProse::Omitted,
    }
}

/// The mounted streamable-HTTP transport over the serving surface, ready for
/// [`sutura_http::ServiceState::with_agent_surface`].
///
/// `sutura_mcp::http::service` is always `Asking::PerRequest` and needs the server's execution
/// bound and per-request deadline; the bound is the process's own `Admission`, shared with the HTTP
/// surface, so both transports answer under one permit set and one deadline.
#[must_use]
pub(crate) fn mount(
    service: Arc<dyn Surface>,
    settings: &sutura_config::Settings,
    admission: sutura_runtime::Admission,
) -> sutura_http::AgentMount {
    sutura_http::AgentMount::new(sutura_mcp::http::service(
        Arc::new(Serving(service)),
        catalog_prose(settings.prompt().catalog_prose()),
        admission,
        settings.server().request_timeout(),
    ))
}

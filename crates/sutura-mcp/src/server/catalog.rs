//! The catalog door, `describe_catalog`, and the per-caller narrowing it shares with `initialize`.

use std::sync::Arc;
use std::time::Instant;

use rmcp::model::{CallToolResult, ContentBlock};
use sutura_app::surface::{Surface, SurfaceFailure};
use sutura_config::RequestTimeout;
use sutura_domain::warehouse::deadline::Deadline;

use crate::wire::CatalogContent;

/// The served bundle, as THIS caller's view, rendered as a tool result.
///
/// **No data system and no slot**: it reads the validated bundle being served, taken once for this
/// call, so a refresh that lands while it renders does not change what it renders. The same
/// judgement `sutura_http::routes::v1::catalog` makes, for the same reason. It is `async` only for
/// [`hidden_for`], which dials a source that narrows the catalog per caller and nothing else.
///
/// **The view is the caller's at this door - `docs/adr/0028`.** A caller sees only the metrics its
/// mapped audiences name, mirrored from `sutura_http::routes::v1::catalog`'s `CatalogBody::of(&view, …)`.
/// `scoped_for` reads an absent or process-owner caller as `Subject::TheDeploymentItself`, which
/// maps to the whole bundle, so an operator in stdio mode still sees exactly what they always saw;
/// a verified caller sees the bundle cut to their grant. That the renderer CANNOT skip the scope is
/// the point of taking `&ScopedView` rather than a bare `&PinnedDefinitions`.
///
/// **The `initialize.instructions` prompt goes through the same view.** `initialize` renders it
/// per caller with `sutura_app::prompt::render`, which takes a `&ScopedView` too, so its
/// metric list and knowledge sections withhold what this listing withholds. What neither scopes is
/// the operator's own text, which is deployment-wide by design.
///
/// **No audit record either, and that is deliberate rather than an omission.**
/// `sutura_domain::audit::CallRecord` records the outcome of a *question*, and this is not one - there
/// is no `ToolOutcome` to derive a record from and `Surface::definitions` writes none. So the
/// invariant *every outcome is recorded before it is returned* is untouched: this produces no outcome.
/// Whether reading the catalog is itself worth a record is a real question and the answer would be a
/// third `RecordedOutcome` variant, which is a change to what a record means rather than a field added
/// to one.
pub(super) async fn describe<S>(
    service: &Arc<S>,
    context: &sutura_domain::identity::RequestContext,
    reply: RequestTimeout,
    prose: sutura_app::prompt::CatalogProse,
    operator_instructions: Option<&str>,
    list_physical_schema: bool,
) -> CallToolResult
where
    S: Surface,
{
    // The setting reaches the CONTENT and not only the rendering, which is the whole of `H1` in
    // `#266`: the text block honoured it while `structured_content` beside it carried every
    // description, so a deployment that had withheld its catalog prose shipped it anyway to any
    // client reading the structured half. `CatalogContent::of_with_physical_schema` cannot be
    // called without the answer.
    //
    // And the view is built here, from the caller this request's own verification resolved - see the
    // doc above for why `scoped_for` maps an absent or process-owner caller to the whole bundle.
    let definitions = service.definitions();
    let hidden = match hidden_for(service, context, &definitions, reply).await {
        Ok(hidden) => hidden,
        Err(ref failure) => return super::outcome::could_not_answer(failure),
    };
    let view = sutura_app::scoped_for(&definitions, context).hiding(hidden);
    // The operator's own text - the raw value, before it was folded into the rendered prompt - rides
    // the catalog tool so a gateway that surfaces only tools (and never delivers
    // `initialize.instructions`) still reaches the operator's rules.
    let listing = CatalogContent::of_with_physical_schema(&view, prose, operator_instructions, list_physical_schema);
    let mut result = CallToolResult::success(vec![ContentBlock::text(listing.as_text())]);
    // `ok()` rather than a propagated error, for the reason `produced` gives: the content is strings,
    // numbers and vectors, so serializing it cannot fail, and there is no `unwrap` in this workspace
    // to say so.
    result.structured_content = serde_json::to_value(&listing).ok();
    result
}

/// The models this caller is not shown, asked on the blocking pool because a source that narrows
/// the catalog per caller is dialled under the caller's own credential -
/// `docs/adr/20261125083027-catalog-visibility-per-caller.md`. Fails closed: a failure is returned,
/// never an empty set standing in for one. A panic on the pool is a [`SurfaceFailure::Miswired`].
pub(super) async fn hidden_for<S>(
    service: &Arc<S>,
    context: &sutura_domain::identity::RequestContext,
    definitions: &Arc<sutura_domain::pinned::PinnedDefinitions>,
    reply: RequestTimeout,
) -> Result<sutura_domain::visibility::HiddenModels, SurfaceFailure>
where
    S: Surface,
{
    let deadline = Deadline::opened_at(Instant::now(), reply.budget());
    let (service, context, pinned) = (Arc::clone(service), context.clone(), Arc::clone(definitions));
    sutura_runtime::spawn_carrying_span(move || service.hidden_models(&context, &pinned, deadline))
        .await
        .unwrap_or_else(|panicked| {
            Err(SurfaceFailure::Miswired {
                cause: Box::new(panicked),
            })
        })
}

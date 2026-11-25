//! The catalog tool's handler, split off `server.rs` for the 1000-line cap `cargo xtask max-lines`
//! enforces - the same reason `crate::wire::catalog` is its own file. Everything here is reached
//! from exactly one match arm, `Capability::DescribeCatalog` in [`super::AgentSurface`]'s
//! `ServerHandler::call_tool`.

use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, ErrorCode};

use sutura_app::surface::Surface;

use super::{MalformedQuestion, RenderedCause, invalid};
use crate::wire::{CatalogContent, DescribeCatalogArgs};

/// The catalog tool's arguments: at most one of `model` or `metric`, never both.
///
/// **Parsed rather than ignored**, and the parse IS the check: `DescribeCatalogArgs` carries
/// `deny_unknown_fields`, so an arguments object naming `sql` or anything else this type does not
/// declare is a named parse error. A handler that skipped this would accept any object and quietly
/// answer, which is the shape that lets a caller believe it narrowed a listing it did not.
pub(super) fn catalog(request: CallToolRequestParams) -> Result<DescribeCatalogArgs, ErrorData> {
    let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
    serde_json::from_value(arguments).map_err(|cause| invalid(&MalformedQuestion::NotAnObject { cause }))
}

/// The served bundle, as THIS caller's view, rendered as a tool result.
///
/// **No blocking pool and no data system**, which is why this is not `async` and takes no slot: it
/// reads the validated bundle being served, taken once for this call, so a refresh that lands while
/// it renders does not change what it renders. The same judgement
/// `sutura_http::routes::v1::catalog` makes, for the same reason.
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
/// to one. A `model` that fails to parse or names nothing this caller may see is refused the same
/// way - a JSON-RPC error with no record - as [`super::no_established_caller`] and
/// [`super::not_granted`] already are: every refusal this handler can make before a `Surface` call is
/// reached shares that absence.
pub(super) fn describe<S>(
    service: &Arc<S>,
    context: &sutura_domain::identity::RequestContext,
    prose: sutura_app::prompt::CatalogProse,
    operator_instructions: Option<&str>,
    list_physical_schema: bool,
    model: Option<String>,
    metric: Option<String>,
) -> Result<CallToolResult, ErrorData>
where
    S: Surface,
{
    // Mutually exclusive by the tool's own doc comment on `DescribeCatalogArgs` - checked before
    // either name is parsed, so a caller that sent both gets one refusal rather than whichever
    // parse error happened to run first.
    if model.is_some() && metric.is_some() {
        return Err(model_and_metric_together());
    }
    // The setting reaches the CONTENT and not only the rendering, which is the whole of `H1` in
    // `#266`: the text block honoured it while `structured_content` beside it carried every
    // description, so a deployment that had withheld its catalog prose shipped it anyway to any
    // client reading the structured half. `CatalogContent::of_with_physical_schema` cannot be
    // called without the answer.
    //
    // And the view is built here, from the caller this request's own verification resolved - see the
    // doc above for why `scoped_for` maps an absent or process-owner caller to the whole bundle.
    let definitions = service.definitions();
    let view = sutura_app::scoped_for(&definitions, context);
    // Parsed into the same newtype every other identifier on this surface goes through, and checked
    // against THIS caller's own view - never the bare bundle - so a `model` naming something real but
    // invisible to this caller is refused exactly as absent would be, per `docs/adr/0028`.
    let model = model
        .map(|raw| sutura_domain::model::ModelName::parse(&raw))
        .transpose()
        .map_err(|error| RenderedCause::of(&error).into_error_data())?;
    // **Accepting the name is not granting the model.** `view.model(name)` alone would refuse this
    // caller a dictionary-inferred model's own name even though a metric already in `view` discloses
    // it; the second arm widens only WHICH NAMES this call answers to, never what
    // `CatalogContent::of_with_physical_schema` goes on to render for that name below - its
    // `metrics` filter and, with `list_physical_schema`, its columns still come from this same
    // unwidened `view`, so a model with no audience of its own still yields no columns here even
    // though its name is accepted.
    if let Some(name) = &model
        && view.model(name).is_none()
        && view.metrics().all(|metric| metric.model() != name)
    {
        return Err(unknown_model(name));
    }
    // `metric` mirrors `model`: parsed into the same newtype every other identifier goes through,
    // then checked against THIS caller's view - `ScopedView::metric` already applies the same
    // visibility rule `view.model`/`view.metrics()` do, so an existing but invisible metric is
    // refused exactly as an absent one is, per `docs/adr/0028`.
    let metric = metric
        .map(|raw| sutura_domain::model::MetricName::parse(&raw))
        .transpose()
        .map_err(|error| RenderedCause::of(&error).into_error_data())?;
    if let Some(name) = &metric
        && view.metric(name).is_none()
    {
        return Err(unknown_metric(name));
    }
    // The operator's own text - the raw value, before it was folded into the rendered prompt - rides
    // the catalog tool so a gateway that surfaces only tools (and never delivers
    // `initialize.instructions`) still reaches the operator's rules.
    let listing = CatalogContent::of_with_physical_schema(
        &view,
        prose,
        operator_instructions,
        list_physical_schema,
        model.as_ref(),
        metric.as_ref(),
    );
    let mut result = CallToolResult::success(vec![ContentBlock::text(listing.as_text())]);
    // `ok()` rather than a propagated error, for the reason `produced` gives: the content is strings,
    // numbers and vectors, so serializing it cannot fail, and there is no `unwrap` in this workspace
    // to say so.
    result.structured_content = serde_json::to_value(&listing).ok();
    Ok(result)
}

/// A `model` argument that parses but names no model this caller may see - whether it does not exist
/// at all or exists outside this caller's granted audiences is not distinguished here, for the same
/// reason [`sutura_domain::pinned::view::ScopedView::model`] folds both into one `None`: a message
/// that told the two apart would disclose a restricted model's existence to a caller who was never
/// granted it.
fn unknown_model(name: &sutura_domain::model::ModelName) -> ErrorData {
    ErrorData::new(
        ErrorCode::INVALID_PARAMS,
        format!("this catalog defines no model called `{name}`"),
        None,
    )
}

/// A `metric` argument that parses but names no metric this caller may see - the same
/// non-disclosing fold [`unknown_model`] does, for the same reason: a message that told "does not
/// exist" apart from "exists outside your grant" would disclose a restricted metric's existence to
/// a caller who was never granted it.
fn unknown_metric(name: &sutura_domain::model::MetricName) -> ErrorData {
    ErrorData::new(
        ErrorCode::INVALID_PARAMS,
        format!("this catalog defines no metric called `{name}`"),
        None,
    )
}

/// `model` and `metric` given together - refused rather than one silently winning, since a caller
/// that sent both likely expected the narrower of the two and silently picking one would answer a
/// question nobody asked.
fn model_and_metric_together() -> ErrorData {
    ErrorData::new(
        ErrorCode::INVALID_PARAMS,
        "give at most one of `model` and `metric`, not both",
        None,
    )
}

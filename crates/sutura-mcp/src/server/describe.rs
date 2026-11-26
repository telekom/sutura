//! The catalog tool's handler, split off `server.rs` for the 1000-line cap `cargo xtask max-lines`
//! enforces - the same reason `crate::wire::catalog` is its own file. Everything here is reached
//! from exactly one match arm, `Capability::DescribeCatalog` in [`super::AgentSurface`]'s
//! `ServerHandler::call_tool`.

use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, ErrorCode};

use sutura_app::surface::Surface;

use super::{MalformedQuestion, RenderedCause, invalid};
use crate::wire::{CatalogContent, DescribeCatalogArgs, NamesIndexContent};

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
/// `names`'s own narrowing and paging knobs, bundled into one argument rather than four of
/// [`describe`]'s own parameters - `clippy.toml`'s `too-many-arguments-threshold` is 10, and
/// `describe` already spends seven on `service`/`context`/`prose`/`operator_instructions`/
/// `list_physical_schema`/`model`/`metric`; a fifth and sixth standalone parameter for the metrics
/// cursor would cross it for real, not by a stale `#[expect]` that never fired.
pub(super) struct NamesQuery<'a> {
    pub(super) model_prefix: Option<&'a str>,
    pub(super) names: bool,
    pub(super) after: Option<&'a str>,
}

pub(super) fn describe<S>(
    service: &Arc<S>,
    context: &sutura_domain::identity::RequestContext,
    prose: sutura_app::prompt::CatalogProse,
    operator_instructions: Option<&str>,
    list_physical_schema: bool,
    model: Option<String>,
    metric: Option<String>,
    query: &NamesQuery<'_>,
) -> Result<CallToolResult, ErrorData>
where
    S: Surface,
{
    let &NamesQuery {
        model_prefix,
        names,
        after,
    } = query;
    // Mutually exclusive by the tool's own doc comment on `DescribeCatalogArgs` - checked before
    // either name is parsed, so a caller that sent both gets one refusal rather than whichever
    // parse error happened to run first.
    if model.is_some() && metric.is_some() {
        return Err(model_and_metric_together());
    }
    // `names` and `model_prefix` are a different mode and a different narrowing from `model`/
    // `metric` - checked up front for the same reason the pair above is, so a caller that combined
    // modes gets one refusal naming the actual conflict rather than a silent pick of one.
    if names && (model.is_some() || metric.is_some()) {
        return Err(names_incompatible_with_model_or_metric());
    }
    if model_prefix.is_some() && (model.is_some() || metric.is_some()) {
        return Err(model_prefix_incompatible_with_model_or_metric());
    }
    if after.is_some() && !names {
        return Err(after_requires_names());
    }
    if model_prefix.is_some_and(str::is_empty) {
        return Err(empty_model_prefix());
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

    // `names: true` is a separate mode entirely - a names-only, paged index rather than the full
    // listing - so it is answered and returned here, before `model`/`metric` are even parsed (the
    // checks above already proved neither was given alongside it).
    if names {
        let listing = NamesIndexContent::of(&view, after, model_prefix).map_err(|error| malformed_names_cursor(&error))?;
        let mut result = CallToolResult::success(vec![ContentBlock::text(listing.as_text())]);
        result.structured_content = serde_json::to_value(&listing).ok();
        return Ok(result);
    }

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
        model_prefix,
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

/// `names: true` given alongside `model` or `metric` - a different mode from either narrowing, not a
/// third narrowing stacked on top of them.
fn names_incompatible_with_model_or_metric() -> ErrorData {
    ErrorData::new(
        ErrorCode::INVALID_PARAMS,
        "`names` is mutually exclusive with `model` and `metric`",
        None,
    )
}

/// `model_prefix` given alongside `model` or `metric` - both already narrow to one name or one
/// metric, so a prefix on top of either names nothing new.
fn model_prefix_incompatible_with_model_or_metric() -> ErrorData {
    ErrorData::new(
        ErrorCode::INVALID_PARAMS,
        "`model_prefix` is mutually exclusive with `model` and `metric`",
        None,
    )
}

/// `after` given without `names: true` - a cursor into an index that was never asked for.
fn after_requires_names() -> ErrorData {
    ErrorData::new(
        ErrorCode::INVALID_PARAMS,
        "`after` is only meaningful with `names: true`",
        None,
    )
}

/// `after` did not parse as a [`sutura_app::catalog_budget::NamesCursor`] - refused rather than read
/// as "start from the beginning", so a hand-typed or truncated value cannot silently restart a walk.
fn malformed_names_cursor(error: &sutura_app::catalog_budget::MalformedNamesCursor) -> ErrorData {
    ErrorData::new(ErrorCode::INVALID_PARAMS, error.to_string(), None)
}

/// `model_prefix` given as the empty string - matches every name, which is already what omitting it
/// does, so an empty prefix is refused rather than silently accepted as a no-op a caller likely did
/// not intend.
fn empty_model_prefix() -> ErrorData {
    ErrorData::new(ErrorCode::INVALID_PARAMS, "`model_prefix` must not be empty", None)
}

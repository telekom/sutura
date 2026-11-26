//! What this catalog defines.
//!
//! Descriptive content only, and that is a governance boundary rather than a scoping decision.
//! `SemanticCatalog::load` takes no request context and cannot be given one, so the bundle is the
//! *pinned* one, the same one every answer is computed from. What this renders of it is the caller's
//! scoped view - `sutura_app::scoped_for` over `asked.context()` - for a reader.
//!
//! It is also the surface a caller needs in order to ask a valid question at all: which metrics
//! exist, at which grains, with which dimensions, and which values a filter may use. Without it
//! every question is a guess that comes back as a refusal.
//!
//! **It is behind the access token when one is configured.** A catalog listing is a description of
//! what this deployment measures, which is business information even when no row of data is in it.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse as _, Response};
use sutura_domain::model::{MetricName, ModelName};

use crate::problem::{Detail, Failure};
use crate::state::ServiceState;
use crate::wire::{CatalogBody, CatalogNamesBody, UnknownCatalogNameBody};

/// The tag this route is grouped under in the generated document.
const TAG: &str = "catalog";

/// `model` and `metric` given together - refused the same way a malformed `model` is, since the
/// request itself asked a question with no single answer rather than naming something this caller
/// may not see.
#[derive(Debug, thiserror::Error)]
#[error("give at most one of `model` and `metric`, not both")]
struct ModelAndMetricTogether;

/// `names=true` given alongside `model` or `metric` - a different mode from either narrowing, not a
/// third narrowing stacked on top of them. Mirrors `sutura-mcp`'s
/// `names_incompatible_with_model_or_metric`.
#[derive(Debug, thiserror::Error)]
#[error("`names` is mutually exclusive with `model` and `metric`")]
struct NamesIncompatibleWithModelOrMetric;

/// `model_prefix` given alongside `model` or `metric` - both already narrow to one name or one
/// metric, so a prefix on top of either names nothing new.
#[derive(Debug, thiserror::Error)]
#[error("`model_prefix` is mutually exclusive with `model` and `metric`")]
struct ModelPrefixIncompatibleWithModelOrMetric;

/// `after` given without `names=true` - a cursor into an index that was never asked for.
#[derive(Debug, thiserror::Error)]
#[error("`after` is only meaningful with `names=true`")]
struct AfterRequiresNames;

/// `model_prefix` given as the empty string - matches every name, which is already what omitting it
/// does, so an empty prefix is refused rather than silently accepted as a no-op.
#[derive(Debug, thiserror::Error)]
#[error("`model_prefix` must not be empty")]
struct EmptyModelPrefix;

/// The query string `GET /catalog` takes: `model` and `metric`, at most one of the two.
///
/// `deny_unknown_fields`: an unrecognized key is a caller's own typo (`?modle=`) or a client built
/// against a key this route never had, and silently ignoring either reads back as "the whole
/// catalog", which is the wrong answer to a narrower question the caller thought they asked. Refused
/// as `400` by `crate::problem::rejected_query`, the same door a malformed `model` already used.
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatalogQuery {
    /// The model's own name - the same identifier its physical schema prints as `model <name>`, on
    /// the one transport that prints physical schema. Omit it for the whole catalog. Mutually
    /// exclusive with `metric`.
    model: Option<String>,
    /// One metric's own name, to read back exactly that metric's entry - the way to read back a
    /// single metric a previous, byte-bounded listing named but left out in `omitted_metrics`. Omit
    /// it for the whole catalog. Mutually exclusive with `model`.
    metric: Option<String>,
    /// Narrows the listing, or the `names` index, to metrics whose `model` starts with this prefix.
    /// Mutually exclusive with `model` and `metric`. Refused if given empty.
    model_prefix: Option<String>,
    /// `true` switches this route to a names-only, paged index of every visible metric - see
    /// [`CatalogNamesBody`]. Mutually exclusive with `model` and `metric`.
    names: Option<bool>,
    /// Resumes a `names=true` index strictly after this metric name - the `next` cursor a previous
    /// `names=true` call returned. Refused unless `names` is also `true`.
    after: Option<String>,
}

/// The metrics this catalog defines, and its knowledge as this caller may see it, with the version
/// and digest that identify the snapshot.
///
/// `model` narrows the answer to one named model's own metrics - mirroring `sutura-mcp`'s
/// `describe_catalog(model: ...)`, `crate::openapi::tests::both_transports_describe_the_same_tools`'s
/// reason to exist. `metric` narrows to one metric's own entry instead - the way to read back a
/// single metric a previous listing named in `omitted_metrics` but left out. Give at most one of
/// the two; both together is `400`. Omit both for the whole catalog.
#[utoipa::path(
    get,
    path = "/catalog",
    // The capability this operation IS, so the generated document names the same thing the agent
    // surface names its tool. It is `sutura_app::Capability::DescribeCatalog::id()`'s literal, written
    // out because a `#[utoipa::path]` attribute takes a literal - and
    // `crate::openapi::tests::both_transports_describe_the_same_tools` is what asserts the two agree.
    operation_id = "describe_catalog",
    tag = TAG,
    params(CatalogQuery),
    responses(
        (status = 200, description = "The pinned bundle, as a reader's view.", body = CatalogBody),
        (
            status = 400,
            description = "`model` does not parse as a model name, `metric` does not parse as a \
                           metric name, both are given together, or the query string carries a key \
                           this route does not take.",
            body = crate::problem::ProblemBody
        ),
        (status = 401, description = "No valid bearer token was presented.", body = crate::problem::ProblemBody),
        (
            status = 403,
            description = "TWO THINGS, both `code` with no `outcome`.\n\n\
                           FAILED (`code: insufficient_scope`): your credential is valid and does not \
                           carry the scope this operation requires; the detail names it. Not a \
                           statement about the catalog: a caller who may read it at all sees the \
                           metrics and notes its own view holds.\n\n\
                           REFUSED BEFORE ANY CREDENTIAL IS READ (`code: host_not_allowed`, no \
                           `outcome`): this deployment does not answer the `Host` the request names. \
                           Nothing in your credential or your question changes it; the operator \
                           lists the name in `server.allowed_hosts`.",
            body = crate::problem::ProblemBody
        ),
        (
            status = 404,
            description = "`model` or `metric` parses but names nothing this caller may see. Its \
                           own shape, not `ProblemBody`: reading a catalog produces no outcome a \
                           `ProblemBody` could carry, and a well-formed name the caller may not see \
                           is not a system failure - see `UnknownCatalogNameBody`'s own documentation.",
            body = UnknownCatalogNameBody
        ),
        (status = 429, description = "Too many requests from this address.", body = crate::problem::ProblemBody),
        (
            status = 503,
            description = "Worth retrying: `identity_unavailable`, the key set this deployment \
                           verifies tokens against could not be read; or `no_randomness`, the host \
                           gave no randomness for the request's job-label id.",
            body = crate::problem::ProblemBody
        ),
    )
)]
pub(crate) async fn catalog(
    State(state): State<ServiceState>,
    // `docs/adr/0028` step 2's ratchet: this handler used to be the one route under
    // `establish_asked`'s layer that declined the identity the layer inserted, rendering the whole
    // bundle to every token holder regardless of what `Asked::context` named. It reads that context
    // now, the same value `crate::routes::v1::query::ask` reads through `sutura_app::Asked`.
    axum::Extension(asked): axum::Extension<sutura_app::Asked>,
    query: Result<Query<CatalogQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(rejection) => return crate::problem::rejected_query(&rejection).into_response(),
    };
    // No blocking pool and no data system: this reads the bundle being served, taken once for this
    // request. A refresh that lands while it renders does not change what it renders.
    //
    // The prose setting is read here, from the settings the state already holds, and `crate::state`
    // is right that a value a handler can read is a value a handler can branch on - so the reason
    // this one is read per request is worth stating rather than leaving to the reader. It is fixed
    // at startup and the branch is on a constant; what makes reading it better than a constructor
    // argument on `ServiceState` is the direction the two fail in. A `catalog_prose` argument that a
    // composition root forgot would take `CatalogProse::default()`, which is `quoted`, and ship the
    // prose of a deployment that asked for none. The settings are the one place that cannot be out
    // of date about what the operator wrote.
    let definitions = state.definitions();
    let view = sutura_app::scoped_for(&definitions, asked.context());
    // Parsed into the same newtype every other identifier on this surface goes through. A malformed
    // `model` is `Failure::NotAQuestion` - the request itself could not be read - never confused with
    // `UnknownCatalogNameBody` below, which is a well-formed name this caller may not see.
    if query.model.is_some() && query.metric.is_some() {
        return Failure::NotAQuestion {
            detail: Detail::of(&ModelAndMetricTogether),
        }
        .into_response();
    }
    let names = query.names.unwrap_or(false);
    if names && (query.model.is_some() || query.metric.is_some()) {
        return Failure::NotAQuestion {
            detail: Detail::of(&NamesIncompatibleWithModelOrMetric),
        }
        .into_response();
    }
    if query.model_prefix.is_some() && (query.model.is_some() || query.metric.is_some()) {
        return Failure::NotAQuestion {
            detail: Detail::of(&ModelPrefixIncompatibleWithModelOrMetric),
        }
        .into_response();
    }
    if query.after.is_some() && !names {
        return Failure::NotAQuestion {
            detail: Detail::of(&AfterRequiresNames),
        }
        .into_response();
    }
    if query.model_prefix.as_deref().is_some_and(str::is_empty) {
        return Failure::NotAQuestion {
            detail: Detail::of(&EmptyModelPrefix),
        }
        .into_response();
    }
    if names {
        let definitions = state.definitions();
        let view = sutura_app::scoped_for(&definitions, asked.context());
        return match CatalogNamesBody::of(&view, query.after.as_deref(), query.model_prefix.as_deref()) {
            Ok(body) => ([(header::CACHE_CONTROL, "private, no-store")], Json(body)).into_response(),
            Err(error) => Failure::NotAQuestion {
                detail: Detail::of(&error),
            }
            .into_response(),
        };
    }
    let model = match query.model {
        Some(raw) => match ModelName::parse(&raw) {
            Ok(name) => Some(name),
            Err(error) => {
                return Failure::NotAQuestion {
                    detail: Detail::of(&error),
                }
                .into_response();
            }
        },
        None => None,
    };
    let metric = match query.metric {
        Some(raw) => match MetricName::parse(&raw) {
            Ok(name) => Some(name),
            Err(error) => {
                return Failure::NotAQuestion {
                    detail: Detail::of(&error),
                }
                .into_response();
            }
        },
        None => None,
    };
    // Checked against THIS caller's own view - never the bare bundle - so a `model` naming something
    // real but invisible to this caller is refused exactly as absent would be, per `docs/adr/0028`.
    //
    // **Accepting the name is not granting the model.** `view.model(name)` alone would refuse a
    // caller a dictionary-inferred model's own name even though a metric the view already carries
    // discloses it; the second arm widens only WHICH NAMES this door answers to, never what
    // `CatalogBody::of` goes on to render for that name - its `metrics` filter and, when physical
    // schema is on, its columns still come from this same unwidened `view`, so a model with no
    // audience of its own still yields no columns here even though its name is accepted.
    if let Some(name) = &model
        && view.model(name).is_none()
        && view.metrics().all(|metric| metric.model() != name)
    {
        return UnknownCatalogNameBody::of(name).into_response();
    }
    // `metric` mirrors `model`: `ScopedView::metric` already applies the same visibility rule, so a
    // metric that exists but is outside this caller's grant is refused exactly as an absent one is.
    if let Some(name) = &metric
        && view.metric(name).is_none()
    {
        return UnknownCatalogNameBody::of_metric(name).into_response();
    }
    (
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(CatalogBody::of(
            &view,
            state.settings().prompt().catalog_prose(),
            model.as_ref(),
            metric.as_ref(),
            query.model_prefix.as_deref(),
        )),
    )
        .into_response()
}

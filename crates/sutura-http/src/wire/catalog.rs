//! What this catalog defines, as a reader's view - the wire shapes `crate::routes::v1::catalog`
//! serves.
//!
//! Its own module for `cargo xtask max-lines`'s sake: `wire.rs` had no headroom left under the
//! thousand-line limit it enforces and cannot exempt, and this is one whole tool's worth of shape
//! rather than a share of lines - the same split `sutura-mcp`'s own `wire::catalog` already made
//! for the mirror-image reason.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sutura_app::catalog_budget::{MAX_CATALOG_LISTING_BYTES, MAX_OMITTED_NAMES, fill_to_budget};
use sutura_app::prompt::CatalogProse;
use sutura_domain::model::{Grain, ModelName};
use sutura_domain::pinned::view::ScopedView;

use super::{ProvenanceBody, bundle_body};

/// What this catalog defines.
///
/// # Why a structured surface reads the prose setting at all
///
/// `prompt.catalog_prose: omitted` is not a mitigation for the forgery `docs/adr/0022` is about -
/// `serde` owns the field boundary here, so a description cannot cross one whatever it spells, and
/// this body escapes nothing. It is a decision about **who may put words in front of an agent**: an
/// operator whose catalog authors are not the people who decide what their agents are told drops the
/// prose, and a description that still reached an agent through a second transport would make that
/// setting a statement about one surface rather than about the deployment. So the omission is
/// honoured wherever the prose is carried, and the escaping stays where the delimiter is.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CatalogBody {
    provenance: ProvenanceBody,
    /// Which way the operator's `prompt.catalog_prose` setting points, so an absent description is a
    /// fact a client can read rather than one it has to infer.
    ///
    /// The operator's own spelling, echoed rather than translated, which is what keeps this body and
    /// the settings file from acquiring two vocabularies for one decision - the value is
    /// `sutura_config::CatalogProse::as_str`, so a third spelling cannot appear here without
    /// appearing in the settings parser too. It reads `quoted` on a surface that quotes nothing
    /// because the word names the SETTING and not this renderer.
    catalog_prose: &'static str,
    /// Held to [`MAX_CATALOG_LISTING_BYTES`] the same way `sutura-mcp`'s physical-schema listing is: a
    /// metric is carried whole or left out and named in [`Self::omitted_metrics`], never truncated
    /// mid-metric.
    metrics: Vec<MetricBody>,
    /// Metrics that exist and are visible, but were left out of [`Self::metrics`] to hold this
    /// listing under [`MAX_CATALOG_LISTING_BYTES`]. Capped at [`MAX_OMITTED_NAMES`] names; a true
    /// count past that cap is appended as a trailing `"...and N more"` entry rather than a second
    /// field. Read an omitted metric back with `?model=` set to its own model.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    omitted_metrics: Vec<String>,
    /// The catalog's own knowledge - glossary, caveats, terms recorded as undefined and worked
    /// examples - as text, the same text the agent surface's `describe_catalog` returns for this
    /// caller.
    ///
    /// Narrowed to what this caller may see: a note about a metric outside its view is absent, and
    /// the text says the withheld part is withheld rather than empty. Note bodies follow
    /// `catalog_prose`. Descriptive only - no field of a question takes anything from it.
    knowledge: String,
}

/// One metric, as much of it as a caller needs to ask a valid question.
///
/// Descriptive content only. Nothing here selects, widens or parameterizes what executes - the
/// catalog port takes no request context and cannot - so this is a reader's view of a pinned
/// bundle rather than an input to one.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct MetricBody {
    name: String,
    /// The model this metric is defined over - the value `?model=` takes to narrow this same
    /// listing, or to read that model's own physical detail where a transport exposes one. The only
    /// place this name is carried next to a metric.
    model: String,
    /// The author's own prose, or absent where the operator omitted it.
    ///
    /// `Option` rather than an empty string, because *this deployment ships no catalog prose* and
    /// *this metric's description is empty* are different facts, and a client rendering the second
    /// for the first would report an operator's decision as a catalog defect. `catalog_prose` on the
    /// body is what says which an absence is.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    /// Coarsest first, which is the order an anchor is checked at.
    grains: Vec<String>,
    dimensions: Vec<DimensionBody>,
}

/// One dimension of one metric.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct DimensionBody {
    name: String,
    /// The author's own prose, or absent where the operator omitted it. See [`MetricBody`].
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    /// Whether this dimension can be filtered on as well as grouped by.
    filterable: bool,
    /// The values a filter may use, when the catalog declares a set. Absent means the dimension is
    /// groupable and not filterable.
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_values: Option<Vec<String>>,
}

impl CatalogBody {
    /// The reader's view of a caller-scoped catalog, under the prose setting this deployment was
    /// started with.
    ///
    /// **Takes a [`ScopedView`], never a bare `&PinnedDefinitions`** - `docs/adr/0028`. A metric
    /// outside the view is not in `metrics` below, so advertisement and invocation cannot disagree
    /// about which metrics exist; the provenance still names the whole bundle's version and digest,
    /// because that is what `docs/adr/0028` says the digest continues to identify.
    ///
    /// **The knowledge is not scoped here.** It is rendered by `sutura_app::prompt::catalog_knowledge`
    /// over the same view, the function the agent surface's catalog tool calls, so both transports
    /// apply one visibility rule and this body holds no second copy of it.
    ///
    /// **A named constructor rather than a `From`, and the argument is the reason.**
    /// `CatalogProse::default()` is `Quoted`, so a conversion reachable without the setting fails
    /// OPEN: it ships the prose of a deployment that asked for none, which is the defect this
    /// function exists to close. A second argument cannot be left out.
    ///
    /// **`model` narrows `metrics` to the ones tied to that model, nothing else.** Absent, the whole
    /// view's metrics are rendered, held to [`MAX_CATALOG_LISTING_BYTES`] the same way `sutura-mcp`'s
    /// physical-schema listing is bounded - this body carries no model's own columns or
    /// relationships, only the metric descriptions a caller already needs to ask a question, but a
    /// deployment with an unusually large metric vocabulary can still make that list itself large.
    /// `model` naming nothing this caller may see is this function's caller's own job to refuse
    /// before reaching it - see `crate::routes::v1::catalog::catalog`.
    #[must_use]
    pub fn of(view: &ScopedView<'_>, prose: sutura_config::CatalogProse, model: Option<&ModelName>) -> Self {
        // Exhaustive rather than `is_quoted()` in an `if`: a question asked of one variant reads every
        // future spelling as the `else`, and on this setting the `else` withholds prose nobody asked
        // to withhold. The app crate's own type is what `catalog_knowledge` takes, so one mapping
        // serves the descriptions here and the knowledge below.
        let shared = match prose {
            sutura_config::CatalogProse::Quoted => CatalogProse::Quoted,
            sutura_config::CatalogProse::Omitted => CatalogProse::Omitted,
        };
        let quoted = shared.is_quoted();
        let all_metrics: Vec<MetricBody> = view
            .metrics()
            .filter(|metric| model.is_none_or(|name| metric.model() == name))
            .map(|metric| MetricBody {
                name: String::from(metric.name().as_str()),
                model: String::from(metric.model().as_str()),
                description: quoted.then(|| String::from(metric.description())),
                grains: {
                    let mut grains: Vec<Grain> = metric.grains().iter().copied().collect();
                    grains.sort_unstable_by(|a, b| b.cmp(a));
                    grains.into_iter().map(|grain| String::from(grain.as_str())).collect()
                },
                dimensions: metric
                    .dimensions()
                    .values()
                    .map(|dimension| DimensionBody {
                        name: String::from(dimension.name().as_str()),
                        description: quoted.then(|| String::from(dimension.description())),
                        filterable: dimension.is_filterable(),
                        allowed_values: dimension
                            .allowed_values()
                            .map(|values| values.iter().map(|value| String::from(value.as_str())).collect()),
                    })
                    .collect(),
            })
            .collect();
        let (metrics, omitted_names, omitted_count) = fill_to_budget(
            all_metrics,
            MAX_CATALOG_LISTING_BYTES,
            MAX_OMITTED_NAMES,
            |metric| serde_json::to_vec(metric).map_or(0, |bytes| bytes.len()),
            |metric| metric.name.clone(),
        );
        let mut omitted_metrics = omitted_names;
        if omitted_count > omitted_metrics.len() {
            omitted_metrics.push(format!("...and {} more", omitted_count - omitted_metrics.len()));
        }
        Self {
            provenance: bundle_body(view.pinned()),
            catalog_prose: prose.as_str(),
            metrics,
            omitted_metrics,
            knowledge: sutura_app::prompt::catalog_knowledge(view, shared),
        }
    }
}

/// A `model` query parameter that parses but names no model this caller may see - whether it does
/// not exist at all or exists outside this caller's granted audiences is not distinguished, for the
/// same reason [`ScopedView::model`] folds both into one `None`: a message that told the two apart
/// would disclose a restricted model's existence to a caller who was never granted it.
///
/// **Neither [`crate::problem::Failure`] nor [`crate::problem::ProblemBody`], and not
/// [`super::RefusalBody`] either.** This is not a system failure - the request was fine and nothing
/// upstream is unwell - and it is not a governance refusal of a *question*: reading the catalog
/// produces no [`sutura_domain::query::ToolOutcome`], so there is no [`sutura_domain::query::RefusalReason`]
/// variant to carry it, and manufacturing one would put a listing's absence into the enum that a
/// query's own refusals are audited through. `sutura-mcp`'s `server::describe::unknown_model` makes
/// the identical call for the identical reason, as its own ad hoc `ErrorData` outside both
/// `ToolOutcome` and this crate's audited refusal path.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct UnknownModelBody {
    /// A stable code, the same shape as [`crate::problem::ProblemBody::code`] so a client already
    /// branching on one field keeps doing so here.
    code: &'static str,
    detail: String,
}

impl UnknownModelBody {
    pub(crate) fn of(name: &ModelName) -> Self {
        Self {
            code: "model_unknown",
            detail: format!("this catalog defines no model called `{name}`"),
        }
    }
}

impl IntoResponse for UnknownModelBody {
    fn into_response(self) -> Response {
        (StatusCode::NOT_FOUND, Json(self)).into_response()
    }
}

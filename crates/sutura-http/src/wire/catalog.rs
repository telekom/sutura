//! What this catalog defines, as a reader's view - the wire shapes `crate::routes::v1::catalog`
//! serves.
//!
//! Its own module for `cargo xtask max-lines`'s sake: `wire.rs` had no headroom left under the
//! thousand-line limit it enforces and cannot exempt, and this is one whole tool's worth of shape
//! rather than a share of lines - the same split `sutura-mcp`'s own `wire::catalog` already made
//! for the mirror-image reason.

use sutura_domain::model::Grain;
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
    metrics: Vec<MetricBody>,
}

/// One metric, as much of it as a caller needs to ask a valid question.
///
/// Descriptive content only. Nothing here selects, widens or parameterizes what executes - the
/// catalog port takes no request context and cannot - so this is a reader's view of a pinned
/// bundle rather than an input to one.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct MetricBody {
    name: String,
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
    /// **A named constructor rather than a `From`, and the argument is the reason.**
    /// `CatalogProse::default()` is `Quoted`, so a conversion reachable without the setting fails
    /// OPEN: it ships the prose of a deployment that asked for none, which is the defect this
    /// function exists to close. A second argument cannot be left out.
    #[must_use]
    pub fn of(view: &ScopedView<'_>, prose: sutura_config::CatalogProse) -> Self {
        // Exhaustive rather than `is_quoted()` in an `if`, which is what this line was: a question
        // asked of one variant reads every future spelling as the `else`, and on this setting the
        // `else` withholds prose nobody asked to withhold. `sutura-mcp`'s twin makes the same
        // decision unrepresentable with a field type; this surface has one carrier and no text half,
        // so the match is where the whole of it fits.
        let quoted = match prose {
            sutura_config::CatalogProse::Quoted => true,
            sutura_config::CatalogProse::Omitted => false,
        };
        let metrics = view
            .metrics()
            .map(|metric| MetricBody {
                name: String::from(metric.name().as_str()),
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
        Self {
            provenance: bundle_body(view.pinned()),
            catalog_prose: prose.as_str(),
            metrics,
        }
    }
}

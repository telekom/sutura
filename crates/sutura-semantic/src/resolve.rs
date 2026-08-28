//! Resolve: every name in a question is looked up in the pinned snapshot, or the question is
//! refused naming the argument that failed.
//!
//! The lookup never reaches a live catalog. That is what makes an answer independent of what a
//! catalog says at the moment of asking, and it is a property of the argument this function takes
//! rather than of anything it does: it is handed a [`PinnedDefinitions`] and has no way to read
//! anything else.
//!
//! Nothing here builds SQL, and nothing here decides where a statement runs. What comes out is a set
//! of references into the bundle, which the plan stage turns into something owned.

use std::collections::BTreeSet;

use sutura_domain::calendar::TimeRange;
use sutura_domain::catalog::{Dimension, Metric, Model, Relationship};
use sutura_domain::model::{DimensionName, Grain, MetricName, ModelName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::query::{MAX_DIMENSIONS, MAX_RANGE_DAYS, Query, RefusalReason};

/// A dimension, and the join needed to reach it.
pub(crate) struct ResolvedDimension<'a> {
    pub(crate) dimension: &'a Dimension,
    /// `None` when the column is on the metric's own model.
    pub(crate) join: Option<ResolvedJoin<'a>>,
}

/// One declared relationship, and the model on its far side.
pub(crate) struct ResolvedJoin<'a> {
    pub(crate) relationship: &'a Relationship,
    pub(crate) model: &'a Model,
}

/// A filter whose value the bundle has already accepted.
///
/// **The one place a [`DimensionValue`] becomes a `String`, and it is the binding site.** A value is
/// parsed text from here back to the wire; from here on it is a bind parameter, and
/// `sutura_domain::warehouse::ParamValue` is the shape a value takes on its way to a data system - by
/// which point the parsing has already happened. Converting here rather than carrying the newtype
/// into the plan keeps the parse boundary where the check is and leaves the execution port speaking
/// in the two things a data system binds: text and a date.
pub(crate) struct ResolvedFilter<'a> {
    pub(crate) dimension: ResolvedDimension<'a>,
    pub(crate) value: String,
}

/// A question whose every name resolved.
pub(crate) struct Resolution<'a> {
    pub(crate) metric: &'a Metric,
    pub(crate) model: &'a Model,
    pub(crate) grain: Grain,
    pub(crate) range: TimeRange,
    pub(crate) keys: Vec<ResolvedDimension<'a>>,
    pub(crate) filters: Vec<ResolvedFilter<'a>>,
}

/// Why resolution did not produce a resolution.
///
/// Two shapes, deliberately separated. A [`RefusalReason`] is an answer to the caller: the question
/// named something that is not there, or not permitted. An absence is a broken bundle, which the
/// caller did nothing to cause and can do nothing about, so it must not be dressed up as a refusal
/// they might retry differently.
///
/// The absence variants should be unreachable: `Definitions::assemble` checks every one of these
/// cross-references before a bundle exists. They are written out rather than reached with an
/// `expect` because a panic path here is reachable from a catalog file, and the no-panic ban is not
/// conditional on another check having been right.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BundleInconsistent {
    #[error("metric {metric} names model {model}, which the pinned bundle does not hold")]
    NoSuchModel { metric: MetricName, model: ModelName },
    #[error("dimension {dimension} names a relationship the pinned bundle does not hold")]
    RelationshipAbsent { dimension: DimensionName },
    #[error("a relationship names model {model}, which the pinned bundle does not hold")]
    JoinTargetMissing { model: ModelName },
}

/// Refused, or the bundle is broken.
///
/// Kept as two variants rather than one enum with a mixed meaning, because the two go to
/// different places: a refusal becomes a result the caller reads, and a broken bundle becomes an
/// error nobody but an operator can act on.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ResolveError {
    #[error("the question was refused")]
    Refused(RefusalReason),
    #[error(transparent)]
    Bundle(#[from] BundleInconsistent),
}

impl From<RefusalReason> for ResolveError {
    fn from(reason: RefusalReason) -> Self {
        Self::Refused(reason)
    }
}

/// Looks up everything a question names.
pub(crate) fn resolve<'a>(query: &Query, pinned: &'a PinnedDefinitions) -> Result<Resolution<'a>, ResolveError> {
    let definitions = pinned.definitions();
    let metric = definitions
        .metric(query.metric())
        .ok_or_else(|| RefusalReason::MetricUnknown {
            metric: query.metric().clone(),
        })?;
    let model = definitions
        .model(metric.model())
        .ok_or_else(|| BundleInconsistent::NoSuchModel {
            metric: metric.name().clone(),
            model: metric.model().clone(),
        })?;

    if !metric.supports_grain(query.grain()) {
        return Err(RefusalReason::GrainNotSupported {
            metric: metric.name().clone(),
            grain: query.grain(),
        }
        .into());
    }

    // The availability boundary. `TimeRange` guarantees two endpoints and says nothing about the
    // distance between them, and both execution paths aggregate everything the date predicate admits
    // before `ORDER BY`/`LIMIT` runs - so `MAX_ROWS` refuses the answer and nothing caps the scan. It
    // does not truncate it either, which is the correction that came with the refusal: an answer over
    // the first `MAX_ROWS` groups is a wrong total, not a smaller one. This
    // is the only place that cap can live: the type is shared with a metric's anchor range, which a
    // catalog author writes and no agent can influence, so a maximum on the constructor would govern
    // authorship in order to govern requests. Here the range belongs to a *question*, which is what
    // the bound is about.
    //
    // Checked before the dimensions are looked up, so a question that is both too long and misspells
    // a dimension is refused for the reason that is about cost. Not before the grain check, though:
    // a grain the metric never declared is a question that could not have been answered at any span.
    let span = query.range().days();
    if span > MAX_RANGE_DAYS {
        return Err(RefusalReason::TimeRangeTooLong {
            days: span,
            limit: MAX_RANGE_DAYS,
        }
        .into());
    }

    if query.dimensions().len() > MAX_DIMENSIONS {
        return Err(RefusalReason::TooManyDimensions {
            requested: query.dimensions().len(),
            limit: MAX_DIMENSIONS,
        }
        .into());
    }

    let mut seen: BTreeSet<&DimensionName> = BTreeSet::new();
    let mut keys = Vec::with_capacity(query.dimensions().len());
    for name in query.dimensions() {
        if !seen.insert(name) {
            return Err(RefusalReason::DuplicateDimension { dimension: name.clone() }.into());
        }
        keys.push(resolve_dimension(pinned, metric, name)?);
    }

    // A separate `seen` set from the group-by keys: filtering on a dimension that is also grouped by
    // is a perfectly ordinary question ("revenue by region, in the north"), while filtering on the
    // same dimension twice is a contradiction.
    let mut filtered: BTreeSet<&DimensionName> = BTreeSet::new();
    let mut filters = Vec::with_capacity(query.filters().len());
    for filter in query.filters() {
        if !filtered.insert(filter.dimension()) {
            return Err(RefusalReason::DuplicateDimension {
                dimension: filter.dimension().clone(),
            }
            .into());
        }
        let resolved = resolve_dimension(pinned, metric, filter.dimension())?;
        if !resolved.dimension.is_filterable() {
            return Err(RefusalReason::DimensionNotFilterable {
                metric: metric.name().clone(),
                dimension: filter.dimension().clone(),
            }
            .into());
        }
        if !resolved.dimension.permits(filter.value()) {
            return Err(RefusalReason::DimensionValueNotAllowed {
                metric: metric.name().clone(),
                dimension: filter.dimension().clone(),
            }
            .into());
        }
        filters.push(ResolvedFilter {
            dimension: resolved,
            value: String::from(filter.value().as_str()),
        });
    }

    Ok(Resolution {
        metric,
        model,
        grain: query.grain(),
        range: query.range(),
        keys,
        filters,
    })
}

/// Finds one dimension and the model its column lives on.
fn resolve_dimension<'a>(
    pinned: &'a PinnedDefinitions,
    metric: &'a Metric,

    name: &DimensionName,
) -> Result<ResolvedDimension<'a>, ResolveError> {
    let dimension = metric.dimension(name).ok_or_else(|| RefusalReason::DimensionNotPermitted {
        metric: metric.name().clone(),
        dimension: name.clone(),
    })?;
    let Some(relationship_name) = dimension.via() else {
        return Ok(ResolvedDimension { dimension, join: None });
    };
    let definitions = pinned.definitions();
    let relationship = definitions
        .relationship(relationship_name)
        .ok_or_else(|| BundleInconsistent::RelationshipAbsent { dimension: name.clone() })?;
    let joined = definitions
        .model(relationship.target_model())
        .ok_or_else(|| BundleInconsistent::JoinTargetMissing {
            model: relationship.target_model().clone(),
        })?;

    Ok(ResolvedDimension {
        dimension,
        join: Some(ResolvedJoin {
            relationship,
            model: joined,
        }),
    })
}

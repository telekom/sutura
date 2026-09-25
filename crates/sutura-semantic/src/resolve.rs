//! Resolve: every name in a question is looked up in the pinned snapshot, or the question is
//! refused naming the argument that failed.
//!
//! The lookup never reaches a live catalog. That is what makes an answer independent of what a
//! catalog says at the moment of asking, and it is a property of the argument this function takes
//! rather than of anything it does: it is handed a [`ScopedView`] and has no way to read anything
//! else.
//!
//! **A metric outside the view is absent here, not merely undescribed** - `docs/adr/0028`. Resolving
//! one reaches [`RefusalReason::MetricUnknown`] through the same branch a genuinely undeclared
//! metric does, because both read through [`ScopedView::metric`]. Unlisted and unaskable are one
//! mechanism for that reason: there is no second check here that could drift from the first.
//!
//! Nothing here builds SQL, and nothing here decides where a statement runs. What comes out is a set
//! of references into the bundle, which the plan stage turns into something owned.

use std::collections::BTreeSet;

use sutura_domain::calendar::TimeRange;
use sutura_domain::catalog::{Dimension, Metric, Model, Relationship};
use sutura_domain::model::{DimensionName, Grain, MetricName, ModelName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::{Filter, MAX_DIMENSIONS, MAX_RANGE_DAYS, Query, RefusalReason, ResultBound, Top};

/// A dimension, and the chain of joins needed to reach it.
pub(crate) struct ResolvedDimension<'a> {
    pub(crate) dimension: &'a Dimension,
    /// Empty when the column is on the metric's own model; otherwise one per hop, in declared
    /// chain order.
    pub(crate) join: Option<Vec<ResolvedJoin<'a>>>,
}

/// One declared relationship, and the model on its far side.
pub(crate) struct ResolvedJoin<'a> {
    pub(crate) relationship: &'a Relationship,
    pub(crate) model: &'a Model,
}

/// A filter's values, resolved to bind-ready text, in the shape
/// [`Filter`](sutura_domain::query::Filter) carried them.
///
/// **The one place a [`DimensionValue`](sutura_domain::catalog::DimensionValue) becomes a
/// `String`, and it is the binding site.** A value is parsed text from here back to the wire; from
/// here on it is a bind parameter, and `sutura_domain::warehouse::ParamValue` is the shape a value
/// takes on its way to a data system - by which point the parsing has already happened. Converting
/// here rather than carrying the newtype into the plan keeps the parse boundary where the check is
/// and leaves the execution port speaking in the two things a data system binds: text and a date.
pub(crate) enum ResolvedFilterValue {
    Eq(String),
    In(sutura_domain::nonempty::NonEmpty<String>),
    NotIn(sutura_domain::nonempty::NonEmpty<String>),
}

/// One filter, every one of whose values the bundle has already accepted.
pub(crate) struct ResolvedFilter<'a> {
    pub(crate) dimension: ResolvedDimension<'a>,
    pub(crate) value: ResolvedFilterValue,
}

/// A question whose every name resolved.
///
/// **`metric` stays the first entry of `metrics`, and nothing else.** Every plan this crate builds
/// today reads a single metric - `metrics` beyond index 0 exists so this function can validate a
/// multi-metric question (every metric's grain, every requested dimension against every metric,
/// every filter value against every metric's own allowlist) before the plan stage refuses to go
/// further with [`RefusalReason::MultiMetricNotExecutable`].
pub(crate) struct Resolution<'a> {
    pub(crate) metric: &'a Metric,
    pub(crate) metrics: Vec<&'a Metric>,
    pub(crate) model: &'a Model,
    pub(crate) grain: Grain,
    pub(crate) range: TimeRange,
    pub(crate) keys: Vec<ResolvedDimension<'a>>,
    pub(crate) filters: Vec<ResolvedFilter<'a>>,
    pub(crate) top: Option<Top>,
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
pub(crate) fn resolve<'a>(query: &Query, view: &ScopedView<'a>, row_ceiling: RowCeiling) -> Result<Resolution<'a>, ResolveError> {
    // A metric outside the view is absent HERE, which is the whole mechanism: there is no second
    // branch downstream that could disagree about which metrics exist. Looked up in the caller's
    // own order - `metrics[0]` is the "first" `MetricsSpanDifferentModels`/`GrainNotSupported` (a
    // per-metric refusal below) will ever report a mismatch against.
    let mut metrics: Vec<&Metric> = Vec::with_capacity(query.metrics().len());
    for name in query.metrics() {
        let found = view
            .metric(name)
            .ok_or_else(|| RefusalReason::MetricUnknown { metric: name.clone() })?;
        metrics.push(found);
    }
    #[expect(
        clippy::indexing_slicing,
        reason = "`query.metrics()` is a `NonEmpty`, so `metrics` holds at least one entry"
    )]
    let metric = metrics[0];
    let definitions = view.pinned().definitions();
    let model = definitions
        .model(metric.model())
        .ok_or_else(|| BundleInconsistent::NoSuchModel {
            metric: metric.name().clone(),
            model: metric.model().clone(),
        })?;

    // Every later metric must share the first one's model AND its time column - two metrics on one
    // model can still declare two different time columns, and a shared bucket over an ambiguous
    // column is not a smaller question, it is a different one. Compared against the FIRST metric
    // only, never pairwise among the rest, so the refusal always names one metric a caller wrote
    // and the one metric it disagreed with, never a third metric's opinion about the other two.
    for other in metrics.iter().skip(1) {
        if other.model() != metric.model() || other.time_column() != metric.time_column() {
            return Err(RefusalReason::MetricsSpanDifferentModels {
                first: metric.name().clone(),
                other: other.name().clone(),
            }
            .into());
        }
    }

    for named in &metrics {
        if !named.supports_grain(query.grain()) {
            return Err(RefusalReason::GrainNotSupported {
                metric: named.name().clone(),
                grain: query.grain(),
            }
            .into());
        }
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

    // `top.n` asks for a caller-chosen number of rows rather than every group, but it is still
    // bounded by the same cap an unbounded question is refused against: a larger `n` takes that
    // existing refusal rather than a silent clamp to whatever this deployment will certify.
    // Checked here, before anything is looked up, for the reason the range check above gives - a
    // question that is both too wide and misspells a dimension is refused for the reason that is
    // about cost.
    if let Some(top) = query.top()
        && top.n().exceeds_the_row_cap(row_ceiling)
    {
        return Err(RefusalReason::ResultTooLarge {
            bound: ResultBound::Rows {
                limit: row_ceiling.get(),
            },
        }
        .into());
    }

    let mut seen: BTreeSet<&DimensionName> = BTreeSet::new();
    let mut keys = Vec::with_capacity(query.dimensions().len());
    for name in query.dimensions() {
        if !seen.insert(name) {
            return Err(RefusalReason::DuplicateDimension { dimension: name.clone() }.into());
        }
        // A group-by key must be a dimension EVERY named metric declares - one metric lacking it
        // makes the grouped answer ambiguous for that metric's own column, not merely narrower.
        for named in &metrics {
            if named.dimension(name).is_none() {
                return Err(RefusalReason::DimensionNotPermitted {
                    metric: named.name().clone(),
                    dimension: name.clone(),
                }
                .into());
            }
        }
        keys.push(resolve_dimension(view.pinned(), metric, name)?);
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
        for named in &metrics {
            let Some(declared) = named.dimension(filter.dimension()) else {
                return Err(RefusalReason::DimensionNotPermitted {
                    metric: named.name().clone(),
                    dimension: filter.dimension().clone(),
                }
                .into());
            };
            if !declared.is_filterable() {
                return Err(RefusalReason::DimensionNotFilterable {
                    metric: named.name().clone(),
                    dimension: filter.dimension().clone(),
                }
                .into());
            }
            // Every value in the set, not only the first - an `In`/`NotIn` filter's unlisted
            // member is refused by the same variant a single unlisted equality value already
            // used, reached once per value rather than once.
            for value in filter.values() {
                if !declared.permits(value) {
                    return Err(RefusalReason::DimensionValueNotAllowed {
                        metric: named.name().clone(),
                        dimension: filter.dimension().clone(),
                    }
                    .into());
                }
            }
        }
        let resolved = resolve_dimension(view.pinned(), metric, filter.dimension())?;
        let value = match filter {
            Filter::Eq { value, .. } => ResolvedFilterValue::Eq(String::from(value.as_str())),
            Filter::In { values, .. } => ResolvedFilterValue::In(values.map(|v| String::from(v.as_str()))),
            Filter::NotIn { values, .. } => ResolvedFilterValue::NotIn(values.map(|v| String::from(v.as_str()))),
        };
        filters.push(ResolvedFilter {
            dimension: resolved,
            value,
        });
    }

    // Every name, grain, dimension and filter value checked - `metric`'s own resolution above,
    // plus every later metric's model/time-column agreement and every metric's grain/dimension/
    // filter-value checks in the loops above. The only thing left is whether this build can turn
    // more than one into one statement, which is the plan stage's own question. See
    // `RefusalReason::MultiMetricNotExecutable`.
    Ok(Resolution {
        metric,
        metrics,
        model,
        grain: query.grain(),
        range: query.range(),
        keys,
        filters,
        top: query.top(),
    })
}

/// Finds one dimension and the chain of models its hops walk.
///
/// Each hop is looked up against the bundle in turn: the consistency check holds every hop's
/// origin to the previous hop's target, so a mismatch here is a bundle the assembler let through,
/// and `RelationshipAbsent`/`JoinTargetMissing` say so rather than guess.
fn resolve_dimension<'a>(
    pinned: &'a PinnedDefinitions,
    metric: &'a Metric,
    name: &DimensionName,
) -> Result<ResolvedDimension<'a>, ResolveError> {
    let dimension = metric.dimension(name).ok_or_else(|| RefusalReason::DimensionNotPermitted {
        metric: metric.name().clone(),
        dimension: name.clone(),
    })?;
    let Some(chain) = dimension.via() else {
        return Ok(ResolvedDimension { dimension, join: None });
    };
    let definitions = pinned.definitions();
    let mut hops = Vec::with_capacity(chain.len());
    for relationship_name in chain {
        let relationship = definitions
            .relationship(relationship_name)
            .ok_or_else(|| BundleInconsistent::RelationshipAbsent { dimension: name.clone() })?;
        let joined = definitions
            .model(relationship.target_model())
            .ok_or_else(|| BundleInconsistent::JoinTargetMissing {
                model: relationship.target_model().clone(),
            })?;
        hops.push(ResolvedJoin {
            relationship,
            model: joined,
        });
    }

    Ok(ResolvedDimension {
        dimension,
        join: Some(hops),
    })
}

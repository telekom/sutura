//! [`AnchorPlan`]: the one shape [`crate::warehouse::Warehouse::verify_anchor`] accepts.
//!
//! Split out of `plan.rs` for `cargo xtask max-lines`'s per-file cap, not for thematic tidiness:
//! this is the one self-contained piece of that file that does not touch the render or the
//! multi-metric shapes above it.

use crate::calendar::TimeRange;
use crate::catalog::Anchor;
use crate::model::{Grain, MetricName};
use crate::pinned::PinnedDefinitions;

use super::{PredicateOrigin, QueryPlan};

/// The one thing [`Warehouse::verify_anchor`](crate::warehouse::Warehouse::verify_anchor) accepts:
/// a plan the pinned bundle itself agrees is one of its anchors' own.
///
/// # What this type is, and what it is not
///
/// **It is a self-check on the boot path, and it is NOT an authority.** That distinction is the whole
/// of what a second review corrected, and getting it wrong once put a false sentence in ten places
/// across seven files - `docs/adr/0008`'s second amendment to its correction 2 lists them. [`Warehouse::execute`](crate::warehouse::Warehouse::execute) cannot be called without a
/// [`Presented`](crate::identity::Presented); `verify_anchor` deliberately takes no credential,
/// because there is no caller at boot, and it therefore runs under whatever identity the deployment
/// configured that adapter with. So the question is what bounds its INPUT.
///
/// [`Self::of`] answers "did the boot path compile the question it meant to" and nothing stronger.
/// The plan has to compute a metric **this bundle** defines, that metric has to declare an anchor,
/// and the plan has to be that anchor's own question: the metric's coarsest declared grain, exactly
/// the range the anchor certifies, no group-by keys, and no predicate a question asked for. Every one
/// of those facts is read off the [`PinnedDefinitions`] rather than accepted as an argument, which is
/// what makes the check worth making - a caller no longer supplies the anchor it will be compared
/// against.
///
/// **What it cannot do is stop code that wants to.** Every value it reads is publicly constructible -
/// [`QueryPlan::new`], [`PinnedDefinitions::pin`], the metric and range types - and Rust has no
/// cross-crate friend visibility, so a constructor `sutura-app` can call is a constructor anything in
/// the workspace can call. A reviewer defeated the previous version of this type in one function by
/// fabricating the tuple it took, and the fix for that class is not a fifth guard: a shape check over
/// caller-constructible values can only ever be a shape check.
///
/// # So what makes the credential-free path boot-only
///
/// A lint, and it is named here rather than implied: `clippy.toml` bans
/// `sutura_domain::warehouse::Warehouse::verify_anchor`, verified to resolve by writing the call and
/// watching clippy reject it. `sutura_app::verify_anchors` holds the single `#[expect]`, so a second
/// call site is an error under `-D warnings` until somebody writes a second expectation a reviewer
/// sees in the diff. That is the same mechanism the ban on the panicking fragment API and the ban on a
/// bare `spawn_blocking` already rest on. **Its limit is that a lint is not a type:** it reaches this
/// workspace and not a crate outside it, and an `#[allow]` walks past it.
///
/// A genuinely closed constructor is not available. The domain cannot compile a plan - compilation is
/// `sutura-semantic`'s and dependencies point inward - and a token only `sutura-app`'s private `proof`
/// module could mint would have to be constructible from `sutura-domain`, which is the same public
/// door one level down. `docs/adr/0008`'s own corrections are the precedent for saying this rather
/// than implying more.
#[derive(Debug)]
pub struct AnchorPlan<'bundle> {
    plan: &'bundle QueryPlan,
}

/// A plan that is not a declared anchor's own, so the boot path did not compile what it meant to.
///
/// **An error and not a refusal**: reaching it means the boot path compiled something other than the
/// anchor's question, which is a defect here rather than anything about a caller.
///
/// `Serialize` for [`crate::pinned::NotExecutedReason::NotAnAnchor`]'s reason: a boot report
/// serializes the whole reason tree, and D10 stopped that variant from flattening this into a
/// string first.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, thiserror::Error)]
pub enum NotAnAnchorsPlan {
    /// The plan computes more than one metric. An anchor's own question names exactly one, so a
    /// multi-metric plan is never one - checked before the name comparison below, which otherwise
    /// reads only the first measure and would accept a plan that also computes others.
    #[error("an anchor's plan computes exactly one metric, and this one computes {measures}")]
    NotSingleMetric { measures: usize },
    /// The plan computes a different metric from the one whose anchor it would be checked against.
    #[error("this plan computes `{plan}` and the anchor certifies `{anchor}`")]
    NotThatMetric { plan: MetricName, anchor: MetricName },
    /// The bundle this plan is checked against does not define the metric at all.
    #[error("this bundle defines no metric `{metric}`, so it has no anchor to be the plan of")]
    MetricNotDefined { metric: MetricName },
    /// The metric is defined and declares no certified number, so there is no anchor to be a plan of.
    #[error("`{metric}` declares no anchor, so no plan of it is an anchor's")]
    DeclaresNoAnchor { metric: MetricName },
    /// The plan groups by something. An anchor is a metric's own number, not a slice of it.
    #[error("an anchor's plan groups by nothing, and this one groups by {keys}")]
    Grouped { keys: usize },
    /// The plan carries a predicate a question asked for, which an anchor's plan never does.
    #[error("an anchor's plan carries only the metric's own predicates, and this one carries a requested one")]
    Requested,
    /// The plan buckets at a finer grain than the metric's coarsest, so it returns a series.
    ///
    /// **The gap a second review found**, and the reason it is not cosmetic: an anchor certifies one
    /// number, and a plan at `Day` grain over the anchor's range comes back as one row per day. The
    /// comparison downstream insists on exactly one row, so this arrived as a mismatch that reads like
    /// a broken definition - and a plan that returns a series is strictly more than the number the
    /// bundle already publishes.
    ///
    /// `coarsest` is an [`Option`] because a set can be empty, and the empty case is folded in here
    /// rather than given a variant of its own: [`Definitions::assemble`](crate::catalog::Definitions)
    /// refuses a metric that declares no grain, so a separate variant would be one no test could
    /// provoke - and this crate's rule is that an enum does not carry one of those.
    #[error(
        "an anchor of `{metric}` is asked at {} and this plan buckets at {plan}",
        .coarsest.map_or("no grain it declares", Grain::as_str)
    )]
    NotTheCoarsestGrain {
        metric: MetricName,
        plan: Grain,
        coarsest: Option<Grain>,
    },
    /// The plan's range is not the range the anchor's author certified.
    #[error("the anchor certifies {anchor} and this plan covers {plan}")]
    NotTheAnchorsRange { plan: TimeRange, anchor: TimeRange },
}

impl<'bundle> AnchorPlan<'bundle> {
    /// Parses a plan as one of `pinned`'s own anchors', reading every fact it compares off the bundle.
    ///
    /// Takes the metric's name as well as the bundle, because the bundle holds many anchors and the
    /// caller is asserting *which* one this plan is of - so the first check is that the plan agrees.
    /// Everything after that is the bundle's own statement about that metric.
    ///
    /// **It does not take a `sutura_app::Validated` bundle, and it cannot:** validating a bundle is
    /// what this call is part of, so the proof does not exist yet. That is one more reason the type is
    /// a self-check rather than an authority.
    ///
    /// The order of the checks is chosen for the diagnostic rather than for cost - every input is
    /// already bounded and in memory. Which metric, then what the bundle says about that metric, then
    /// the two shapes only a question has, then the two values an anchor's own question pins.
    pub fn of(plan: &'bundle QueryPlan, pinned: &PinnedDefinitions, metric: &MetricName) -> Result<Self, NotAnAnchorsPlan> {
        if plan.measures().len() != 1 {
            return Err(NotAnAnchorsPlan::NotSingleMetric {
                measures: plan.measures().len(),
            });
        }
        let planned_metric = plan.measures().first().metric();
        if planned_metric != metric {
            return Err(NotAnAnchorsPlan::NotThatMetric {
                plan: planned_metric.clone(),
                anchor: metric.clone(),
            });
        }
        let definition = pinned
            .definitions()
            .metric(metric)
            .ok_or_else(|| NotAnAnchorsPlan::MetricNotDefined { metric: metric.clone() })?;
        let anchor: &Anchor = definition
            .anchor()
            .ok_or_else(|| NotAnAnchorsPlan::DeclaresNoAnchor { metric: metric.clone() })?;
        // The coarsest grain the metric declares, because that is the one grain at which the anchor's
        // range yields a single number. Read here rather than passed in: a caller-supplied grain is a
        // caller-supplied answer to the question this check is asking.
        let coarsest = definition.grains().iter().copied().max();
        if !plan.keys().is_empty() {
            return Err(NotAnAnchorsPlan::Grouped { keys: plan.keys().len() });
        }
        if plan
            .filters()
            .iter()
            .any(|filter| matches!(filter.origin(), PredicateOrigin::Requested))
        {
            return Err(NotAnAnchorsPlan::Requested);
        }
        if coarsest != Some(plan.bucket().grain()) {
            return Err(NotAnAnchorsPlan::NotTheCoarsestGrain {
                metric: metric.clone(),
                plan: plan.bucket().grain(),
                coarsest,
            });
        }
        if plan.range() != anchor.range() {
            return Err(NotAnAnchorsPlan::NotTheAnchorsRange {
                plan: plan.range(),
                anchor: anchor.range(),
            });
        }
        Ok(Self { plan })
    }

    /// The plan, for the adapter that has to execute it.
    #[inline]
    #[must_use]
    pub const fn plan(&self) -> &QueryPlan {
        self.plan
    }
}

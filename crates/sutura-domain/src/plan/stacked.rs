//! Several metrics on one data system, answered as one statement on it.
//!
//! `docs/adr/20261031173920-several-metrics-on-one-data-system.md` is the record. Each arm is the
//! whole-answer [`QueryPlan`] its metric would be if asked alone, and the data system joins the arms
//! on their keys and bucket, so sutura holds no arm's rows.

use crate::model::{MetricName, SourceName};
use crate::nonempty::NonEmpty;
use crate::plan::{PlanKey, QueryPlan};
use crate::query::Top;
use crate::warehouse::ParamValue;

/// Two or more whole-answer plans on one data system, joined by it on their keys and bucket.
///
/// The source, keys, bucket and range are the first arm's, and every later arm is checked against
/// them, as [`DrillAcrossPlan`](crate::plan::DrillAcrossPlan) checks its arms. An arm's model, joins,
/// filters and time column are its own.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StackedPlan {
    arms: NonEmpty<QueryPlan>,
    /// Every arm's parameters, in arm order, which is the order a renderer binds them in.
    params: Vec<ParamValue>,
    top: Option<Top>,
}

impl StackedPlan {
    /// A plan over `first`, `second` and every arm of `rest`.
    ///
    /// # Errors
    ///
    /// [`StackedPlanError`] for an arm carrying a `top`, a result label two arms both project, or an
    /// arm whose source, keys, bucket or range differ from the first arm's.
    pub fn new(first: QueryPlan, second: QueryPlan, rest: Vec<QueryPlan>) -> Result<Self, StackedPlanError> {
        let every = || [&first, &second].into_iter().chain(&rest);
        for (index, arm) in every().enumerate() {
            let metric = arm.measures().first().metric().clone();
            if arm.top().is_some() {
                return Err(StackedPlanError::ArmCarriesTop { metric });
            }
            if every().take(index).any(|earlier| {
                earlier
                    .measures()
                    .iter()
                    .any(|e| arm.measures().iter().any(|m| m.label() == e.label()))
            }) {
                return Err(StackedPlanError::DuplicateLabel { metric });
            }
            if arm.source() != first.source() {
                return Err(StackedPlanError::SourceDiffers { metric });
            }
            if !arm
                .keys()
                .iter()
                .map(PlanKey::label)
                .eq(first.keys().iter().map(PlanKey::label))
            {
                return Err(StackedPlanError::KeysDiffer { metric });
            }
            if (arm.bucket().label(), arm.bucket().grain()) != (first.bucket().label(), first.bucket().grain()) {
                return Err(StackedPlanError::BucketDiffers { metric });
            }
            if arm.range() != first.range() {
                return Err(StackedPlanError::RangeDiffers { metric });
            }
        }
        let params = every().flat_map(|arm| arm.params().iter().cloned()).collect();
        let mut tail = vec![second];
        tail.extend(rest);
        Ok(Self {
            arms: NonEmpty::of(first, tail),
            params,
            top: None,
        })
    }

    /// Attaches a whole-question `top`, applied after every arm has been joined.
    #[inline]
    #[must_use]
    pub const fn with_top(mut self, top: Top) -> Self {
        self.top = Some(top);
        self
    }

    #[inline]
    pub const fn top(&self) -> Option<Top> {
        self.top
    }

    /// The arms, in question order of their metrics.
    #[inline]
    pub const fn arms(&self) -> &NonEmpty<QueryPlan> {
        &self.arms
    }

    /// The one data system every arm reads.
    #[inline]
    pub const fn source(&self) -> &SourceName {
        self.arms.first().source()
    }

    /// Every arm's parameters, in arm order.
    #[inline]
    pub fn params(&self) -> &[ParamValue] {
        &self.params
    }

    /// The most rows the joined result may carry, as [`QueryPlan::max_rows`].
    #[inline]
    pub const fn max_rows(&self) -> u32 {
        self.arms.first().max_rows()
    }

    /// One more than [`Self::max_rows`], as [`QueryPlan::row_limit`].
    #[inline]
    pub const fn row_limit(&self) -> u32 {
        self.arms.first().row_limit()
    }

    /// The keys, the bucket, then every arm's measures in arm order.
    pub fn result_labels(&self) -> Vec<String> {
        let mut labels = self.arms.first().result_labels();
        for arm in self.arms.iter().skip(1) {
            labels.extend(arm.measures().iter().map(|m| String::from(m.label())));
        }
        labels
    }
}

/// Why a [`StackedPlan`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StackedPlanError {
    /// An arm carries a `top`, which would cut that arm before the join.
    #[error("the arm of `{metric}` carries a top, which cuts that arm before the join")]
    ArmCarriesTop { metric: MetricName },
    /// Two arms project one result label, which the joined statement would emit twice.
    #[error("the arm of `{metric}` projects a label an earlier arm projects")]
    DuplicateLabel { metric: MetricName },
    /// An arm reads another data system than the first arm, which one statement cannot.
    #[error("the arm of `{metric}` reads another data system than the first arm")]
    SourceDiffers { metric: MetricName },
    /// An arm groups by other key labels, or in another order, than the first arm.
    #[error("the arm of `{metric}` groups by other keys than the first arm")]
    KeysDiffer { metric: MetricName },
    /// An arm buckets by another label or grain than the first arm.
    #[error("the arm of `{metric}` buckets by another label or grain than the first arm")]
    BucketDiffers { metric: MetricName },
    /// An arm reads another time range than the first arm.
    #[error("the arm of `{metric}` reads another time range than the first arm")]
    RangeDiffers { metric: MetricName },
}

#[cfg(test)]
mod tests;

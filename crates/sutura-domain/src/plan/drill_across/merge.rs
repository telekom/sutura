//! The port that joins a drill-across plan's arms: what runs above every adapter, after every leg.
//!
//! A supertrait of [`FederationCombiner`] rather than one more method on it, so the implementor's
//! error and its two classifying predicates are the ones the two-leg combine already answers with. A
//! combiner that joins arms is a combiner; one that does not still combines two legs.
//!
//! The domain names no engine here either: the batches are Arrow, the join is the implementor's.

use crate::model::MetricName;
use crate::plan::DrillAcrossPlan;
use crate::plan::federated::{FederationCombiner, Legs};
use crate::warehouse::ResultBatches;

/// One arm's leg results, in the shape its [`Arm`](crate::plan::Arm) takes.
///
/// The variant is the arm's, so a joined arm's two legs cannot be handed over as a local arm's one.
/// A local arm's result is tagged with the metric of the fact leg it came from, because two local arms'
/// results share a shape and a swap would answer one metric's number under the other's name. Whether
/// the variants line up with the plan's arms, and whether a local arm's metric is the arm's own, is
/// the combiner's to check: a slice cannot say. A joined arm's [`Legs`] carry a side and not a metric, so
/// two joined arms' results are not told apart here.
#[derive(Debug, Clone)]
pub enum ArmLegs<'a> {
    /// The fact and lookup results of an [`Arm::Joined`](crate::plan::Arm::Joined).
    Joined(Legs<'a>),
    /// The one fact result of an [`Arm::Local`](crate::plan::Arm::Local), under the metric its leg
    /// measured.
    Local {
        /// The metric of the fact leg that ran, which the combiner compares to the arm's.
        metric: &'a MetricName,
        /// The leg's result.
        batches: &'a ResultBatches,
    },
}

/// Joins every arm of a [`DrillAcrossPlan`] on the answer's keys and time bucket.
pub trait DrillAcrossCombiner: FederationCombiner {
    /// One answer's rows from every arm's leg results, in the arms' order.
    ///
    /// The arms are joined FULL on every key and the bucket, with a null key equal to a null key, so a
    /// group a metric has no rows for answers null under that metric and a null key two arms share is
    /// one row. The answer's columns are the keys in question order, then the bucket, then one measure
    /// column per arm in the arms' order - the order the single-statement path emits.
    ///
    /// `working_set_bytes` bounds the whole question's combine, not each arm's: the same ceiling
    /// [`FederationCombiner::combine`] states, and an implementor that cannot hold it must refuse.
    ///
    /// # Errors
    ///
    /// [`FederationCombiner::Error`], which its two predicates classify.
    fn merge(&self, plan: &DrillAcrossPlan, arms: &[ArmLegs<'_>], working_set_bytes: u64) -> Result<ResultBatches, Self::Error>;
}

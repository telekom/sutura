//! The port that joins a drill-across plan's arms: what runs above every adapter, after every leg.
//!
//! A supertrait of [`FederationCombiner`] rather than one more method on it, so the implementor's
//! error and its two classifying predicates are the ones the two-leg combine already answers with. A
//! combiner that joins arms is a combiner; one that does not still combines two legs.
//!
//! The domain names no engine here either: the batches are Arrow, the join is the implementor's.

use crate::plan::federated::{ExecuteError, Executed, FederationCombiner, LegResult, LegRunner, Legs};
use crate::plan::{Arm, DrillAcrossPlan, LegPlan};
use crate::warehouse::ResultBatches;

/// One arm's leg results, in the shape its [`Arm`](crate::plan::Arm) takes.
///
/// The variant is the arm's, so a joined arm's two legs cannot be handed over as a local arm's one.
/// Whether the variants line up with the plan's arms is the combiner's to check: a slice cannot say.
#[derive(Debug, Clone, Copy)]
pub enum ArmLegs<'a> {
    /// The fact and lookup results of an [`Arm::Joined`](crate::plan::Arm::Joined).
    Joined(Legs<'a>),
    /// The one fact result of an [`Arm::Local`](crate::plan::Arm::Local).
    Local(&'a ResultBatches),
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

    /// Runs every leg of `plan` through `runner`, then [`Self::merge`]s their results.
    ///
    /// Arm by arm, one leg at a time, so the answer's latency is the sum of its legs': the runner holds
    /// any deadline. A joined arm runs its fact, its lookup, then its second fact when it has one, the
    /// order [`Cut::legs`](crate::plan::Cut::legs) names and not the one [`Arm::legs`] lists, which puts
    /// the second fact before the lookup. The first leg to fail ends the run and no leg after it
    /// starts, and every leg's error wins over the merge's. Provided, for the reason
    /// [`FederationCombiner::plan`] is.
    ///
    /// # Errors
    ///
    /// [`ExecuteError`].
    fn execute_across<R>(&self, plan: &DrillAcrossPlan, runner: &R, working_set_bytes: u64) -> Executed<R, Self>
    where
        R: LegRunner,
    {
        let run = |leg: &LegPlan| -> Ran<R, Self> {
            runner
                .run(leg)
                .map(|batches| LegResult::of(leg, batches))
                .map_err(ExecuteError::Leg)
        };
        let held = plan
            .arms()
            .iter()
            .map(|arm| match arm {
                Arm::Joined(joined) => Ok(Held::Joined {
                    fact: run(joined.fact())?,
                    lookup: run(joined.lookup())?,
                    second_fact: joined.second_fact().map(run).transpose()?,
                }),
                Arm::Local(local) => run(local.fact()).map(Held::Local),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let arms = held
            .iter()
            .map(|arm| match arm {
                Held::Joined {
                    fact,
                    lookup,
                    second_fact,
                } => Legs::of(fact, lookup)
                    .and_then(|legs| legs.with_second_fact(second_fact.as_ref()))
                    .map(ArmLegs::Joined),
                Held::Local(fact) => Ok(ArmLegs::Local(fact.batches())),
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(ExecuteError::Miswired)?;
        self.merge(plan, &arms, working_set_bytes).map_err(ExecuteError::Combine)
    }
}

/// One leg's result, or why it did not answer.
type Ran<R, C> = Result<LegResult, ExecuteError<<R as LegRunner>::Error, <C as FederationCombiner>::Error>>;

/// One arm's leg results, held until every arm has run.
enum Held {
    Joined {
        fact: LegResult,
        lookup: LegResult,
        second_fact: Option<LegResult>,
    },
    Local(LegResult),
}

#[cfg(any(test, feature = "fixtures"))]
impl DrillAcrossCombiner for crate::plan::RefusingCombiner {
    fn merge(
        &self,
        _plan: &DrillAcrossPlan,
        _arms: &[ArmLegs<'_>],
        _working_set_bytes: u64,
    ) -> Result<ResultBatches, Self::Error> {
        Err(crate::plan::NothingCombined)
    }
}

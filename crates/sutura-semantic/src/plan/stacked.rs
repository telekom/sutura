//! Several metrics on one data system: each metric's whole-answer plan, joined by that data system
//! in one statement (`docs/adr/20261031173920-several-metrics-on-one-data-system.md`).
//!
//! Each arm is the plan its metric would have as a question of its own, refused by the same
//! `prechecked`, and [`StackedPlan::new`] checks the arms agree.

use sutura_domain::plan::StackedPlan;

use super::chain::Lookups;
use super::{PlanError, mono_plan, one_statement, prechecked};
use crate::resolve::{Resolution, Resolved};

/// A stacked plan, or the refusal one of its metrics earns.
pub(crate) type Planned = Result<Box<StackedPlan>, PlanError>;

/// The stacked plan for a question over several metrics that are not one grouped statement and
/// all read the first metric's data system, reaching no other; `None` for any other question.
pub(crate) fn plan_resolved(resolved: &Resolved<'_>) -> Option<Planned> {
    match resolved.per_metric.as_slice() {
        [first, second, rest @ ..]
            if !one_statement(resolved) && resolved.per_metric.iter().all(|each| local_to(each, first)) =>
        {
            Some(plan(first, second, rest))
        }
        _ => None,
    }
}

/// Whether `each` reaches no other data system and reads the one `first` reads.
fn local_to(each: &Resolution<'_>, first: &Resolution<'_>) -> bool {
    Lookups::of(each).is_empty() && each.model.source() == first.model.source()
}

/// One whole-answer arm for `first`, `second` and each of `rest`, stacked into one statement.
///
/// # Errors
///
/// A refusal any one metric earns as a question of its own.
fn plan(first: &Resolution<'_>, second: &Resolution<'_>, rest: &[Resolution<'_>]) -> Planned {
    let arm = |resolution: &Resolution<'_>| {
        let (measure, _) = prechecked(resolution)?;
        mono_plan(resolution, measure)
    };
    let rest = rest.iter().map(arm).collect::<Result<Vec<_>, _>>()?;
    let plan = StackedPlan::new(arm(first)?, arm(second)?, rest)?;
    Ok(Box::new(match first.top {
        Some(top) => plan.with_top(top),
        None => plan,
    }))
}

#[cfg(test)]
mod tests;

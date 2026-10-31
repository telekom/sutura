//! Several metrics on one data system: each metric's whole-answer plan, joined by that data system
//! in one statement (`docs/adr/20261031173920-several-metrics-on-one-data-system.md`).
//!
//! Each arm is the plan its metric would have as a question of its own, refused by the same
//! `prechecked`, and [`StackedPlan::new`] checks the arms agree.

use sutura_domain::plan::StackedPlan;

use super::chain::Lookups;
use super::{Plan, PlanError, mono_plan, prechecked};
use crate::resolve::Resolution;

/// Whether `each` reaches no other data system and reads the one `first` reads.
pub(super) fn local_to(each: &Resolution<'_>, first: &Resolution<'_>) -> bool {
    Lookups::of(each).is_empty() && each.model.source() == first.model.source()
}

/// One whole-answer arm for `first`, `second` and each of `rest`, stacked into one statement.
///
/// # Errors
///
/// A refusal any one metric earns as a question of its own.
pub(super) fn plan(first: &Resolution<'_>, second: &Resolution<'_>, rest: &[Resolution<'_>]) -> Result<Plan, PlanError> {
    let arm = |resolution: &Resolution<'_>| {
        let (measure, _) = prechecked(resolution)?;
        mono_plan(resolution, measure)
    };
    let rest = rest.iter().map(arm).collect::<Result<Vec<_>, _>>()?;
    let plan = StackedPlan::new(arm(first)?, arm(second)?, rest)?;
    Ok(Plan::Stacked(Box::new(match first.top {
        Some(top) => plan.with_top(top),
        None => plan,
    })))
}

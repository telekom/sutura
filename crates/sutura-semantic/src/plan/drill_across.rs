//! The drill-across splitter: a question over several metrics becomes one arm per metric.
//!
//! **Each arm is the plan its metric would have as a question of its own**, so every refusal a single
//! metric can earn is raised per arm by the same `prechecked` the one-statement path runs. A metric
//! that reaches a remote dimension is the [`FederatedPlan`] `federated_plan` builds; one that reaches
//! none is a [`LocalArm`], one fact leg grouped by the answer's keys and the bucket. Neither reads
//! two fact tables in one statement, so the chasm trap stays impossible, and the arms are joined above.
//!
//! Nothing here decides the join: [`DrillAcrossPlan::new`] checks the arms agree and the combiner
//! joins them.

use std::collections::BTreeSet;

use sutura_domain::federation::Federation;
use sutura_domain::measure::Measure;
use sutura_domain::plan::{
    Arm, DrillAcrossPlan, DrillAcrossPlanError, FederatedPlan, LegPlan, LocalArm, PlanBucket, PlanColumn, PlanKey, ResultLabel,
    StatementTables,
};
use sutura_domain::query::RefusalReason;

use super::chain::{chain_joins, column_of};
use super::federated::{federated_plan, leg_terms};
use super::{MAX_SOURCES, Plan, PlanError, prechecked, predicates_and_params};
use crate::resolve::{Resolution, ResolvedFilter};

/// An arm for `first`, `second` and each of `rest`, joined above.
///
/// # Errors
///
/// A refusal any one metric earns as a question of its own, [`RefusalReason::QuestionNeedsTooManyLegs`]
/// for a question over more legs than one answer may run, and [`RefusalReason::PlanSpansTooManySources`]
/// for arms that read more than two data systems between them.
pub(super) fn plan(first: &Resolution<'_>, second: &Resolution<'_>, rest: &[Resolution<'_>]) -> Result<Plan, PlanError> {
    let rest = rest.iter().map(arm).collect::<Result<Vec<_>, _>>()?;
    let plan = DrillAcrossPlan::new(arm(first)?, arm(second)?, rest).map_err(|cause| match cause {
        DrillAcrossPlanError::TooManyLegs { legs, limit } => {
            PlanError::Refused(RefusalReason::QuestionNeedsTooManyLegs { legs, limit })
        }
        other => PlanError::NotAssembledAcross(other),
    })?;
    let sources = plan.sources().collect::<BTreeSet<_>>().len();
    if sources > MAX_SOURCES {
        return Err(PlanError::Refused(RefusalReason::PlanSpansTooManySources {
            sources,
            limit: MAX_SOURCES,
        }));
    }
    Ok(Plan::DrillAcross(Box::new(plan)))
}

/// One metric's arm: joined through a lookup leg when it reaches one remote source, local when none.
fn arm(resolution: &Resolution<'_>) -> Result<Arm, PlanError> {
    let (measure, remote) = prechecked(resolution)?;
    match remote {
        0 => local_arm(resolution, measure).map(|local| Arm::Local(Box::new(local))),
        1 => federated_plan(resolution, measure).map(|joined: FederatedPlan| Arm::Joined(Box::new(joined))),
        _ => Err(PlanError::Refused(RefusalReason::PlanSpansTooManySources {
            sources: 1 + remote,
            limit: MAX_SOURCES,
        })),
    }
}

/// A metric whose dimensions are all on its own source: its one fact leg, grouped by every answer key.
///
/// A distinct count's leaf key rides on the leg as one more key, which [`LocalArm::new`] checks.
fn local_arm(resolution: &Resolution<'_>, closed: &Measure) -> Result<LocalArm, PlanError> {
    let model = resolution.model;
    let own_table = model.table_name();
    let federation = Federation::of_metric(closed, model.name());
    let labels: Vec<ResultLabel> = resolution
        .keys
        .iter()
        .map(|key| ResultLabel::dimension(key.dimension.name()))
        .collect();
    let mut keys: Vec<PlanKey> = resolution
        .keys
        .iter()
        .zip(&labels)
        .map(|(key, label)| PlanKey::new(label.clone(), column_of(key, own_table)))
        .collect();
    // No second fact: a local arm names no remote dimension, so a cross-model ratio is refused by
    // `prechecked` before this, and `LocalArm::new` refuses a leaf that still names another model.
    let (terms, _second) = leg_terms(&federation, own_table, own_table, &mut keys);
    let tables = StatementTables::parse(model.table().clone(), chain_joins(resolution, own_table, model.source())).map_err(
        |ambiguous| RefusalReason::PlanTablesShareAnIdentifier {
            table: ambiguous.alias().clone(),
        },
    )?;
    let time_column = PlanColumn::new(own_table.clone(), resolution.metric.time_column().clone());
    let requested: Vec<&ResolvedFilter> = resolution.filters.iter().collect();
    let fact = LegPlan::Fact {
        source: model.source().clone(),
        metric: resolution.metric.name().clone(),
        tables,
        bucket: PlanBucket::new(ResultLabel::bucket(), resolution.grain, time_column.clone()),
        keys,
        terms,
        bindings: predicates_and_params(resolution, &requested, own_table, &time_column)?,
        range: resolution.range,
    };
    Ok(LocalArm::new(fact, federation, labels)?)
}

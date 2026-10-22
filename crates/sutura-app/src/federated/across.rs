//! The drill-across answer path: every arm's legs run through their own adapters, and the combiner
//! joins the arms above them.
//!
//! **One mint and one summed charge, over every leg of every arm** - the guards the two-source
//! path holds, asked once for the whole question rather than once per arm. A source two arms read is
//! one source: it is minted for once, recorded once, and its legs are priced in one sum
//! (`docs/adr/0030`'s all-or-nothing, before any leg executes).
//!
//! **The limit, next to the claim:** the provenance's `ExecutedAs` is keyed by SOURCE, so it records
//! how each data system was reached and not how each leg was - a source two arms read under one
//! posture is one entry. The legs run one at a time, so latency is their sum and the one `Deadline`
//! bounds it.

use sutura_domain::identity::{CredentialBroker, RequestContext};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{Arm, DrillAcrossCombiner, DrillAcrossPlan, RowCeilings};
use sutura_domain::warehouse::Warehouse;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::schema::SchemaDigest;

use super::{LegsAtTheirSources, bounded_answer, credentials_for, executed_as_of, preflight_and_charge, settled, warehoused};
use crate::{Answering, ServiceError, SpendLedger, Warehouses};

/// Executes a question over several metrics: each arm's legs, a pre-flight of each and one charge over all of
/// them, then the combiner's join.
///
/// Reached only from [`Compiled::DrillAcross`](sutura_semantic::Compiled::DrillAcross). A data system
/// that is not open, or whose adapter runs no leg, refuses the whole answer before anything is minted.
/// A drill-across plan carries no `top`: a `top` over several metrics is refused when the question
/// resolves.
pub(crate) fn answer_drill_across<W, B, C>(
    (pinned, schema_digest): (&PinnedDefinitions, &SchemaDigest),
    plan: &DrillAcrossPlan,
    context: &RequestContext,
    broker: &B,
    warehouses: &Warehouses<W>,
    combiner: &C,
    working_set_bytes: u64,
    deadline: Deadline,
    ledger: &SpendLedger,
    row_ceilings: RowCeilings,
) -> Answering<W, B, C>
where
    W: Warehouse + Sync,
    W::Error: Send,
    B: CredentialBroker,
    B::Error: Send,
    C: DrillAcrossCombiner,
    C::Error: Send,
{
    // `Arm::legs` starts at the arm's fact, so the first arm's fact is the first of `plan.legs()`.
    let fact = plan.arms().first().fact();
    let (fact_warehouse, other_legs) = match warehoused(warehouses, fact, plan.legs().into_iter().skip(1)) {
        Ok(found) => found,
        Err(declined) => return Ok(*declined),
    };
    let executed_as = executed_as_of::<_, B, C>(fact, fact_warehouse, &other_legs)?;
    let credentials = match credentials_for::<W, B, C>(broker, context, fact, &other_legs) {
        Ok(credentials) => credentials,
        Err(done) => return *done,
    };
    // The legs are the plan's own, by address: `Charged` spends a token per `&LegPlan` that
    // `plan.legs()` returned, and `execute_across` walks `Arm::legs` over the same plan.
    let every_leg = core::iter::once((fact, fact_warehouse)).chain(other_legs.iter().copied());
    let charged = match preflight_and_charge::<_, B, C>(every_leg, &credentials, deadline, ledger, context) {
        Ok(charged) => charged,
        Err(error) => return error.answered_under(&credentials),
    };
    let runner = LegsAtTheirSources::<W, B, C>::new(charged, warehouses, &credentials, deadline);
    let assembled = match settled::<W, B, C>(
        combiner,
        &credentials,
        combiner.execute_across(plan, &runner, working_set_bytes),
    ) {
        Ok(batches) => batches,
        Err(done) => return *done,
    };
    let answer = assembled.to_rows().map_err(|cause| ServiceError::Unreadable { cause })?;
    bounded_answer::<W, B, C>(
        answer,
        row_ceilings,
        &credentials,
        (pinned, schema_digest),
        executed_as,
        plan.arms().iter().map(Arm::metric),
    )
}

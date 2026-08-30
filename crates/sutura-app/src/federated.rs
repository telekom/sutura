//! The two-source answer path: run each leg through its own adapter and combine the results.
//!
//! Split out of `answer`'s file for the repository's `max-lines` cap, not for thematic tidiness: the
//! two-source path shares every guard the mono path uses (mint once, agree the grant, execute under
//! a presented credential) and differs only in that there are two of each. Keeping it as its own
//! crate module also keeps the one decision only federation makes - an adapter may run a leg
//! ([`Warehouse::EXECUTES_LEGS`]) or the question is refused before anything is minted - in one
//! place.

use sutura_domain::identity::{Agreed, BoundToTheRequest, CredentialBroker, RequestContext, SourceSet};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{Executable, FederatedFailure, FederatedPlan, LegPlan};
use sutura_domain::query::{RefusalReason, ToolOutcome};
use sutura_domain::warehouse::{RowSet, Warehouse};

use crate::{Answered, Answering, ServiceError, Warehouses, exceeds_row_cap, now_in_unix_seconds};

/// The refusal for a source this deployment does not serve, and the one the mono path gives before
/// a credential is minted.
pub(crate) fn source_unavailable(source: &SourceName) -> ToolOutcome {
    ToolOutcome::Refusal {
        reason: RefusalReason::SourceUnavailable { source: source.clone() },
    }
}

/// A non-`Answered` payload carried over `answer`'s error type, so a helper can return a `RowSet`
/// without repeating the two-generic `ServiceError` inline (which trips `type_complexity`).
pub(crate) type FederatedLeg<W, B> = Result<RowSet, ServiceError<<W as Warehouse>::Error, <B as CredentialBroker>::Error>>;

#[expect(
    clippy::too_many_arguments,
    reason = "six inputs is what a federated answer needs; naming each beats a struct nobody else reads"
)]
/// Executes a two-source question: one leg per data system, combined above them.
///
/// Reached only from [`Compiled::Federated`]. Every data system the plan reads must be open AND be
/// able to execute a leg (`Warehouse::EXECUTES_LEGS`), or the answer is refused as
/// [`RefusalReason::FederationNotExecutable`]. That check here, rather than in an adapter, is what
/// keeps a shipped binary - whose adapters declare `false` - refusing a two-source question
/// cleanly instead of letting a typed leg refusal surface as a retryable 503.
///
/// The rest mirrors the mono path leg for leg: one mint over both sources, the agreed grant checked
/// against the request, each leg's own presented credential, and a provenance that records BOTH
/// identities via [`ExecutedAs::and`]. The combiner applies the working-set ceiling, and the answer
/// carries the usual row cap.
pub(crate) fn answer_federated<W, B>(
    pinned: &PinnedDefinitions,
    plan: &FederatedPlan,
    context: &RequestContext,
    broker: &B,
    warehouses: &Warehouses<W>,
    working_set_bytes: u64,
) -> Answering<W, B>
where
    W: Warehouse,
    B: CredentialBroker,
{
    // The capability gate comes FIRST, and that ordering is pinned by an HTTP test: on a build whose
    // adapters cannot run a leg (`EXECUTES_LEGS = false`), a two-source question is refused as
    // `FederationNotExecutable` no matter which sources it names - a build that cannot federate at all
    // says so deterministically, rather than first reporting one of its sources as closed. Only a
    // build that CAN execute a leg then falls through to the per-source availability check. Decided
    // here rather than in an adapter: a shipped binary's adapters declare `false`, so this refuses
    // cleanly before minting or running anything, instead of surfacing a typed leg refusal as a
    // retryable 503.
    if !W::EXECUTES_LEGS {
        return Ok(Answered::declined_before_minting(ToolOutcome::Refusal {
            reason: RefusalReason::FederationNotExecutable,
        }));
    }
    // Both data systems, so a missing one is the same refusal the mono path gives before any
    // credential is minted. `FederatedPlan::new` guarantees the two sources are DISTINCT, so the two
    // registry lookups cannot collide.
    let Some(fact_warehouse) = warehouses.get(plan.fact().source()) else {
        return Ok(Answered::declined_before_minting(source_unavailable(plan.fact().source())));
    };
    let Some(lookup_warehouse) = warehouses.get(plan.lookup().source()) else {
        return Ok(Answered::declined_before_minting(source_unavailable(plan.lookup().source())));
    };
    // Execution records for BOTH legs, so provenance names both identities. `FederatedPlan::new`
    // refuses same-source legs, so the two records belong to distinct sources and `and` cannot
    // collide; the Err arm of `and` is kept (rather than an expect) because the compile cannot know
    // that, and nothing can answer for a splitter invariant that changed.
    let Some(fact_record) = warehouses.executed_on(plan.fact().source()) else {
        return Ok(Answered::declined_before_minting(source_unavailable(plan.fact().source())));
    };
    let Some(lookup_record) = warehouses.executed_on(plan.lookup().source()) else {
        return Ok(Answered::declined_before_minting(source_unavailable(plan.lookup().source())));
    };
    let Some(lookup_posture) = lookup_record.posture(plan.lookup().source()).cloned() else {
        return Ok(Answered::declined_before_minting(source_unavailable(plan.lookup().source())));
    };
    let executed_as = match fact_record.and(plan.lookup().source().clone(), lookup_posture) {
        Ok(executed_as) => executed_as,
        Err(_collision) => {
            // The splitter refuses same-source legs, so a collision is a splitter invariant that
            // changed and nothing can answer for it.
            let metric = match plan.fact() {
                LegPlan::Fact { metric, .. } => metric.as_str(),
                LegPlan::Lookup { .. } => "revenue",
            };
            return Err(ServiceError::Federated {
                cause: FederatedFailure::DuplicateLabels {
                    side: "fact",
                    label: String::from(metric),
                },
            });
        }
    };

    // One mint over the whole set, exactly like the mono path: the broker answers for every source
    // this answer reads, and `agreeing_with` compares that answer against this request.
    let requested = SourceSet::of(plan.fact().source().clone()).and(plan.lookup().source().clone());
    let minted = broker
        .mint(context, &requested)
        .map_err(|cause| ServiceError::Broker { cause })?;
    let credentials = match minted
        .agreeing_with(context.chain().subject(), &requested, now_in_unix_seconds())
        .map_err(|cause| ServiceError::Credentials { cause })?
    {
        Agreed::Refused { source } => {
            return Ok(Answered::declined_before_minting(ToolOutcome::Refusal {
                reason: RefusalReason::CredentialUnavailable { source },
            }));
        }
        Agreed::Granted { credentials } => credentials,
    };

    let fact = execute_leg::<_, B>(fact_warehouse, &credentials, plan.fact())?;
    let lookup = execute_leg::<_, B>(lookup_warehouse, &credentials, plan.lookup())?;

    // The row cap applies to the ANSWER, not to a leg - a leg carries none. The combiner checks the
    // working-set ceiling as it groups; exhaustion here is a governance refusal, anything else the
    // combiner reports is an internal defect.
    let combined = match plan.combine(&fact, &lookup, working_set_bytes) {
        Ok(rows) => rows,
        Err(FederatedFailure::ResourcesExhausted { ceiling_bytes }) => {
            return Ok(Answered::under(
                &credentials,
                ToolOutcome::Refusal {
                    reason: RefusalReason::ResourcesExhausted { ceiling_bytes },
                },
            ));
        }
        Err(cause) => return Err(ServiceError::Federated { cause }),
    };
    if exceeds_row_cap(combined.rows().len(), sutura_domain::plan::MAX_ROWS) {
        return Ok(Answered::under(
            &credentials,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge {
                    limit: sutura_domain::plan::MAX_ROWS,
                },
            },
        ));
    }
    Ok(Answered::under(
        &credentials,
        ToolOutcome::Answer {
            provenance: pinned.provenance(executed_as),
            rows: combined,
        },
    ))
}

/// Runs one leg against its own adapter, under that source's own presented credential.
///
/// The same guards the mono path applies run here for the same reasons: the presented credential
/// agrees with the posture the adapter was opened with, the plan is pre-flighted where that is
/// cheaper than running it, and the credential is still usable this instant. A deadline that ages
/// out during the OTHER leg's work is caught by this leg's own `still_usable_at`.
pub(crate) fn execute_leg<W, B>(warehouse: &W, credentials: &BoundToTheRequest, leg: &LegPlan) -> FederatedLeg<W, B>
where
    W: Warehouse,
    B: CredentialBroker,
{
    let presented = credentials
        .presented_for(leg.source())
        .map_err(|cause| ServiceError::Credentials { cause })?;
    presented
        .agrees_with(warehouse.posture(), leg.source())
        .map_err(|cause| ServiceError::Posture { cause })?;
    warehouse
        .dry_run(Executable::Leg(leg), presented)
        .map_err(|cause| ServiceError::Warehouse { cause })?;
    credentials
        .still_usable_at(now_in_unix_seconds())
        .map_err(|cause| ServiceError::Credentials { cause })?;
    warehouse
        .execute(Executable::Leg(leg), presented)
        .map_err(|cause| ServiceError::Warehouse { cause })
}

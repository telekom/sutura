//! The two-source answer path: run each leg through its own adapter and combine the results.
//!
//! Split out of `answer`'s file for the repository's `max-lines` cap, not for thematic tidiness: the
//! two-source path shares every guard the mono path uses (mint once, agree the grant, execute under
//! a presented credential) and differs only in that there are two of each. Keeping it as its own
//! crate module also keeps the one decision only federation makes - an adapter may run a leg
//! ([`Warehouse::EXECUTES_LEGS`]) or the question is refused before anything is minted - in one
//! place.
//!
//! **This path is reachable from a published artefact now**, because `sutura-exec-datafusion`
//! declares that constant and is non-optional in both shipped binaries. What that does NOT make it
//! is two-identity: every adapter a release links declares
//! `ImpersonationCapability::NoPlaceForASubject`, so both legs of a shipped two-source answer run
//! under one operating-system identity and
//! [`ExecutedAs::and`](sutura_domain::source::ExecutedAs::and) records the same shared posture
//! twice. Single-player federation.

use std::time::Instant;

use sutura_domain::identity::{Agreed, CredentialBroker, RequestContext, SourceSet};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{FederatedAnswerRefusal, FederatedFailure, FederatedPlan};
use sutura_domain::query::{RefusalReason, ResultBound, ToolOutcome};
use sutura_domain::source::ExecutedAs;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{PreFlight, RowSet, Warehouse};

use crate::{
    Answered, Answering, ServiceError, SpendLedger, Warehouses, exceeds_response_bound, exceeds_row_cap, now_in_unix_seconds,
};

/// The refusal for a source this deployment does not serve, and the one the mono path gives before
/// a credential is minted.
pub(crate) fn source_unavailable(source: &SourceName) -> ToolOutcome {
    ToolOutcome::Refusal {
        reason: RefusalReason::SourceUnavailable { source: source.clone() },
    }
}

/// The two shapes a leg's execution can fail as: a governance refusal (the same predicates the mono
/// path asks of its own `execute`) or a real failure that leaves as an error.
pub(crate) enum LegError<E, Q> {
    /// The predicates said this is a bound, so the caller learns a refusal rather than a retryable
    /// `503`.
    Refusal(RefusalReason),
    /// Anything else - a dead data system, a mis-wired source - leaves as the typed error.
    Failure(ServiceError<E, Q>),
}

impl<E, Q> From<ServiceError<E, Q>> for LegError<E, Q> {
    fn from(error: ServiceError<E, Q>) -> Self {
        Self::Failure(error)
    }
}

/// The leg execution's return type, named so `run_leg`'s signature is not a `type_complexity`
/// finding.
pub(crate) type LegResult<W, B> = Result<RowSet, LegError<<W as Warehouse>::Error, <B as CredentialBroker>::Error>>;

/// The leg pre-flight's return type, named for the same `type_complexity` reason [`LegResult`] is.
pub(crate) type LegPreflight<W, B> = Result<PreFlight, LegError<<W as Warehouse>::Error, <B as CredentialBroker>::Error>>;

/// Executes a two-source question: one leg per data system, combined above them.
///
/// Reached only from [`Compiled::Federated`](sutura_semantic::Compiled::Federated). Every data
/// system the plan reads must be open AND be able to execute a leg (`Warehouse::EXECUTES_LEGS`), or
/// the answer is refused as [`RefusalReason::FederationNotExecutable`]. That check here, rather
/// than in an adapter, is what keeps a build whose adapter declares `false` refusing a two-source
/// question cleanly instead of letting a typed leg refusal surface as a retryable 503 - which is
/// still every build linking `sutura-exec-bigquery` or a fake, and is no longer the shipped engine.
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
    deadline: Deadline,
    ledger: &SpendLedger,
) -> Answering<W, B>
where
    W: Warehouse,
    B: CredentialBroker,
{
    // The capability gate comes FIRST, and that ordering is pinned by an HTTP test: on a build whose
    // adapter cannot run a leg (`EXECUTES_LEGS = false`), a two-source question is refused as
    // `FederationNotExecutable` no matter which sources it names - a build that cannot federate at all
    // says so deterministically, rather than first reporting one of its sources as closed. Only a
    // build that CAN execute a leg then falls through to the per-source availability check. Decided
    // here rather than in an adapter, so a build that cannot federate refuses before minting or
    // running anything instead of surfacing a typed leg refusal as a retryable 503.
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
    // Execution records for BOTH legs, so provenance names both identities. Read off the two
    // adapters this answer would run on rather than off a settings tree, for the reason
    // `Warehouse::posture` gives. `FederatedPlan::new` refuses same-source legs, so the two records
    // belong to distinct sources and `and` cannot collide; the Err arm of `and` is kept (rather than
    // an expect) because the compile cannot know that, and nothing can answer for a splitter
    // invariant that changed.
    let record = match ExecutedAs::of(plan.fact().source().clone(), fact_warehouse.posture().clone())
        .and(plan.lookup().source().clone(), lookup_warehouse.posture().clone())
    {
        Ok(record) => record,
        Err(_collision) => {
            // The splitter refuses same-source legs, so a collision is a splitter invariant that
            // changed and nothing can answer for it.
            //
            // A3: `plan.metric()` is the metric this whole `FederatedPlan` measures - `FederatedPlan`
            // already carries it as its own field, so matching `plan.fact()` to reach it was
            // guessing at a value already in hand, and the `LegPlan::Lookup` arm it needed to match
            // against was a fabricated `"revenue"` literal nothing about this plan asserts.
            return Err(ServiceError::Federated {
                cause: FederatedFailure::DuplicateLabels {
                    side: "fact",
                    label: String::from(plan.metric().as_str()),
                },
            });
        }
    };
    // **The one verdict only a two-leg answer needs, and it is HERE - above the mint and above
    // either leg - on purpose.** One answer is one asker: rows a shared identity was permitted to
    // see, added to rows the asking subject was permitted to see, make a total no identity is
    // entitled to, carrying a certified metric name and valid provenance. Refused before a
    // credential exists, so nothing is minted and nothing is read; the `UniformlyExecuted` this
    // returns is then the only thing `pinned.provenance` accepts, which is what stops the rows
    // reaching a caller if this line is ever moved below execution.
    //
    // The refusal carries the posture LABELS. It must never carry a `SourcePosture`: the shared
    // variant holds the operator's acknowledgement prose and both types are `Serialize`.
    let executed_as = match record.uniform() {
        Ok(uniform) => uniform,
        Err(differently) => {
            return Ok(Answered::declined_before_minting(ToolOutcome::Refusal {
                reason: RefusalReason::LegsDecideIdentityDifferently {
                    postures: differently.into_postures(),
                },
            }));
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

    // Both legs pre-flighted before either one executes, and the pair's own estimates summed and
    // charged against the ledger BEFORE either `execute` runs - `docs/adr/0030`'s "all-or-nothing":
    // a two-source answer is refused as a whole rather than after one leg has already spent.
    let fact_preflight = match dry_run_leg::<_, B>(fact_warehouse, &credentials, plan.fact(), deadline) {
        Ok(preflight) => preflight,
        Err(LegError::Refusal(reason)) => {
            return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
        }
        Err(LegError::Failure(error)) => return Err(error),
    };
    // The SAME `Deadline`, shared rather than divided (`docs/adr/0029` decision 3).
    let lookup_preflight = match dry_run_leg::<_, B>(lookup_warehouse, &credentials, plan.lookup(), deadline) {
        Ok(preflight) => preflight,
        Err(LegError::Refusal(reason)) => {
            return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
        }
        Err(LegError::Failure(error)) => return Err(error),
    };
    // Only a `Some` leg is counted, and a leg that priced nothing contributes nothing - "not
    // counted", never "free". Nothing is charged at all when NEITHER leg priced, so an
    // all-`None` federated answer (every adapter but BigQuery, today) never touches the ledger.
    let priced = |preflight: PreFlight| match preflight {
        PreFlight::Accepted {
            estimated_bytes: Some(bytes),
        } => Some(bytes.bytes()),
        PreFlight::Accepted { estimated_bytes: None } | PreFlight::NotAsked => None,
    };
    let fact_estimate = priced(fact_preflight);
    let lookup_estimate = priced(lookup_preflight);
    if fact_estimate.is_some() || lookup_estimate.is_some() {
        let total = fact_estimate.unwrap_or(0).saturating_add(lookup_estimate.unwrap_or(0));
        if let Some(reason) = crate::charge_subject(ledger, context, total, Instant::now()) {
            return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
        }
    }

    let fact = match run_leg::<_, B>(fact_warehouse, &credentials, plan.fact(), deadline) {
        Ok(rows) => rows,
        Err(LegError::Refusal(reason)) => {
            return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
        }
        Err(LegError::Failure(error)) => return Err(error),
    };
    let lookup = match run_leg::<_, B>(lookup_warehouse, &credentials, plan.lookup(), deadline) {
        Ok(rows) => rows,
        Err(LegError::Refusal(reason)) => {
            return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
        }
        Err(LegError::Failure(error)) => return Err(error),
    };

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
        // D19 + A4: a deterministic combine failure - the same plan against the same rows fails
        // again - is a governance refusal, not a data-system outage. `FederatedAnswerRefusal::of`
        // is the total classification; `None` is left as the remaining wiring defects, which stay
        // an internal `ServiceError` because no caller caused them and none can fix them.
        Err(cause) => match FederatedAnswerRefusal::of(&cause) {
            Some(federated) => {
                return Ok(Answered::under(
                    &credentials,
                    ToolOutcome::Refusal {
                        reason: RefusalReason::FederatedAnswerNotWellFormed { federated },
                    },
                ));
            }
            None => return Err(ServiceError::Federated { cause }),
        },
    };
    if exceeds_row_cap(combined.rows().len(), sutura_domain::plan::MAX_ROWS) {
        return Ok(Answered::under(
            &credentials,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge {
                    bound: sutura_domain::query::ResultBound::Rows {
                        limit: sutura_domain::plan::MAX_ROWS,
                    },
                },
            },
        ));
    }
    // The same third bound the mono-source path checks, over the COMBINED result.
    if let Some(limit_bytes) = exceeds_response_bound(&combined) {
        return Ok(Answered::under(
            &credentials,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge {
                    bound: ResultBound::Encoded { limit_bytes },
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

// The leg's own pre-flight and its own execute, split out to `federated/leg.rs` for
// `cargo xtask max-lines`'s cap AND for `docs/adr/0030`'s "all-or-nothing before any leg
// executes": both legs are dry-run (`dry_run_leg`) - and the pair's own estimates summed and
// charged against the spend ledger, here in `answer_federated` - before either one's `execute`
// (`run_leg`) runs. Their tests stay here, in `mod tests` below, exercising them through
// `answer_federated` exactly as `execute_leg`'s did before the split - moving PRODUCTION code
// across files is the gate's ordinary case, unlike moving tests away from the implementation they
// hold red-before-green evidence for (see that module's own doc for why).
pub(crate) use leg::{dry_run_leg, run_leg};
mod leg;

/// The federated answer orchestration: the two-source answer path exercised above fake leg-executing
/// adapters.
///
/// **Beside the implementation on purpose, and the reason is a gate rather than taste.** These tests
/// sat in a `tests_fed.rs` of their own until `cargo xtask test-causality` was run against them: that
/// gate proves red-before-green by reverting the non-test files and re-running the changed tests, and
/// `answer_federated` arrived in a NEW file, so reverting it took `lib.rs`'s `mod` declaration with it
/// and the suite was silently ORPHANED - never compiled, never run, and reported as "green against
/// base behaviour". Impl and tests in one file is the shape the gate recognises and reports as
/// `NOT MECHANICALLY SEPARABLE`, which is the honest verdict here: the two-source path and its first
/// test arrived together, so the evidence goes in the pull request rather than out of a reverted tree.
///
/// The fakes - `LegsWarehouse`, which declares `EXECUTES_LEGS`, and the shared posture they are opened
/// with - sit between the orchestrator and the domain's own combiner suite: what these pin is that
/// `answer_federated` mints once, runs both legs, records both identities, and refuses the working-set
/// ceiling, while the arithmetic of combining is proven in `sutura_domain::plan::federated`.
#[cfg(test)]
mod tests;

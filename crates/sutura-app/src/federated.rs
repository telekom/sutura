//! The two-source answer path: run each leg through its own adapter and combine the results.
//!
//! Split out of `answer`'s file for the repository's `max-lines` cap, not for thematic tidiness: the
//! two-source path shares every guard the mono path uses (mint once, agree the grant, execute under
//! a presented credential) and differs only in that there are two of each. Keeping it as its own
//! crate module also keeps the one decision only federation makes - an adapter may run a leg
//! ([`Warehouse::executes_legs`]) or the question is refused before anything is minted - in one
//! place.
//!
//! **The gate is PER LEG, read off each leg's own adapter INSTANCE, not off one type parameter.**
//! `Warehouses<W>` is generic in one `W`, so a build that links exactly one kind still asks the same
//! question it always did - both legs share a type, so both legs share an answer. What changed is
//! the shape of the question: `telekom/sutura#112`'s closed enum lets `W` itself be "whichever
//! kind this build linked", and `Warehouse::IMPERSONATION` being a required associated constant
//! means that enum cannot carry a type-level `EXECUTES_LEGS` either - an enum-wide constant would
//! have to lie for one of its variants. `Warehouse::executes_legs` is the instance method every
//! adapter gets for free, defaulted to its own constant, so a heterogeneous registry asks each leg's
//! CONCRETE adapter rather than the enum wrapping it - and a leg-capable kind sitting beside a
//! leg-declining one refuses only the leg that cannot run, never the whole answer.
//!
//! **This path is reachable from a published artefact now**, because `sutura-exec-datafusion`
//! declares the constant and is non-optional in the shipped binary, and `sutura-exec-postgres`
//! declares it behind a feature the shipped binary enables. What that does NOT make it is
//! per-subject: both leg-executing kinds a release links declare
//! `ImpersonationCapability::NoPlaceForASubject`, so
//! [`ExecutedAs::and`](sutura_domain::source::ExecutedAs::and) records the same shared posture
//! twice - but the same posture is not the same identity, since each Postgres leg runs as its own
//! source entry's database role. Single-player federation.
//!
//! **Two legs CAN now each run as the asking subject, and only on a `bigquery` build.**
//! `sutura-exec-bigquery` declares the constant since `telekom/sutura#929` and is the one adapter
//! declaring `PerSubjectCredential`, so `sutura serve --features bigquery` over two `bigquery`
//! sources reaches this path with two IMPERSONATING legs rather than two shared ones - and a
//! `bigquery` leg beside any other adapter's is a CROSS-POSTURE answer, disclosed per leg rather
//! than refused (`docs/adr/0040`). `BigQuery` being the only impersonating adapter is why that had to
//! be: every heterogeneous federation is cross-posture by construction. The single mint below does
//! not collapse them: that adapter's
//! `DeclaredPrincipalBroker::mint` walks the `SourceSet` and resolves each source's OWN declared
//! account for the asking subject out of that source's own map, so one mint over two sources yields
//! one credential per leg
//! (`one_subject_federating_two_sources_is_minted_each_sources_own_declared_account`). **The limits,
//! beside the claim:** no published artefact links that adapter, and no federated answer has been
//! produced against a real dataset - what is held is that each leg renders for the dialect and is
//! submitted with that subject's own credential and that source's configured byte ceiling.

use std::time::Instant;

use sutura_domain::identity::{Agreed, BoundToTheRequest, CredentialBroker, RequestContext, SourceSet};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{FederatedPlan, FederationCombiner, LegResult, Legs, RowCeiling};
use sutura_domain::query::{RefusalReason, ResultBound, ToolOutcome};
use sutura_domain::source::ExecutedAs;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{PreFlight, ResultBatches, RowSet, Warehouse};

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
pub(crate) enum LegError<E, Q, C> {
    /// The predicates said this is a bound, so the caller learns a refusal rather than a retryable
    /// `503`.
    Refusal(RefusalReason),
    /// Anything else - a dead data system, a mis-wired source - leaves as the typed error.
    Failure(ServiceError<E, Q, C>),
}

impl<E, Q, C> From<ServiceError<E, Q, C>> for LegError<E, Q, C> {
    fn from(error: ServiceError<E, Q, C>) -> Self {
        Self::Failure(error)
    }
}

/// The leg execution's return type, named so `run_leg`'s signature is not a `type_complexity`
/// finding.
pub(crate) type LegAnswer<W, B, C> =
    Result<ResultBatches, LegError<<W as Warehouse>::Error, <B as CredentialBroker>::Error, <C as FederationCombiner>::Error>>;

/// The leg pre-flight's return type, named for the same `type_complexity` reason [`LegResult`] is.
pub(crate) type LegPreflight<W, B, C> =
    Result<PreFlight, LegError<<W as Warehouse>::Error, <B as CredentialBroker>::Error, <C as FederationCombiner>::Error>>;

/// Executes a two-source question: one leg per data system, combined above them.
///
/// Reached only from [`Compiled::Federated`](sutura_semantic::Compiled::Federated). Every data
/// system the plan reads must be open AND be able to execute a leg
/// (`Warehouse::executes_legs`, asked of that source's own adapter), or the answer is refused as
/// [`RefusalReason::FederationNotExecutable`]. That check here, rather than in an adapter, is what
/// keeps a build whose adapter declares `false` refusing a two-source question cleanly instead of
/// letting a typed leg refusal surface as a retryable 503. That is no longer the shipped engine, and
/// since `telekom/sutura#929` it is no longer `sutura-exec-bigquery` either - what still reaches it
/// is an adapter with no leg venue of its own (`sutura-exec-postgres`, `sutura-exec-clickhouse`,
/// `sutura-exec-oracle`) or a fake taking the port's default.
///
/// The rest mirrors the mono path leg for leg: one mint over both sources, the agreed grant checked
/// against the request, each leg's own presented credential, and a provenance that records BOTH
/// identities via [`ExecutedAs::and`]. The combiner applies the working-set ceiling, and the answer
/// carries the usual row cap - unless the plan carries case 2's `top`
/// (`github.com/telekom/sutura#777`), in which case `row_ceiling` bounds the combined set BEFORE it
/// is ranked, and the row cap below never runs for this answer at all.
pub(crate) fn answer_federated<W, B, C>(
    pinned: &PinnedDefinitions,
    plan: &FederatedPlan,
    context: &RequestContext,
    broker: &B,
    warehouses: &Warehouses<W>,
    combiner: &C,
    working_set_bytes: u64,
    deadline: Deadline,
    ledger: &SpendLedger,
    row_ceiling: RowCeiling,
) -> Answering<W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    // Both data systems, so a missing one is the same refusal the mono path gives before any
    // credential is minted. `FederatedPlan::new` guarantees the two sources are DISTINCT, so the two
    // registry lookups cannot collide. Looked up BEFORE the capability gate below, for the reason
    // the gate itself is now per-instance rather than per-type: there is no adapter to ask about a
    // source nobody opened.
    let Some(fact_warehouse) = warehouses.get(plan.fact().source()) else {
        return Ok(Answered::declined_before_minting(source_unavailable(plan.fact().source())));
    };
    let Some(lookup_warehouse) = warehouses.get(plan.lookup().source()) else {
        return Ok(Answered::declined_before_minting(source_unavailable(plan.lookup().source())));
    };
    // The capability gate, PER LEG: a heterogeneous registry - one closed-enum variant per LINKED
    // kind - can mix a leg-capable adapter with one that takes the port's default, so this reads
    // each leg's own instance rather than one `W::EXECUTES_LEGS` for the whole build. Refuses
    // before minting or running anything, exactly as the type-level check used to, whichever side
    // (or both) cannot run a leg.
    if !fact_warehouse.executes_legs() || !lookup_warehouse.executes_legs() {
        return Ok(Answered::declined_before_minting(ToolOutcome::Refusal {
            reason: RefusalReason::FederationNotExecutable,
        }));
    }
    // Execution records for BOTH legs, so provenance names both identities. Read off the two
    // adapters this answer would run on rather than off a settings tree, for the reason
    // `Warehouse::posture` gives. `FederatedPlan::new` refuses same-source legs, so the two records
    // belong to distinct sources and `and` cannot collide; the Err arm of `and` is kept (rather than
    // an expect) because the compile cannot know that, and nothing can answer for a splitter
    // invariant that changed.
    let executed_as = match ExecutedAs::of(plan.fact().source().clone(), fact_warehouse.posture().clone())
        .and(plan.lookup().source().clone(), lookup_warehouse.posture().clone())
    {
        Ok(record) => record,
        Err(_collision) => {
            // The splitter refuses same-source legs, so a collision is a splitter invariant that
            // changed and nothing can answer for it.
            //
            // It leaves as its OWN typed shape now. This arm used to mint a
            // a `DuplicateLabels` combine failure - a failure about a leg RESULT - out of the
            // plan's metric name, which said nothing true about what went wrong; the combine's
            // failure vocabulary belongs to the combiner since `docs/adr/0039` step 3 anyway.
            return Err(ServiceError::Miswired {
                cause: crate::FederationMiswired::LegsCollide {
                    at: plan.fact().source().clone(),
                },
            });
        }
    };
    // **No verdict over the two postures, and `docs/adr/0040` is why the one that stood here is
    // gone.** It refused an answer whose legs decided identity differently - and BigQuery is the
    // only impersonating adapter, so that refused every heterogeneous federation rather than an
    // edge case. The reasoning it carried stays TRUE and is written rather than softened: rows a
    // shared identity was permitted to see, added to rows the asking subject was permitted to see,
    // make a total no identity is entitled to, under a certified metric name and valid provenance.
    //
    // What answers for it is not `executed_as`, which reaches a caller in the SAME body as the rows
    // and is therefore a disclosure rather than a control. It is the boot-time acknowledgement: a mixed
    // answer does span two authorization domains, an operator declared each one in writing on its
    // own entry before this process started, and the answer names which leg came from which. Held
    // three ways, each ahead of this line - `sutura_config::Settings::refusals` refuses the
    // deployment before a listener binds; the settings parse produces no shared `SourcePosture` at
    // all without a witness; and every adapter constructor refuses a source with no identity, which
    // arrives here as the `source_unavailable` above.
    //
    // **And one budget over two authorization domains.** `charge_subject` below sums both legs'
    // estimates against ONE subject key (`docs/adr/0030`, all-or-nothing) regardless of which leg
    // ran as whom.
    //
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
    let fact_preflight = match dry_run_leg::<_, B, C>(fact_warehouse, &credentials, plan.fact(), deadline) {
        Ok(preflight) => preflight,
        Err(LegError::Refusal(reason)) => {
            return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
        }
        Err(LegError::Failure(error)) => return Err(error),
    };
    // The SAME `Deadline`, shared rather than divided (`docs/adr/0029` decision 3).
    let lookup_preflight = match dry_run_leg::<_, B, C>(lookup_warehouse, &credentials, plan.lookup(), deadline) {
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

    // **The legs' results never become rows, and that is `docs/adr/0039` step 2's whole point
    // meeting step 3's.** Each `LegResult` is tagged with the side its own `LegPlan` names, so the
    // pair the combiner receives cannot have the two legs swapped - which would group the fact
    // leg's measure by the lookup leg's keys and answer a wrong number under a certified name.
    let fact = match run_leg::<_, B, C>(fact_warehouse, &credentials, plan.fact(), deadline) {
        Ok(batches) => LegResult::of(plan.fact(), batches),
        Err(LegError::Refusal(reason)) => {
            return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
        }
        Err(LegError::Failure(error)) => return Err(error),
    };
    let lookup = match run_leg::<_, B, C>(lookup_warehouse, &credentials, plan.lookup(), deadline) {
        Ok(batches) => LegResult::of(plan.lookup(), batches),
        Err(LegError::Refusal(reason)) => {
            return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
        }
        Err(LegError::Failure(error)) => return Err(error),
    };
    let legs = Legs::of(&fact, &lookup).map_err(|cause| ServiceError::Miswired {
        cause: crate::FederationMiswired::LegsAreNotOneOfEach { cause },
    })?;

    // **The combine, through the port.** The row cap applies to the ANSWER and not to a leg - a leg
    // carries none - and the two governance outcomes are taken off the combiner's error FIRST, in
    // the order the mono path asks its own adapter: the ceiling, then a deterministic refusal about
    // what the legs returned, then this workspace's own defect.
    let assembled = match combiner.combine(plan, legs, working_set_bytes) {
        Ok(batches) => batches,
        Err(cause) => {
            if let Some(ceiling_bytes) = combiner.working_set_exhausted(&cause) {
                return Ok(Answered::under(
                    &credentials,
                    ToolOutcome::Refusal {
                        reason: RefusalReason::ResourcesExhausted { ceiling_bytes },
                    },
                ));
            }
            // D19 + A4: a deterministic combine failure - the same plan against the same legs fails
            // again - is a governance refusal and not a data-system outage, so a caller is never
            // told to retry one. `None` is this workspace's own wiring, which stays an internal
            // `ServiceError` because no caller caused it and none can fix it.
            if let Some(federated) = combiner.answer_not_well_formed(&cause) {
                return Ok(Answered::under(
                    &credentials,
                    ToolOutcome::Refusal {
                        reason: RefusalReason::FederatedAnswerNotWellFormed { federated },
                    },
                ));
            }
            return Err(ServiceError::Combine { cause });
        }
    };
    // The presentation edge for a two-source answer, and the only decode on this path: the legs
    // stayed Arrow all the way into the combine, so one call turns the ANSWER into the rows the
    // bounds below count and a caller reads.
    let answer = assembled.to_rows().map_err(|cause| ServiceError::Unreadable { cause })?;
    // A `top` still needs ranking HERE, after the combine - `github.com/telekom/sutura#777`'s
    // case 2. `FederatedPlan::combine` sorts its own output ascending by key cell UNCONDITIONALLY
    // (its own contract for a question with no `top`), which un-ranks a joined answer, so the rank
    // is taken once, above the combine.
    if let Some(top) = plan.top() {
        return ranked_answer::<W, B, C>(plan, &answer, row_ceiling, top, &credentials, pinned, executed_as);
    }
    if exceeds_row_cap(answer.rows().len(), sutura_domain::plan::MAX_ROWS) {
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
    if let Some(limit_bytes) = exceeds_response_bound(&answer) {
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
            rows: answer,
        },
    ))
}

/// Either case's `top`, applied to an already-answer answer - split out of [`answer_federated`]
/// for `cargo xtask max-lines`'s per-function cap.
///
/// The set BEFORE ranking, not after - the top ten of an arbitrary `row_ceiling` rows
/// is not the top ten of the dimension, so this is refused before it is ranked rather than answered
/// with a caveat. `top.n()` is checked against this same ceiling at resolve time
/// (`sutura_semantic::resolve`), so a `top.n()` this large could not have
/// compiled at all - this is strictly about the WIDTH of the group-by beneath it, which `top.n()`
/// says nothing about.
fn ranked_answer<W, B, C>(
    plan: &FederatedPlan,
    combined: &RowSet,
    row_ceiling: RowCeiling,
    top: sutura_domain::query::Top,
    credentials: &BoundToTheRequest,
    pinned: &PinnedDefinitions,
    executed_as: ExecutedAs,
) -> Answering<W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    if plan.top().is_some() && exceeds_row_cap(combined.rows().len(), row_ceiling.get()) {
        return Ok(Answered::under(
            credentials,
            ToolOutcome::Refusal {
                reason: RefusalReason::TopOverUncertifiedRows {
                    ceiling: row_ceiling.get(),
                },
            },
        ));
    }
    let ranked = FederatedPlan::rank(combined, top).map_err(|cause| ServiceError::Miswired {
        cause: crate::FederationMiswired::RankedAnswer { cause },
    })?;
    if let Some(limit_bytes) = exceeds_response_bound(&ranked) {
        return Ok(Answered::under(
            credentials,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge {
                    bound: ResultBound::Encoded { limit_bytes },
                },
            },
        ));
    }
    Ok(Answered::under(
        credentials,
        ToolOutcome::Answer {
            provenance: pinned.provenance(executed_as),
            rows: ranked,
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

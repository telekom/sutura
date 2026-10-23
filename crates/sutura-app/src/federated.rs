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
//! **This path is reachable from a published artefact.** `DataFusion` and `Postgres` execute legs
//! under `ImpersonationCapability::NoPlaceForASubject`, so a federation between them records the
//! shared posture twice. Each source may still hold a different deployment identity. `BigQuery` also
//! ships and is the one adapter that can execute a leg as the asking subject.
//!
//! **Two legs CAN each run as the asking subject when both sources are `bigquery`.**
//! `sutura-exec-bigquery` declares the constant since `telekom/sutura#929` and is the one adapter
//! declaring `PerSubjectCredential`, so `sutura serve --features bigquery` over two `bigquery`
//! sources reaches this path with two IMPERSONATING legs rather than two shared ones - and a
//! `bigquery` leg beside a leg-executing shared-posture adapter is a CROSS-POSTURE answer,
//! disclosed per leg rather than refused (`docs/adr/0040`). `BigQuery` then being the only impersonating adapter is why that had to
//! be: every `BigQuery` federation with a shared-posture adapter is cross-posture by construction.
//! The single mint below does not collapse them: `DeclaredPrincipalBroker::mint` walks the
//! `SourceSet` and presents one credential per leg, each the asking subject's own. **The limits,
//! beside the claim:** the published artefact links that adapter, but no federated answer has been
//! produced against a real dataset - what is held is that each leg renders for the dialect and is
//! submitted with that subject's own credential and that source's configured byte ceiling.

use sutura_domain::identity::{BoundToTheRequest, CredentialBroker, RequestContext, SourceSet};
use sutura_domain::model::{MetricName, SourceName};
use sutura_domain::pinned::{PinnedDefinitions, Provenance};
use sutura_domain::plan::{Ceilings, ExecuteError, FederatedPlan, FederationCombiner, LegPlan};
use sutura_domain::query::{RefusalReason, ResultBound, ToolOutcome};
use sutura_domain::source::ExecutedAs;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::schema::SchemaDigest;
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

/// What a leg's error concludes the question with, over the same three error types.
pub(crate) type Concluded<E, Q, C> = Result<Answered, ServiceError<E, Q, C>>;

impl<E, Q, C> LegError<E, Q, C> {
    /// A refusal is the answer, under the credentials the leg ran with; a failure leaves as an error.
    pub(crate) fn answered_under(self, credentials: &BoundToTheRequest) -> Concluded<E, Q, C> {
        match self {
            Self::Refusal(reason) => Ok(Answered::refused(credentials, reason)),
            Self::Failure(error) => Err(error),
        }
    }
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

/// The token of every leg pre-flighted and charged, or why not - named for the same
/// `type_complexity` reason.
pub(crate) type LegCleared<'p, W, B, C> =
    Result<leg::Charged<'p>, LegError<<W as Warehouse>::Error, <B as CredentialBroker>::Error, <C as FederationCombiner>::Error>>;

/// The execution record of every leg, or the miswiring that kept one out of it.
type Recorded<W, B, C> =
    Result<ExecutedAs, ServiceError<<W as Warehouse>::Error, <B as CredentialBroker>::Error, <C as FederationCombiner>::Error>>;

/// The leg pre-flight's return type, named for the same `type_complexity` reason [`LegAnswer`] is.
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
/// carries a row cap, `ceilings.federated()` (`github.com/telekom/sutura#828`) - unless the plan
/// carries case 2's `top` (`github.com/telekom/sutura#777`), in which case `ceilings.top()` bounds
/// the combined set BEFORE it is ranked, and the row cap below never runs for this answer at all.
pub(crate) fn answer_federated<W, B, C>(
    (pinned, schema_digest): (&PinnedDefinitions, &SchemaDigest),
    plan: &FederatedPlan,
    context: &RequestContext,
    broker: &B,
    warehouses: &Warehouses<W>,
    combiner: &C,
    working_set_bytes: u64,
    deadline: Deadline,
    ledger: &SpendLedger,
    ceilings: Ceilings,
) -> Answering<W, B, C>
where
    W: Warehouse + Sync,
    W::Error: Send,
    B: CredentialBroker,
    B::Error: Send,
    C: FederationCombiner,
    C::Error: Send,
{
    // Every leg's data system, so a missing one is the same refusal the mono path gives before any
    // credential is minted. Read off `FederatedPlan::legs`, the one list of the plan's legs, so no
    // lookup leg can be left out here, of the credential request or of the posture record.
    let (fact_warehouse, other_legs) = match warehoused(warehouses, plan.fact(), plan.legs().into_iter().skip(1)) {
        Ok(found) => found,
        Err(declined) => return Ok(*declined),
    };
    let executed_as = executed_as_of::<_, B, C>(plan.fact(), fact_warehouse, &other_legs)?;
    // **No verdict over the two postures, and `docs/adr/0040` is why the one that stood here is
    // gone.** It refused an answer whose legs decided identity differently - and BigQuery was then the
    // only impersonating adapter, so that prevented BigQuery from federating with any
    // shared-posture adapter. The reasoning it carried stays TRUE: rows a shared identity was
    // permitted to see, added to rows the asking subject was permitted to see,
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
    let credentials = match credentials_for::<W, B, C>(broker, context, plan.fact(), &other_legs) {
        Ok(credentials) => credentials,
        Err(done) => return *done,
    };

    // **The cut, then the charge, then the legs** (`docs/adr/20261010140559-datafusion-federates-the-sql-tool-and-the-metrics.md`
    // decision 4). `plan` executes nothing. Every leg of the cut is pre-flighted, and what they
    // priced is charged as one sum before any leg executes - `docs/adr/0030`'s all-or-nothing,
    // under the SAME `Deadline` (`docs/adr/0029` decision 3).
    let cut = combiner.plan(plan);
    let every_leg = match cut
        .legs()
        .map(|leg| warehouses.get(leg.source()).map(|warehouse| (leg, warehouse)).ok_or(leg))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(every_leg) => every_leg,
        Err(leg) => return Ok(Answered::under(&credentials, source_unavailable(leg.source()))),
    };
    let charged = match preflight_and_charge::<_, B, C>(every_leg, &credentials, deadline, ledger, context) {
        Ok(charged) => charged,
        Err(error) => return error.answered_under(&credentials),
    };
    // **The legs run through the combiner, which calls back into `run_leg` through the leg runner
    // port**, so each leg keeps its credential, deadline and refusal mapping here. The combiner runs
    // each lookup leg on a scoped OS thread of its own beside the fact leg on THIS thread: `answer_federated`
    // already holds the blocking-pool thread a transport spawned, and a pool task per leg would
    // deadlock on the pool's own slots at the configured ceiling. A failed fact leg does not cancel
    // a started lookup leg, which still runs, billed at the source under the caller's identity.
    //
    // **The combine.** The row cap applies to the ANSWER and not to a leg - a leg carries none - and
    // a leg's refusal wins over a combine failure; see [`settled`].
    let runner = LegsAtTheirSources::<W, B, C>::new(charged, warehouses, &credentials, deadline);
    let assembled = match settled::<W, B, C>(combiner, &credentials, combiner.execute(cut, &runner, working_set_bytes)) {
        Ok(batches) => batches,
        Err(done) => return *done,
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
        return ranked_answer::<W, B, C>(plan, &answer, ceilings, top, &credentials, pinned, schema_digest, executed_as);
    }
    bounded_answer::<W, B, C>(
        answer,
        ceilings,
        &credentials,
        (pinned, schema_digest),
        executed_as,
        [plan.metric()],
    )
}

/// The answer a step ends the question with, boxed so an `Err` of it is not paid on every `Ok`.
type Ended<W, B, C> = Box<Answering<W, B, C>>;

/// The fact leg's adapter and every other leg beside its own, or the refusal that is the answer.
type Warehoused<'a, W> = Result<(&'a W, Vec<(&'a LegPlan, &'a W)>), Box<Answered>>;

/// The credentials a mint granted, or the answer that stops the question.
type Credentialed<W, B, C> = Result<BoundToTheRequest, Ended<W, B, C>>;

/// What a combine produced, or the answer that stops the question.
type Settled<W, B, C> = Result<ResultBatches, Ended<W, B, C>>;

/// What a combiner's `execute` or `execute_across` returned, over this path's own leg errors.
type Ran<W, B, C> = Result<
    ResultBatches,
    ExecuteError<
        LegError<<W as Warehouse>::Error, <B as CredentialBroker>::Error, <C as FederationCombiner>::Error>,
        <C as FederationCombiner>::Error,
    >,
>;

/// Every leg beside the adapter its source opened, or the refusal that is the answer before anything
/// is minted: a source nobody opened, or an adapter that runs no leg.
///
/// Asked of each leg's own adapter INSTANCE: a heterogeneous registry - one closed-enum variant per
/// LINKED kind - can mix a leg-capable adapter with one that takes the port's default, so this reads
/// each leg's instance rather than one `W::EXECUTES_LEGS` for the whole build, and refuses whichever
/// leg cannot run. Looked up BEFORE that gate: there is no adapter to ask about a source nobody opened.
fn warehoused<'a, W>(
    warehouses: &'a Warehouses<W>,
    fact: &'a LegPlan,
    others: impl IntoIterator<Item = &'a LegPlan>,
) -> Warehoused<'a, W>
where
    W: Warehouse,
{
    let unavailable = |leg: &LegPlan| Box::new(Answered::declined_before_minting(source_unavailable(leg.source())));
    let fact_warehouse = warehouses.get(fact.source()).ok_or_else(|| unavailable(fact))?;
    let others = others
        .into_iter()
        .map(|leg| {
            warehouses
                .get(leg.source())
                .map(|warehouse| (leg, warehouse))
                .ok_or_else(|| unavailable(leg))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !fact_warehouse.executes_legs() || others.iter().any(|&(_, warehouse)| !warehouse.executes_legs()) {
        return Err(Box::new(Answered::declined_before_minting(ToolOutcome::Refusal {
            reason: RefusalReason::FederationNotExecutable,
        })));
    }
    Ok((fact_warehouse, others))
}

/// One mint over every source the legs read, compared with the request, or the answer that stops here.
///
/// A source two legs read is asked for once: a `SourceSet` holds each source once.
fn credentials_for<W, B, C>(
    broker: &B,
    context: &RequestContext,
    fact: &LegPlan,
    others: &[(&LegPlan, &W)],
) -> Credentialed<W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    let requested = others
        .iter()
        .fold(SourceSet::of(fact.source().clone()), |requested, &(leg, _)| {
            requested.and(leg.source().clone())
        });
    let minted = broker
        .mint(context, &requested)
        .map_err(|cause| Box::new(Err(ServiceError::Broker { cause })))?;
    let agreed = minted
        .agreeing_with(context.chain().subject(), &requested, now_in_unix_seconds())
        .map_err(|cause| Box::new(Err(ServiceError::Credentials { cause })))?;
    crate::granted(agreed).map_err(|reason| Box::new(Ok(Answered::declined_before_minting(ToolOutcome::Refusal { reason }))))
}

/// What a combine produced, or the answer a failure of it is.
///
/// A leg's refusal wins over a combine failure. The two governance outcomes are taken off the
/// combiner's error FIRST, in the order the mono path asks its own adapter: the ceiling, then a
/// deterministic refusal about what the legs returned, then this workspace's own defect.
fn settled<W, B, C>(combiner: &C, credentials: &BoundToTheRequest, executed: Ran<W, B, C>) -> Settled<W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    let cause = match executed {
        Ok(batches) => return Ok(batches),
        Err(ExecuteError::Leg(error)) => return Err(Box::new(error.answered_under(credentials))),
        Err(ExecuteError::Miswired(cause)) => {
            return Err(Box::new(Err(ServiceError::Miswired {
                cause: crate::FederationMiswired::LegsMiswired { cause },
            })));
        }
        Err(ExecuteError::Combine(cause)) => cause,
    };
    if let Some(ceiling_bytes) = combiner.working_set_exhausted(&cause) {
        return Err(Box::new(Ok(Answered::refused(
            credentials,
            RefusalReason::ResourcesExhausted { ceiling_bytes },
        ))));
    }
    // D19 + A4: a deterministic combine failure - the same plan against the same legs fails
    // again - is a governance refusal and not a data-system outage, so a caller is never
    // told to retry one. `None` is this workspace's own wiring, which stays an internal
    // `ServiceError` because no caller caused it and none can fix it.
    Err(Box::new(combiner.answer_not_well_formed(&cause).map_or_else(
        || Err(ServiceError::Combine { cause }),
        |federated| {
            Ok(Answered::refused(
                credentials,
                RefusalReason::FederatedAnswerNotWellFormed { federated },
            ))
        },
    )))
}

/// A combined answer held to the federated row cap and the response bound, then certified for
/// `metrics`.
fn bounded_answer<'m, W, B, C>(
    answer: RowSet,
    ceilings: Ceilings,
    credentials: &BoundToTheRequest,
    (pinned, schema_digest): (&PinnedDefinitions, &SchemaDigest),
    executed_as: ExecutedAs,
    metrics: impl IntoIterator<Item = &'m MetricName>,
) -> Answering<W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    let federated_ceiling = ceilings.federated().get();
    if exceeds_row_cap(answer.rows().len(), federated_ceiling) {
        return Ok(too_many_rows(credentials, federated_ceiling));
    }
    // The same third bound the mono-source path checks, over the COMBINED result.
    if let Some(limit_bytes) = exceeds_response_bound(&answer) {
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
            provenance: certified_provenance(pinned, schema_digest, executed_as, metrics)?,
            rows: answer,
        },
    ))
}

/// The provenance for the metrics a federated answer was formed over, or the typed cause a splitter
/// or registry invariant that changed would leave behind.
///
/// Its own function rather than a `map_err` closure repeated at [`bounded_answer`]'s and
/// [`ranked_answer`]'s exit points: a federated plan names exactly one metric (`FederatedPlan::metric`
/// is a field, not a first-of-several) and a drill-across plan names one per arm.
type Certified<E, M, C> = Result<Provenance, ServiceError<E, M, C>>;

fn certified_provenance<'m, E, M, C>(
    pinned: &PinnedDefinitions,
    schema_digest: &SchemaDigest,
    executed_as: ExecutedAs,
    metrics: impl IntoIterator<Item = &'m MetricName>,
) -> Certified<E, M, C> {
    pinned
        .provenance_for(schema_digest, executed_as, metrics)
        .map_err(|cause| ServiceError::AnswersDoNotCertify { cause })
}

/// The refusal for a combined answer over the federated ceiling, naming that ceiling.
const fn too_many_rows(credentials: &BoundToTheRequest, limit: u32) -> Answered {
    Answered::under(
        credentials,
        ToolOutcome::Refusal {
            reason: RefusalReason::ResultTooLarge {
                bound: ResultBound::Rows { limit },
            },
        },
    )
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
///
/// **Two ceilings.** The combined set is held to `top_row_ceiling` (the set a rank is certified
/// over) first, then to the federated ceiling as every federated answer is. The ranked rows are a
/// subset of it, so they are bounded too.
fn ranked_answer<W, B, C>(
    plan: &FederatedPlan,
    combined: &RowSet,
    ceilings: Ceilings,
    top: sutura_domain::query::Top,
    credentials: &BoundToTheRequest,
    pinned: &PinnedDefinitions,
    schema_digest: &SchemaDigest,
    executed_as: ExecutedAs,
) -> Answering<W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    let top_ceiling = ceilings.top().get();
    if plan.top().is_some() && exceeds_row_cap(combined.rows().len(), top_ceiling) {
        return Ok(Answered::under(
            credentials,
            ToolOutcome::Refusal {
                reason: RefusalReason::TopOverUncertifiedRows { ceiling: top_ceiling },
            },
        ));
    }
    let federated_ceiling = ceilings.federated().get();
    if exceeds_row_cap(combined.rows().len(), federated_ceiling) {
        return Ok(too_many_rows(credentials, federated_ceiling));
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
            provenance: certified_provenance(pinned, schema_digest, executed_as, [plan.metric()])?,
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
/// Execution records for every leg, so provenance names each source. Read off the adapters this
/// answer would run on rather than off a settings tree, for the reason `Warehouse::posture` gives.
///
/// A source two legs read - a second fact on the fact's source, or a chain that crosses away and
/// comes back - is recorded once, under the one adapter both legs run on.
fn executed_as_of<W, B, C>(fact: &LegPlan, warehouse: &W, other_legs: &[(&LegPlan, &W)]) -> Recorded<W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    let collide = |at: &SourceName| ServiceError::Miswired {
        cause: crate::FederationMiswired::LegsCollide { at: at.clone() },
    };
    other_legs.iter().try_fold(
        ExecutedAs::of(fact.source().clone(), warehouse.posture().clone()),
        |record, &(leg, warehouse)| {
            if record.posture(leg.source()).is_some() {
                return Ok(record);
            }
            record
                .and(leg.source().clone(), warehouse.posture().clone())
                .map_err(|_collision| collide(leg.source()))
        },
    )
}

mod across;
pub(crate) use across::answer_drill_across;
pub(crate) use leg::{LegsAtTheirSources, preflight_and_charge};
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

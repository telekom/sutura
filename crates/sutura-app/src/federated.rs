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

use sutura_domain::identity::{Agreed, Attribution, CredentialBroker, RequestContext, SourceSet};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{FederatedFailure, FederatedPlan, LegPlan};
use sutura_domain::query::{RefusalReason, ResultBound, ToolOutcome};
use sutura_domain::source::ExecutedAs;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{PreFlight, RowSet, Warehouse};

use crate::{
    Answered, Answering, Charge, ServiceError, SpendLedger, Warehouses, exceeds_response_bound, exceeds_row_cap,
    now_in_unix_seconds,
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
        let subject = match context.chain().attribution() {
            Attribution::BareSubject { subject } | Attribution::ActingFor { subject, .. } => subject,
        };
        if let Charge::Refused { reset_after } = ledger.charge(subject, total, Instant::now()) {
            return Ok(Answered::under(
                &credentials,
                ToolOutcome::Refusal {
                    reason: RefusalReason::BudgetExhausted {
                        reset_after_seconds: reset_after.as_secs(),
                    },
                },
            ));
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
        Err(cause) => return Err(ServiceError::Federated { cause }),
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
mod tests {
    use std::time::Instant;

    use super::answer_federated;
    use crate::Warehouses;
    use crate::spend::SpendLedger;
    use crate::tests::{asked_by_a_person, bundle, june, metric, shared, test_deadline};
    use crate::tests_support::{
        AdapterFailure, DryRunOutcome, FixedBroker, LegDeadlineExceededWarehouse, LegPreflightWarehouse, RecordingLegsWarehouse,
    };
    use sutura_domain::model::{Grain, SourceName};
    use sutura_domain::query::{RefusalReason, ResultBound, ToolOutcome};
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_domain::warehouse::{RowSet, Value};

    // ---------------------------------------------------------------------------
    // The federated answer orchestration
    // ---------------------------------------------------------------------------

    /// An amount of time only the budget test would cross: every correctness path below hands the
    /// combiner this effectively unbounded ceiling so only the memory-ceiling test reaches the refusal.
    const FEDERATED_BUDGET: u64 = 1 << 30;

    /// A fact leg and a lookup leg on two sources, the shape the splitter emits for a two-source
    /// question. The rows the two fake warehouses return are the same fixture the domain's combiner
    /// tests feed it, so this test is about the ORCHESTRATOR (mint once, run both, record both) and
    /// leans on the combiner suite for the arithmetic.
    fn federated_plan() -> sutura_domain::plan::FederatedPlan {
        use sutura_domain::measure::{AggregatedColumn, Measure, Term};
        use sutura_domain::model::Aggregate;
        use sutura_domain::model::{ColumnName, DimensionName, TableName};
        use sutura_domain::plan::{
            AnswerKey, InternalLabel, LegPlan, PlanBindings, PlanBucket, PlanColumn, PlanKey, ResultLabel, StatementTables,
        };

        let fact_source = SourceName::parse("facts").expect("a test source");
        let lookup_source = SourceName::parse("geo").expect("a test source");
        let table = TableName::parse("fct_subscription_monthly").expect("a test table");
        let column = |n: &str| ColumnName::parse(n).expect("a test column");
        let tablecol = |n: &str| PlanColumn::new(table.clone(), column(n));
        let dimension = |n: &str| DimensionName::parse(n).expect("a test dimension");
        let key = |n: &str| PlanKey::new(ResultLabel::dimension(&dimension(n)), tablecol(n));
        // The link column, under the reserved label both legs project it as. The splitter names it
        // from `InternalLabel` and this fake does too, so the shape stays the shape it emits.
        let link = || PlanKey::new(ResultLabel::internal(InternalLabel::Link), tablecol("customer_key"));
        let bucket = |c: &str| PlanBucket::new(ResultLabel::bucket(), Grain::Month, PlanColumn::new(table.clone(), column(c)));

        let fact = LegPlan::Fact {
            source: fact_source,
            metric: metric(),
            tables: StatementTables::only(table.clone()),
            bucket: bucket("month"),
            keys: vec![key("product_family"), link()],
            terms: Vec::new(),
            bindings: PlanBindings::none(),
            range: june(),
        };
        let lookup = LegPlan::Lookup {
            source: lookup_source,
            table: table.clone().into(),
            keys: vec![link(), key("region")],
            bindings: PlanBindings::none(),
        };
        let sum = Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))));
        sutura_domain::plan::FederatedPlan::new(
            metric(),
            ResultLabel::measure(&metric()),
            bucket("month"),
            fact,
            lookup,
            true,
            sutura_domain::federation::Federation::of(&sum),
            vec![
                AnswerKey::fact(ResultLabel::dimension(&dimension("product_family"))),
                AnswerKey::lookup(ResultLabel::dimension(&dimension("region"))),
            ],
        )
        .expect("a valid two-leg plan")
    }

    fn federated_fact_rows() -> RowSet {
        use sutura_domain::plan::InternalLabel;
        RowSet::new(
            vec![
                String::from("product_family"),
                InternalLabel::Link.label(),
                String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
                InternalLabel::Leaf(0).label(),
            ],
            vec![
                vec![
                    Value::Text("A".into()),
                    Value::Text("c1".into()),
                    Value::Text("2026-06".into()),
                    Value::Integer(100),
                ],
                vec![
                    Value::Text("A".into()),
                    Value::Text("c2".into()),
                    Value::Text("2026-06".into()),
                    Value::Integer(200),
                ],
                vec![
                    Value::Text("B".into()),
                    Value::Text("c1".into()),
                    Value::Text("2026-06".into()),
                    Value::Integer(50),
                ],
            ],
        )
        .expect("a well-formed test fact result")
    }

    fn federated_lookup_rows() -> RowSet {
        use sutura_domain::plan::InternalLabel;
        RowSet::new(
            vec![InternalLabel::Link.label(), String::from("region")],
            vec![
                vec![Value::Text("c1".into()), Value::Text("north".into())],
                vec![Value::Text("c2".into()), Value::Text("north".into())],
            ],
        )
        .expect("a well-formed test lookup result")
    }

    #[test]
    fn a_federated_answer_mints_once_runs_both_legs_and_records_both_identities() {
        let fact_source = SourceName::parse("facts").expect("a test source");
        let lookup_source = SourceName::parse("geo").expect("a test source");
        let shared = shared();
        let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
            fact_source,
            shared.clone(),
            federated_fact_rows(),
        ))
        .and(crate::tests_support::LegsWarehouse::answering(
            lookup_source,
            shared,
            federated_lookup_rows(),
        ))
        .expect("two sources, one registry");

        let broker = crate::tests_support::CountingBroker::default();
        let plan = federated_plan();
        let outcome = answer_federated(
            &bundle(),
            &plan,
            &asked_by_a_person(),
            &broker,
            &warehouses,
            FEDERATED_BUDGET,
            test_deadline(),
            &SpendLedger::no_budget(),
        )
        .expect("a federated answer is not an error")
        .into_outcome();

        let ToolOutcome::Answer { provenance, .. } = outcome else {
            panic!("a two-source question whose adapters execute legs is answered, not {outcome:?}");
        };
        let legs: Vec<&str> = provenance.executed_as().legs().map(|(source, _)| source.as_str()).collect();
        assert_eq!(
            legs,
            vec!["facts", "geo"],
            "provenance records BOTH identities a federated answer ran as"
        );
        assert_eq!(
            broker.asked(),
            1,
            "a federated answer mints once over both sources, not once per leg"
        );
    }

    #[test]
    fn a_federated_answer_sums_both_legs_estimates_before_charging_the_ledger_once() {
        // `docs/adr/0030`'s "all-or-nothing": neither leg's own price is under the ceiling here, and
        // the answer must still refuse, because the CEILING is over the SUM (600 + 600 = 1200) and
        // not over either leg alone (600 < 1000). A mutation that summed only the first leg's
        // estimate would see 600, admit it, and answer instead of refusing - which is exactly the
        // substitute this cell exists to catch where `test-causality` cannot separate it from base.
        let fact_source = SourceName::parse("facts").expect("a test source");
        let lookup_source = SourceName::parse("geo").expect("a test source");
        let shared = shared();
        let warehouses = Warehouses::of(crate::tests_support::PricedWarehouse::pricing(
            fact_source,
            shared.clone(),
            federated_fact_rows(),
            Some(600),
        ))
        .and(crate::tests_support::PricedWarehouse::pricing(
            lookup_source,
            shared,
            federated_lookup_rows(),
            Some(600),
        ))
        .expect("two sources, one registry");
        let ledger = SpendLedger::new(Some(crate::spend::SpendBudget::new(
            1_000,
            std::time::Duration::from_secs(60),
        )));
        let outcome = answer_federated(
            &bundle(),
            &federated_plan(),
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &warehouses,
            FEDERATED_BUDGET,
            test_deadline(),
            &ledger,
        )
        .expect("a refusal is an Ok")
        .into_outcome();
        assert!(
            matches!(
                outcome,
                ToolOutcome::Refusal {
                    reason: RefusalReason::BudgetExhausted { .. }
                }
            ),
            "the summed estimate (1200) is over the ceiling (1000), even though neither leg alone is: {outcome:?}"
        );
        // The ledger's POSITION, not just the outcome, and on BOTH legs: a mutation that charged
        // after `run_leg` (rather than after both `dry_run_leg`s and before either `run_leg`) would
        // still refuse - the estimates are unchanged - so the outcome assertion above cannot see it.
        for leg in ["facts", "geo"] {
            let source = SourceName::parse(leg).expect("a test source");
            assert_eq!(
                warehouses.get(&source).expect("both legs are registered").executions(),
                0,
                "a federated answer refused for spend must never reach either leg's `execute`"
            );
        }
    }

    /// `docs/adr/0029` decision 3's own RED cell - split out so this file stays under the
    /// `max-lines` cap it was already at before this record.
    mod deadline_test;

    /// Three leg-level refusals, split out for the same `max-lines` reason `deadline_test` was.
    ///
    /// `#[cfg(test)]` here is redundant under this file's own gate and present anyway - the same
    /// reason `telekom/sutura#657` states at its own `mod refresh;`: `xtask test-causality` reverts
    /// a file that adds no `#[test]` of its own, and a bare `mod leg_refusal_test;` declares
    /// nothing the scan reads as one, so a later diff dropping only that line back to base would
    /// orphan this module rather than fail loud. The attribute makes the declaration itself read as
    /// `TestModule`, which the gate holds.
    #[cfg(test)]
    mod leg_refusal_test;

    #[test]
    fn a_federated_fact_preflight_refusal_is_not_a_partial_answer() {
        let shared = shared();
        let fact = LegPreflightWarehouse::new(
            SourceName::parse("facts").expect("a test source"),
            shared.clone(),
            federated_fact_rows(),
            DryRunOutcome::SourceRefused,
        );
        let lookup = LegPreflightWarehouse::new(
            SourceName::parse("geo").expect("a test source"),
            shared,
            federated_lookup_rows(),
            DryRunOutcome::Accepted,
        );
        let warehouses = Warehouses::of(fact).and(lookup).expect("two sources, one registry");
        let outcome = answer_federated(
            &bundle(),
            &federated_plan(),
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &warehouses,
            FEDERATED_BUDGET,
            test_deadline(),
            &SpendLedger::no_budget(),
        )
        .expect("a pre-flight refusal is a governed answer")
        .into_outcome();
        assert!(matches!(
            outcome,
            ToolOutcome::Refusal { reason: RefusalReason::SourceRefused { ref source } }
                if source.as_str() == "facts"
        ));
        assert_eq!(
            warehouses
                .get(&SourceName::parse("facts").expect("a test source"))
                .expect("facts is registered")
                .executions(),
            0
        );
        assert_eq!(
            warehouses
                .get(&SourceName::parse("geo").expect("a test source"))
                .expect("geo is registered")
                .executions(),
            0
        );
    }

    #[test]
    fn a_federated_lookup_preflight_refusal_means_neither_leg_ever_executes() {
        // **Renamed, and the number this test pins CHANGED with it.** Before the spend ledger, the
        // fact leg's own `dry_run` and `execute` ran as one step, so a fact leg that dry-ran clean
        // executed before the lookup leg's `dry_run` was even asked - a refusal on the lookup side
        // then discarded a fact leg that had ALREADY run. Now every leg is dry-run before either one
        // executes (`docs/adr/0030`'s "all-or-nothing", needed so the two legs' estimates can be
        // summed and charged once before any leg spends anything real) - so a lookup pre-flight
        // refusal is caught before the fact leg's own `execute` is ever reached, and the fact data
        // system is never asked at all.
        let shared = shared();
        let fact = LegPreflightWarehouse::new(
            SourceName::parse("facts").expect("a test source"),
            shared.clone(),
            federated_fact_rows(),
            DryRunOutcome::Accepted,
        );
        let lookup = LegPreflightWarehouse::new(
            SourceName::parse("geo").expect("a test source"),
            shared,
            federated_lookup_rows(),
            DryRunOutcome::SourceRefused,
        );
        let warehouses = Warehouses::of(fact).and(lookup).expect("two sources, one registry");
        let outcome = answer_federated(
            &bundle(),
            &federated_plan(),
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &warehouses,
            FEDERATED_BUDGET,
            test_deadline(),
            &SpendLedger::no_budget(),
        )
        .expect("a pre-flight refusal is a governed answer")
        .into_outcome();
        assert!(matches!(
            outcome,
            ToolOutcome::Refusal { reason: RefusalReason::SourceRefused { ref source } }
                if source.as_str() == "geo"
        ));
        assert_eq!(
            warehouses
                .get(&SourceName::parse("facts").expect("a test source"))
                .expect("facts is registered")
                .executions(),
            0,
            "the fact leg's own dry run succeeded, but its execute must never be reached once the \
             lookup leg's pre-flight refuses"
        );
        assert_eq!(
            warehouses
                .get(&SourceName::parse("geo").expect("a test source"))
                .expect("geo is registered")
                .executions(),
            0
        );
    }

    #[test]
    fn a_federated_fact_preflight_failure_keeps_its_warehouse_cause_and_no_partial_answer() {
        let shared = shared();
        let fact = LegPreflightWarehouse::new(
            SourceName::parse("facts").expect("a test source"),
            shared.clone(),
            federated_fact_rows(),
            DryRunOutcome::TransientFailure,
        );
        let lookup = LegPreflightWarehouse::new(
            SourceName::parse("geo").expect("a test source"),
            shared,
            federated_lookup_rows(),
            DryRunOutcome::Accepted,
        );
        let warehouses = Warehouses::of(fact).and(lookup).expect("two sources, one registry");
        let failure = answer_federated(
            &bundle(),
            &federated_plan(),
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &warehouses,
            FEDERATED_BUDGET,
            test_deadline(),
            &SpendLedger::no_budget(),
        )
        .expect_err("a transient pre-flight failure remains a warehouse error");
        assert!(matches!(
            failure,
            super::ServiceError::Warehouse {
                cause: AdapterFailure::Statement {
                    cause: super::super::tests_support::DriverFailure
                }
            }
        ));
        assert_eq!(
            warehouses
                .get(&SourceName::parse("facts").expect("a test source"))
                .expect("facts is registered")
                .executions(),
            0
        );
        assert_eq!(
            warehouses
                .get(&SourceName::parse("geo").expect("a test source"))
                .expect("geo is registered")
                .executions(),
            0,
            "the failed fact leg cannot leave a partial answer or execute the lookup"
        );
    }

    #[test]
    fn a_federated_lookup_preflight_failure_means_the_fact_leg_never_executes() {
        // Renamed for the same reason and with the same pinned number changed as
        // `a_federated_lookup_preflight_refusal_means_neither_leg_ever_executes` above: every leg is
        // now dry-run before either one executes, so a failure discovered while pre-flighting the
        // lookup leg is caught before the fact leg's own `execute` ever runs.
        let shared = shared();
        let fact = LegPreflightWarehouse::new(
            SourceName::parse("facts").expect("a test source"),
            shared.clone(),
            federated_fact_rows(),
            DryRunOutcome::Accepted,
        );
        let lookup = LegPreflightWarehouse::new(
            SourceName::parse("geo").expect("a test source"),
            shared,
            federated_lookup_rows(),
            DryRunOutcome::TransientFailure,
        );
        let warehouses = Warehouses::of(fact).and(lookup).expect("two sources, one registry");
        let failure = answer_federated(
            &bundle(),
            &federated_plan(),
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &warehouses,
            FEDERATED_BUDGET,
            test_deadline(),
            &SpendLedger::no_budget(),
        )
        .expect_err("a transient pre-flight failure remains a warehouse error");
        assert!(matches!(
            failure,
            super::ServiceError::Warehouse {
                cause: AdapterFailure::Statement {
                    cause: super::super::tests_support::DriverFailure
                }
            }
        ));
        assert_eq!(
            warehouses
                .get(&SourceName::parse("facts").expect("a test source"))
                .expect("facts is registered")
                .executions(),
            0,
            "the fact leg's own dry run succeeded, but its execute must never be reached once the \
             lookup leg's pre-flight fails"
        );
        assert_eq!(
            warehouses
                .get(&SourceName::parse("geo").expect("a test source"))
                .expect("geo is registered")
                .executions(),
            0
        );
    }

    #[test]
    fn a_federated_answer_that_crosses_the_working_set_is_refused_not_error() {
        let fact_source = SourceName::parse("facts").expect("facts");
        let lookup_source = SourceName::parse("geo").expect("geo");
        let shared = shared();
        let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
            fact_source,
            shared.clone(),
            federated_fact_rows(),
        ))
        .and(crate::tests_support::LegsWarehouse::answering(
            lookup_source,
            shared,
            federated_lookup_rows(),
        ))
        .expect("two sources");

        let plan = federated_plan();
        let outcome = answer_federated(
            &bundle(),
            &plan,
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &warehouses,
            1,
            test_deadline(),
            &SpendLedger::no_budget(),
        )
        .expect("a refusal is an Ok")
        .into_outcome();
        assert!(
            matches!(
                outcome,
                ToolOutcome::Refusal {
                    reason: RefusalReason::ResourcesExhausted { .. }
                }
            ),
            "a combined answer over the ceiling is a governance refusal, not {outcome:?}"
        );
    }

    #[test]
    fn an_answer_whose_legs_would_run_under_two_postures_is_refused_before_minting() {
        // **The refusal this whole change is, and the assertion that matters is the mint count.**
        // Two leg-executing adapters, one `shared-service-user` and one `impersonation-at-source`,
        // so combining them would add rows one identity was permitted to see to rows another
        // identity was permitted to see - a total neither is entitled to, under a certified metric
        // name and with valid provenance. Refused above the mint, so no credential exists and
        // neither leg runs.
        //
        // A registration rather than a new fake: `LegsWarehouse::answering` already takes a posture
        // per instance, which is the whole of what a mixed deployment is.
        let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
            SourceName::parse("facts").expect("a test source"),
            shared(),
            federated_fact_rows(),
        ))
        .and(crate::tests_support::LegsWarehouse::answering(
            SourceName::parse("geo").expect("a test source"),
            sutura_domain::source::SourcePosture::ImpersonationAtSource,
            federated_lookup_rows(),
        ))
        .expect("two sources, one registry");

        let broker = crate::tests_support::CountingBroker::default();
        let plan = federated_plan();
        let outcome = answer_federated(
            &bundle(),
            &plan,
            &asked_by_a_person(),
            &broker,
            &warehouses,
            FEDERATED_BUDGET,
            test_deadline(),
            &SpendLedger::no_budget(),
        )
        .expect("a refusal is an Ok")
        .into_outcome();
        let ToolOutcome::Refusal {
            reason: RefusalReason::LegsDecideIdentityDifferently { postures },
        } = outcome
        else {
            panic!("two postures in one answer is refused, not {outcome:?}");
        };
        assert_eq!(
            postures.iter().copied().collect::<Vec<&str>>(),
            vec!["impersonation-at-source", "shared-service-user"],
            "the refusal names both postures, by label"
        );
        assert_eq!(
            broker.asked(),
            0,
            "the verdict is above the mint, so no credential is minted for an answer that will not be given"
        );
        // And the operator's acknowledgement prose never leaves the deployment. `Debug` is the
        // rendering that reaches a log by accident; the serialized body is asserted in
        // `sutura_domain::query`, which has a format parser.
        let rendered = format!("{:?}", RefusalReason::LegsDecideIdentityDifferently { postures });
        assert!(!rendered.contains("a directory of CSVs"), "{rendered}");
    }

    #[test]
    fn two_shared_sources_with_different_acknowledgements_are_still_answered() {
        // **The strand guard, at the orchestrator.** `SourcePosture` derives `PartialEq` and the
        // acknowledgement is resolved per source, so a predicate comparing VALUES would refuse this
        // - and this is the only federating shape that ships today, since every adapter a release
        // links declares it has nowhere for a subject to arrive. The domain's own cell asserts the
        // same property one layer down; this one asserts that the answer path still answers.
        let acknowledged = |text: &str| sutura_domain::source::SourcePosture::SharedServiceUser {
            declared: sutura_domain::source::SharedIdentityDeclared::of(
                sutura_domain::source::AcknowledgementReason::parse(text).expect("a test reason is a reason"),
            ),
        };
        let facts = SourceName::parse("facts").expect("a test source");
        let geo = SourceName::parse("geo").expect("a test source");
        let postures = [
            (facts.clone(), acknowledged("a directory of CSVs this deployment owns")),
            (geo.clone(), acknowledged("a reference dataset every team reads")),
        ];
        let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
            facts,
            postures[0].1.clone(),
            federated_fact_rows(),
        ))
        .and(crate::tests_support::LegsWarehouse::answering(
            geo,
            postures[1].1.clone(),
            federated_lookup_rows(),
        ))
        .expect("two sources, one registry");

        // Per-source witnesses, because `Presented::agrees_with` compares the acknowledgement prose
        // by equality - so a broker minting ONE witness for both sources is refused here by that
        // guard rather than by the one under test, which is exactly the confusion this fake removes.
        let broker = crate::tests_support::AcknowledgingBroker::over(&postures);
        let plan = federated_plan();
        let outcome = answer_federated(
            &bundle(),
            &plan,
            &asked_by_a_person(),
            &broker,
            &warehouses,
            FEDERATED_BUDGET,
            test_deadline(),
            &SpendLedger::no_budget(),
        )
        .expect("two shared legs are answered")
        .into_outcome();
        let ToolOutcome::Answer { provenance, .. } = outcome else {
            panic!("two shared legs are one posture, so this is answered, not {outcome:?}");
        };
        assert_eq!(
            provenance
                .executed_as()
                .legs()
                .map(|(_, posture)| posture.as_str())
                .collect::<Vec<&str>>(),
            vec!["shared-service-user", "shared-service-user"],
            "both legs record the same posture, with two different acknowledgements behind them"
        );
    }

    #[test]
    fn a_federated_answer_is_refused_when_no_adapter_executes_a_leg() {
        // What `answer` does on a build whose adapter declares `EXECUTES_LEGS = false`: it refuses
        // cleanly BEFORE minting or running a leg, rather than surfacing a typed leg refusal as a
        // retryable 503. `FixedWarehouse` below takes the port's default, which is what puts this
        // test on that branch.
        //
        // **Not the shipped binary any more, and the correction matters here of all places.**
        // `sutura-exec-datafusion` declares the constant and is non-optional in both published
        // binaries, so a release ANSWERS a two-source question - see
        // `crates/sutura-serve/tests/served.rs`. This cell is about the gate, not about the shipped
        // set: what still reaches it is `sutura-exec-bigquery`, any adapter taking the default, and
        // this fake.
        let fact_source = SourceName::parse("facts").expect("facts");
        let lookup_source = SourceName::parse("geo").expect("geo");
        let shared = shared();
        let warehouses = Warehouses::of(crate::tests_support::FixedWarehouse::answering(
            fact_source,
            shared.clone(),
            federated_fact_rows(),
        ))
        .and(crate::tests_support::FixedWarehouse::answering(
            lookup_source,
            shared,
            federated_lookup_rows(),
        ))
        .expect("two sources");

        let plan = federated_plan();
        let refused = answer_federated(
            &bundle(),
            &plan,
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &warehouses,
            FEDERATED_BUDGET,
            test_deadline(),
            &SpendLedger::no_budget(),
        )
        .expect("a refusal is an Ok")
        .into_outcome();
        assert!(
            matches!(
                refused,
                ToolOutcome::Refusal {
                    reason: RefusalReason::FederationNotExecutable
                }
            ),
            "{refused:?}"
        );
    }
}

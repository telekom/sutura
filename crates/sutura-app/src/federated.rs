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
use sutura_domain::query::{RefusalReason, ResultBound, ToolOutcome};
use sutura_domain::warehouse::{RowSet, Warehouse};

use crate::{Answered, Answering, ServiceError, Warehouses, exceeds_row_cap, now_in_unix_seconds};

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

/// The leg execution's return type, named so `execute_leg`'s signature is not a `type_complexity`
/// finding.
pub(crate) type LegResult<W, B> = Result<RowSet, LegError<<W as Warehouse>::Error, <B as CredentialBroker>::Error>>;

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

    let fact = match execute_leg::<_, B>(fact_warehouse, &credentials, plan.fact()) {
        Ok(rows) => rows,
        Err(LegError::Refusal(reason)) => {
            return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
        }
        Err(LegError::Failure(error)) => return Err(error),
    };
    let lookup = match execute_leg::<_, B>(lookup_warehouse, &credentials, plan.lookup()) {
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
pub(crate) fn execute_leg<W, B>(warehouse: &W, credentials: &BoundToTheRequest, leg: &LegPlan) -> LegResult<W, B>
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
    match warehouse.execute(Executable::Leg(leg), presented) {
        Ok(rows) => Ok(rows),
        Err(cause) => {
            // The two governance predicates the mono path asks of its own `execute`, asked here for
            // the same reasons (see `sutura_app::answer`), and in the same order: exhaustion is
            // refused first, then a result the data system would not return at once, otherwise the
            // failure leaves as the `503` an outage produces. Without this, a leg-executing adapter
            // that hit either bound reached a caller as `503` - a status inviting the very retry that
            // would return the same reply. `dry_run` above is deliberately not given the treatment,
            // mirroring the mono path: a check reads no data, so neither bound can have fired there.
            if let Some(ceiling_bytes) = warehouse.working_set_exhausted(&cause) {
                return Err(LegError::Refusal(RefusalReason::ResourcesExhausted { ceiling_bytes }));
            }
            if warehouse.result_did_not_fit(&cause) {
                return Err(LegError::Refusal(RefusalReason::ResultTooLarge {
                    bound: ResultBound::Volume,
                }));
            }
            Err(LegError::Failure(ServiceError::Warehouse { cause }))
        }
    }
}

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
    use super::answer_federated;
    use crate::Warehouses;
    use crate::tests::{asked_by_a_person, bundle, june, metric, shared};
    use crate::tests_support::FixedBroker;
    use sutura_domain::model::{Grain, SourceName};
    use sutura_domain::query::{RefusalReason, ResultBound, ToolOutcome};
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
        use sutura_domain::catalog::TIME_BUCKET_LABEL;
        use sutura_domain::measure::{AggregatedColumn, Measure, Term};
        use sutura_domain::model::Aggregate;
        use sutura_domain::model::{ColumnName, DimensionName, TableName};
        use sutura_domain::plan::{
            AnswerKey, InternalLabel, LegPlan, PlanBucket, PlanColumn, PlanKey, ResultLabel, StatementTables,
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
        let bucket = |c: &str| {
            PlanBucket::new(
                String::from(TIME_BUCKET_LABEL),
                Grain::Month,
                PlanColumn::new(table.clone(), column(c)),
            )
        };

        let fact = LegPlan::Fact {
            source: fact_source,
            metric: metric(),
            tables: StatementTables::only(table.clone()),
            bucket: bucket("month"),
            keys: vec![key("product_family"), link()],
            terms: Vec::new(),
            filters: Vec::new(),
            params: Vec::new(),
            range: june(),
        };
        let lookup = LegPlan::Lookup {
            source: lookup_source,
            table: table.clone().into(),
            keys: vec![link(), key("region")],
            filters: Vec::new(),
            params: Vec::new(),
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
        let outcome = answer_federated(&bundle(), &plan, &asked_by_a_person(), &broker, &warehouses, FEDERATED_BUDGET)
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
    fn a_federated_leg_that_hits_the_volume_bound_is_refused_not_a_503() {
        // The federated half of the volume bound, and the reason `execute_leg` asks the predicates at
        // all. A leg against a data system that will not return the whole result at once used to leave
        // as `ServiceError::Warehouse` - the `503` a dead data system produces - so a caller was told
        // to retry a reply that returns the same page. Both legs here answer `Err` with
        // `result_did_not_fit` true, and the fact leg runs first, so it must be refused as
        // `ResultTooLarge` carrying `Volume`.
        let shared = shared();
        let warehouses = Warehouses::of(crate::tests_support::PageBoundLegsWarehouse::new(
            SourceName::parse("facts").expect("a test source"),
            shared.clone(),
        ))
        .and(crate::tests_support::PageBoundLegsWarehouse::new(
            SourceName::parse("geo").expect("a test source"),
            shared,
        ))
        .expect("two sources, one registry");

        let plan = federated_plan();
        let outcome = answer_federated(
            &bundle(),
            &plan,
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &warehouses,
            FEDERATED_BUDGET,
        )
        .expect("a bound is a refusal, not an error")
        .into_outcome();
        assert!(
            matches!(
                outcome,
                ToolOutcome::Refusal {
                    reason: RefusalReason::ResultTooLarge {
                        bound: ResultBound::Volume
                    }
                }
            ),
            "a leg the data system will not return at once must be refused as the volume bound, not {outcome:?}"
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
    fn a_federated_answer_is_refused_when_no_adapter_executes_a_leg() {
        // The shipped binary's adapters declare `EXECUTES_LEGS = false`, and this is what `answer` does
        // on that build: it refuses cleanly BEFORE minting or running a leg, rather than surfacing a
        // typed leg refusal as a retryable 503.
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

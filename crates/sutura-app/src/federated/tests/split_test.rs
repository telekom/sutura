//! A lookup leg split at its source - `github.com/telekom/sutura#828` - answered through
//! `answer_federated` over a registry that names the source for splitting.
//!
//! The fake lives in this `#[cfg(test)]` file for the reason `dry_run_count_test` states. It splits
//! the lookup leg itself; how a data system cuts one, and the per-stream bound each partition is
//! read under, are the adapter's and its own tier's to hold.

use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use core::time::Duration;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use super::*;
use crate::ServiceError;
use crate::tests_support::BrokerUnreachable;
use sutura_domain::identity::{
    BillTo, BillingAccount, CredentialBroker, CredentialsDoNotFitTheRequest, Expiry, LegCredentials, Minted, PrincipalChain,
    RequestContext, SourceSet, Subject,
};
use sutura_domain::plan::{
    EstimatedRows, Grantee, LegPlan, LinkKind, LookupSplit, Partitioned, SplitDecision, SplitRefusal, decide,
};
use sutura_domain::warehouse::PreFlight;
use sutura_domain::warehouse::billed::{Billed, BilledResult};
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::labels::JobLabels;

/// What [`SplitLegs::split_lookup`] answers.
#[derive(Clone, Copy)]
enum Splits {
    /// One partition per row.
    PerRow,
    /// One partition per row, the first read outliving
    /// [`FixedBroker::GrantsSomethingExpiringWithinTheSecond`]'s grant.
    PerRowOutlivingTheGrant,
    /// No partition count the readers allow keeps each under the source's per-stream bound.
    Refused(SplitRefusal),
    /// One partition per row, granted to the leg's readers when a grantee is offered - each one a
    /// descriptor another node can read, or none of them where `portable` is `false`, and handed
    /// back an hour later where `late` - else read here.
    Granting { portable: bool, late: bool },
    /// [`Self::Granting`] of portable partitions, handed back only after
    /// [`FixedBroker::GrantsSomethingExpiringWithinTheSecond`]'s grant ended - else read here.
    GrantingOutlivingTheGrant,
}

/// A [`crate::tests_support::LegsWarehouse`] that splits a lookup leg as `splits` says, takes any
/// payer, and counts the legs it reads whole, the splits it is asked for and the partitions read -
/// and records whether a split was offered a grantee, and whom and until when it granted.
struct SplitLegs {
    legs: crate::tests_support::LegsWarehouse,
    rows: RowSet,
    splits: Splits,
    executes: AtomicUsize,
    splits_asked: AtomicUsize,
    partitions_read: AtomicUsize,
    offered: AtomicBool,
    granted: OnceLock<GrantedTo>,
    fail_with: Option<FailureFactory>,
}

type FailureFactory = fn() -> AdapterFailure;

/// Whom, and until when, a split was granted.
type GrantedTo = (Subject, SystemTime);

impl SplitLegs {
    fn answering(source: &str, rows: RowSet, splits: Splits) -> Self {
        Self {
            legs: crate::tests_support::LegsWarehouse::answering(
                SourceName::parse(source).expect("a test source"),
                shared(),
                rows.clone(),
            ),
            rows,
            splits,
            executes: AtomicUsize::new(0),
            splits_asked: AtomicUsize::new(0),
            partitions_read: AtomicUsize::new(0),
            offered: AtomicBool::new(false),
            granted: OnceLock::new(),
            fail_with: None,
        }
    }

    fn failing_with(mut self, error: FailureFactory) -> Self {
        self.fail_with = Some(error);
        self
    }

    /// One partition per row granted to `grantee`, recording whom and until when.
    fn granting(&self, grantee: Grantee, portable: bool, late: bool) -> Partitioned<'_, AdapterFailure> {
        let schema = Arc::clone(crate::tests_support::canned(&self.rows).schema());
        let partitions = (0..self.rows.rows().len())
            .map(|index| portable.then(|| vec![u8::try_from(index).expect("a test leg has few rows")]));
        let handed_back = SystemTime::now() + if late { Duration::from_secs(3600) } else { Duration::ZERO };
        match grantee.grant(schema, partitions, NonZeroU32::MIN, EstimatedBytes::parse(0), (), handed_back) {
            Ok(made) => {
                drop(self.granted.set((made.caller().clone(), made.until())));
                Partitioned::Granted(made)
            }
            Err(refused) => Partitioned::NotGranted(refused),
        }
    }
}

#[cfg(test)]
mod warehouse;

fn geo() -> SourceName {
    SourceName::parse("geo").expect("a test source")
}

/// The fact leg and a lookup leg at `geo` that splits as `splits` says.
fn registry(splits: Splits) -> Warehouses<SplitLegs> {
    Warehouses::of(SplitLegs::answering("facts", federated_fact_rows(), splits))
        .and(SplitLegs::answering("geo", federated_lookup_rows(), splits))
        .expect("two sources, one registry")
}

/// [`FixedBroker::GrantsShared`], billing every leg to the payer the request names.
struct NamingThePayer;

impl CredentialBroker for NamingThePayer {
    type Error = BrokerUnreachable;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        Ok(match FixedBroker::GrantsShared.mint(context, sources)? {
            Minted::Granted { credentials } => Minted::Granted {
                credentials: credentials.named_by(context),
            },
            refused => refused,
        })
    }
}

/// A shared-service-user grant for the asker that expires ten seconds from now: before a
/// thirty-second deadline, and after any question a cell asks under it.
struct ExpiringInTenSeconds;

impl CredentialBroker for ExpiringInTenSeconds {
    type Error = BrokerUnreachable;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let expires = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs())
            .saturating_add(10);
        let presented = sources
            .iter()
            .map(|source| {
                let shared = Presented::SharedServiceUser {
                    declared: crate::tests_support::acknowledged(),
                };
                (source.clone(), shared)
            })
            .collect();
        let credentials = LegCredentials::minted(
            context.chain().subject().clone(),
            Expiry::At { unix_seconds: expires },
            sources,
            presented,
        )
        .map_err(|_uncoverable| BrokerUnreachable)?;
        Ok(Minted::Granted { credentials })
    }
}

fn combiner() -> sutura_exec_datafusion::DataFusionCombiner {
    sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds")
}

fn asked<B>(
    warehouses: &Warehouses<SplitLegs>,
    context: &RequestContext,
    broker: &B,
) -> crate::Answering<SplitLegs, B, sutura_exec_datafusion::DataFusionCombiner>
where
    B: CredentialBroker,
    B::Error: Send,
{
    answer_federated(
        (&bundle(), PinnedSchemas::default().digest()),
        &federated_plan(),
        context,
        broker,
        warehouses,
        &combiner(),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::Ceilings::DEFAULT,
    )
}

fn answered(warehouses: &Warehouses<SplitLegs>) -> ToolOutcome {
    asked(warehouses, &asked_by_a_person(), &FixedBroker::GrantsShared)
        .expect("a split leg is an answer or a refusal, not an error")
        .into_outcome()
}

fn geo_leg(warehouses: &Warehouses<SplitLegs>) -> &SplitLegs {
    warehouses.get(&geo()).expect("geo is registered")
}

fn whole_reads(warehouses: &Warehouses<SplitLegs>) -> usize {
    geo_leg(warehouses).executes.load(Ordering::SeqCst)
}

#[test]
fn a_lookup_leg_split_at_its_source_answers_as_the_leg_read_whole() {
    let whole = registry(Splits::PerRow);
    let split = registry(Splits::PerRow).splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);

    let (ToolOutcome::Answer { rows: read_whole, .. }, ToolOutcome::Answer { rows: read_split, .. }) =
        (answered(&whole), answered(&split))
    else {
        panic!("both registries answer the question");
    };

    // The exact differential: the same question with the lookup leg read whole.
    assert_eq!(read_split, read_whole);
    assert_eq!(whole_reads(&whole), 1, "the unsplit lookup leg is read whole once");
    assert_eq!(whole_reads(&split), 0, "a split lookup leg is read by its partitions alone");
}

#[test]
fn a_lookup_leg_no_partition_count_fits_is_refused_as_the_volume_bound() {
    let one = NonZeroUsize::MIN;
    let SplitDecision::Refused(refusal) = decide(EstimatedBytes::parse(2), EstimatedRows::parse(2), NonZeroU64::MIN, one, one)
    else {
        panic!("two rows over a one-row bound with one reader are refused");
    };
    let warehouses = registry(Splits::Refused(refusal)).splitting(geo(), NonZeroU64::MIN, one);

    let outcome = answered(&warehouses);

    assert_eq!(
        outcome,
        ToolOutcome::Refusal {
            reason: RefusalReason::ResultTooLarge {
                bound: ResultBound::Volume
            }
        }
    );
    assert_eq!(whole_reads(&warehouses), 0, "a refused split is not read whole instead");
}

#[test]
fn a_lookup_leg_naming_a_payer_is_read_whole_and_its_source_is_never_asked_to_split() {
    let warehouses = registry(Splits::PerRow).splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);
    let payer = BillingAccount::parse("payer").expect("a test billing account");

    let outcome = asked(&warehouses, &asked_by_a_person().naming(Some(payer)), &NamingThePayer)
        .expect("a source that takes the payer answers")
        .into_outcome();

    assert!(matches!(outcome, ToolOutcome::Answer { .. }), "{outcome:?}");
    assert_eq!(
        geo_leg(&warehouses).splits_asked.load(Ordering::SeqCst),
        0,
        "a split reads no payer, so a leg that names one is never split"
    );
    assert_eq!(whole_reads(&warehouses), 1, "the leg is read whole, under its payer");
}

#[test]
fn a_grant_that_expires_during_one_partition_refuses_the_leg_before_the_next_is_read() {
    let warehouses = registry(Splits::PerRowOutlivingTheGrant).splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);

    let failure = asked(
        &warehouses,
        &asked_by_a_person(),
        &FixedBroker::GrantsSomethingExpiringWithinTheSecond,
    )
    .expect_err("a grant that ran out between two partitions reads no further partition");

    assert!(
        matches!(
            failure,
            ServiceError::Credentials {
                cause: CredentialsDoNotFitTheRequest::Expired { .. }
            }
        ),
        "{failure:?}"
    );
    assert!(
        geo_leg(&warehouses).partitions_read.load(Ordering::SeqCst) < 2,
        "the partition after the grant expired is never read"
    );
}

/// A person nobody verified: the deployment itself.
fn asked_by_nobody() -> RequestContext {
    RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))
}

fn granted(warehouses: &Warehouses<SplitLegs>) -> Option<GrantedTo> {
    geo_leg(warehouses).granted.get().cloned()
}

fn read_here(warehouses: &Warehouses<SplitLegs>) -> usize {
    geo_leg(warehouses).partitions_read.load(Ordering::SeqCst)
}

#[test]
fn a_split_lookup_leg_is_granted_for_the_asking_caller_until_the_question_ends_and_never_read_here() {
    let warehouses = registry(Splits::Granting {
        portable: true,
        late: false,
    })
    .splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);
    let asked_at = SystemTime::now();

    let outcome = answered(&warehouses);

    // One node and no reader of `geo`: the grant is refused where it would be read, never read here.
    assert_eq!(
        outcome,
        ToolOutcome::Refusal {
            reason: RefusalReason::SourceUnavailable { source: geo() }
        }
    );
    let (caller, until) = granted(&warehouses).expect("the split was granted");
    assert_eq!(
        caller,
        Subject::verified("someone@example.com").expect("a test subject is a subject"),
        "the grant names the caller who asked"
    );
    assert!(until > asked_at, "the grant outlives the moment it was asked for");
    assert!(
        until <= SystemTime::now() + Duration::from_secs(30),
        "the grant ends no later than the question's thirty-second deadline"
    );
    assert_eq!(whole_reads(&warehouses), 0, "a granted leg is not read whole here");
    assert_eq!(read_here(&warehouses), 0, "a granted leg's partitions are not read here");
}

#[test]
fn a_split_lookup_leg_is_granted_no_longer_than_the_request_s_own_grant_lasts() {
    let warehouses = registry(Splits::Granting {
        portable: true,
        late: false,
    })
    .splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);
    let asked_at = SystemTime::now();

    let outcome = asked(&warehouses, &asked_by_a_person(), &ExpiringInTenSeconds)
        .expect("a granted leg is an answer or a refusal, not an error")
        .into_outcome();

    assert!(matches!(outcome, ToolOutcome::Refusal { .. }), "{outcome:?}");
    let (_, until) = granted(&warehouses).expect("the split was granted");
    assert!(
        until <= asked_at + Duration::from_secs(11),
        "the grant ends with the credential that expires in ten seconds, not the thirty-second deadline"
    );
}

#[test]
fn a_split_lookup_leg_asked_by_no_verified_caller_is_refused_before_its_source_is_asked_anything() {
    let warehouses = registry(Splits::Granting {
        portable: true,
        late: false,
    })
    .splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);

    let outcome = asked(&warehouses, &asked_by_nobody(), &FixedBroker::GrantsShared)
        .expect("a refused leg is an answer, not an error")
        .into_outcome();

    assert_eq!(
        outcome,
        ToolOutcome::Refusal {
            reason: RefusalReason::CredentialUnavailable { source: geo() }
        }
    );
    assert_eq!(
        geo_leg(&warehouses).splits_asked.load(Ordering::SeqCst),
        0,
        "the source is not asked to estimate, export or describe for no verified caller"
    );
    assert_eq!(
        whole_reads(&warehouses),
        0,
        "the refused leg is not read whole as the deployment"
    );
    assert_eq!(
        read_here(&warehouses),
        0,
        "the refused leg is not read here as the deployment"
    );
}

#[test]
fn a_lookup_leg_whose_link_declares_a_comparison_asked_by_no_verified_caller_is_refused_too() {
    let warehouses = registry(Splits::Granting {
        portable: true,
        late: false,
    })
    .splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);

    let outcome = answer_federated(
        (&bundle(), PinnedSchemas::default().digest()),
        &federated_plan().with_link_comparison(Some(LinkKind::Text)),
        &asked_by_nobody(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &combiner(),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::Ceilings::DEFAULT,
    )
    .expect("a refused leg is an answer, not an error")
    .into_outcome();

    assert_eq!(
        outcome,
        ToolOutcome::Refusal {
            reason: RefusalReason::CredentialUnavailable { source: geo() }
        }
    );
    assert_eq!(
        read_here(&warehouses),
        0,
        "a leg that would be read here is not read as the deployment"
    );
}

#[test]
fn a_split_lookup_leg_whose_grant_ended_before_its_partitions_came_back_is_refused_as_the_deadline() {
    let warehouses = registry(Splits::Granting {
        portable: true,
        late: true,
    })
    .splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);

    let outcome = answered(&warehouses);

    let ToolOutcome::Refusal {
        reason: RefusalReason::DeadlineExceeded { .. },
    } = outcome
    else {
        panic!("a grant that ended before its partitions came back is the question's deadline: {outcome:?}");
    };
    assert_eq!(granted(&warehouses), None, "an ended grant is not made");
    assert_eq!(read_here(&warehouses), 0, "an ended grant is not read here instead");
}

#[test]
fn a_split_lookup_leg_whose_grant_ended_with_the_request_s_own_grant_is_refused_as_that_grant_expired() {
    let warehouses = registry(Splits::GrantingOutlivingTheGrant).splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);

    let failure = asked(
        &warehouses,
        &asked_by_a_person(),
        &FixedBroker::GrantsSomethingExpiringWithinTheSecond,
    )
    .expect_err("a grant that ended with the request's own grant is that grant expiring");

    assert!(
        matches!(
            failure,
            ServiceError::Credentials {
                cause: CredentialsDoNotFitTheRequest::Expired { .. }
            }
        ),
        "{failure:?}"
    );
    assert_eq!(granted(&warehouses), None, "an ended grant is not made");
    assert_eq!(read_here(&warehouses), 0, "an ended grant is not read here instead");
}

#[test]
fn a_split_lookup_leg_with_a_partition_no_other_node_can_read_is_refused_as_source_unavailable() {
    let warehouses = registry(Splits::Granting {
        portable: false,
        late: false,
    })
    .splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);

    let outcome = answered(&warehouses);

    assert_eq!(
        outcome,
        ToolOutcome::Refusal {
            reason: RefusalReason::SourceUnavailable { source: geo() }
        }
    );
    assert_eq!(granted(&warehouses), None, "a partition only this node reads is not granted");
    assert_eq!(whole_reads(&warehouses), 0, "a refused grant is not read whole instead");
    assert_eq!(read_here(&warehouses), 0, "a refused grant is not read here instead");
}

#[test]
fn a_lookup_leg_whose_link_declares_a_comparison_is_never_offered_for_granting_and_is_read_here() {
    let warehouses = registry(Splits::Granting {
        portable: true,
        late: false,
    })
    .splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN);

    let outcome = answer_federated(
        (&bundle(), PinnedSchemas::default().digest()),
        &federated_plan().with_link_comparison(Some(LinkKind::Text)),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &combiner(),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::Ceilings::DEFAULT,
    )
    .expect("a leg read here is an answer")
    .into_outcome();

    assert!(matches!(outcome, ToolOutcome::Answer { .. }), "{outcome:?}");
    assert!(
        !geo_leg(&warehouses).offered.load(Ordering::SeqCst),
        "a leg the combine may cast is split without a grantee"
    );
    assert_eq!(granted(&warehouses), None);
    assert_eq!(read_here(&warehouses), 2, "each partition is read here");
}

fn refused_rows() -> AdapterFailure {
    AdapterFailure::Unannounced {
        cause: sutura_domain::warehouse::UnannouncedBatch::OverBound { most: 17 },
    }
}

fn refused_bytes() -> AdapterFailure {
    AdapterFailure::Unannounced {
        cause: sutura_domain::warehouse::UnannouncedBatch::OverBudget { most_bytes: 31 },
    }
}

fn refused_opaquely() -> AdapterFailure {
    AdapterFailure::TooMuchData
}

fn serialized_refusal(outcome: &ToolOutcome) -> serde_json::Value {
    let ToolOutcome::Refusal { reason } = outcome else {
        panic!("a source bound is a refusal: {outcome:?}");
    };
    serde_json::to_value(reason).expect("a refusal serializes")
}

fn assert_limit_outputs(outcome: &ToolOutcome, known: Option<&str>) {
    let http = sutura_http::wire::Outcome::from(outcome);
    assert_eq!(http.status().as_u16(), 413);
    for body in [
        serde_json::to_value(http.body()).expect("HTTP body serializes"),
        serde_json::to_value(sutura_mcp::wire::OutcomeContent::from(outcome)).expect("MCP body serializes"),
    ] {
        assert_eq!(body["outcome"], "refusal");
        assert_eq!(body["reason"]["code"], "result_too_large");
        assert!(body.get("rows").is_none(), "no partial answer: {body}");
        let detail = body["reason"]["detail"].as_str().expect("a refusal has detail");
        if let Some(limit) = known {
            assert!(detail.contains(limit), "known source threshold lost: {detail}");
            assert!(detail.contains("deployment operator"), "no operator remedy: {detail}");
            assert!(detail.contains("NOT truncated"), "no completeness statement: {detail}");
            assert!(!detail.contains("not told"), "known bound described as unknown: {detail}");
        } else {
            assert!(!detail.chars().any(char::is_numeric), "invented unknown limit: {detail}");
        }
    }
}

fn a_limited_lookup(error: FailureFactory, partitioned: bool) -> ToolOutcome {
    let warehouses = Warehouses::of(SplitLegs::answering("facts", federated_fact_rows(), Splits::PerRow))
        .and(SplitLegs::answering("geo", federated_lookup_rows(), Splits::PerRow).failing_with(error))
        .expect("two sources");
    let warehouses = if partitioned {
        warehouses.splitting(geo(), NonZeroU64::MIN, NonZeroUsize::MIN)
    } else {
        warehouses
    };
    let outcome = answered(&warehouses);
    let lookup = geo_leg(&warehouses);
    assert_eq!(whole_reads(&warehouses), usize::from(!partitioned));
    assert_eq!(lookup.partitions_read.load(Ordering::SeqCst), usize::from(partitioned));
    assert_eq!(lookup.splits_asked.load(Ordering::SeqCst), usize::from(partitioned));
    assert_eq!(
        warehouses
            .get(&SourceName::parse("facts").expect("a source"))
            .expect("the fact is registered")
            .executes
            .load(Ordering::SeqCst),
        1
    );
    outcome
}

fn a_limited_mono_answer(error: FailureFactory) -> ToolOutcome {
    let validating = Warehouses::of(crate::tests_support::FixedWarehouse::answering(
        crate::tests::source(),
        shared(),
        crate::tests::certified(),
    ));
    let validated = crate::verify_and_validate(bundle(), &validating).expect("the fixture anchor holds");
    let warehouses = Warehouses::of(SplitLegs::answering("local", crate::tests::certified(), Splits::PerRow).failing_with(error));
    let question = sutura_domain::query::Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    crate::answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_domain::plan::RefusingCombiner,
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::Ceilings::DEFAULT,
    )
    .expect("a source bound is an outcome")
    .into_outcome()
}

fn node_replies(leg: &LegPlan, local: bool, count: usize) -> Vec<sutura_exec_node::test_support::Reply> {
    use sutura_exec_node::test_support::Reply;
    std::iter::repeat_with(|| {
        if local {
            let labels = Executable::Leg(leg).result_labels();
            let rows = RowSet::new(labels.clone(), vec![vec![Value::Integer(7); labels.len()]; 8])
                .expect("eight rectangular integer rows");
            Reply::Answer(crate::tests_support::canned(&rows).into_batches())
        } else {
            Reply::Refuse(tonic::Code::FailedPrecondition, "result_too_large")
        }
    })
    .take(count)
    .collect()
}

fn node_client(source: &SourceName, node: &sutura_exec_node::test_support::Node) -> sutura_exec_node::NodeWarehouse {
    let budget = sutura_domain::warehouse::ResultBudget::of_bytes(
        NonZeroUsize::new(sutura_exec_node::test_support::receipt().len() + 31).expect("receipt plus a positive row budget"),
    );
    sutura_exec_node::NodeWarehouse::open(
        source.clone(),
        SourcePosture::ImpersonationAtSource,
        &sutura_exec_node::NodeEndpoint::parse(&format!("https://localhost:{}", node.port())).expect("a TLS origin"),
        [node.anchor().clone()],
        budget,
    )
    .expect("a real node client starts")
}

fn a_node_size_refusal(local: bool) -> ToolOutcome {
    use sutura_exec_node::test_support::Node;
    let plan = federated_plan();
    let lookup = plan
        .legs()
        .into_iter()
        .find(|leg| leg.source() == &geo())
        .expect("the fixture has one lookup");
    let fact_calls = if local { 2 } else { 1 };
    let fact = Node::try_answering(node_replies(plan.fact(), local, fact_calls)).expect("a TLS fact node starts");
    let geo_node = Node::try_answering(node_replies(lookup, local, 1)).expect("a TLS lookup node starts");
    let warehouses = Warehouses::of(node_client(plan.fact().source(), &fact))
        .and(node_client(lookup.source(), &geo_node))
        .expect("two real node clients");
    if local {
        let error = warehouses
            .get(plan.fact().source())
            .expect("the fact client is registered")
            .execute(
                Executable::Leg(plan.fact()),
                &Presented::SubjectToken {
                    material: sutura_domain::identity::Secret::new("an-exchanged-token"),
                },
                test_deadline(),
            )
            .expect_err("the decoded batches exceed their remaining allowance");
        assert!(
            matches!(
                error,
                sutura_exec_node::NodeError::Result {
                    cause: sutura_domain::warehouse::UnannouncedBatch::OverBudget { most_bytes: 31 }
                }
            ),
            "{error:?}"
        );
    }
    let broker = crate::tests_support::AcknowledgingBroker::over(&[
        (plan.fact().source().clone(), SourcePosture::ImpersonationAtSource),
        (lookup.source().clone(), SourcePosture::ImpersonationAtSource),
    ]);
    let outcome = answer_federated(
        (&bundle(), PinnedSchemas::default().digest()),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        &combiner(),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::Ceilings::DEFAULT,
    )
    .expect("a decoded or remote size bound is an outcome")
    .into_outcome();
    assert_eq!(fact.received().len(), fact_calls);
    assert_eq!(geo_node.received().len(), 1);
    outcome
}

#[test]
fn whole_leg_row_bound_reaches_the_outputs() {
    let outcome = a_limited_lookup(refused_rows, false);
    let domain = serialized_refusal(&outcome);
    assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
    assert_limit_outputs(&outcome, None);
}

#[test]
fn local_partition_row_bound_reaches_the_outputs() {
    let outcome = a_limited_lookup(refused_rows, true);
    let domain = serialized_refusal(&outcome);
    assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
    assert_limit_outputs(&outcome, None);
}

#[test]
fn whole_leg_byte_bound_reaches_the_outputs() {
    let outcome = a_limited_lookup(refused_bytes, false);
    let domain = serialized_refusal(&outcome);
    assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
    assert_limit_outputs(&outcome, None);
}

#[test]
fn local_partition_byte_bound_reaches_the_outputs() {
    let outcome = a_limited_lookup(refused_bytes, true);
    let domain = serialized_refusal(&outcome);
    assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
    assert_limit_outputs(&outcome, None);
}

#[test]
fn an_opaque_whole_or_partition_bound_stays_numberless() {
    for partitioned in [false, true] {
        let outcome = a_limited_lookup(refused_opaquely, partitioned);
        let domain = serialized_refusal(&outcome);
        assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
        assert_limit_outputs(&outcome, None);
    }
}

#[test]
fn a_mono_row_bound_reaches_both_transport_outputs() {
    let outcome = a_limited_mono_answer(refused_rows);
    let domain = serialized_refusal(&outcome);
    assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
    assert_limit_outputs(&outcome, None);
}

#[test]
fn a_mono_byte_bound_reaches_both_transport_outputs() {
    let outcome = a_limited_mono_answer(refused_bytes);
    let domain = serialized_refusal(&outcome);
    assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
    assert_limit_outputs(&outcome, None);
}

#[test]
fn a_mono_opaque_bound_stays_numberless() {
    let outcome = a_limited_mono_answer(refused_opaquely);
    let domain = serialized_refusal(&outcome);
    assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
    assert_limit_outputs(&outcome, None);
}

#[test]
fn a_local_node_decode_bound_reaches_the_application() {
    let outcome = a_node_size_refusal(true);
    let domain = serialized_refusal(&outcome);
    assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
    assert_limit_outputs(&outcome, None);
}

#[test]
fn a_remote_node_size_code_stays_numberless_in_the_application() {
    let outcome = a_node_size_refusal(false);
    let domain = serialized_refusal(&outcome);
    assert_eq!(domain["ResultTooLarge"]["bound"], serde_json::json!("Volume"), "{domain}");
    assert_limit_outputs(&outcome, None);
}

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
    BillTo, BillingAccount, CredentialBroker, CredentialsDoNotFitTheRequest, Minted, PrincipalChain, RequestContext, SourceSet,
    Subject, SubjectKey,
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
}

/// Whom, and until when, a split was granted.
type GrantedTo = (SubjectKey, SystemTime);

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
        }
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

impl Warehouse for SplitLegs {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        self.legs.source()
    }

    fn posture(&self) -> &SourcePosture {
        self.legs.posture()
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.legs.dry_run(executable, presented, deadline)
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.executes.fetch_add(1, Ordering::SeqCst);
        self.legs.execute(executable, presented, deadline)
    }

    fn dry_run_billed(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        _bill_to: &BillTo,
        _labels: &JobLabels,
        deadline: Deadline,
    ) -> BilledResult<PreFlight, Self::Error> {
        self.dry_run(executable, presented, deadline).map_err(Billed::Source)
    }

    fn execute_billed(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        _bill_to: &BillTo,
        _labels: &JobLabels,
        deadline: Deadline,
    ) -> BilledResult<ResultBatches, Self::Error> {
        self.execute(executable, presented, deadline).map_err(Billed::Source)
    }

    fn split_lookup(
        &self,
        _leg: &LegPlan,
        _presented: &Presented,
        _deadline: Deadline,
        _above_bytes: NonZeroU64,
        _readers: NonZeroUsize,
        grantee: Option<Grantee>,
    ) -> LookupSplit<'_, Self::Error> {
        self.splits_asked.fetch_add(1, Ordering::SeqCst);
        self.offered.store(grantee.is_some(), Ordering::SeqCst);
        let first_read_takes = match (self.splits, grantee) {
            (Splits::PerRow, _) | (Splits::Granting { .. } | Splits::GrantingOutlivingTheGrant, None) => Duration::ZERO,
            (Splits::PerRowOutlivingTheGrant, _) => Duration::from_millis(1200),
            (Splits::Refused(refusal), _) => return Ok(Partitioned::Refused(refusal)),
            (Splits::Granting { portable, late }, Some(grantee)) => return Ok(self.granting(grantee, portable, late)),
            (Splits::GrantingOutlivingTheGrant, Some(grantee)) => {
                std::thread::sleep(Duration::from_millis(1200));
                return Ok(self.granting(grantee, true, false));
            }
        };
        Ok(Partitioned::Split(Box::new(self.rows.rows().iter().enumerate().map(
            move |(index, row)| {
                if index == 0 {
                    std::thread::sleep(first_read_takes);
                }
                self.partitions_read.fetch_add(1, Ordering::SeqCst);
                Ok(crate::tests_support::canned(
                    &RowSet::new(self.rows.columns().to_vec(), vec![row.clone()]).expect("one row of a row set"),
                ))
            },
        ))))
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.rows.clone()))
    }
}

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
        SubjectKey::parse("someone@example.com").expect("a test subject is a subject"),
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

    let outcome = asked(
        &warehouses,
        &asked_by_a_person(),
        &FixedBroker::GrantsSomethingExpiringInTenSeconds,
    )
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

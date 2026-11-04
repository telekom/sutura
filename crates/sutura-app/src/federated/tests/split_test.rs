//! A lookup leg split at its source - `github.com/telekom/sutura#828` - answered through
//! `answer_federated` over a registry that names the source for splitting.
//!
//! The fake lives in this `#[cfg(test)]` file for the reason `dry_run_count_test` states. It splits
//! the lookup leg itself; how a data system cuts one, and the per-stream bound each partition is
//! read under, are the adapter's and its own tier's to hold.

use core::num::{NonZeroU64, NonZeroUsize};
use core::time::Duration;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::ServiceError;
use crate::tests_support::BrokerUnreachable;
use sutura_domain::identity::{
    BillTo, BillingAccount, CredentialBroker, CredentialsDoNotFitTheRequest, Minted, RequestContext, SourceSet,
};
use sutura_domain::plan::{EstimatedRows, LegPlan, LookupSplit, Partitioned, SplitDecision, SplitRefusal, decide};
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
}

/// A [`crate::tests_support::LegsWarehouse`] that splits a lookup leg as `splits` says, takes any
/// payer, and counts the legs it reads whole, the splits it is asked for and the partitions read.
struct SplitLegs {
    legs: crate::tests_support::LegsWarehouse,
    rows: RowSet,
    splits: Splits,
    executes: AtomicUsize,
    splits_asked: AtomicUsize,
    partitions_read: AtomicUsize,
}

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
    ) -> LookupSplit<'_, Self::Error> {
        self.splits_asked.fetch_add(1, Ordering::SeqCst);
        let first_read_takes = match self.splits {
            Splits::PerRow => Duration::ZERO,
            Splits::PerRowOutlivingTheGrant => Duration::from_millis(1200),
            Splits::Refused(refusal) => return Ok(Partitioned::Refused(refusal)),
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

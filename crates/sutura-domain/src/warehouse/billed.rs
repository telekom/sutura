//! A port call that carries who pays for it.

use crate::identity::BillTo;
use crate::model::SourceName;
use crate::query::RefusalReason;

/// What a billed port call answers. Named because the spelled-out form is over the
/// `type_complexity` threshold this workspace tightened.
pub type BilledResult<V, E> = Result<V, Billed<E>>;

/// Why a billed port call did not run.
#[derive(Debug, thiserror::Error)]
pub enum Billed<E> {
    /// The leg states a payer this source does not bill, so nothing ran.
    #[error("`{at}` does not bill the payer this request states")]
    NotTaken { at: SourceName },
    /// No payer could be decided for this source's job, so nothing ran.
    #[error("`{at}` could not decide who pays for this job")]
    Undecided { at: SourceName },
    /// The source's own failure.
    #[error(transparent)]
    Source(E),
}

impl<E> Billed<E> {
    /// The source's own failure, or the refusal a payer decision made before the source was asked.
    ///
    /// # Errors
    ///
    /// The [`RefusalReason`] for [`Billed::NotTaken`] and [`Billed::Undecided`].
    pub fn into_source(self) -> Result<E, RefusalReason> {
        match self {
            Self::Source(cause) => Ok(cause),
            Self::NotTaken { at } => Err(RefusalReason::BillToNotAllowed { source: at }),
            Self::Undecided { at } => Err(RefusalReason::NoBillingProject { source: at }),
        }
    }
}

/// Refuses a stated payer at a source that bills no job to one.
///
/// # Errors
///
/// [`Billed::NotTaken`] for anything but [`BillTo::Unstated`].
pub fn unstated<E>(bill_to: &BillTo, source: &SourceName) -> BilledResult<(), E> {
    match bill_to {
        BillTo::Unstated => Ok(()),
        BillTo::Group(_) | BillTo::Named(_) => Err(Billed::NotTaken { at: source.clone() }),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::Billed;
    use crate::identity::{BillTo, BillingAccount, Presented, Secret};
    use crate::model::{SourceName, TableName};
    use crate::plan::{AnchorPlan, Executable, LegPlan, PlanBindings};
    use crate::source::{ImpersonationCapability, SourcePosture};
    use crate::warehouse::deadline::{Budget, Deadline};
    use crate::warehouse::{AnchorRows, PreFlight, ResultBatches, Warehouse};

    #[derive(Debug, thiserror::Error)]
    #[error("the source was asked")]
    struct Asked;

    /// A source that keeps the defaulted billed calls, and fails if it is ever asked.
    struct BillsNoPayer(SourceName);

    impl Warehouse for BillsNoPayer {
        type Error = Asked;

        const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;

        fn source(&self) -> &SourceName {
            &self.0
        }

        fn posture(&self) -> &SourcePosture {
            &SourcePosture::ImpersonationAtSource
        }

        fn dry_run(&self, _: Executable<'_>, _: &Presented, _: Deadline) -> Result<PreFlight, Asked> {
            Err(Asked)
        }

        fn execute(&self, _: Executable<'_>, _: &Presented, _: Deadline) -> Result<ResultBatches, Asked> {
            Err(Asked)
        }

        fn verify_anchor(&self, _: AnchorPlan<'_>) -> Result<AnchorRows, Asked> {
            Err(Asked)
        }
    }

    #[test]
    fn each_defaulted_billed_call_refuses_a_stated_payer_before_the_source_is_asked() {
        let at = SourceName::parse("ledger").expect("a test source is a source");
        let leg = LegPlan::Lookup {
            source: at.clone(),
            table: TableName::parse("dim_customer").expect("a test table is a table").into(),
            keys: Vec::new(),
            bindings: PlanBindings::none(),
        };
        let presented = Presented::SubjectToken {
            material: Secret::new("an-exchanged-token"),
        };
        let deadline = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_secs(30)).expect("thirty seconds is a budget"),
        );
        let warehouse = BillsNoPayer(at.clone());
        for payer in [
            BillTo::Named(BillingAccount::parse("caller-pays-1").expect("a test account is an account")),
            BillTo::Group(BillingAccount::parse("group-pays-1").expect("a test account is an account")),
        ] {
            let dry_run = warehouse.dry_run_billed(Executable::Leg(&leg), &presented, &payer, deadline);
            assert!(
                matches!(dry_run, Err(Billed::NotTaken { at: ref refused }) if *refused == at),
                "{dry_run:?}"
            );
            let execute = warehouse.execute_billed(Executable::Leg(&leg), &presented, &payer, deadline);
            assert!(
                matches!(execute, Err(Billed::NotTaken { at: ref refused }) if *refused == at),
                "{execute:?}"
            );
        }
    }
}

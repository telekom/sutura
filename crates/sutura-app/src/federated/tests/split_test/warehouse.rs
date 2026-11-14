use super::*;

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
        if let Some(error) = self.fail_with {
            return Err(error());
        }
        self.legs.execute(executable, presented, deadline)
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(
            error,
            AdapterFailure::TooMuchData
                | AdapterFailure::Unannounced {
                    cause: sutura_domain::warehouse::UnannouncedBatch::OverBound { .. }
                        | sutura_domain::warehouse::UnannouncedBatch::OverBudget { .. }
                }
        )
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
        _labels: &JobLabels,
    ) -> LookupSplit<'_, Self::Error> {
        self.splits_asked.fetch_add(1, Ordering::SeqCst);
        self.offered.store(grantee.is_some(), Ordering::SeqCst);
        if let Some(error) = self.fail_with {
            return Ok(Partitioned::Split(Box::new(std::iter::once_with(move || {
                self.partitions_read.fetch_add(1, Ordering::SeqCst);
                Err(error())
            }))));
        }
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

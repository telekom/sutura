//! [`RawCapableWarehouse`] - split out of `tests_support.rs` for the same reason that file is
//! split out of `lib.rs`: the parent hit the 1000-line gate.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::Executable;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::cardinality::{DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, Warehouse};

use super::{AdapterFailure, DriverFailure};

/// A data system that accepts a raw statement - `docs/adr/0013`'s raw SQL tool - and answers with a
/// fixed row count, or refuses when handed the one magic statement text `"refuse me"`.
///
/// **Row count, not row CONTENT, is the whole point of this fake.** It exists for two things no
/// certified-path fixture can stand in for: proving `crate::run_sql` actually reaches
/// `crate::exceeds_row_cap` with a real `RawRows` (`#666`'s review, finding 4 - a wire-serialisation
/// test can construct `RawRefusalReason::TooManyRows` directly and never prove anything reaches it
/// through the port), and proving `sutura_app::surface::LocalService::run_sql` writes a record for
/// each of a raw answer and a raw refusal (finding 3).
pub(crate) struct RawCapableWarehouse {
    source: SourceName,
    posture: SourcePosture,
    rows: usize,
}

impl RawCapableWarehouse {
    /// Answers every statement except `"refuse me"` with `rows` rows of one integer column.
    pub(crate) const fn answering_rows(source: SourceName, posture: SourcePosture, rows: usize) -> Self {
        Self { source, posture, rows }
    }

    fn deliverable(&self, presented: &Presented) -> Result<(), AdapterFailure> {
        match *presented {
            Presented::SharedServiceUser { .. } => Ok(()),
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => Err(AdapterFailure::NoPlaceForASubject {
                at: String::from(self.source.as_str()),
                presented: presented.as_str(),
            }),
        }
    }
}

impl Warehouse for RawCapableWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const ACCEPTS_RAW_STATEMENTS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    /// Unreached by the raw path and never exercised by the certified one in these tests.
    fn dry_run(&self, _executable: Executable<'_>, presented: &Presented, _deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.deliverable(presented)?;
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.deliverable(presented)?;
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }

    fn declared_key(&self, _key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        Ok(KeyUniqueness::NotAsked)
    }

    fn execute_raw(
        &self,
        statement: &sutura_domain::raw::RawStatement,
        presented: &Presented,
    ) -> sutura_domain::warehouse::RawExecution<Self::Error> {
        if let Err(cause) = self.deliverable(presented) {
            return Some(Err(cause));
        }
        if statement.as_str() == "refuse me" {
            return Some(Err(AdapterFailure::RefusedBySource));
        }
        let labels = vec![String::from("n")];
        let rows: Vec<Vec<sutura_domain::warehouse::Value>> = (0..self.rows)
            .map(|n| vec![sutura_domain::warehouse::Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))])
            .collect();
        Some(Ok(sutura_domain::warehouse::RawRows::of(labels, rows)))
    }
}

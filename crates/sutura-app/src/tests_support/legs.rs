//! The two leg-executing fakes: the one that answers a leg, and the one that refuses to hand a
//! whole result over at once.
//!
//! **One concept, and its own module for the reason `priced` and `authored` have theirs:**
//! `tests_support.rs` is at the unexemptable `max-lines` cap, and *a data system that declares
//! `Warehouse::EXECUTES_LEGS`* is the seam that was already there rather than wherever the counter
//! fell. Everything here is a fake the FEDERATED path runs; nothing here is on the mono path.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::Executable;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, RowSet, Warehouse};

use super::{AdapterFailure, canned};

/// A fake that can run one half of a federated answer.
///
/// Unlike [`FixedWarehouse`] it declares [`Warehouse::EXECUTES_LEGS`], so the federated path will
/// not refuse it : that is the whole difference, and it is why the port exposes the capability
/// rather than letting `answer` assume. Each instance holds one scripted result and answers any
/// statement with it, which is enough to exercise the orchestrator above real legs - the combiner's
/// own correctness is proven in the domain suite against the same plan shapes this feeds it.
pub(crate) struct LegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
}

impl LegsWarehouse {
    pub(crate) fn answering(source: SourceName, posture: SourcePosture, result: RowSet) -> Self {
        Self { source, posture, result }
    }
}

impl Warehouse for LegsWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Ok(canned(&self.result))
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

/// A leg-executing fake whose `execute` fails because the data system would not return the whole
/// result at once.
///
/// The instrument for the federated half of the volume bound: the same reach
/// [`LegsWarehouse`] gives (it declares [`Warehouse::EXECUTES_LEGS`], so the federated path runs it),
/// but its `execute` returns `Err` and its [`Warehouse::result_did_not_fit`] answers `true`, so
/// `run_leg` must turn it into a [`RefusalReason::ResultTooLarge`] carrying
/// [`ResultBound::Volume`] and never into the `503` a dead data system produces. `federated.rs`'s
/// `a_federated_leg_that_hits_the_volume_bound_is_refused_not_a_503` pins that.
pub(crate) struct PageBoundLegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
}

impl PageBoundLegsWarehouse {
    pub(crate) fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self { source, posture }
    }
}

impl Warehouse for PageBoundLegsWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Err(AdapterFailure::TooMuchData)
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::TooMuchData)
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(error, AdapterFailure::TooMuchData)
    }
}

/// A leg-executing fake whose `execute` fails transiently - not a refusal, not a bound, not a
/// deadline - as `NoPlaceForASubject` naming its own source.
///
/// **The instrument for the precedence pin when BOTH legs fail transiently.** Both legs run in the
/// concurrent scope, so both produce a `LegError::Failure`; the fact leg is inspected first in
/// `answer_federated`, so ITS warehouse error must be the one surfaced. The error carries the
/// failing source's name (this fake's `source`), so whichever leg won is distinguishable - and
/// `NoPlaceForASubject` satisfies none of `run_leg`'s governance predicates, so both failures leave
/// as `Err` rather than a `ToolOutcome::Refusal`.
pub(crate) struct TransientlyFailingLegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
}

impl TransientlyFailingLegsWarehouse {
    pub(crate) fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self { source, posture }
    }
}

impl Warehouse for TransientlyFailingLegsWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Err(AdapterFailure::NoPlaceForASubject {
            at: String::from(self.source.as_str()),
            presented: "a-leg-executing-fake",
        })
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::NoPlaceForASubject {
            at: String::from(self.source.as_str()),
            presented: "a-leg-executing-fake",
        })
    }
}

//! One leg's own pre-flight and its own execution, split out of `federated.rs` for `cargo xtask
//! max-lines`'s cap - the same reason that file is split out of `lib.rs`'s, not for thematic
//! tidiness. Its tests stay in `federated.rs`'s own `mod tests`, which exercises these functions
//! through [`super::answer_federated`] exactly as the one function they used to be did before the
//! split.
//!
//! **Two functions rather than one**, and that split is `docs/adr/0030`'s, not `max-lines`'s:
//! `answer_federated` dry-runs both legs (`dry_run_leg`), sums and charges their own estimates
//! against the spend ledger, and only then runs either one's `execute` (`run_leg`) - "all-or-nothing
//! before any leg executes". The posture agreement is checked once, inside `dry_run_leg`, because it
//! is a property of the credential and the adapter rather than of which call is about to run - the
//! mono path's own `sutura_app::answer` checks it once too, before its own `dry_run`.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use sutura_domain::identity::{BoundToTheRequest, CredentialBroker, RequestContext};
use sutura_domain::plan::{Executable, FederationCombiner, LegPlan, LegRunner};
use sutura_domain::query::{RefusalReason, ResultBound};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{PreFlight, ResultBatches, Warehouse};

use crate::{ServiceError, SpendLedger, Warehouses, deadline_exceeded, now_in_unix_seconds};

use super::{LegAnswer, LegCleared, LegError, LegPreflight};

/// Pre-flights one leg against its own adapter, under that source's own presented credential.
pub(crate) fn dry_run_leg<W, B, C>(
    warehouse: &W,
    credentials: &BoundToTheRequest,
    leg: &LegPlan,
    deadline: Deadline,
) -> LegPreflight<W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    let presented = credentials
        .presented_for(leg.source())
        .map_err(|cause| ServiceError::Credentials { cause })?;
    presented
        .agrees_with(warehouse.posture(), leg.source())
        .map_err(|cause| ServiceError::Posture { cause })?;
    // What the shared `Deadline` has left (`docs/adr/0029` decision 3), asked before every leg.
    if deadline.remaining_at(Instant::now()).is_none() {
        return Err(LegError::Refusal(deadline_exceeded(deadline)));
    }
    // The same refusal a leg's `execute` can carry, asked of the pre-flight for the same reason
    // (see `sutura_app::answer`): a data system may refuse the statement as this identity while it
    // prepares, and that refusal must reach the caller as `SourceRefused` - the leg refusing as it
    // would on `execute` - never as the retryable `ServiceError::Warehouse` a dead data system
    // produces. `working_set_exhausted` and `result_did_not_fit` are deliberately not asked of the
    // pre-flight, mirroring the mono path: a check reads no data, so neither bound can have fired.
    match warehouse.dry_run(Executable::Leg(leg), presented, deadline) {
        Ok(preflight) => Ok(preflight),
        Err(cause) => {
            if warehouse.deadline_exceeded(&cause) {
                return Err(LegError::Refusal(deadline_exceeded(deadline)));
            }
            if warehouse.source_refused(&cause) {
                return Err(LegError::Refusal(RefusalReason::SourceRefused {
                    source: warehouse.source().clone(),
                }));
            }
            Err(LegError::Failure(ServiceError::Warehouse { cause }))
        }
    }
}

/// Runs one leg's `execute`, after its own dry run (and the spend ledger, above both legs) have
/// already cleared it.
///
/// The same guards `execute_leg` applied run here for the same reasons: the credential is still
/// usable this instant, checked again here because a deadline that ages out during the OTHER leg's
/// own dry run is caught by this leg's own `still_usable_at`.
pub(crate) fn run_leg<W, B, C>(
    warehouse: &W,
    credentials: &BoundToTheRequest,
    leg: &LegPlan,
    deadline: Deadline,
) -> LegAnswer<W, B, C>
where
    W: Warehouse + Sync,
    W::Error: Send,
    B: CredentialBroker,
    B::Error: Send,
    C: FederationCombiner,
    C::Error: Send,
{
    let presented = credentials
        .presented_for(leg.source())
        .map_err(|cause| ServiceError::Credentials { cause })?;
    credentials
        .still_usable_at(now_in_unix_seconds())
        .map_err(|cause| ServiceError::Credentials { cause })?;
    if deadline.remaining_at(Instant::now()).is_none() {
        return Err(LegError::Refusal(deadline_exceeded(deadline)));
    }
    match warehouse.execute(Executable::Leg(leg), presented, deadline) {
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
            // The deadline, in the mono path's own order: after the two size bounds, before identity.
            if warehouse.deadline_exceeded(&cause) {
                return Err(LegError::Refusal(deadline_exceeded(deadline)));
            }
            // The same guard, for the same reason: the data system refused THIS leg's statement at
            // the identity/authorization level. It used to leave as `LegError::Failure` and reach a
            // caller as the `503` an outage produces, so a caller was told to retry a refusal that
            // returns the same reply.
            if warehouse.source_refused(&cause) {
                return Err(LegError::Refusal(RefusalReason::SourceRefused {
                    source: warehouse.source().clone(),
                }));
            }
            Err(LegError::Failure(ServiceError::Warehouse { cause }))
        }
    }
}

/// Pre-flights every leg in order, then charges what they priced as one sum against `context`'s
/// subject - `docs/adr/0030`'s all-or-nothing: a federated answer is refused as a whole before any
/// leg executes, never after one has spent.
///
/// Only a priced leg is counted, and a leg that priced nothing contributes nothing - "not counted",
/// never "free". Nothing is charged at all when NO leg priced, so an all-`None` federated answer
/// (every adapter but `BigQuery`, today) never touches the ledger.
///
/// What it returns is one token for each leg it cleared, which the leg runner spends.
pub(crate) fn preflight_and_charge<'a, W, B, C>(
    legs: impl IntoIterator<Item = (&'a LegPlan, &'a W)>,
    credentials: &BoundToTheRequest,
    deadline: Deadline,
    ledger: &SpendLedger,
    context: &RequestContext,
) -> LegCleared<'a, W, B, C>
where
    W: Warehouse + 'a,
    B: CredentialBroker,
    C: FederationCombiner,
{
    let mut priced: Option<u64> = None;
    let mut cleared = Vec::new();
    for (leg, warehouse) in legs {
        if let PreFlight::Accepted {
            estimated_bytes: Some(bytes),
        } = dry_run_leg::<_, B, C>(warehouse, credentials, leg, deadline)?
        {
            priced = Some(priced.unwrap_or(0).saturating_add(bytes.bytes()));
        }
        cleared.push((leg, AtomicBool::new(false)));
    }
    priced
        .and_then(|total| crate::charge_subject(ledger, context, total, Instant::now()))
        .map_or(Ok(Charged(cleared)), |reason| Err(LegError::Refusal(reason)))
}

/// One token for each leg the charge cleared, each spent once by the leg runner.
///
/// A leg is its address in the plan the cut borrows, so a leg a combiner built for itself, even an
/// equal one, has no token.
pub(crate) struct Charged<'p>(Vec<(&'p LegPlan, AtomicBool)>);

impl Charged<'_> {
    /// Spends `leg`'s token: `false` for a leg that is not among the cut's own legs, or one spent before.
    fn spend(&self, leg: &LegPlan) -> bool {
        self.0
            .iter()
            .any(|(given, spent)| core::ptr::eq(*given, leg) && !spent.swap(true, Ordering::Relaxed))
    }
}

/// [`run_leg`] behind the leg runner port, for the combiner to call once for each leg of the cut.
///
/// Built on the request's own thread, so `span` is the request's span, and each leg enters it:
/// a leg the combiner runs on a scoped thread is still attributable to the question it belongs to.
///
/// A leg without a token from `charged` is refused as this workspace's own defect, before its
/// warehouse is looked up or its credential read: it was never charged.
pub(crate) struct LegsAtTheirSources<'a, W, B, C> {
    pub(crate) charged: Charged<'a>,
    pub(crate) warehouses: &'a Warehouses<W>,
    pub(crate) credentials: &'a BoundToTheRequest,
    pub(crate) deadline: Deadline,
    pub(crate) span: tracing::Span,
    pub(crate) ports: Ports<B, C>,
}

/// The broker and combiner the runner's error is typed by, held as a type and never as a value.
pub(crate) type Ports<B, C> = PhantomData<fn() -> (B, C)>;

impl<W, B, C> LegRunner for LegsAtTheirSources<'_, W, B, C>
where
    W: Warehouse + Sync,
    W::Error: Send,
    B: CredentialBroker,
    B::Error: Send,
    C: FederationCombiner,
    C::Error: Send,
{
    type Error = super::LegError<W::Error, B::Error, C::Error>;

    fn run(&self, leg: &LegPlan) -> Result<ResultBatches, Self::Error> {
        let _entered = self.span.enter();
        if !self.charged.spend(leg) {
            return Err(LegError::Failure(ServiceError::Miswired {
                cause: crate::FederationMiswired::LegNotGivenOut {
                    at: leg.source().clone(),
                },
            }));
        }
        let Some(warehouse) = self.warehouses.get(leg.source()) else {
            return Err(LegError::Refusal(RefusalReason::SourceUnavailable {
                source: leg.source().clone(),
            }));
        };
        run_leg::<_, B, C>(warehouse, self.credentials, leg, self.deadline)
    }
}

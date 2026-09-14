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

use std::time::Instant;

use sutura_domain::identity::{BoundToTheRequest, CredentialBroker};
use sutura_domain::plan::{Executable, LegPlan};
use sutura_domain::query::{RefusalReason, ResultBound};
use sutura_domain::warehouse::Warehouse;
use sutura_domain::warehouse::deadline::Deadline;

use crate::{ServiceError, deadline_exceeded, now_in_unix_seconds};

use super::{LegError, LegPreflight, LegResult};

/// Pre-flights one leg against its own adapter, under that source's own presented credential.
pub(crate) fn dry_run_leg<W, B>(
    warehouse: &W,
    credentials: &BoundToTheRequest,
    leg: &LegPlan,
    deadline: Deadline,
) -> LegPreflight<W, B>
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
pub(crate) fn run_leg<W, B>(warehouse: &W, credentials: &BoundToTheRequest, leg: &LegPlan, deadline: Deadline) -> LegResult<W, B>
where
    W: Warehouse,
    B: CredentialBroker,
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

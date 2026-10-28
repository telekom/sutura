//! Node B's own leg: runs one remote leg this node certifies, under the caller already established
//! at this node - the serving side of `docs/adr/20261017032054-leg-transport-between-sutura-nodes.md`'s node-to-node channel, `telekom/sutura#137`
//! step 3.
//!
//! # Mirrors [`crate::answer`]'s credential handling exactly, for a single leg
//!
//! Mint once, check the grant agrees with the request, pre-flight and charge, execute - the same
//! guard order the mono path in `lib.rs` uses, over the pre-flight-and-charge-and-run machinery
//! `crate::federated::leg` already built for a federated answer's own single leg: that machinery is
//! per leg already, so this is its one-leg case rather than a second copy of it.
//!
//! # What is different from [`crate::answer`], and why
//!
//! **The plan arrives already typed, as a [`LegWire`], rather than compiled from a
//! [`Query`](sutura_domain::query::Query).**
//! There is no question to compile: a remote leg crossed from another node as a
//! [`LegPlan`](sutura_domain::plan::leg::LegPlan) already narrowed to one source, and
//! [`LegWire::parse`] is this node's own certification of it against its own pin - the parse this
//! module's whole job is to gate on.
//!
//! **Decision 7 refuses a source this node will not run a remote leg through at all, before
//! anything is minted.** A source's POSTURE VARIANT decides it, compared with `matches!` rather
//! than a value: `SourcePosture::ImpersonationAtSource` and [`Warehouse::executes_legs`] both true,
//! or the source is refused the same way an uncertified one is - a shared source and a source that
//! declines legs are refused by name, never distinguished from a digest mismatch, so a hostile
//! sender learns nothing about which of those this node's refusal means.
//!
//! **The outcome is [`RemoteLegOutcome`], never [`ToolOutcome`](sutura_domain::query::ToolOutcome).**
//! [`sutura_domain::raw::RawOutcome`]'s own module documentation gives the reason this module
//! inherits unchanged: neither variant carries a [`sutura_domain::pinned::Provenance`], so a remote
//! leg's answer cannot be mistaken for this node's own certified one at the type level.
//!
//! # Not built here, named rather than silently absent
//!
//! **`kind: node` does not exist yet.** Decision 9 - refusing a source that is itself another
//! node, rather than a data system - needs a `SourceKind` this node's registry does not carry in
//! this slice; it is `telekom/sutura#137` step 3's slice 4. Nothing here special-cases a node
//! source, so one is refused the same way an uncertified source is, under the same closed code -
//! which is not Decision 9's own refusal, and is recorded here as the open point rather than
//! implied to be covered.

use sutura_domain::identity::{BoundToTheRequest, CredentialBroker, Expiry, RequestContext, SourceSet};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::remote::{LegWire, RemoteLegOutcome};
use sutura_domain::plan::{FederationCombiner, LegRunner};
use sutura_domain::query::RefusalReason;
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_domain::warehouse::{ParamValue, Warehouse};

use crate::federated::LegError;
use crate::{ServiceError, SpendLedger, Warehouses, granted, now_in_unix_seconds};

/// One remote-leg call's result: what the caller is told, and what it ran under - this path's own
/// [`crate::Answered`], over [`RemoteLegOutcome`] rather than
/// [`ToolOutcome`](sutura_domain::query::ToolOutcome).
#[derive(Debug)]
pub struct RanRemoteLeg {
    outcome: RemoteLegOutcome,
    executed_until: Option<Expiry>,
}

impl RanRemoteLeg {
    /// An outcome decided before any credential was minted.
    const fn declined_before_minting(outcome: RemoteLegOutcome) -> Self {
        Self {
            outcome,
            executed_until: None,
        }
    }

    /// An outcome decided with a checked grant in hand.
    const fn under(credentials: &BoundToTheRequest, outcome: RemoteLegOutcome) -> Self {
        Self {
            outcome,
            executed_until: Some(credentials.not_after()),
        }
    }

    /// What the caller is told.
    #[inline]
    #[must_use]
    pub const fn outcome(&self) -> &RemoteLegOutcome {
        &self.outcome
    }

    /// What the caller is told, owned, for a transport that is about to render it.
    #[inline]
    #[must_use]
    pub fn into_outcome(self) -> RemoteLegOutcome {
        self.outcome
    }

    /// How long the credential this call ran under was good for. `None` if none was minted.
    #[inline]
    #[must_use]
    pub const fn executed_until(&self) -> Option<Expiry> {
        self.executed_until
    }
}

/// What running a remote leg produced, or why it could not.
pub type RunningRemoteLeg<W, B, C> =
    Result<RanRemoteLeg, ServiceError<<W as Warehouse>::Error, <B as CredentialBroker>::Error, <C as FederationCombiner>::Error>>;

/// The refusal this module gives for a source it will not even try to parse `leg` against: not
/// configured, not certified against this node's own pin, or refused by Decision 7's posture
/// check - all one closed code, naming only the source.
const fn not_certified(source: sutura_domain::model::SourceName) -> RemoteLegOutcome {
    RemoteLegOutcome::Refusal {
        reason: RefusalReason::RemoteLegNotCertified { source },
    }
}

/// Runs one remote leg this node certifies, under the caller already established at this node, or
/// says why it will not - this module's own documentation has the shape and the limits.
pub fn run_remote_leg<W, B, C>(
    pinned: &PinnedDefinitions,
    leg: LegWire,
    params: Vec<ParamValue>,
    context: &RequestContext,
    broker: &B,
    warehouses: &Warehouses<W>,
    deadline: Deadline,
    ledger: &SpendLedger,
) -> RunningRemoteLeg<W, B, C>
where
    W: Warehouse + Sync,
    W::Error: Send,
    B: CredentialBroker,
    B::Error: Send,
    C: FederationCombiner,
    C::Error: Send,
{
    let source = leg.source().clone();
    let Some(warehouse) = warehouses.get(&source) else {
        return Ok(RanRemoteLeg::declined_before_minting(not_certified(source)));
    };
    // Decision 7, read off the adapter's own posture variant and its own declared capability -
    // never a value a sender could have shaped to look like a match.
    if !(matches!(warehouse.posture(), SourcePosture::ImpersonationAtSource) && warehouse.executes_legs()) {
        return Ok(RanRemoteLeg::declined_before_minting(not_certified(source)));
    }
    let plan = match leg.parse(pinned, params) {
        Ok(plan) => plan,
        // Every certification cause collapses here, discarding which one: the sending node's own
        // definitions, or which of this node's own it disagrees with, are not this refusal's
        // business to disclose. See this module's own documentation.
        Err(_cause) => return Ok(RanRemoteLeg::declined_before_minting(not_certified(source))),
    };
    let labels = JobLabels::of(context);
    let requested = SourceSet::of(source);
    let minted = broker
        .mint(context, &requested)
        .map_err(|cause| ServiceError::Broker { cause })?;
    let agreed = minted
        .agreeing_with(context.chain().subject(), &requested, now_in_unix_seconds())
        .map_err(|cause| ServiceError::Credentials { cause })?;
    let credentials = match granted(agreed) {
        Ok(credentials) => credentials,
        Err(reason) => return Ok(RanRemoteLeg::declined_before_minting(RemoteLegOutcome::Refusal { reason })),
    };
    // The pre-flight, the spend charge and the deadline check already made for a federated answer's
    // own single leg - reused rather than re-derived, so this path refuses a spent budget before the
    // port exactly where `answer`'s own single-leg path does.
    let charged = match crate::federated::preflight_and_charge::<W, B, C>(
        core::iter::once((&plan, warehouse)),
        (&credentials, &labels),
        deadline,
        ledger,
        context,
    ) {
        Ok(charged) => charged,
        Err(LegError::Refusal(reason)) => {
            return Ok(RanRemoteLeg::under(&credentials, RemoteLegOutcome::Refusal { reason }));
        }
        Err(LegError::Failure(failure)) => return Err(failure),
    };
    let runner = crate::federated::LegsAtTheirSources::<W, B, C>::new(charged, warehouses, (&credentials, &labels), deadline);
    match LegRunner::run(&runner, &plan) {
        Ok(batches) => Ok(RanRemoteLeg::under(&credentials, RemoteLegOutcome::Answer { batches })),
        Err(LegError::Refusal(reason)) => Ok(RanRemoteLeg::under(&credentials, RemoteLegOutcome::Refusal { reason })),
        Err(LegError::Failure(failure)) => Err(failure),
    }
}

#[cfg(test)]
mod tests;

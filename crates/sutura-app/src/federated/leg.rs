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

use core::iter;
use core::num::{NonZeroU64, NonZeroUsize};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime};

use sutura_domain::identity::{BillTo, BoundToTheRequest, CredentialBroker, Presented, RequestContext};
use sutura_domain::plan::{
    Executable, FederationCombiner, Granted, Grantee, LegPlan, LegRunner, LinkCast, Looked, NotGranted, NotOffered,
    PartitionReader, Partitioned,
};
use sutura_domain::query::{RefusalReason, ResultBound};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_domain::warehouse::{PreFlight, ResultBatches, Warehouse};

use crate::{ServiceError, SpendLedger, Warehouses, deadline_exceeded, now_in_unix_seconds};

use super::{LegAnswer, LegCleared, LegError, LegPreflight};

/// Pre-flights one leg against its own adapter, under that source's own presented credential.
pub(crate) fn dry_run_leg<W, B, C>(
    warehouse: &W,
    (credentials, labels): (&BoundToTheRequest, &JobLabels),
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
    let bill_to = credentials.bill_to_for(leg.source());
    match warehouse.dry_run_billed(Executable::Leg(leg), presented, bill_to, labels, deadline) {
        Ok(preflight) => Ok(preflight),
        Err(billed) => {
            let cause = billed.into_source().map_err(LegError::Refusal)?;
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

/// A leg's failure, over the three error types [`LegAnswer`] names.
type Failed<W, B, C> = LegError<<W as Warehouse>::Error, <B as CredentialBroker>::Error, <C as FederationCombiner>::Error>;

/// `T`, or [`Failed`] - named for the `type_complexity` reason [`LegAnswer`] is.
type Leg<T, W, B, C> = Result<T, Failed<W, B, C>>;

/// What a split asked of a lookup leg's source answered: partitions read here, or granted to the
/// nodes that read it.
enum Asked<'r, E> {
    Here(PartitionReader<'r, E>),
    Granted(Granted<'r>),
}

/// A lookup leg's partitions, or why it was not split - named for the same reason.
type Partitions<'r, W, B, C> = Leg<Asked<'r, Failed<W, B, C>>, W, B, C>;

/// The credential `leg` is read under, once it is still usable and `deadline` has time left.
///
/// The same guards `execute_leg` applied run here for the same reasons: the credential is still
/// usable this instant, checked again here because a deadline that ages out during the OTHER leg's
/// own dry run is caught by this leg's own `still_usable_at`.
fn usable<'c, W, B, C>(credentials: &'c BoundToTheRequest, leg: &LegPlan, deadline: Deadline) -> Leg<&'c Presented, W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
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
    Ok(presented)
}

/// Runs one leg's `execute`, after its own dry run (and the spend ledger, above both legs) have
/// already cleared it, behind [`usable`]'s guards.
pub(crate) fn run_leg<W, B, C>(
    warehouse: &W,
    (credentials, labels): (&BoundToTheRequest, &JobLabels),
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
    let presented = usable::<W, B, C>(credentials, leg, deadline)?;
    match warehouse.execute_billed(
        Executable::Leg(leg),
        presented,
        credentials.bill_to_for(leg.source()),
        labels,
        deadline,
    ) {
        Ok(rows) => Ok(rows),
        Err(billed) => {
            let cause = billed.into_source().map_err(LegError::Refusal)?;
            Err(classified::<W, B, C>(warehouse, cause, deadline))
        }
    }
}

/// `warehouse`'s failure to read a leg, or one partition of it, as the caller learns it.
fn classified<W, B, C>(warehouse: &W, cause: W::Error, deadline: Deadline) -> Failed<W, B, C>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    // The two governance predicates the mono path asks of its own `execute`, asked here for
    // the same reasons (see `sutura_app::answer`), and in the same order: exhaustion is
    // refused first, then a result the data system would not return at once, otherwise the
    // failure leaves as the `503` an outage produces. Without this, a leg-executing adapter
    // that hit either bound reached a caller as `503` - a status inviting the very retry that
    // would return the same reply. `dry_run` above is deliberately not given the treatment,
    // mirroring the mono path: a check reads no data, so neither bound can have fired there.
    if let Some(ceiling_bytes) = warehouse.working_set_exhausted(&cause) {
        return LegError::Refusal(RefusalReason::ResourcesExhausted { ceiling_bytes });
    }
    if warehouse.result_did_not_fit(&cause) {
        return LegError::Refusal(RefusalReason::ResultTooLarge {
            bound: ResultBound::Volume,
        });
    }
    // The deadline, in the mono path's own order: after the two size bounds, before identity.
    if warehouse.deadline_exceeded(&cause) {
        return LegError::Refusal(deadline_exceeded(deadline));
    }
    // Another node declined a leg it does not certify: refused, naming this node's source.
    // Asked here, not of the pre-flight: a node client has none, so `execute` first reaches B.
    if warehouse.leg_not_certified(&cause) {
        return LegError::Refusal(RefusalReason::RemoteLegNotCertified {
            source: warehouse.source().clone(),
        });
    }
    // The same guard, for the same reason: the data system refused THIS leg's statement at
    // the identity/authorization level. It used to leave as `LegError::Failure` and reach a
    // caller as the `503` an outage produces, so a caller was told to retry a refusal that
    // returns the same reply.
    if warehouse.source_refused(&cause) {
        return LegError::Refusal(RefusalReason::SourceRefused {
            source: warehouse.source().clone(),
        });
    }
    LegError::Failure(ServiceError::Warehouse { cause })
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
    asked: (&BoundToTheRequest, &JobLabels),
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
        } = dry_run_leg::<_, B, C>(warehouse, asked, leg, deadline)?
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
    charged: Charged<'a>,
    warehouses: &'a Warehouses<W>,
    credentials: &'a BoundToTheRequest,
    labels: &'a JobLabels,
    deadline: Deadline,
    span: tracing::Span,
    ports: Ports<B, C>,
}

/// The broker and combiner the runner's error is typed by, held as a type and never as a value.
type Ports<B, C> = PhantomData<fn() -> (B, C)>;

impl<'a, W, B, C> LegsAtTheirSources<'a, W, B, C> {
    /// A runner for the legs `charged` cleared, on the request's own span.
    pub(crate) fn new(
        charged: Charged<'a>,
        warehouses: &'a Warehouses<W>,
        (credentials, labels): (&'a BoundToTheRequest, &'a JobLabels),
        deadline: Deadline,
    ) -> Self {
        Self {
            charged,
            warehouses,
            credentials,
            labels,
            deadline,
            span: tracing::Span::current(),
            ports: PhantomData,
        }
    }
}

impl<W, B, C> LegsAtTheirSources<'_, W, B, C>
where
    W: Warehouse + Sync,
    W::Error: Send,
    B: CredentialBroker,
    B::Error: Send,
    C: FederationCombiner,
    C::Error: Send,
{
    /// `leg`'s warehouse, once its token is spent.
    fn cleared(&self, leg: &LegPlan) -> Leg<&W, W, B, C> {
        if !self.charged.spend(leg) {
            return Err(LegError::Failure(ServiceError::Miswired {
                cause: crate::FederationMiswired::LegNotGivenOut {
                    at: leg.source().clone(),
                },
            }));
        }
        self.warehouses.get(leg.source()).ok_or_else(|| {
            LegError::Refusal(RefusalReason::SourceUnavailable {
                source: leg.source().clone(),
            })
        })
    }

    /// `leg` asked of a source [`Warehouses::splitting`] named: its partitions, its one read when
    /// the source answers [`Partitioned::Whole`], its grant, or why none of them.
    ///
    /// [`usable`]'s guards are asked before the split, and the grant's `still_usable_at` again before
    /// each partition is asked for - the call after the last one too, so a grant that expires during
    /// the last read refuses the leg. Each partition's failure is [`classified`] as a whole leg's is.
    /// [`Partitioned::Refused`] is `ResultTooLarge`: no partition count the declared readers allow
    /// keeps each under the source's per-stream bound, so nothing is read.
    ///
    /// **A request with no verified caller is refused here, before the source is asked anything**,
    /// whatever the leg's size, columns or link: a split leg may be granted, a grant names its
    /// caller, and this deployment does not read such a leg as itself instead. `sutura_config` refuses
    /// at boot a serving node whose peers read a source it declares while `security.inbound` is
    /// absent (`NotFitToServe::ReadingNodeWithoutInboundIdentity`), so this is the defence behind
    /// that, for a caller a declared inbound mode still leaves unverified. A leg whose `link` is
    /// [`LinkCast::Undeclared`] is then offered for granting ([`Self::grantee`]), and a grant the
    /// source would not make is refused ([`Self::not_granted`]), never read here instead.
    fn partitioned(
        &self,
        leg: &LegPlan,
        (above_bytes, readers): (NonZeroU64, NonZeroUsize),
        link: LinkCast,
    ) -> Partitions<'_, W, B, C> {
        let _entered = self.span.enter();
        let warehouse = self.cleared(leg)?;
        let deadline = self.deadline;
        let presented = usable::<W, B, C>(self.credentials, leg, deadline)?;
        let grantee = match self.grantee(leg) {
            Err(NotOffered::NoCaller) => {
                return Err(LegError::Refusal(RefusalReason::CredentialUnavailable {
                    source: leg.source().clone(),
                }));
            }
            Err(NotOffered::NotSendable(_)) => None,
            Ok(grantee) => (link == LinkCast::Undeclared).then_some(grantee),
        };
        match warehouse.split_lookup(leg, presented, deadline, above_bytes, readers, grantee) {
            Ok(Partitioned::Whole) => Ok(Asked::Here(Box::new(iter::once(run_leg::<_, B, C>(
                warehouse,
                (self.credentials, self.labels),
                leg,
                deadline,
            ))))),
            Ok(Partitioned::Split(mut parts)) => {
                let (span, credentials) = (&self.span, self.credentials);
                Ok(Asked::Here(Box::new(iter::from_fn(move || {
                    let _entered = span.enter();
                    if let Err(cause) = credentials.still_usable_at(now_in_unix_seconds()) {
                        return Some(Err(ServiceError::Credentials { cause }.into()));
                    }
                    parts
                        .next()
                        .map(|part| part.map_err(|cause| classified::<W, B, C>(warehouse, cause, deadline)))
                }))))
            }
            Ok(Partitioned::Granted(grant)) => Ok(Asked::Granted(grant)),
            Ok(Partitioned::Refused(_)) => Err(LegError::Refusal(RefusalReason::ResultTooLarge {
                bound: ResultBound::Volume,
            })),
            Ok(Partitioned::NotGranted(refused)) => Err(self.not_granted(leg, refused)),
            Err(cause) => Err(classified::<W, B, C>(warehouse, cause, deadline)),
        }
    }

    /// `leg` granted for the caller the request's grant names, until the earlier of the question's
    /// deadline and that grant's own expiry - [`NotOffered::NotSendable`] for a leg that does not
    /// encode, which is then read here as it was before a leg could be granted.
    fn grantee(&self, leg: &LegPlan) -> Result<Grantee, NotOffered> {
        let now = SystemTime::now();
        let left = self.deadline.remaining_at(Instant::now()).unwrap_or_default();
        Grantee::new(leg, self.credentials, now.checked_add(left).unwrap_or(now))
    }

    /// The failure a grant `leg`'s source would not make is: a partition no other node can read is
    /// `SourceUnavailable`, and a grant that ended before the source handed back its partitions is
    /// the request's own grant expiring when that has passed, else the question's deadline.
    fn not_granted(&self, leg: &LegPlan, refused: NotGranted) -> Failed<W, B, C> {
        match refused {
            NotGranted::NotPortable => LegError::Refusal(RefusalReason::SourceUnavailable {
                source: leg.source().clone(),
            }),
            NotGranted::Expired(_) => match self.credentials.still_usable_at(now_in_unix_seconds()) {
                Err(cause) => ServiceError::Credentials { cause }.into(),
                Ok(()) => LegError::Refusal(deadline_exceeded(self.deadline)),
            },
        }
    }
}

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
        let warehouse = self.cleared(leg)?;
        run_leg::<_, B, C>(warehouse, (self.credentials, self.labels), leg, self.deadline)
    }

    /// Split at the source when [`Warehouses::splitting`] named `leg`'s source and `leg` names no
    /// payer - a split reads no payer - read here or granted, otherwise [`Self::run`]'s one read.
    fn lookup(&self, leg: &LegPlan, link: LinkCast) -> Result<Looked<'_>, Self::Error> {
        let at = self
            .warehouses
            .split_at(leg.source())
            .filter(|_| matches!(self.credentials.bill_to_for(leg.source()), BillTo::Unstated));
        let Some(at) = at else {
            return self.run(leg).map(|rows| Looked::Rows(vec![rows]));
        };
        match self.partitioned(leg, at, link)? {
            Asked::Here(parts) => parts.collect::<Result<Vec<_>, _>>().map(Looked::Rows),
            Asked::Granted(granted) => Ok(Looked::Granted(granted)),
        }
    }
}

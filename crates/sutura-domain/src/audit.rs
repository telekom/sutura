//! The record one call is written to, and the port it is written through.
//!
//! # Why this is sutura's job and cannot be delegated downstream
//!
//! `docs/adr/0008` walks every identity model a data system offers and finds that only one of them
//! can express "an agent acting for a human" in the session itself. The token exchange this design
//! uses issues an *impersonation* token rather than a *delegation* one - there is no `act` claim to
//! carry - so the source's own audit log says "this person" and cannot say "sutura, for this
//! person". The principal chain therefore exists nowhere downstream, and a record of it has to be
//! written here or nowhere.
//!
//! # Written before the outcome returns, and retained not at all
//!
//! Two claims, and they answer different questions.
//!
//! **Written.** One record per call, refusals included, before the outcome goes back to the caller.
//! Before rather than after, because a record written after the response is the record a crash
//! loses, and the call worth having a record of is the one that went wrong. It is a different
//! channel from [`crate::pinned::Provenance`]: provenance rides on the result and a client is free
//! to drop it, and a record only the caller holds is not a record.
//!
//! **Retained nothing.** No archive, no rotation, no retention window, no query interface over past
//! calls, and no obligation inherited from any of those. Everything after the write belongs to the
//! deployment: where the records go, how long they are kept, who may read them.
//!
//! **The limit, next to the claim.** An emitted record is worth what the sink behind it is worth,
//! and sutura cannot vouch for a sink it does not retain. A deployment whose sink drops records has
//! no audit trail on this side and nothing here can tell it so - which is why the sources' own logs,
//! written under the asking subject, carry the part of the obligation that matters.
//!
//! # The incident question, and which half of it this record can answer
//!
//! `docs/adr/0008` fixes the full content as the chain, the outcome, **the sources the plan read and
//! the posture each leg ran under, and the expiry the credentials carried.**
//!
//! **The posture is reachable, and it was not named until a review asked the question it exists for.**
//! A verified subject's question can be answered under the *deployment's* own identity on a source
//! declared `shared-service-user` - that is honest, acknowledged and not impersonation - and the
//! incident question is then "whose access filtered these rows". The answer is
//! [`crate::source::UniformlyExecuted`], which rides on the [`Provenance`] an answer carries, and
//! [`CallRecord::executed_as`] is the accessor: a sink writing an audit line does not have to know
//! that provenance transitively holds it. It answers `None` for a refusal, because nothing executed.
//!
//! **`asked_by` is the chain, and it is deliberately not a second field - and that sentence used to
//! be an assumption rather than a fact.** `LegCredentials::asked_by` is the broker's copy of who
//! asked and the chain is the transport's. This paragraph said they *agree by construction, because
//! `mint` reads the request context* - and a review pointed out that this is a claim about what a
//! well-behaved broker does, not a property of the types: `LegCredentials::minted` is `pub` and takes
//! any [`crate::identity::Subject`], so a broker that returned somebody else's grant would have
//! executed under one principal and been recorded under another. **It is a fact now**, because
//! `sutura_app::answer` compares the two and refuses a disagreement before anything reaches an
//! adapter - see [`crate::identity::Minted::agreeing_with`]. So the reason there is one field stands,
//! and what makes it safe is a check somebody can point at rather than a habit brokers are trusted to
//! have.
//!
//! **The expiry IS here**, as [`CallRecord::executed_until`], and it arrived for the same reason the
//! posture did: a review found the value carried and read by nobody. It is enforced first - a
//! credential whose deadline has passed never reaches an adapter - and recorded second, so an
//! incident can ask how much life the credential that read these rows had left. `None` means nothing
//! was minted for this call, which is the honest answer for a question refused before the broker was
//! asked.
//!
//! A plan reads from the sources a `Compiled::Federated` answer spans, or from a single source for a
//! `Compiled::Planned` one (a question spanning three or more is refused at plan time by
//! [`crate::query::RefusalReason::PlanSpansTooManySources`]), so the source set is a name a reader
//! already has from the bundle.

use crate::identity::{Expiry, PrincipalChain};
use crate::pinned::Provenance;
use crate::query::{RefusalReason, ToolOutcome};
use crate::source::UniformlyExecuted;

/// Where a record of one call goes.
///
/// # Returns nothing a caller can branch on
///
/// Deliberately. A sink that could refuse would make writing the record a step the query path has
/// to decide about - continue without a record, or refuse the question - and both answers are worse
/// than the question. Continuing silently is the failure this port exists to prevent; refusing a
/// question because a log pipeline is unwell is an availability decision nobody asked for. So the
/// port takes the record and owns everything that happens to it, including failing, which is the
/// deployment's half of the bargain the module header states.
///
/// `&self` rather than `&mut self`, so one sink is shared by every request without a lock in the
/// port's signature. Synchronous, because the ports either side of it are: the interior names no
/// framework, and a transport that answers on a blocking pool is already off the reactor.
///
/// # Its first implementor
///
/// `sutura_runtime::TracingAuditSink`, a structured writer over the tracing subscriber this
/// repository already composes. It needs nothing from anybody, which is what makes it the sink a
/// deployment that attaches nothing else gets - and what keeps this trait from being a guess at a
/// signature.
pub trait AuditSink {
    /// Writes one record. Called before the outcome is returned to the caller.
    fn record(&self, record: &CallRecord<'_>);
}

/// A shared sink is a sink.
///
/// The port takes `&self` and has no state to own, so a composition root that already holds its sink
/// behind an `Arc` - because something else observes it, or because two surfaces share one - should
/// not have to wrap it in a newtype to satisfy a bound. `?Sized` so an `Arc<dyn AuditSink>` works
/// too, which is what a composition root choosing a sink at runtime would hold.
impl<T: AuditSink + ?Sized> AuditSink for std::sync::Arc<T> {
    fn record(&self, record: &CallRecord<'_>) {
        (**self).record(record);
    }
}

/// What one call is recorded as.
///
/// Borrows rather than owns: it is built at the call site, handed to the sink, and dropped. A sink
/// that needs to keep something copies what it needs, which is the sink's decision rather than a
/// cost this type imposes on every call.
#[derive(Debug)]
pub struct CallRecord<'a> {
    chain: &'a PrincipalChain,
    outcome: RecordedOutcome<'a>,
    executed_until: Option<Expiry>,
}

/// How the call ended, as the two outcomes a question has.
///
/// **A refusal is a variant here for the same reason it is one in [`ToolOutcome`]**, and it is the
/// half a log line gets wrong by omission: refusals are the demand signal for which questions have
/// no certified answer, and a channel that records only answers cannot report it.
#[derive(Debug)]
pub enum RecordedOutcome<'a> {
    /// The question was answered. The row count sizes it; the provenance says which definitions
    /// produced it, so a record can be matched against the bundle that was serving.
    Answered { rows: usize, provenance: &'a Provenance },
    /// The question was declined. The variant is what a reader needs - not a sentence - because it
    /// is what an aggregate over records can group by.
    Refused { reason: &'a RefusalReason },
}

impl<'a> CallRecord<'a> {
    /// The only constructor, and it derives the outcome half from the outcome itself.
    ///
    /// There is no way to build a record that describes an answer as a refusal or the other way
    /// round: the match is here, once, rather than at every call site that would otherwise be
    /// trusted to get it right. That is the same reason [`crate::pinned::PinnedDefinitions::pin`]
    /// takes no digest parameter.
    /// `executed_until` is the deadline the credentials this call ran under carried, and `None` means
    /// nothing was minted for it - a question refused before the broker was asked. It is a parameter
    /// rather than something derived from the outcome because the credential is deliberately not on
    /// the [`ToolOutcome`]: provenance rides to the caller, and what a credential's lifetime is is
    /// not the caller's business.
    #[must_use]
    pub fn of(chain: &'a PrincipalChain, outcome: &'a ToolOutcome, executed_until: Option<Expiry>) -> Self {
        let recorded = match *outcome {
            ToolOutcome::Answer {
                ref provenance,
                ref rows,
            } => RecordedOutcome::Answered {
                rows: rows.rows().len(),
                provenance,
            },
            ToolOutcome::Refusal { ref reason } => RecordedOutcome::Refused { reason },
        };
        Self {
            chain,
            outcome: recorded,
            executed_until,
        }
    }

    /// How long the credentials this call ran under were good for.
    ///
    /// **A field rather than a derivation, unlike [`Self::executed_as`]**, because the deadline is
    /// deliberately absent from the [`ToolOutcome`]: the outcome's provenance is caller-facing, and a
    /// credential's lifetime is this deployment's business rather than the asker's.
    ///
    /// Three states and a reader has to name all three, which is why it is an `Option<Expiry>` rather
    /// than a number: nothing was minted for this call, the credential does not expire, or it expires
    /// at an instant. The first is the honest answer for a question declined before the broker was
    /// asked - `sutura_app`'s own suite pins that a refused question never reaches it - and the second
    /// is what every credential the shipping broker mints answers, because it mints from a file.
    ///
    /// **Recording is not the control.** A credential whose deadline had passed never reached an
    /// adapter, and `crate::identity::Minted::agreeing_with` is where that is decided; this is what
    /// lets an incident ask afterwards how much life was left.
    #[inline]
    #[must_use]
    pub const fn executed_until(&self) -> Option<Expiry> {
        self.executed_until
    }

    /// Which identity produced each leg, where anything executed.
    ///
    /// **The incident question's own accessor**, and it is derived rather than stored: the value is
    /// the [`Provenance`]'s, which the [`ToolOutcome`] already carried, so there is one place it lives
    /// and nothing here can describe a leg as impersonated that ran shared. `None` on a refusal, and
    /// that is a case a reader names rather than an absence to interpret - a refused question reached
    /// no data system, so there is no identity it ran as.
    ///
    /// **Recording is not a control**, the way `Provenance`'s own documentation says: this reaches a
    /// sink after the rows were read. What it is for is being able to answer, afterwards, whether a
    /// verified subject's question was filtered by that subject's own access or by the identity this
    /// deployment holds for the source.
    #[must_use]
    pub const fn executed_as(&self) -> Option<&UniformlyExecuted> {
        match self.outcome {
            RecordedOutcome::Answered { provenance, .. } => Some(provenance.executed_as()),
            RecordedOutcome::Refused { .. } => None,
        }
    }

    /// Who the call is attributable to.
    #[inline]
    pub const fn chain(&self) -> &PrincipalChain {
        self.chain
    }

    /// How it ended.
    #[inline]
    pub const fn outcome(&self) -> &RecordedOutcome<'a> {
        &self.outcome
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::{AuditSink, CallRecord, RecordedOutcome};
    use crate::identity::{Actor, ActorChain, Attribution, PrincipalChain, Subject, SubjectId};
    use crate::model::{DimensionName, MetricName};
    use crate::query::{RefusalReason, ToolOutcome};

    fn a_person() -> Subject {
        Subject::Verified {
            id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
        }
    }

    fn a_refusal() -> ToolOutcome {
        ToolOutcome::Refusal {
            reason: RefusalReason::DimensionNotPermitted {
                metric: MetricName::parse("revenue").expect("a test metric is a metric"),
                dimension: DimensionName::parse("salary_band").expect("a test dimension is a dimension"),
            },
        }
    }

    /// Everything one sink was handed, as text, in the order it arrived.
    ///
    /// A fake rather than a mocked pipeline: the port is what makes the whole outcome space
    /// assertable without a log collector, which is why `AGENTS.md` asks for fakes.
    ///
    /// `RefCell` and not a `Mutex`: the trait itself requires no thread-safety - the bound that does
    /// lives on `sutura_app::LocalService`'s impl - and this test writes from one thread. The
    /// workspace bans `std::sync::Mutex` anyway, and there is no async runtime in this crate's
    /// dependency list to reach for instead.
    #[derive(Debug, Default)]
    struct Recorded {
        lines: RefCell<Vec<String>>,
    }

    impl AuditSink for Recorded {
        fn record(&self, record: &CallRecord<'_>) {
            let who = match record.chain().attribution() {
                Attribution::BareSubject { subject } => format!("subject={}", subject.established()),
                Attribution::ActingFor { subject, actors } => {
                    format!("subject={} acting={actors}", subject.established())
                }
            };
            let how = match *record.outcome() {
                RecordedOutcome::Answered { rows, .. } => format!("answered rows={rows}"),
                RecordedOutcome::Refused { reason } => format!("refused reason={reason:?}"),
            };
            self.lines.borrow_mut().push(format!("{who} {how}"));
        }
    }

    #[test]
    fn a_record_naming_a_subject_is_distinguishable_from_one_naming_an_agent_acting_for_them() {
        // THE REASON THIS STEP COULD NOT BE DEFERRED, asserted rather than described. Both calls are
        // attributable to the SAME person and the same refusal, so if the two records came out equal
        // there would be nothing in a stored row that could ever separate them again.
        let alone = PrincipalChain::of(a_person());
        let through_an_agent =
            PrincipalChain::of(a_person()).acting(ActorChain::of(Actor::parse("query_agent").expect("a test actor is an actor")));
        let outcome = a_refusal();

        let sink = Recorded::default();
        sink.record(&CallRecord::of(&alone, &outcome, None));
        sink.record(&CallRecord::of(&through_an_agent, &outcome, None));

        let lines = sink.lines.borrow().clone();
        assert_eq!(lines.len(), 2, "one record per call");
        assert_ne!(
            lines.first(),
            lines.get(1),
            "the two records are indistinguishable, which is the bug this step exists to prevent: {lines:?}"
        );
        assert!(
            lines.first().is_some_and(|line| !line.contains("acting=")),
            "the bare subject's record claims an actor: {lines:?}"
        );
        assert!(
            lines.get(1).is_some_and(|line| line.contains("acting=query_agent")),
            "the agent's record does not name it: {lines:?}"
        );
    }

    #[test]
    fn a_record_cannot_describe_an_answer_as_a_refusal() {
        // The outcome half is derived from the outcome, so there is no parameter to get wrong. A
        // refusal records its VARIANT, which is what an aggregate over records groups by.
        let chain = PrincipalChain::of(Subject::TheDeploymentItself);
        let refusal = a_refusal();
        let record = CallRecord::of(&chain, &refusal, None);
        let RecordedOutcome::Refused { reason } = *record.outcome() else {
            panic!("a refusal is recorded as one, not as {:?}", record.outcome());
        };
        assert!(matches!(*reason, RefusalReason::DimensionNotPermitted { .. }));
        assert_eq!(record.chain(), &chain);
    }
}

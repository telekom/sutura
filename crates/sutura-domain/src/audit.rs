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
//! # What the record does NOT carry yet, said here rather than implied
//!
//! `docs/adr/0008` fixes the full content as the chain, the outcome, **the sources the plan read and
//! the posture each leg ran under, and the expiry the credentials carried.** The last two are absent
//! from [`CallRecord`], and the reason is now narrower than "the types do not exist": they do -
//! [`crate::source::ExecutedAs`] carries the per-leg posture and
//! [`crate::identity::Expiry`] the deadline - and neither is REACHED from here, because a record is
//! built from the chain and the [`ToolOutcome`], and the outcome carries provenance only on an answer.
//! A refusal would have to carry them separately, which is a change to what a record is made of. A
//! plan reads exactly one source today - [`crate::query::RefusalReason::PlanSpansTwoSources`] is
//! what makes that true - so the source set is one name a reader already has from the bundle.

use crate::identity::PrincipalChain;
use crate::pinned::Provenance;
use crate::query::{RefusalReason, ToolOutcome};

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
    #[must_use]
    pub fn of(chain: &'a PrincipalChain, outcome: &'a ToolOutcome) -> Self {
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
        sink.record(&CallRecord::of(&alone, &outcome));
        sink.record(&CallRecord::of(&through_an_agent, &outcome));

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
        let record = CallRecord::of(&chain, &refusal);
        let RecordedOutcome::Refused { reason } = *record.outcome() else {
            panic!("a refusal is recorded as one, not as {:?}", record.outcome());
        };
        assert!(matches!(*reason, RefusalReason::DimensionNotPermitted { .. }));
        assert_eq!(record.chain(), &chain);
    }
}

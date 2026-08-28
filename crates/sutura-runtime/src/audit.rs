//! The one implementor of [`AuditSink`] that needs nothing from anybody.
//!
//! A structured writer over the tracing subscriber this crate already composes. It is here rather
//! than in a transport for the reason everything in this crate is here: the subscriber is
//! process-global, and the sink is a use of it rather than a second installation.
//!
//! # Why this is the first implementor rather than a fake
//!
//! `AGENTS.md` says a port trait arrives with its first implementor, because a trait with no
//! implementor is a guess at a signature. This is the sink a deployment that attaches nothing else
//! gets: it writes into the log pipeline the deployment already runs, so there is no configuration,
//! no credential and no second egress path to decide about. **Whether that pipeline keeps anything
//! is the deployment's answer, not ours** - which is the limit `sutura_domain::audit` states, and
//! this type is where it is most visible.
//!
//! # It replaces a log line rather than adding a channel beside one
//!
//! Before this existed, `sutura_http`'s query handler wrote one `tracing::info!` per outcome, and
//! its own doc comment said there was no audit sink and nothing recorded a principal chain. The
//! fields that line carried - the row count, the definition version, the refusal variant - are
//! carried here, so an operator's existing filters keep working while the line gains the thing it
//! was missing. What that line also carried, and this does not, is the HTTP status: the status is
//! the transport's and the application cannot see it. Nothing is lost, because `tower_http`'s
//! response line already carries it inside the same request span, configured at `info` in
//! `sutura_http::router` - which is where a status belongs.

use sutura_domain::audit::{AuditSink, CallRecord, RecordedOutcome};
use sutura_domain::identity::PrincipalChain;

/// Writes each record as one structured event on the process subscriber.
///
/// There is nothing to configure, and a `new` that took a level or a target would be two ways to
/// write the same record. The subscriber decides where the event goes, which is the one decision a
/// deployment already makes.
///
/// A private marker field rather than a unit struct, for two reasons and the first is the honest
/// one: `cargo xtask check-boundaries` reads a file line by line, and a unit `pub struct` leaves its
/// scan waiting for a body - so the next braced block in the file is read as this struct's field
/// list and a `pub fn` in it is reported as a `pub` field. That is a defect in the gate rather than
/// in this type, and it is written down here rather than worked around silently. The second reason
/// stands on its own: a field, even an empty one, means [`Self::new`] is the only way in from
/// outside this crate, where a unit struct is its own literal.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingAuditSink {
    _private: (),
}

impl TracingAuditSink {
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

/// The chain, flattened into the fields one line carries.
///
/// A struct rather than four values threaded through the two arms below, because the two arms both
/// need all four and a positional argument list is where they drift apart.
struct Attributed<'a> {
    /// `verified` or `deployment`: what established the subject, from an exhaustive match in the
    /// domain rather than a string chosen here.
    established: &'static str,
    /// The verified subject's identifier, or empty when the deployment is the only principal. Read
    /// beside `established`, never on its own - an empty value here is not an unnamed person.
    subject: &'a str,
    /// The actors, nearest the subject first, or empty when nothing acted for them.
    actors: String,
    /// How many actors acted. `0` is the case every call in this workspace produces today, and it is
    /// a number rather than an absence so an aggregate over records can count it.
    acting: usize,
    /// The task the call belonged to, or empty when none was named.
    task: &'a str,
}

impl<'a> Attributed<'a> {
    fn of(chain: &'a PrincipalChain) -> Self {
        let subject = chain.subject();
        Self {
            established: subject.established(),
            subject: subject.id().map_or("", |id| id.as_str()),
            actors: chain.actors().map_or_else(String::new, ToString::to_string),
            acting: chain.actors().map_or(0, sutura_domain::identity::ActorChain::count),
            task: chain.task().map_or("", |task| task.as_str()),
        }
    }
}

impl AuditSink for TracingAuditSink {
    /// One event per call, at `info`, inside whatever span the caller is running in.
    ///
    /// The message is `answered` or `refused` - the same two words the log line this replaced used,
    /// so a filter written against that line still finds these.
    #[expect(
        clippy::cognitive_complexity,
        reason = "both arms are a tracing macro expanding into branches; the control flow is one match"
    )]
    fn record(&self, record: &CallRecord<'_>) {
        let who = Attributed::of(record.chain());
        match *record.outcome() {
            RecordedOutcome::Answered { rows, provenance } => tracing::info!(
                subject_established = who.established,
                subject = who.subject,
                actors = who.actors,
                acting = who.acting,
                task = who.task,
                rows,
                definition_version = %provenance.version(),
                "answered"
            ),
            // `Debug` of a refusal reason is safe to log: the domain has a test asserting that a
            // rejected filter value is not in it.
            RecordedOutcome::Refused { reason } => tracing::info!(
                subject_established = who.established,
                subject = who.subject,
                actors = who.actors,
                acting = who.acting,
                task = who.task,
                reason = ?reason,
                "refused"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::audit::{AuditSink as _, CallRecord};
    use sutura_domain::identity::{Actor, ActorChain, PrincipalChain, Subject, SubjectId};
    use sutura_domain::model::{DimensionName, MetricName};
    use sutura_domain::query::{RefusalReason, ToolOutcome};

    use super::TracingAuditSink;

    fn a_refusal() -> ToolOutcome {
        ToolOutcome::Refusal {
            reason: RefusalReason::DimensionNotPermitted {
                metric: MetricName::parse("revenue").expect("a test metric is a metric"),
                dimension: DimensionName::parse("salary_band").expect("a test dimension is a dimension"),
            },
        }
    }

    /// Writes one record with a subscriber installed for this thread, and returns the bytes.
    ///
    /// The machine-readable rendering, because that is what a collector reads and what a field name
    /// can be asserted against - a pretty line would let a field be asserted by substring and pass
    /// while being on the wrong key.
    fn written(chain: &PrincipalChain) -> String {
        let outcome = a_refusal();
        crate::testing::capture(|| {
            TracingAuditSink::new().record(&CallRecord::of(chain, &outcome));
        })
    }

    #[test]
    fn a_refused_question_is_recorded_with_its_chain() {
        // The half a log line gets wrong by omission. The chain is on the record as well as the
        // refusal variant, so a refusal is attributable rather than merely counted.
        let chain = PrincipalChain::of(Subject::Verified {
            id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
        });
        let rendered = written(&chain);
        assert!(rendered.contains("refused"), "{rendered}");
        assert!(rendered.contains("someone@example.com"), "{rendered}");
        assert!(rendered.contains("verified"), "{rendered}");
        assert!(rendered.contains("DimensionNotPermitted"), "{rendered}");
    }

    #[test]
    fn a_record_says_whether_an_agent_acted_and_the_two_lines_differ() {
        // The same person, the same refusal, two calls - and the written lines are not the same
        // bytes. This is the domain's distinguishability test carried through to what is actually
        // emitted, because a type that can tell them apart and a writer that flattens both to the
        // same line would still lose the distinction.
        let subject = || Subject::Verified {
            id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
        };
        let alone = written(&PrincipalChain::of(subject()));
        let acted_for = written(
            &PrincipalChain::of(subject()).acting(
                ActorChain::of(Actor::parse("orchestrator").expect("a test actor is an actor"))
                    .acting_through(Actor::parse("query_agent").expect("a test actor is an actor")),
            ),
        );
        assert!(alone.contains("\"acting\":0"), "a bare subject claims an actor: {alone}");
        assert!(
            acted_for.contains("\"acting\":2") && acted_for.contains("orchestrator > query_agent"),
            "the actors are not on the line, in order: {acted_for}"
        );
    }

    #[test]
    fn the_deployment_is_recorded_as_the_deployment_and_names_no_person() {
        // What every call in this workspace writes today. The `subject` field is empty and
        // `subject_established` says why - so a reader is not left to guess whether the caller was
        // anonymous or unrecorded.
        let rendered = written(&PrincipalChain::of(Subject::TheDeploymentItself));
        assert!(rendered.contains("\"subject_established\":\"deployment\""), "{rendered}");
        assert!(rendered.contains("\"subject\":\"\""), "{rendered}");
    }
}

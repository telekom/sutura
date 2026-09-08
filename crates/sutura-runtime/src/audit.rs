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
use sutura_domain::identity::{Expiry, PrincipalChain};

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
    fn record(&self, record: &CallRecord<'_>) {
        let who = Attributed::of(record.chain());
        match *record.outcome() {
            // `executed_as` is the field that answers the incident question this line exists for, and
            // it was missing until a review asked it: a verified subject's question can be ANSWERED
            // under the identity this deployment holds for the source, on a leg declared
            // `shared-service-user`. That is honest and acknowledged and it is not the asker's access
            // filtering the rows - so a record naming only the subject and the definition version
            // cannot say afterwards whose access produced the answer. It is read off
            // `CallRecord::executed_as`, which derives it from the provenance the outcome carried, so
            // there is one place the value lives.
            RecordedOutcome::Answered { rows, provenance } => tracing::info!(
                subject_established = who.established,
                subject = who.subject,
                actors = who.actors,
                acting = who.acting,
                task = who.task,
                rows,
                definition_version = %provenance.version(),
                executed_as = executed_as(record),
                credential_until = executed_until(record),
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

/// Which identity each leg of an answer ran as, one field, in source order.
///
/// **A rendering rather than a `Debug`**, so a collector reads a stable key-value list rather than a
/// Rust type's formatting. It carries the source and the posture's own spelling - the one
/// `SourcePosture::as_str` defines, so a startup line and an audit line cannot drift into two words
/// for one posture - and it carries **no operator acknowledgement prose**, which is what the
/// transports already refuse to send and is not a thing a log line needs either.
///
/// The empty string is unreachable for an answer: `ExecutedAs` has no empty form, and
/// `CallRecord::executed_as` is `None` only for a refusal, which is the other arm.
/// How long the credential this call ran under was good for, as one field.
///
/// **Three states, spelled rather than left to a `Debug`**, because all three mean something
/// different to whoever reads the line: `none` is a question declined before the broker was asked,
/// `never` is a credential an operator wrote in a file, and a number is a deadline. A collector
/// grouping by this field gets three stable words rather than a Rust type's formatting.
///
/// It is a rendering and not a control: `sutura_domain::identity::Minted::agreeing_with` is what stops
/// a credential whose deadline had already passed from reaching an adapter, and this is what lets an
/// incident ask afterwards how much life was left.
fn executed_until(record: &CallRecord<'_>) -> String {
    match record.executed_until() {
        None => String::from("none"),
        Some(Expiry::NothingExpires) => String::from("never"),
        Some(Expiry::At { unix_seconds }) => unix_seconds.to_string(),
    }
}

fn executed_as(record: &CallRecord<'_>) -> String {
    record.executed_as().map_or_else(String::new, |ran| {
        ran.legs()
            .map(|(source, posture)| format!("{source}={}", posture.as_str()))
            .collect::<Vec<String>>()
            .join(",")
    })
}

#[cfg(test)]
mod tests {
    use sutura_domain::audit::{AuditSink as _, CallRecord};
    use sutura_domain::identity::{Actor, ActorChain, Expiry, PrincipalChain, Subject, SubjectId};
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
            TracingAuditSink::new().record(&CallRecord::of(chain, &outcome, None));
        })
    }

    /// One answer, on one source, under the identity this deployment holds for it.
    ///
    /// Assembled from empty definitions on purpose: what is under test is the identity half of the
    /// record, and a bundle with metrics in it would be a fixture to keep in step for no assertion.
    fn an_answer_on_a_shared_source() -> ToolOutcome {
        use sutura_domain::capabilities::MetadataCapabilities;
        use sutura_domain::catalog::Definitions;
        use sutura_domain::knowledge::Knowledge;
        use sutura_domain::model::SourceName;
        use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
        use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture, UniformlyExecuted};
        use sutura_domain::warehouse::{RowSet, Value};

        let pinned = PinnedDefinitions::pin(
            DefinitionVersion::parse("2026-08-29").expect("a test version is a version"),
            Definitions::assemble(Vec::new(), Vec::new(), Vec::new()).expect("empty definitions are consistent"),
            Knowledge::none(),
            ContributionManifest::single(
                SourceName::parse("local").expect("a test source is a source"),
                Contribution::of(MetadataCapabilities::nothing()),
            ),
        )
        .expect("the test definitions hash");
        let ran_as = UniformlyExecuted::of(
            SourceName::parse("warehouse").expect("a test source is a source"),
            SourcePosture::SharedServiceUser {
                declared: SharedIdentityDeclared::of(
                    AcknowledgementReason::parse("one connection, one identity").expect("a test reason is a reason"),
                ),
            },
        );
        ToolOutcome::Answer {
            provenance: pinned.provenance(ran_as),
            rows: RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(1)]])
                .expect("one column and one cell is rectangular"),
        }
    }

    #[test]
    fn an_answer_records_which_identity_produced_it_and_not_only_who_asked() {
        // THE INCIDENT QUESTION, and it is the one this branch created: a VERIFIED subject's question
        // is answered under the identity this deployment holds for a source declared
        // `shared-service-user`. That is honest and acknowledged and it is not the asker's own access
        // filtering the rows - so a record naming the subject and the definition version and nothing
        // else cannot answer, afterwards, whose access produced the answer. The record says both.
        let outcome = an_answer_on_a_shared_source();
        let chain = PrincipalChain::of(Subject::Verified {
            id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
        });
        let rendered = crate::testing::capture(|| {
            TracingAuditSink::new().record(&CallRecord::of(&chain, &outcome, Some(Expiry::NothingExpires)));
        });
        assert!(rendered.contains("answered"), "{rendered}");
        assert!(
            rendered.contains("someone@example.com"),
            "the asker is on the line: {rendered}"
        );
        assert!(
            rendered.contains("\"executed_as\":\"warehouse=shared-service-user\""),
            "the line does not say which identity produced the rows: {rendered}"
        );
        // And the acknowledgement PROSE is not on the line, which is the position both transports
        // already take about it: an operator's sentence is for a startup log, not for every call.
        assert!(
            !rendered.contains("one connection, one identity"),
            "the operator's acknowledgement is not audit content: {rendered}"
        );
    }

    #[test]
    fn an_answer_records_how_long_the_credential_it_ran_under_was_good_for() {
        // The other half of `docs/adr/0008`'s record content, and it arrived for the reason the
        // posture did: a review found `Expiry` computed, carried and read by nobody. Enforced first -
        // a credential whose deadline has passed never reaches an adapter - and recorded second, so an
        // incident can ask afterwards how much life was left.
        //
        // Three states and all three are words a collector can group by, which is why this is a
        // rendering rather than a `Debug` of an `Option`.
        let chain = PrincipalChain::of(Subject::TheDeploymentItself);
        let outcome = an_answer_on_a_shared_source();
        let never = crate::testing::capture(|| {
            TracingAuditSink::new().record(&CallRecord::of(&chain, &outcome, Some(Expiry::NothingExpires)));
        });
        assert!(
            never.contains("\"credential_until\":\"never\""),
            "a credential an operator wrote in a file does not expire, and the line says so: {never}"
        );
        let expiring = crate::testing::capture(|| {
            TracingAuditSink::new().record(&CallRecord::of(
                &chain,
                &outcome,
                Some(Expiry::At {
                    unix_seconds: 1_777_000_000,
                }),
            ));
        });
        assert!(
            expiring.contains("\"credential_until\":\"1777000000\""),
            "a deadline is the instant, so an incident can compare it: {expiring}"
        );
        let none = crate::testing::capture(|| {
            TracingAuditSink::new().record(&CallRecord::of(&chain, &outcome, None));
        });
        assert!(
            none.contains("\"credential_until\":\"none\""),
            "nothing minted is its own word, not an empty value a reader has to interpret: {none}"
        );
    }

    #[test]
    fn a_refusal_records_no_executing_identity_because_nothing_executed() {
        // The other arm, and it is a case rather than an empty field: a refused question reached no
        // data system, so there is no identity it ran as. Asserted so that the field's absence stays
        // a decision rather than becoming something a reader has to interpret.
        let chain = PrincipalChain::of(Subject::TheDeploymentItself);
        let refusal = a_refusal();
        assert!(CallRecord::of(&chain, &refusal, None).executed_as().is_none());
        assert!(
            CallRecord::of(&chain, &an_answer_on_a_shared_source(), Some(Expiry::NothingExpires))
                .executed_as()
                .is_some()
        );
        let rendered = written(&chain);
        assert!(!rendered.contains("executed_as"), "{rendered}");
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

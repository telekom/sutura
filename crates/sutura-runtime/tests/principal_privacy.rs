//! Principal rendering through the event the shipped audit sink emits.
//!
//! The actor and task values are synthetic: no published transport populates those tail positions.

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use sutura_domain::audit::{AuditSink as _, CallRecord};
    use sutura_domain::identity::{Actor, ActorChain, PrincipalChain, Subject, SubjectId, TaskId};
    use sutura_domain::model::{DimensionName, MetricName};
    use sutura_domain::query::{RefusalReason, ToolOutcome};
    use sutura_runtime::TracingAuditSink;

    fn audit_event(chain: &PrincipalChain) -> Value {
        let outcome = ToolOutcome::Refusal {
            reason: RefusalReason::DimensionNotPermitted {
                metric: MetricName::parse("revenue").expect("a test metric is a metric"),
                dimension: DimensionName::parse("salary_band").expect("a test dimension is a dimension"),
            },
        };
        let sink = sutura_runtime::testing::Capture::new();
        let telemetry = sutura_config::TelemetrySettings::new(
            sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
            sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
            sutura_config::LogFormat::Bunyan,
            true,
        );
        let subscriber =
            sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
        tracing::subscriber::with_default(subscriber, || {
            TracingAuditSink::new().record(&CallRecord::of(chain, &outcome, None));
        });
        serde_json::from_str(sink.contents().trim()).expect("one audit record is one JSON object")
    }

    #[test]
    fn an_info_audit_event_masks_each_principal_and_still_names_the_event() {
        let chain = PrincipalChain::of(Subject::Verified {
            id: SubjectId::parse("firstname.lastname@company.com").expect("a test subject is a subject"),
        })
        .acting(
            ActorChain::of(Actor::parse("somename@company.com").expect("a test actor is an actor"))
                .acting_through(Actor::parse("service.bot@company.com").expect("a test actor is an actor")),
        )
        .for_task(TaskId::parse("nightly-reconciliation").expect("a test task is a task"));

        let event = audit_event(&chain);
        assert_eq!(event["level"], 30, "the audit record is still an info event: {event}");
        assert_eq!(event["msg"], "refused", "deleting the event cannot satisfy privacy: {event}");
        assert_eq!(event["subject"], "f***.l***@c***.c***", "{event}");
        assert_eq!(event["actors"], "s***@c***.c*** > s***.b***@c***.c***", "{event}");
        assert_eq!(
            event["task"], "n***",
            "a non-email principal has the same fixed mask: {event}"
        );

        let line = event.to_string();
        for raw in [
            "firstname.lastname@company.com",
            "somename@company.com",
            "service.bot@company.com",
            "nightly-reconciliation",
        ] {
            assert!(!line.contains(raw), "the info event disclosed {raw}: {line}");
        }
    }

    #[test]
    fn the_masked_type_holds_no_plaintext_and_no_surface_emits_it() {
        let id = SubjectId::parse("firstname.lastname@company.com").expect("a test subject is a subject");

        assert_eq!(id.to_string(), "f***.l***@c***.c***");
        assert_eq!(format!("{id:?}"), "f***.l***@c***.c***");

        let short = SubjectId::parse("a.bc@example.com").expect("a short test subject is a subject");
        assert_eq!(short.to_string(), "a***.b***@e***.c***");
        assert_eq!(format!("{short:?}"), "a***.b***@e***.c***");

        // The value the type actually holds is the masked form, not the raw: the raw was consumed at
        // the parse boundary, so no render path (Display, Debug, or the stored string) can emit it.
        // Seating the raw in this type's state and masking at render makes these green assertions red.
        assert_eq!(
            id.as_str(),
            "f***.l***@c***.c***",
            "as_str is the stored masked form; the raw is not retained"
        );
        for surface in [id.to_string(), format!("{id:?}"), String::from(id.as_str())] {
            for raw in [
                "firstname.lastname@company.com",
                "a.bc@example.com",
                "firstname",
                "lastname",
                "company",
            ] {
                assert!(!surface.contains(raw), "a render surface reached the raw `{raw}`: {surface}");
            }
        }
    }

    #[test]
    fn an_opaque_suffix_after_at_is_masked_too() {
        let id = SubjectId::parse("opaque@full-secret-id").expect("an opaque test subject is a subject");

        assert_eq!(id.to_string(), "o***@f***");
        assert_eq!(format!("{id:?}"), "o***@f***");
        assert!(!id.to_string().contains("full-secret-id"));
    }
}

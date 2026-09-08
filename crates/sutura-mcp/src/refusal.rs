//! A refused question, as this transport says it: the code a caller branches on, and the sentence a
//! person or a model reads.
//!
//! # One exhaustive match, no wildcard arm
//!
//! Deliberately, and it is the same mechanism `sutura_http::wire::refusal` already carries: a
//! `RefusalReason` variant added to the domain **fails to compile here** until somebody decides what
//! it is on the agent surface. A wildcard arm would let a new governance outcome reach a model as an
//! unnamed one, which is the failure this whole enum exists to prevent.
//!
//! **The cost is stated rather than hidden:** a branch that adds a refusal variant has to add an arm
//! here as well as there. That is the price of an adapter not reaching into another adapter, and it
//! is a compile error at a named line rather than a silent gap.
//!
//! # Why there is no status here
//!
//! MCP has none. The HTTP surface decides a status *and* a code *and* a sentence in one match,
//! because on that transport the status is what a monitor counts. Here the code is the whole
//! machine-readable contract, so this module decides two things instead of three.
//!
//! # The codes are the HTTP surface's codes, and that is checkable
//!
//! Not by comparing the two modules - this crate cannot see that one - but by both following one
//! derivation: **the code is the `snake_case` spelling of the variant's own name.**
//! `the_code_is_the_variant_name_in_snake_case` below asserts it for every variant, reading the name out of the
//! domain type's own `Serialize` rather than from a list typed beside it. A hand-written code that
//! drifted from the variant would fail that test, and a hand-written code that drifted from the
//! other transport's would have to drift from the variant first.
//!
//! What is NOT guaranteed is that the two transports' *sentences* agree. They are written for
//! different readers - one for a client's error surface, one for a model's context - and nothing
//! compares them.

use sutura_domain::query::{RefusalReason, ResultBound};

/// The code and the sentence for one refusal.
///
/// The match is exhaustive with no wildcard arm, and it decides both at once rather than in two
/// matches that could drift apart.
pub(crate) fn refused(reason: &RefusalReason) -> (&'static str, String) {
    match *reason {
        RefusalReason::MetricUnknown { ref metric } => {
            ("metric_unknown", format!("this catalog defines no metric called `{metric}`"))
        }
        RefusalReason::GrainNotSupported { ref metric, grain } => {
            ("grain_not_supported", format!("`{metric}` is not defined at `{grain}` grain"))
        }
        RefusalReason::DimensionNotPermitted {
            ref metric,
            ref dimension,
        } => (
            "dimension_not_permitted",
            format!("`{metric}` does not declare a dimension called `{dimension}`"),
        ),
        RefusalReason::DimensionNotFilterable {
            ref metric,
            ref dimension,
        } => (
            "dimension_not_filterable",
            format!("`{dimension}` can be grouped by on `{metric}` but not filtered on"),
        ),
        // The value is deliberately absent. `RefusalReason` does not carry it, for the reason
        // `crate::wire::RefusalContent` restates: caller text that reaches a model's context is
        // caller text that becomes somebody else's input.
        RefusalReason::DimensionValueNotAllowed {
            ref metric,
            ref dimension,
        } => (
            "dimension_value_not_allowed",
            format!("the value given for `{dimension}` is not one `{metric}` declares"),
        ),
        RefusalReason::DuplicateDimension { ref dimension } => {
            ("duplicate_dimension", format!("`{dimension}` is listed more than once"))
        }
        RefusalReason::TooManyDimensions { requested, limit } => (
            "too_many_dimensions",
            format!("{requested} dimensions were asked for; at most {limit} are allowed"),
        ),
        // The one refusal that is decided AFTER a data system has answered, which is why it names a
        // bound rather than a field: nothing about the question was wrong, and the same question over
        // a narrower range or with fewer dimensions is answerable.
        //
        // One code for both bounds - the remedy an agent has to carry out is the same narrowing - and
        // a nested exhaustive match for the sentence, so a bound with no number cannot be described
        // using the other one's.
        RefusalReason::ResultTooLarge { bound } => (
            "result_too_large",
            match bound {
                ResultBound::Rows { limit } => {
                    format!("the answer would have been more than {limit} rows; ask a narrower question")
                }
                // No figure: the bound is the data system's and this deployment is not told it, so
                // there is nothing to divide a range by. What an agent needs instead is that the
                // remedy is still narrowing and that a retry is not one - see `ResultBound::Volume`.
                ResultBound::Volume => String::from(
                    "the answer was more data than the data system would return at once; nothing was \
                     cut down to fit and retrying will not help. Ask a narrower question - a shorter \
                     period, or fewer dimensions.",
                ),
            },
        ),
        RefusalReason::TimeRangeTooLong { days, limit } => (
            "time_range_too_long",
            format!("the period asked about is {days} days; at most {limit} are allowed"),
        ),
        RefusalReason::PlanSpansTooManySources { sources, limit } => (
            "plan_spans_too_many_sources",
            format!("this question would read from {sources} data systems, and one answer reads from {limit} at most"),
        ),
        RefusalReason::FederationNotExecutable => (
            "federation_not_executable",
            String::from(
                "this deployment has no adapter that can execute one half of a question spanning two \
                 data systems. Nothing you can change in the question helps and this is not an outage \
                 to retry; ask without the dimension on the second data system, or report it.",
            ),
        ),
        RefusalReason::FederationLinkAmbiguous { ref source } => (
            "federation_link_ambiguous",
            format!(
                "the dimensions on `{source}` join through more than one relationship, and the two \
                 legs link on a single column"
            ),
        ),
        RefusalReason::MeasureDoesNotFederate { ref metric, aggregate } => (
            "measure_does_not_federate",
            format!(
                "`{metric}` cannot be computed across two data systems because its {aggregate} \
                 aggregate is not additive; ask it without the dimension that sits on the second \
                 data system"
            ),
        ),
        // Written for an agent, so it says which of the two moves is available rather than only that
        // this one failed: unlike the two-source refusal there usually is another question, because a
        // dimension that needs no join is still answered.
        RefusalReason::PlanTablesShareAnIdentifier { ref table } => (
            "plan_tables_share_an_identifier",
            format!(
                "answering this would read two different tables both called `{table}`, and one \
                 statement cannot tell them apart. Try a dimension that needs no join; if every \
                 useful one does, say so to the person you are acting for."
            ),
        ),
        RefusalReason::SourceUnavailable { ref source } => (
            "source_unavailable",
            format!("the data system `{source}` is not one this process opened"),
        ),
        RefusalReason::ResourcesExhausted { ceiling_bytes } => (
            "resources_exhausted",
            format!(
                "answering this needed more working memory than this deployment allows \
                 ({ceiling_bytes} bytes) and was refused rather than allowed to exhaust the \
                 process. Asking again unchanged will be refused again: narrow the period, ask \
                 for fewer dimensions, or add a filter."
            ),
        ),
        // Written for an agent, which is a different reader from the HTTP surface's: what an agent
        // needs is to stop, not to adapt. There is no narrower question that helps and no retry that
        // succeeds, so the sentence says both and tells it what to do instead - report it to the
        // person it is acting for, who can ask for access.
        RefusalReason::CredentialUnavailable { ref source } => (
            "credential_unavailable",
            format!(
                "the person you are acting for has no access to the data system `{source}`, and \
                 this deployment will not read it under its own identity instead. Nothing you can \
                 change in the question helps and retrying will not either. Say so, and say that \
                 access to `{source}` is what would be needed."
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::{Aggregate, DimensionName, Grain, MetricName, SourceName, TableName};
    use sutura_domain::query::{RefusalReason, ResultBound};

    use super::refused;

    fn metric() -> MetricName {
        MetricName::parse("revenue").expect("a test metric is a metric")
    }

    fn dimension() -> DimensionName {
        DimensionName::parse("region").expect("a test dimension is a dimension")
    }

    /// Every variant a question can reach, so the assertions below run over the whole enum rather
    /// than over whichever ones somebody remembered.
    fn every_reason() -> Vec<RefusalReason> {
        vec![
            RefusalReason::MetricUnknown { metric: metric() },
            RefusalReason::GrainNotSupported {
                metric: metric(),
                grain: Grain::Week,
            },
            RefusalReason::DimensionNotPermitted {
                metric: metric(),
                dimension: dimension(),
            },
            RefusalReason::DimensionNotFilterable {
                metric: metric(),
                dimension: dimension(),
            },
            RefusalReason::DimensionValueNotAllowed {
                metric: metric(),
                dimension: dimension(),
            },
            RefusalReason::DuplicateDimension { dimension: dimension() },
            RefusalReason::TooManyDimensions { requested: 5, limit: 4 },
            RefusalReason::ResultTooLarge {
                bound: ResultBound::Rows { limit: 10_000 },
            },
            RefusalReason::ResourcesExhausted {
                ceiling_bytes: 1024 * 1024 * 1024,
            },
            RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 },
            RefusalReason::PlanSpansTooManySources { sources: 3, limit: 2 },
            RefusalReason::FederationNotExecutable,
            RefusalReason::FederationLinkAmbiguous {
                source: SourceName::parse("warehouse").expect("a test source is a source"),
            },
            RefusalReason::MeasureDoesNotFederate {
                metric: metric(),
                aggregate: Aggregate::CountDistinct,
            },
            RefusalReason::PlanTablesShareAnIdentifier {
                table: TableName::parse("orders").expect("a test table is a table"),
            },
            RefusalReason::SourceUnavailable {
                source: SourceName::parse("elsewhere").expect("a test source is a source"),
            },
            RefusalReason::CredentialUnavailable {
                source: SourceName::parse("warehouse").expect("a test source is a source"),
            },
        ]
    }

    /// The derivation that keeps this transport's vocabulary equal to the HTTP surface's without
    /// either crate being able to see the other.
    ///
    /// The variant name is read out of the domain type's own `Serialize` - externally tagged, so the
    /// one key of the serialized object IS the variant name - rather than from a list typed here,
    /// which would be the same hand-written table this test exists to check.
    #[test]
    fn the_code_is_the_variant_name_in_snake_case() {
        for reason in every_reason() {
            let value = serde_json::to_value(&reason).expect("a refusal serializes");
            // A struct variant serializes externally tagged, so its one key IS the variant name; a
            // unit variant (`FederationNotExecutable`) serializes as the bare name string.
            let variant = match value {
                serde_json::Value::Object(map) => map.keys().next().cloned().expect("an externally tagged enum has one key"),
                serde_json::Value::String(name) => name,
                _ => panic!("a refusal serializes to an object or a unit string"),
            };
            let (code, _) = refused(&reason);
            assert_eq!(code, &snake_case(&variant), "{variant}");
        }
    }

    #[test]
    fn every_refusal_has_a_sentence_and_no_two_share_a_code() {
        let mut codes: Vec<&str> = Vec::new();
        for reason in every_reason() {
            let (code, detail) = refused(&reason);
            assert!(!detail.is_empty(), "{code} has no sentence");
            codes.push(code);
        }
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "two refusals share a code: {codes:?}");
    }

    /// The numbers a caller needs in order to ask a narrower question survive into the sentence.
    #[test]
    fn a_bound_that_was_exceeded_says_what_the_bound_is() {
        // THE PHRASES, not the numbers. What this cell means is that each number arrives in its own
        // ROLE - the one asked for, and the bound - and presence cannot tell those apart. An earlier
        // round of this fix argued exactly that for the dimensions line below while clearing THIS
        // line on probability grounds: `9000` and `3653` are four characters each and cannot land by
        // accident, which is true and is not the point. Swapping the two in the renderer left both
        // present and the assertion green.
        let (_, detail) = refused(&RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 });
        assert!(
            detail.contains("the period asked about is 9000 days") && detail.contains("at most 3653 are allowed"),
            "{detail}"
        );
        let (_, detail) = refused(&RefusalReason::TooManyDimensions { requested: 5, limit: 4 });
        assert!(
            detail.contains("5 dimensions were asked for") && detail.contains("at most 4 are allowed"),
            "{detail}"
        );
    }

    /// The bound with no number still tells an agent what to do, and names nothing it was not told.
    #[test]
    fn a_bound_with_no_number_still_says_narrow_and_says_not_to_retry() {
        let (code, detail) = refused(&RefusalReason::ResultTooLarge {
            bound: ResultBound::Volume,
        });
        // The same code as the row cap, because it is the same answer and the same remedy.
        assert_eq!(code, "result_too_large");
        // An agent that reads a figure will try to divide a range by it. There is none: the bound is
        // the data system's and this deployment is not told it, so a digit here would be invented.
        assert!(
            !detail.chars().any(char::is_numeric),
            "the sentence names a bound nobody measured: {detail}"
        );
        assert!(detail.contains("narrower"), "{detail}");
        assert!(detail.contains("retrying will not help"), "{detail}");
    }

    fn snake_case(name: &str) -> String {
        let mut out = String::with_capacity(name.len().saturating_add(4));
        for (index, character) in name.char_indices() {
            if character.is_ascii_uppercase() {
                if index != 0 {
                    out.push('_');
                }
                out.push(character.to_ascii_lowercase());
            } else {
                out.push(character);
            }
        }
        out
    }
}

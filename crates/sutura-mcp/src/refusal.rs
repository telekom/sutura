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

use sutura_domain::query::RefusalReason;

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
        // limit rather than a field: nothing about the question was wrong, and the same question over
        // a narrower range or with fewer dimensions is answerable.
        RefusalReason::ResultTooLarge { limit } => (
            "result_too_large",
            format!("the answer would have been more than {limit} rows; ask a narrower question"),
        ),
        RefusalReason::TimeRangeTooLong { days, limit } => (
            "time_range_too_long",
            format!("the period asked about is {days} days; at most {limit} are allowed"),
        ),
        RefusalReason::PlanSpansTwoSources { sources } => (
            "plan_spans_two_sources",
            format!("this question would read from {sources} data systems, and one answer reads from one"),
        ),
        RefusalReason::SourceUnavailable { ref source } => (
            "source_unavailable",
            format!("the data system `{source}` is not one this process opened"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::{DimensionName, Grain, MetricName, SourceName};
    use sutura_domain::query::RefusalReason;

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
            RefusalReason::ResultTooLarge { limit: 10_000 },
            RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 },
            RefusalReason::PlanSpansTwoSources { sources: 2 },
            RefusalReason::SourceUnavailable {
                source: SourceName::parse("elsewhere").expect("a test source is a source"),
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
            let object = value.as_object().expect("every variant carries fields, so it is an object");
            let variant = object.keys().next().expect("an externally tagged enum has one key");
            let (code, _) = refused(&reason);
            assert_eq!(code, &snake_case(variant), "{variant}");
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
        let (_, detail) = refused(&RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 });
        assert!(detail.contains("9000") && detail.contains("3653"), "{detail}");
        let (_, detail) = refused(&RefusalReason::TooManyDimensions { requested: 5, limit: 4 });
        assert!(detail.contains('5') && detail.contains('4'), "{detail}");
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

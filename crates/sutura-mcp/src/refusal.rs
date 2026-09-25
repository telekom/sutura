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
//! MCP has none. The HTTP surface decides a status and a sentence for a refusal, because on that
//! transport the status is what a monitor counts. Here there is only the code and the sentence.
//!
//! # The code comes from the domain, not from this transport
//!
//! [`RefusalReason::code`] is the single place the code is decided, and both transports read it -
//! this one and the HTTP surface's - so a code cannot drift between them without first drifting
//! from the variant. The domain holds that derivation with its own test: the code is the
//! `snake_case` spelling of the variant's own name, read off the domain type's own `Serialize`
//! rather than from a list typed beside it. A hand-written code that drifted from the variant would
//! fail that test, and a hand-written code that drifted from the other transport's would have to
//! drift from the variant first. `the_code_is_the_variant_name_in_snake_case` below holds this
//! transport's own output to the same derivation, against a reintroduced literal here.
//!
//! What is NOT guaranteed is that the two transports' *sentences* agree. They are written for
//! different readers - one for a client's error surface, one for a model's context - and nothing
//! compares them.

use sutura_domain::query::{RefusalReason, ResultBound};

/// The code and the sentence for one refusal.
///
/// The code is the domain's ([`RefusalReason::code`]); the match below is exhaustive with no
/// wildcard arm and decides the sentence, so the two cannot drift apart from each other.
pub(crate) fn refused(reason: &RefusalReason) -> (&'static str, String) {
    // The code is the domain's, read once - this transport spells no `code` of its own, so it
    // cannot drift from the HTTP surface. The match below decides the sentence only.
    let code = reason.code();
    let detail = match *reason {
        RefusalReason::MetricUnknown { ref metric } => {
            format!("this catalog defines no metric called `{metric}`")
        }
        RefusalReason::MetricsSpanDifferentModels { ref first, ref other } => {
            format!("`{first}` and `{other}` do not share a model and a time column")
        }
        RefusalReason::MultiMetricNotExecutable { requested } => {
            format!("this deployment does not yet answer a question naming {requested} metrics together")
        }
        RefusalReason::GrainNotSupported { ref metric, grain } => {
            format!("`{metric}` is not defined at `{grain}` grain")
        }
        RefusalReason::DimensionNotPermitted {
            ref metric,
            ref dimension,
        } => format!("`{metric}` does not declare a dimension called `{dimension}`"),
        RefusalReason::DimensionNotFilterable {
            ref metric,
            ref dimension,
        } => format!("`{dimension}` can be grouped by on `{metric}` but not filtered on"),
        // The value is deliberately absent. `RefusalReason` does not carry it, for the reason
        // `crate::wire::RefusalContent` restates: caller text that reaches a model's context is
        // caller text that becomes somebody else's input.
        RefusalReason::DimensionValueNotAllowed {
            ref metric,
            ref dimension,
        } => format!("the value given for `{dimension}` is not one `{metric}` declares"),
        RefusalReason::DuplicateDimension { ref dimension } => {
            format!("`{dimension}` is listed more than once")
        }
        RefusalReason::TooManyDimensions { requested, limit } => {
            format!("{requested} dimensions were asked for; at most {limit} are allowed")
        }
        // The one refusal that is decided AFTER a data system has answered, which is why it names a
        // bound rather than a field: nothing about the question was wrong, and the same question over
        // a narrower range or with fewer dimensions is answerable.
        //
        // One code for both bounds - the remedy an agent has to carry out is the same narrowing - and
        // a nested exhaustive match for the sentence, so a bound with no number cannot be described
        // using the other one's.
        RefusalReason::ResultTooLarge { bound } => match bound {
            ResultBound::Rows { limit } => format!(
                "the answer would have been more than {limit} rows; ask a narrower question, or add \
                 `top: {{ n, by, direction }}` to ask for exactly the rows you want ranked by the \
                 metric or the period - {limit} is this deployment's own bound and not one you or \
                 the caller can raise; report it to whoever operates this deployment if it is \
                 consistently too small"
            ),
            // No figure: the bound is the data system's and this deployment is not told it, so
            // there is nothing to divide a range by. What an agent needs instead is that the
            // remedy is still narrowing and that a retry is not one - see `ResultBound::Volume`.
            ResultBound::Volume => String::from(
                "the answer was more data than the data system would return at once; nothing was \
                 cut down to fit and retrying will not help. Ask a narrower question - a shorter \
                 period, or fewer dimensions.",
            ),
            // This deployment's own ceiling, and unlike `Volume` a figure it was told - so the
            // sentence names it rather than leaving an agent to guess how much to narrow by.
            ResultBound::Encoded { limit_bytes } => format!(
                "the answer would have occupied more than {limit_bytes} bytes once rendered; nothing \
                 was cut down to fit and retrying will not help. Ask a narrower question - a shorter \
                 period, or fewer dimensions."
            ),
        },
        RefusalReason::TimeRangeTooLong { days, limit } => {
            format!("the period asked about is {days} days; at most {limit} are allowed")
        }
        RefusalReason::PlanSpansTooManySources { sources, limit } => {
            format!("this question would read from {sources} data systems, and one answer reads from {limit} at most")
        }
        RefusalReason::FederationNotExecutable => String::from(
            "this deployment has no adapter that can execute one half of a question spanning two \
             data systems. Nothing you can change in the question helps and this is not an outage \
             to retry; ask without the dimension on the second data system, or report it.",
        ),
        RefusalReason::FederationLinkAmbiguous { ref source } => format!(
            "the dimensions on `{source}` join through more than one relationship, and the two \
             legs link on a single column"
        ),
        RefusalReason::FederationLinkCompound {
            ref source,
            ref relationship,
        } => format!(
            "the relationship `{relationship}` crossing into `{source}` declares more than one join \
             key, and the two legs link on a single column"
        ),
        RefusalReason::MeasureDoesNotFederate { ref metric, aggregate } => format!(
            "`{metric}` cannot be computed across two data systems because its {aggregate} \
             aggregate is not additive; ask it without the dimension that sits on the second \
             data system"
        ),
        // Written for an agent: D19 + A4's own reason applies here too. A non-finite ratio or an
        // ambiguous join is the same plan against the same rows failing again - not an outage - so
        // retrying it will not change the answer.
        RefusalReason::FederatedAnswerNotWellFormed { .. } => String::from(
            "answering this across two data systems hit a division by zero, a join key that \
             matched more than one row, or a join key that can never match because the two data \
             systems store it as two different types. Retrying it unchanged will be refused again: \
             ask the same metric without the dimension on the second data system, or say so to the \
             person you are acting for.",
        ),
        // Written for an agent, so it says which of the two moves is available rather than only that
        // this one failed: unlike the two-source refusal there usually is another question, because a
        // dimension that needs no join is still answered.
        //
        // D7: no schema identifier here, for the same reason `sutura_app::prompt` never puts a table
        // name in an agent's context - this refusal is a tool output an agent reads exactly as it
        // would read the prompt.
        RefusalReason::PlanTablesShareAnIdentifier { .. } => String::from(
            "answering this would read two different tables that answer to one identifier, and one \
             statement cannot tell them apart. Try a dimension that needs no join; if every \
             useful one does, say so to the person you are acting for.",
        ),
        RefusalReason::SourceUnavailable { ref source } => {
            format!("the data system `{source}` is not one this process opened")
        }
        RefusalReason::ResourcesExhausted { ceiling_bytes } => format!(
            "answering this needed more working memory than this deployment allows \
             ({ceiling_bytes} bytes) and was refused rather than allowed to exhaust the \
             process. Asking again unchanged will be refused again: narrow the period, ask \
             for fewer dimensions, or add a filter."
        ),
        // Written for an agent, which is a different reader from the HTTP surface's: what an agent
        // needs is to stop, not to adapt. There is no narrower question that helps and no retry that
        // succeeds, so the sentence says both and tells it what to do instead - report it to the
        // person it is acting for, who can ask for access.
        RefusalReason::CredentialUnavailable { ref source } => format!(
            "the person you are acting for has no access to the data system `{source}`, and \
             this deployment will not read it under its own identity instead. Nothing you can \
             change in the question helps and retrying will not either. Say so, and say that \
             access to `{source}` is what would be needed."
        ),
        // Written for an agent: this is the data system itself saying no about WHO asked, not a
        // credential the caller is missing. There is no retry that succeeds and no narrower
        // question that helps, so the sentence says stop and names the fix - a grant at that data
        // system, which only the person the agent is acting for can ask for.
        RefusalReason::SourceRefused { ref source } => format!(
            "the data system `{source}` refused this question: the identity it would run as is \
             not permitted to ask it. This is an authorization decision made there, not an \
             outage and not something asking again will change. Say so, and say that access to \
             `{source}` is what would be needed."
        ),
        // Written for an agent: stop, and say why, rather than retry. This deployment decided the
        // bound and the data system enforced it - something WAS judged - so retrying unchanged
        // spends the whole budget again. A narrower question is what changes the outcome, which is
        // the guide `sutura_app::prompt::refusal` gives for the same reason.
        RefusalReason::DeadlineExceeded { budget_seconds } => format!(
            "this deployment stopped the question after {budget_seconds} seconds, its configured \
             budget for one answer. Retrying it unchanged will be refused again: narrow the \
             period, ask for fewer dimensions, or add a filter."
        ),
        // Written for an agent, and USUALLY the one refusal here that self-heals: unlike every
        // other arm above, waiting is usually a real remedy. `docs/adr/0030` is the record. The one
        // exception is stated too: a question whose own estimate is over the ceiling by itself is
        // refused every window, and only narrowing helps there.
        RefusalReason::BudgetExhausted { reset_after_seconds } => format!(
            "the person you are acting for has spent this deployment's per-replica byte ceiling \
             for the current window. Asking again right now will be refused again; it usually \
             becomes answerable in {reset_after_seconds} seconds, when the window resets, and \
             narrowing does not usually help - the ceiling is about how much has already been \
             spent, not about this question's shape. If it is refused again immediately in a fresh \
             window, this question's own estimate is over the ceiling by itself: waiting will never \
             help that case, and narrowing the question is the only remedy."
        ),
        // Written for an agent: the combined set was cut before it could be ranked, so the sentence
        // says what happened to the data rather than implying the question was malformed - and it
        // names who can move the number, because narrowing is not always the caller's remedy here.
        RefusalReason::TopOverUncertifiedRows { ceiling } => format!(
            "this question asks for a ranked, bounded result over two data systems, and the \
             combined set before ranking already reached this deployment's ceiling of {ceiling} \
             rows; the top rows of that set are not the top rows of the dimension. Retrying it \
             unchanged will be refused again: narrow the range, add a filter, or report it to \
             whoever operates this deployment, who can raise this ceiling."
        ),
        // Written for an agent: nothing you asked was wrong, and nothing you can change fixes it -
        // the metric names a fact model this workspace does not yet know how to join a second one
        // against. Say so, and name the metric.
        RefusalReason::CrossModelRatioNotExecutable { ref metric, ref model } => format!(
            "`{metric}` measures a ratio side on model `{model}`, and this deployment does not yet \
             build the second fact leg such a term needs. Nothing you can change in the question \
             helps and this is not an outage to retry; report it to the person you are acting for."
        ),
    };
    (code, detail)
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
            RefusalReason::MetricsSpanDifferentModels {
                first: metric(),
                other: MetricName::parse("margin").expect("a test metric is a metric"),
            },
            RefusalReason::MultiMetricNotExecutable { requested: 2 },
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
            RefusalReason::FederationLinkCompound {
                source: SourceName::parse("warehouse").expect("a test source is a source"),
                relationship: sutura_domain::model::RelationshipName::parse("usage_subscription")
                    .expect("a test relationship is a relationship"),
            },
            RefusalReason::MeasureDoesNotFederate {
                metric: metric(),
                aggregate: Aggregate::CountDistinct,
            },
            RefusalReason::FederatedAnswerNotWellFormed {
                federated: sutura_domain::plan::FederatedAnswerRefusal::AmbiguousLink,
            },
            RefusalReason::PlanTablesShareAnIdentifier {
                table: TableName::parse("orders").expect("a test table is a table"),
            },
            RefusalReason::SourceUnavailable {
                source: SourceName::parse("elsewhere").expect("a test source is a source"),
            },
            RefusalReason::SourceRefused {
                source: SourceName::parse("warehouse").expect("a test source is a source"),
            },
            RefusalReason::CredentialUnavailable {
                source: SourceName::parse("warehouse").expect("a test source is a source"),
            },
            RefusalReason::DeadlineExceeded { budget_seconds: 29 },
            RefusalReason::BudgetExhausted { reset_after_seconds: 41 },
            RefusalReason::TopOverUncertifiedRows { ceiling: 10_000 },
            RefusalReason::CrossModelRatioNotExecutable {
                metric: metric(),
                model: sutura_domain::model::ModelName::parse("customers").expect("a test model is a model"),
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

    #[test]
    fn a_row_cap_refusal_points_at_top_and_at_the_deployments_operator() {
        // The pointer `github.com/telekom/sutura#777` adds: the caller cannot raise the row cap,
        // but `top` bounds the same question without hitting it, and an operator is who could move
        // the compiled bound itself.
        let (_, detail) = refused(&RefusalReason::ResultTooLarge {
            bound: ResultBound::Rows { limit: 10_000 },
        });
        assert!(detail.contains("top:"), "{detail}");
        assert!(detail.contains("operates this deployment"), "{detail}");
        assert!(
            !detail.contains("raise") || detail.contains("not one you"),
            "the sentence must not promise the caller can raise the bound: {detail}"
        );
    }

    #[test]
    fn a_federated_top_over_the_ceiling_names_the_bound_and_the_operator() {
        let (code, detail) = refused(&RefusalReason::TopOverUncertifiedRows { ceiling: 10_000 });
        assert_eq!(code, "top_over_uncertified_rows");
        assert!(detail.contains("10000"), "{detail}");
        assert!(detail.contains("operates this deployment"), "{detail}");
    }

    #[test]
    fn a_stopped_deadline_names_its_budget_in_the_sentence() {
        // `docs/adr/0029`'s own claim for this wire: "the sentence names the configured budget in
        // seconds". `RefusalContent` is `{code, detail}`, so the sentence is the ONLY place
        // `budget_seconds` reaches this wire at all.
        let (code, detail) = refused(&RefusalReason::DeadlineExceeded { budget_seconds: 29 });
        assert_eq!(code, "deadline_exceeded");
        assert!(
            detail.contains("29 seconds"),
            "the sentence does not name the budget: {detail}"
        );
    }

    #[test]
    fn a_spent_budget_names_its_code_and_its_reset_in_the_sentence() {
        let (code, detail) = refused(&RefusalReason::BudgetExhausted { reset_after_seconds: 41 });
        assert_eq!(code, "budget_exhausted");
        assert!(
            detail.contains("41 seconds"),
            "the sentence does not name when the window resets: {detail}"
        );
    }

    #[test]
    fn a_shared_table_identifier_never_reaches_an_agents_context() {
        // D7: this refusal is a tool result an agent reads, exactly the surface
        // `sutura_app::prompt` never puts a schema name in.
        let (_, detail) = refused(&RefusalReason::PlanTablesShareAnIdentifier {
            table: TableName::parse("orders").expect("a test table is a table"),
        });
        assert!(!detail.contains("orders"), "{detail}");
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

    /// The third bound, and unlike `Volume` it names a number - this deployment's own ceiling.
    #[test]
    fn the_encoded_bound_names_the_ceiling_it_measured() {
        let (code, detail) = refused(&RefusalReason::ResultTooLarge {
            bound: ResultBound::Encoded {
                limit_bytes: 8 * 1024 * 1024,
            },
        });
        assert_eq!(code, "result_too_large");
        assert!(detail.contains("8388608"), "the sentence does not name the ceiling: {detail}");
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

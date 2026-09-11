//! Everything about a refusal, in one file.
//!
//! The table, the total match that will not compile when the domain gains a variant, the accessor a
//! composition root prints from, and the section the prompt renders.
//!
//! **Its own module because `prompt.rs` was two lines under the thousand-line limit
//! `cargo xtask max-lines` enforces and cannot exempt**, at the cut `knowledge.rs` already made
//! once, and the seam is not the line count: this is the one place `RefusalReason` is read. Nothing
//! else in the prompt looks at a governance decision at all - the rest of the document is derived
//! from the tool list and the pinned bundle - so the file that holds the refusal wording is the file
//! that holds every reader of that enum, and a variant added to the domain lands here and nowhere
//! else.
//!
//! Why the wording is strong, why an operator cannot replace it, and why a refusal lives inside the
//! `Ok` at all are argued in `prompt.rs`'s own header.

use sutura_domain::query::RefusalReason;

use super::wrap;

/// One refusal, and what an agent should do about it.
///
/// `remedy` is the load-bearing field. A refusal an agent cannot act on is a refusal it retries, so
/// every entry either says what to change or says outright that there is nothing to change and the
/// answer is to stop.
pub(super) struct Guide {
    /// The reason as the tool surface names it: the domain's own machine code for this variant,
    /// which is [`RefusalReason::code`] itself. Every transport writes the same `&'static str`, so
    /// the guide's proof of WHICH variant fired is the domain's canonical name and cannot drift
    /// from the HTTP body's `code` or the agent surface's.
    pub(super) reason: &'static str,
    /// What happened, in the caller's terms.
    pub(super) meaning: &'static str,
    /// What to do next.
    pub(super) remedy: &'static str,
}

const METRIC_UNKNOWN: Guide = Guide {
    reason: "metric_unknown",
    meaning: "no metric of that name is defined here",
    remedy: "Use a name from the metric list below, exactly as spelled. Do not try a variant \
             spelling, a plural, or a name you remember from another deployment: the list is the \
             whole of what exists.",
};

const GRAIN_NOT_SUPPORTED: Guide = Guide {
    reason: "grain_not_supported",
    meaning: "the metric exists and does not declare that time resolution",
    remedy: "Ask at a grain the metric lists. A finer grain is not a narrower version of the same \
             question here - it is a number nobody certified, which is why it is refused rather \
             than approximated.",
};

const DIMENSION_NOT_PERMITTED: Guide = Guide {
    reason: "dimension_not_permitted",
    meaning: "the metric does not declare that dimension",
    remedy: "Use one of the dimensions listed under that metric. There is no way to reach an \
             attribute a metric did not declare, so do not substitute a similar-sounding name.",
};

const DIMENSION_NOT_FILTERABLE: Guide = Guide {
    reason: "dimension_not_filterable",
    meaning: "the dimension can be grouped by and not filtered, because the definitions declare no \
              set of values for it",
    remedy: "Drop the filter, group by the dimension instead, and read the row you wanted out of \
             the result.",
};

const DIMENSION_VALUE_NOT_ALLOWED: Guide = Guide {
    reason: "dimension_value_not_allowed",
    meaning: "the dimension is filterable and the value is not one the definitions declare",
    remedy: "Use a value from that dimension's list below. The refusal does not repeat your value \
             back to you, on purpose, so compare against the list rather than expecting a \
             correction.",
};

const DUPLICATE_DIMENSION: Guide = Guide {
    reason: "duplicate_dimension",
    meaning: "the same dimension was sent twice in one question",
    remedy: "Send it once. It is refused rather than de-duplicated because a caller who sent it \
             twice believed something about the result that is not true.",
};

const TOO_MANY_DIMENSIONS: Guide = Guide {
    reason: "too_many_dimensions",
    meaning: "more group-by keys than one question may carry",
    remedy: "Ask a narrower question, or ask two questions. Do not resend the same list.",
};

// ONE guide for two bounds, and the prose says both rather than the row cap alone. The refusal
// carries a `ResultBound` naming which one fired, and a caller reads that in the sentence the
// transport rendered; what the prompt lists is what a refusal MEANS and what to do about it, and
// those are the same for both - too much data, ask a narrower question. A second guide would put a
// second entry under one variant name and give an agent two paragraphs saying one thing.
const RESULT_TOO_LARGE: Guide = Guide {
    reason: "result_too_large",
    meaning: "the answer was too much data to certify - more rows than the cap, or more than the \
              data system would return at once - and it was refused rather than cut short",
    remedy: "Narrow the period, drop a dimension, or add a filter, and ask again. Nothing partial \
             is returned and nothing will be: a total over some of the groups is a different number \
             wearing the same name. Retrying the same question returns the same refusal. The \
             refusal names the row cap where the cap is what fired; where the data system's own \
             bound is, there is no number to read, so narrow by a visible step rather than \
             computing one.",
};

const TIME_RANGE_TOO_LONG: Guide = Guide {
    reason: "time_range_too_long",
    meaning: "the period asked about is longer than one question may span",
    remedy: "Split it into consecutive shorter periods and ask about each. The refusal carries both \
             day counts, so the split can be computed rather than guessed.",
};

const RESOURCES_EXHAUSTED: Guide = Guide {
    reason: "resources_exhausted",
    meaning: "answering would have needed more working memory than this deployment allows, so it was \
              refused rather than allowed to exhaust the process",
    remedy: "Narrow the period, drop a dimension, or add a filter, and ask again. Retrying the same \
             question returns the same refusal: the ceiling is a configured number, not a passing \
             condition, so this is not an outage to wait out.",
};

const PLAN_SPANS_TOO_MANY_SOURCES: Guide = Guide {
    reason: "plan_spans_too_many_sources",
    meaning: "answering would need to read from more data systems than this deployment serves (two at \
              most)",
    remedy: "Nothing you can change. Report it to a person: it is a fact about how the metric is \
             defined, not about how you asked. Do not retry and do not try a different dimension \
             in the hope of avoiding it.",
};

const PLAN_TABLES_SHARE_AN_IDENTIFIER: Guide = Guide {
    reason: "plan_tables_share_an_identifier",
    meaning: "answering would read two different tables that carry the same name, and one statement \
              cannot tell them apart",
    remedy: "Try a dimension that does not need that join - it is the join that puts both tables in \
             one statement, so a question without it is still answered. If every dimension you need \
             goes through it, report it to a person: it is a fact about where the tables live, not \
             about how you asked.",
};

const FEDERATION_NOT_EXECUTABLE: Guide = Guide {
    reason: "federation_not_executable",
    meaning: "this deployment has no adapter that can execute one half of a question spanning two data \
              systems, so the question cannot be answered yet",
    remedy: "Nothing you can change by re-asking, and do not retry it as if it were an outage: this is \
             not a data system being down. Ask the same metric without the dimension on the second \
             data system, or report it to a person.",
};

const FEDERATION_LINK_AMBIGUOUS: Guide = Guide {
    reason: "federation_link_ambiguous",
    meaning: "the question's dimensions on the second data system join the metric through more than \
              one relationship, and the two legs link on a single column",
    remedy: "Nothing you can change about the question. Report it to a person: it is a fact about how \
             the metric is defined.",
};

const MEASURE_DOES_NOT_FEDERATE: Guide = Guide {
    reason: "measure_does_not_federate",
    meaning: "across two data systems this measure cannot be computed and recombined - its aggregate \
              (a distinct count) is not additive the way a sum or an average is",
    remedy: "Nothing you can change about the question. Ask the same metric without the dimension that \
             sits on the second data system, or report it to a person.",
};

const LEGS_DECIDE_IDENTITY_DIFFERENTLY: Guide = Guide {
    reason: "legs_decide_identity_differently",
    meaning: "the question spans two data systems that decide who is asking differently, so one \
              answer would add rows read under one identity to rows read under another - a total \
              neither identity is entitled to",
    remedy: "Ask the same metric without the dimension that sits on the second data system, and \
             the mono-source question on each is still answered. Retrying changes nothing, and \
             this is not an outage: it is a fact about how the two data systems are declared. If \
             every dimension you need crosses them, say so to the person you are acting for.",
};

const SOURCE_UNAVAILABLE: Guide = Guide {
    reason: "source_unavailable",
    meaning: "the data system that metric lives in is not one this deployment opened",
    remedy: "Nothing you can change. Report it to a person. This is the one refusal that looks like \
             an outage and is still not one to retry: the answer will not become available by \
             asking again.",
};

const CREDENTIAL_UNAVAILABLE: Guide = Guide {
    reason: "credential_unavailable",
    meaning: "the person you are acting for has no access to the data system that metric lives in, \
              and this deployment will not read it under its own identity instead",
    remedy: "Nothing you can change, and this is the one refusal where that matters most: a \
             narrower question, a different grain and a shorter period all return it again. Say so, \
             and say that access to that data system is what would be needed - a person can ask for \
             it, and you cannot.",
};

const SOURCE_REFUSED: Guide = Guide {
    reason: "source_refused",
    meaning: "the data system itself refused the question because the identity it would run as is \
              not permitted to ask it - an authorization decision made there, not an outage",
    remedy: "Nothing you can change in the question, and retrying it will be refused again at the \
             same place: this is not a data system being down and it is not waiting out. Report it \
             to the person you are acting for, and say that access to that data system is what \
             would be needed.",
};

/// Every refusal a caller can be given, in the order the prompt lists them.
///
/// Ordered so the ones an agent can act on come first and the two it cannot come last, because a
/// reader who stops early should have read the actionable ones.
///
/// The exhaustiveness mechanism is in `prompt::tests`: a function there maps every
/// `RefusalReason` variant to its entry with a total match,
/// so a variant added to the domain does not compile until somebody opens this file. *What that does
/// not force is the corpus in that test gaining a member, so the set equality it asserts is a second
/// net rather than the first.*
pub(super) const GUIDES: &[&Guide] = &[
    &METRIC_UNKNOWN,
    &GRAIN_NOT_SUPPORTED,
    &DIMENSION_NOT_PERMITTED,
    &DIMENSION_NOT_FILTERABLE,
    &DIMENSION_VALUE_NOT_ALLOWED,
    &DUPLICATE_DIMENSION,
    &TOO_MANY_DIMENSIONS,
    &RESULT_TOO_LARGE,
    &TIME_RANGE_TOO_LONG,
    // Actionable, and last of the actionable ones: the remedy is the same narrowing
    // `ResultTooLarge` asks for, and an agent reaching this one has already read that.
    &RESOURCES_EXHAUSTED,
    &PLAN_SPANS_TOO_MANY_SOURCES,
    &FEDERATION_NOT_EXECUTABLE,
    &FEDERATION_LINK_AMBIGUOUS,
    &MEASURE_DOES_NOT_FEDERATE,
    // With the federation family rather than with the two an agent cannot act on, because it IS
    // actionable and the move is the same one: drop the dimension that pulls in the second data
    // system. An agent reading it here has just read that a refusal about the two-source shape is
    // not one to retry.
    &LEGS_DECIDE_IDENTITY_DIFFERENTLY,
    // Actionable, and the reason it sits after the source-count refusals rather than with the
    // narrowing ones: the move is to drop a JOIN rather than to narrow anything, and an agent
    // reaching it has already read that a refusal about the plan's shape is not one to retry
    // unchanged.
    &PLAN_TABLES_SHARE_AN_IDENTIFIER,
    &SOURCE_UNAVAILABLE,
    &SOURCE_REFUSED,
    &CREDENTIAL_UNAVAILABLE,
];

/// The guide for one refusal.
///
/// **This match exists in order not to compile.** It is total, so a variant added to
/// [`RefusalReason`] is a compile error here until somebody writes the guidance for it - which is
/// what makes the refusal section unable to fall silently behind the domain.
///
/// It used to live in `prompt::tests` under `#[cfg(test)]`, on the reasoning that nothing in the rendered
/// output needs an instance of a refusal. That stopped being true when a composition root needed the
/// same two sentences to print a refused question with: a second match in the binary would have been
/// a second table to keep in step, and a third copy of prose that already exists here and on the HTTP
/// surface. Moved rather than copied, so there is still exactly one match.
pub(super) const fn guide_for(reason: &RefusalReason) -> &'static Guide {
    match *reason {
        RefusalReason::MetricUnknown { .. } => &METRIC_UNKNOWN,
        RefusalReason::GrainNotSupported { .. } => &GRAIN_NOT_SUPPORTED,
        RefusalReason::DimensionNotPermitted { .. } => &DIMENSION_NOT_PERMITTED,
        RefusalReason::DimensionNotFilterable { .. } => &DIMENSION_NOT_FILTERABLE,
        RefusalReason::DimensionValueNotAllowed { .. } => &DIMENSION_VALUE_NOT_ALLOWED,
        RefusalReason::DuplicateDimension { .. } => &DUPLICATE_DIMENSION,
        RefusalReason::TooManyDimensions { .. } => &TOO_MANY_DIMENSIONS,
        RefusalReason::ResultTooLarge { .. } => &RESULT_TOO_LARGE,
        RefusalReason::TimeRangeTooLong { .. } => &TIME_RANGE_TOO_LONG,
        RefusalReason::ResourcesExhausted { .. } => &RESOURCES_EXHAUSTED,
        RefusalReason::PlanSpansTooManySources { .. } => &PLAN_SPANS_TOO_MANY_SOURCES,
        RefusalReason::FederationNotExecutable => &FEDERATION_NOT_EXECUTABLE,
        RefusalReason::FederationLinkAmbiguous { .. } => &FEDERATION_LINK_AMBIGUOUS,
        RefusalReason::MeasureDoesNotFederate { .. } => &MEASURE_DOES_NOT_FEDERATE,
        RefusalReason::PlanTablesShareAnIdentifier { .. } => &PLAN_TABLES_SHARE_AN_IDENTIFIER,
        RefusalReason::SourceUnavailable { .. } => &SOURCE_UNAVAILABLE,
        RefusalReason::SourceRefused { .. } => &SOURCE_REFUSED,
        RefusalReason::CredentialUnavailable { .. } => &CREDENTIAL_UNAVAILABLE,
        RefusalReason::LegsDecideIdentityDifferently { .. } => &LEGS_DECIDE_IDENTITY_DIFFERENTLY,
    }
}

/// What a refusal means and what to do about it: `(meaning, remedy)`, in the order the prompt renders
/// them.
///
/// **The accessor a composition root prints from, and it publishes no new prose.** `sutura-cli` used
/// to hand a person the Rust `Debug` of a governance decision, which names the variant and says
/// nothing about what to do; the wording it needed was already written twice - here for the
/// agent-facing prompt, and on the HTTP surface for a client - so this is a third READER of the first
/// table rather than a third table.
///
/// `&'static str` because [`GUIDES`] owns the wording: nothing here composes a message and nothing
/// here reads the refusal's own fields. A caller that wants those still has the [`RefusalReason`] it
/// passed in.
#[must_use]
pub const fn guidance(reason: &RefusalReason) -> (&'static str, &'static str) {
    let guide = guide_for(reason);
    (guide.meaning, guide.remedy)
}

/// The refusal section, which is the one an agent most needs and most often lacks.
pub(super) fn refusals() -> String {
    let mut lines = vec![
        String::from("## A refusal is an answer, not an error\n"),
        String::from(REFUSAL_INTRO),
        String::new(),
        String::from("Every reason you can be given, and what to do about each:\n"),
    ];
    for guide in GUIDES {
        lines.push(wrap("- ", &format!("**{}** - {}.", guide.reason, guide.meaning), "  "));
        lines.push(wrap("  ", guide.remedy, "  "));
    }
    lines.join("\n")
}

const REFUSAL_INTRO: &str = "\
A declined question was ANSWERED, not dropped: the service read it, applied the definitions, and the
answer is no. The outcome is `refusal`, carrying a typed reason and one sentence saying what to
change. It is not a timeout, not an outage, and not a malformed request.

**Over HTTP a refusal arrives with an error status rather than a success one**, which is not a
transport problem and not a reason to try again. The reason is what to act on: the body repeats it
as a machine-readable `code`, and the name below is that same `code` - lower case, with
underscores. The status only says which class of no it is.

**Do not retry a refused question unchanged.** Repeating the same question will not change the
answer, and a loop that keeps asking is a loop that consumes the deployment's budget to learn
nothing. Either change the question in the specific way the reason names, or tell the user what was
declined and why. Never report a refusal to a user as a failure of the system, and never describe it
as a temporary problem.

A refusal never echoes your own text back. `DimensionValueNotAllowed` names the dimension and stops
there, so treat the lists in this document as the authority on what a value may be.";

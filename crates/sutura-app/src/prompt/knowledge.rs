//! The knowledge sections of the prompt: the glossary, the caveats, the terms recorded as undefined,
//! and the worked questions.
//!
//! Its own module because `prompt.rs` is at four fifths of the thousand-line limit
//! `cargo xtask max-lines` enforces and cannot be exempted, and because these four sections share one
//! decision that the rest of the document does not have to make: **what a capability licenses this
//! text to claim.**
//!
//! # The declaration decides what may be said, not what happens to be there
//!
//! `sutura_domain::knowledge::KnowledgeCapabilities` is what an adapter declares. An empty collection
//! means two different things depending on it, and only one of them licenses a sentence:
//!
//! * `Absences` declared and nothing recorded - somebody keeps that list and it is empty. This
//!   document may say so, and an agent may read the absence of an entry as "nothing was recorded".
//! * `Absences` not declared - this provider cannot record such a thing. This document must imply
//!   NOTHING about what is undefined, because there is nothing behind the implication.
//!
//! [`claim`] is where that lives, and it is a total match over
//! [`Capability`](sutura_domain::knowledge::Capability) on purpose: a fifth capability is a compile
//! error in the one place that decides what a prompt may say about it, rather than a kind that
//! silently renders nothing.
//!
//! # The glossary renders from the STRUCTURE, not from the prose
//!
//! Every token of a glossary line is a phrase the domain parsed - one line, bounded, no control
//! characters - or a name the pinned bundle already declares. No unbounded author text reaches that
//! section, so the assertion `prompt::tests` makes about model, table and column names holds for it
//! without a second argument. Note BODIES are the other thing: they are catalog prose, they go through
//! [`quote`](super::quote) like a metric's description, and they ride the same
//! `prompt.catalog_prose` switch. There is no new configuration key.
//!
//! # Where each section sits, and why
//!
//! * The terms recorded as undefined go **immediately after the refusal section**, which is where
//!   `MetricUnknown`'s remedy is: an agent that has just been told "use a name from the list" is the
//!   agent that needs to know which names were considered and rejected.
//! * Caveats are rendered **inside the block of the metric they are about**, not as a preamble. A
//!   caveat read by somebody skimming the top of a document is a caveat read by nobody who was about
//!   to ask that question.
//! * The glossary and the worked questions sit beside the metric list, which is what they are about.
//!
//! # What is NOT here
//!
//! No server-side resolution, and the glossary section says so in as many words. The agent resolves a
//! phrase to a metric name and states which it chose; `sutura_domain::knowledge` argues why that has
//! to stay true, and the type system holds it up - there is no field on a question that a phrase fits
//! in.

use sutura_domain::knowledge::{Capability, Caveat, Knowledge, Phrase, Referent};
use sutura_domain::model::MetricName;
use sutura_domain::query::Query;

use super::{CatalogProse, quote, wrap};

/// What one capability licenses this document to say, and what its absence licenses instead.
///
/// Two sentences per capability rather than one, because the interesting half is the second: a
/// deployment whose provider cannot record an absence must not be described in a way that lets an
/// agent infer anything from a term not being listed.
struct Claim {
    /// Rendered when the capability IS declared.
    declared: &'static str,
    /// Rendered when it is not.
    absent: &'static str,
}

/// The one exhaustive match over [`Capability`] in this crate.
///
/// **It exists in order not to compile.** A capability added to the domain is a compile error here -
/// in the place that decides what this document may claim about it - rather than a kind that renders
/// nothing and is noticed by nobody. `Capability::next` in the domain is the same trick for the same
/// reason, and the two are deliberately separate: what a capability IS belongs to the domain, and what
/// it licenses a prompt to say does not.
const fn claim(capability: Capability) -> Claim {
    match capability {
        Capability::Glossary => Claim {
            declared: "**A glossary** - the words people use for these metrics, and the one thing each of them means. It \
                       is listed below. Turning a question's wording into a metric name is YOUR step and worth naming in \
                       your answer.",
            absent: "**No glossary.** Nothing here maps a phrase to a metric, so match the user's words against the \
                     metric names and their descriptions, and say which one you chose and why.",
        },
        Capability::Caveats => Claim {
            declared: "**Caveats** - things to know before trusting a number. Each one is printed under the metric it is \
                       about and applies to every question about that metric.",
            absent: "**No caveats.** Nothing here records a trap in a definition, which is not a statement that there \
                     are none.",
        },
        Capability::Absences => Claim {
            declared: "**A reviewed list of terms that are deliberately NOT defined here**, above, after the refusal \
                       section. It is authoritative for what it names: a question about one of those terms is to be \
                       declined with the reason given, not approximated from the metrics that are defined.",
            absent: "**No record of what is deliberately undefined.** This deployment cannot tell you which terms were \
                     considered and rejected, so infer NOTHING from a term being absent from this document. The metric \
                     list is the only authority on what may be asked.",
        },
        Capability::Examples => Claim {
            declared: "**Worked questions** - real questions with the request that answers each, at the end of this \
                       document. They are the shape to copy.",
            absent: "**No worked questions.** Compose from the metric list and the bounds above.",
        },
    }
}

/// What this deployment records beyond its definitions.
///
/// Rendered only when at least one capability is declared. A provider that declares none gets no
/// section at all, which is the honest rendering: there is no distinction to draw, and four lines
/// saying "not recorded" would be noise rather than information. The document then claims nothing
/// about any of the four, which is exactly what an undeclared capability licenses.
pub(super) fn declaration(knowledge: &Knowledge) -> String {
    if knowledge.declares().is_empty() {
        return String::new();
    }
    let mut lines = vec![
        String::from("## What this deployment records about its own definitions\n"),
        wrap(
            "",
            "A catalog may carry notes about what its numbers mean, and not every deployment carries every kind. This \
             one records the following, and what it does NOT record is listed too - because a kind that is not recorded \
             is a kind you must not draw a conclusion from.",
            "",
        ),
        String::new(),
    ];
    // Over `Capability::every` rather than over four hand-written branches, so a capability added to
    // the domain appears here without this function being edited - and `claim` is what fails to
    // compile until somebody decides what it licenses.
    for capability in Capability::every() {
        let entry = claim(capability);
        let text = if knowledge.declares().declares(capability) {
            entry.declared
        } else {
            entry.absent
        };
        lines.push(wrap("- ", text, "  "));
    }
    lines.join("\n")
}

/// The glossary, rendered from the structure.
pub(super) fn glossary(knowledge: &Knowledge, prose: CatalogProse) -> String {
    if !knowledge.supports(Capability::Glossary) || knowledge.glossary().is_empty() {
        return String::new();
    }
    let mut lines = vec![
        String::from("## The words a question may arrive in\n"),
        wrap("", GLOSSARY_INTRO, ""),
        String::new(),
    ];
    for entry in knowledge.glossary().values() {
        lines.push(wrap(
            "- ",
            &format!("{} -> {}", phrases(entry.phrases()), referent(entry.means())),
            "  ",
        ));
        push_body(&mut lines, entry.body().as_str(), prose);
    }
    lines.join("\n")
}

const GLOSSARY_INTRO: &str = "Each line is a phrase somebody may use, and the one thing this deployment takes it to mean. \
                              **You do the resolving.** There is no field on a question that a phrase fits in - a request \
                              carries a metric name - so a wrong turn here is yours to catch rather than something the \
                              service will decline. Say which phrase you resolved and what you resolved it to, so the \
                              person can correct you.";

/// The terms recorded as deliberately undefined.
///
/// Three renderings, and the difference between the second and the third is the whole reason the
/// capability is declared rather than inferred: an empty list somebody keeps is a fact, and an empty
/// list nobody keeps is nothing at all.
pub(super) fn not_defined(knowledge: &Knowledge, prose: CatalogProse) -> String {
    if !knowledge.supports(Capability::Absences) {
        return String::new();
    }
    let mut lines = vec![String::from("## Terms this deployment records as NOT defined\n")];
    if knowledge.absences().is_empty() {
        lines.push(wrap("", NOTHING_RECORDED, ""));
        return lines.join("\n");
    }
    lines.push(wrap("", NOT_DEFINED_INTRO, ""));
    lines.push(String::new());
    for note in knowledge.absences().values() {
        lines.push(wrap("- ", &phrases(note.phrases()), "  "));
        push_body(&mut lines, note.body().as_str(), prose);
    }
    lines.join("\n")
}

const NOTHING_RECORDED: &str = "None. This deployment keeps such a list and there is nothing on it, so there is no term \
                                you are being asked to decline on that ground. `MetricUnknown` above is still the whole \
                                of what does not exist here.";

const NOT_DEFINED_INTRO: &str = "These terms were considered and deliberately have no definition here. A question about one of them is to be \
     DECLINED, with the reason below, and never approximated out of the metrics that are defined: a plausible number \
     under a name nobody certified is the failure this entire surface is arranged to prevent. Say what is missing and \
     that adding it is a decision for a person. The list is authoritative for what it names and is not a complete \
     inventory of everything undefined - a term that is on neither this list nor the metric list is simply not \
     answerable, which the refusal above already covers.";

/// The caveats about one metric, for the block that metric is rendered in.
///
/// Returns the empty string when there are none, so the metric block is unchanged for a deployment
/// with no caveats.
pub(super) fn caveats_about(knowledge: &Knowledge, metric: &MetricName, prose: CatalogProse) -> String {
    if !knowledge.supports(Capability::Caveats) {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::new();
    for note in knowledge.caveats_about(metric) {
        // The body is quoted first, because whether there IS one decides the punctuation. A colon
        // with nothing under it is what this rendered before, under `CatalogProse::Omitted`, and it
        // reads as a document that lost a paragraph rather than one that withheld it deliberately.
        let body = if prose.is_quoted() {
            quote(note.body().as_str())
        } else {
            String::new()
        };
        let ending = if body.is_empty() { '.' } else { ':' };
        lines.push(String::new());
        lines.push(wrap(
            "",
            &format!("**Caveat `{}`** - {}{ending}", note.name(), scope(note, metric)),
            "",
        ));
        if !body.is_empty() {
            lines.push(body);
        }
    }
    lines.join("\n")
}

/// What a caveat is about, as far as THIS metric is concerned.
///
/// A caveat may be scoped to several things at once; the metric block renders only the part of that
/// scope which belongs to the metric being read. A caveat about the whole metric and one about a
/// single permitted value are different warnings, and a reader deciding whether it applies to their
/// question needs to know which.
fn scope(note: &Caveat, metric: &MetricName) -> String {
    let mine: Vec<String> = note
        .about()
        .iter()
        .filter(|referent| referent.metric() == metric)
        .map(|referent| match (referent.dimension(), referent.value()) {
            (None, _) => String::from("about this metric as a whole"),
            (Some(dimension), None) => format!("about its `{dimension}`"),
            (Some(dimension), Some(value)) => format!("about `{value}` of its `{dimension}`"),
        })
        .collect();
    // `caveats_about` only yields notes that name this metric, so the list cannot be empty. Written
    // as a fallback rather than an `expect` because the no-panic rule is not conditional on a caller
    // being right, and the fallback is a true sentence rather than a placeholder.
    if mine.is_empty() {
        return String::from("about this metric");
    }
    mine.join(", ")
}

/// The worked questions.
pub(super) fn examples(knowledge: &Knowledge, prose: CatalogProse) -> String {
    if !knowledge.supports(Capability::Examples) || knowledge.examples().is_empty() {
        return String::new();
    }
    let mut lines = vec![String::from("## Worked questions\n"), wrap("", EXAMPLES_INTRO, "")];
    for note in knowledge.examples().values() {
        lines.push(String::new());
        lines.push(format!("### {}\n", note.name()));
        lines.push(wrap("- ", &format!("Asked as: {}", phrases(note.asked().iter())), "  "));
        lines.push(wrap("- ", &format!("Send: {}", request(note.question())), "  "));
        push_body(&mut lines, note.body().as_str(), prose);
    }
    lines.join("\n")
}

const EXAMPLES_INTRO: &str = "Questions somebody actually asked, with the request that answers each. Copy the shape. \
                              Every value in them is one this snapshot declares, so a question below is one this \
                              deployment answers rather than one it would decline.";

/// One question, as the fields a caller sends.
///
/// Rendered from the typed fields rather than serialized, and that is not only about avoiding a
/// format crate in this crate's dependency table. Every token below is a name the bundle declares, a
/// value from a declared allowlist, or a date - so the worked-question section carries no more author
/// text than the glossary does.
fn request(question: &Query) -> String {
    let mut parts = vec![
        format!("metric `{}`", question.metric()),
        format!("grain `{}`", question.grain()),
        format!(
            "period `{}` to `{}`",
            question.range().start().to_iso(),
            question.range().end().to_iso()
        ),
    ];
    if !question.dimensions().is_empty() {
        let names: Vec<String> = question.dimensions().iter().map(|name| format!("`{name}`")).collect();
        parts.push(format!("grouped by {}", names.join(", ")));
    }
    for filter in question.filters() {
        parts.push(format!("`{}` = `{}`", filter.dimension(), filter.value()));
    }
    parts.join(", ")
}

/// A run of phrases, quoted and comma-separated.
///
/// Double quotes around each, because a phrase has spaces in it and an unquoted list of them is
/// unreadable. The phrases themselves cannot carry a quote-and-comma that would forge a boundary -
/// they carry no control characters and the rendering is one line - but a reader still needs to see
/// where one ends.
fn phrases<'a>(phrases: impl Iterator<Item = &'a Phrase>) -> String {
    phrases
        .map(|phrase| format!("{:?}", phrase.as_str()))
        .collect::<Vec<String>>()
        .join(", ")
}

/// One referent, in the words a caller can act on.
///
/// A metric name, a dimension name, a declared value: the same three things `GET /v1/catalog`
/// exposes, and nothing else. There is no variant of a referent that could name a model, a table or a
/// column, which is what makes that true of this function without a check in it.
fn referent(referent: &Referent) -> String {
    match (referent.dimension(), referent.value()) {
        (None, _) => format!("metric `{}`", referent.metric()),
        (Some(dimension), None) => format!("dimension `{}` of metric `{}`", dimension, referent.metric()),
        (Some(dimension), Some(value)) => {
            format!(
                "value `{}` of dimension `{}` of metric `{}`",
                value,
                dimension,
                referent.metric()
            )
        }
    }
}

/// A glossary entry's, a caveat's, an absence's or an example's own prose, quoted, when this
/// deployment includes prose at all.
///
/// The same [`quote`] every metric description goes through and the same `prompt.catalog_prose`
/// switch, deliberately: a note body is catalog content written by whoever authored the catalog, and
/// an operator who does not trust those authors has already said so once.
fn push_body(lines: &mut Vec<String>, body: &str, prose: CatalogProse) {
    if !prose.is_quoted() {
        return;
    }
    let quoted = quote(body);
    if !quoted.is_empty() {
        lines.push(quoted);
    }
}

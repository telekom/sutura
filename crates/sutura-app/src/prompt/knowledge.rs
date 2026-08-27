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
//! **Three sentences per capability, not two, and the third is the one that used to be missing.**
//! `declared` and `absent` were not enough: the declaration told an agent that a glossary "is listed
//! below" and that worked questions are "at the end of this document" while [`glossary`] and
//! [`examples`] returned nothing at all when their collection was empty, so a provider that declared
//! all four and had recorded nothing produced a document pointing at two sections that were not
//! there. The distinction the whole capability mechanism exists to carry was implemented for
//! `Absences` alone, where `NOTHING_RECORDED` says the list is kept and empty. All four are held to
//! that now: the declaration line says "and nothing is recorded yet", and each kind with a section of
//! its own renders that section with the same sentence in it. Caveats have no section of their own by
//! design - they are printed inside the metric block they are about - so the declaration line is the
//! whole of it for that one, which is why the third sentence exists rather than three more headings.
//!
//! # The glossary renders from the STRUCTURE, not from the prose
//!
//! Every token of a glossary line is a phrase the domain parsed - one line, bounded, normalised, no
//! control characters - or a name the pinned bundle already declares. So the STRUCTURED half of every
//! section here cannot name a model, a table or a column: there is no
//! [`Referent`](sutura_domain::knowledge::Referent) variant that could hold one, which is a property
//! of the type rather than a review of each rendering.
//!
//! **The narrow claim is the true one, and it is worth stating at its width.** A `Phrase` and a
//! `NoteBody` are free text, both reach this document, and nothing mechanical stops an author writing
//! a column name into either - just as nothing stops one in a metric description, which is a channel
//! this repository already ships: the example catalog's own prose names `mrr_cents` and `status`, and
//! `sutura-cli`'s prompt snapshot records that it does. Note BODIES go through
//! [`quote`](super::quote) like a metric's description and ride the same `prompt.catalog_prose`
//! switch, so an operator who does not trust the catalog's authors has already said so once. There is
//! no new configuration key, and there is no load-time content scan - what bounds authored prose is
//! that a catalog is reviewed content whose digest moves when a word of it changes.
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
/// Three sentences per capability rather than one, because the interesting halves are the second and
/// the third: a deployment whose provider cannot record an absence must not be described in a way
/// that lets an agent infer anything from a term not being listed, and a provider that CAN and has
/// not yet must not be described as though the list were populated.
struct Claim {
    /// Rendered when the capability is declared and something is recorded.
    declared: &'static str,
    /// Rendered when it is declared and nothing is recorded yet. A kept list that is empty is a fact
    /// an agent may use; a section it was pointed at and that is not there is not.
    empty: &'static str,
    /// Rendered when it is not declared at all.
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
            empty: "**A glossary**, and nothing is recorded in it yet. This deployment keeps one, so the section below \
                    is empty rather than missing - match the user's words against the metric names and their \
                    descriptions, and say which one you chose and why.",
            absent: "**No glossary.** Nothing here maps a phrase to a metric, so match the user's words against the \
                     metric names and their descriptions, and say which one you chose and why.",
        },
        Capability::Caveats => Claim {
            declared: "**Caveats** - things to know before trusting a number. Each one is printed under the metric it is \
                       about and applies to every question about that metric.",
            empty: "**Caveats**, and none is recorded yet. This deployment keeps them, printed under the metric each \
                    one is about, and today no metric has one - which is not a statement that there are no traps.",
            absent: "**No caveats.** Nothing here records a trap in a definition, which is not a statement that there \
                     are none.",
        },
        Capability::Absences => Claim {
            declared: "**A reviewed list of terms that are deliberately NOT defined here**, above, after the refusal \
                       section. It is authoritative for what it names: a question about one of those terms is to be \
                       declined with the reason given, not approximated from the metrics that are defined.",
            empty: "**A reviewed list of terms that are deliberately NOT defined here**, above, after the refusal \
                    section, with nothing on it. Somebody keeps that list and it is empty, so there is no term you are \
                    being asked to decline on that ground.",
            absent: "**No record of what is deliberately undefined.** This deployment cannot tell you which terms were \
                     considered and rejected, so infer NOTHING from a term being absent from this document. The metric \
                     list is the only authority on what may be asked.",
        },
        Capability::Examples => Claim {
            declared: "**Worked questions** - real questions with the request that answers each, at the end of this \
                       document. They are the shape to copy.",
            empty: "**Worked questions**, and none is recorded yet. This deployment keeps them, at the end of this \
                    document, and today there are none - so compose from the metric list and the bounds above.",
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
        let text = if !knowledge.declares().declares(capability) {
            entry.absent
        } else if recorded(knowledge, capability) == 0 {
            entry.empty
        } else {
            entry.declared
        };
        lines.push(wrap("- ", text, "  "));
    }
    lines.join("\n")
}

/// How many notes of one kind this bundle carries.
///
/// A total match, so a fifth capability is a compile error here as well as in [`claim`]: what a
/// declaration may claim depends on whether anything is recorded, and a kind whose count nobody wrote
/// would be described as populated whatever it held.
fn recorded(knowledge: &Knowledge, capability: Capability) -> usize {
    match capability {
        Capability::Glossary => knowledge.glossary().len(),
        Capability::Caveats => knowledge.caveats().len(),
        Capability::Absences => knowledge.absences().len(),
        Capability::Examples => knowledge.examples().len(),
    }
}

/// One section heading, and the sentence a kept-and-empty collection gets instead of a list.
///
/// The shape all three sections with a heading share, so "declared and empty says so" cannot be true
/// of one of them and false of the next - which is how it came to be implemented for the absences
/// alone. `None` means the capability is not declared and the section is not rendered at all.
fn section(heading: &str, nothing: &str, declared: bool, is_empty: bool) -> Option<Vec<String>> {
    if !declared {
        return None;
    }
    let mut lines = vec![format!("{heading}\n")];
    if is_empty {
        lines.push(wrap("", nothing, ""));
    }
    Some(lines)
}

/// The glossary, rendered from the structure.
pub(super) fn glossary(knowledge: &Knowledge, prose: CatalogProse) -> String {
    let Some(mut lines) = section(
        GLOSSARY_HEADING,
        NO_GLOSSARY_RECORDED,
        knowledge.supports(Capability::Glossary),
        knowledge.glossary().is_empty(),
    ) else {
        return String::new();
    };
    if knowledge.glossary().is_empty() {
        return lines.join("\n");
    }
    lines.push(wrap("", GLOSSARY_INTRO, ""));
    lines.push(String::new());
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

const GLOSSARY_HEADING: &str = "## The words a question may arrive in";

const GLOSSARY_INTRO: &str = "Each line is a phrase somebody may use, and the one thing this deployment takes it to mean. \
                              **You do the resolving.** There is no field on a question that a phrase fits in - a request \
                              carries a metric name - so a wrong turn here is yours to catch rather than something the \
                              service will decline. Say which phrase you resolved and what you resolved it to, so the \
                              person can correct you.";

const NO_GLOSSARY_RECORDED: &str = "None. This deployment keeps a glossary and there is nothing in it yet, so no phrase \
                                    below this line has been given a meaning here. Match the user's words against the \
                                    metric names and their descriptions, and say which one you chose and why.";

/// The terms recorded as deliberately undefined.
///
/// Three renderings, and the difference between the second and the third is the whole reason the
/// capability is declared rather than inferred: an empty list somebody keeps is a fact, and an empty
/// list nobody keeps is nothing at all.
pub(super) fn not_defined(knowledge: &Knowledge, prose: CatalogProse) -> String {
    let Some(mut lines) = section(
        NOT_DEFINED_HEADING,
        NOTHING_RECORDED,
        knowledge.supports(Capability::Absences),
        knowledge.absences().is_empty(),
    ) else {
        return String::new();
    };
    if knowledge.absences().is_empty() {
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

const NOT_DEFINED_HEADING: &str = "## Terms this deployment records as NOT defined";

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
/// with no caveats. **The one kind with no section of its own**, deliberately: a caveat belongs beside
/// the question it is about. So "declared and empty" is said by [`declaration`] for this kind rather
/// than by a heading over nothing, which is what the third sentence of a [`Claim`] is for.
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
    let Some(mut lines) = section(
        EXAMPLES_HEADING,
        NO_EXAMPLES_RECORDED,
        knowledge.supports(Capability::Examples),
        knowledge.examples().is_empty(),
    ) else {
        return String::new();
    };
    if knowledge.examples().is_empty() {
        return lines.join("\n");
    }
    lines.push(wrap("", EXAMPLES_INTRO, ""));
    for note in knowledge.examples().values() {
        lines.push(String::new());
        lines.push(format!("### {}\n", note.name()));
        lines.push(wrap("- ", &format!("Asked as: {}", phrases(note.asked().iter())), "  "));
        lines.push(wrap("- ", &format!("Send: {}", request(note.question())), "  "));
        push_body(&mut lines, note.body().as_str(), prose);
    }
    lines.join("\n")
}

const EXAMPLES_HEADING: &str = "## Worked questions";

const EXAMPLES_INTRO: &str = "Questions somebody actually asked, with the request that answers each. Copy the shape. \
                              Every value in them is one this snapshot declares, and every one of them is inside the \
                              bounds above - the period, the group-by count - because a bundle carrying one that is not \
                              does not load. So a question below is one this deployment answers rather than one it \
                              would decline.";

const NO_EXAMPLES_RECORDED: &str = "None. This deployment keeps worked questions and none is recorded yet, so there is \
                                    no shape here to copy - compose from the metric list and the bounds above.";

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

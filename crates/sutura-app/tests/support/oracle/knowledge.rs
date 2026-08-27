//! The oracle's other half: what the catalog says ABOUT what it defines, written out in Rust by hand.
//!
//! Split out of `oracle.rs` for the reason that file was split out of `support/mod.rs`: a hand-written
//! catalog is a list of literals, it grows with the corpus, and `cargo xtask max-lines` fails at a
//! thousand lines under `crates/` with no exemption available.
//!
//! # Transcribed from the PROSE, reconciled with the frontmatter afterwards
//!
//! Same procedure as the metrics, and it matters here for the same reason: **the failure mode of this
//! file is a GREEN test that proves nothing.** Copying each document's frontmatter into Rust produces
//! an oracle that agrees by transcription - it shares whatever misreading the copying carried, and the
//! comparison then reports success over two statements of one mistake.
//!
//! So every note below was written from what its document SAYS - "a subscriber count is a SUBSCRIPTION
//! count and never a customer count", "the value is `business`, in the singular and in lower case",
//! "neither of these can be broken down at all" - and each document's frontmatter was read last, as a
//! check on the transcription rather than as its source. The one-line summary above each note is the
//! sentence it came from.
//!
//! Where the two disagree, **the disagreement is the finding**, and the fix is in the document.
//!
//! # Bodies are not restated, and that is the same rule the descriptions follow
//!
//! Prose lives in the markdown and nowhere else, so [`without_bodies`] blanks every body on both sides
//! before they are compared. What IS compared is everything that decides what the prompt says: which
//! capabilities are declared, which phrases resolve to which referent, which metric each caveat is
//! attached to, which terms are recorded as undefined, and the exact question in each worked example.
//! A `NoteBody` cannot be empty, so "blanked" means one fixed placeholder rather than nothing - the
//! same trick as the empty `String` the descriptions use, adjusted for a type with an invariant.
//!
//! # Every phrase here is ASCII, and that is a lint rather than a design decision
//!
//! The glossary is bilingual on purpose - resolving "monatlicher Umsatz" to `recurring_revenue` is the
//! case it exists for - but the workspace denies `clippy::non_ascii_literal`, so a German phrase with
//! an umlaut cannot be written as a literal in this file. The catalog documents are therefore written
//! with ASCII-only German too, because a phrase this oracle cannot state is a phrase the comparison
//! cannot check. `sutura_domain::knowledge::Phrase` itself permits non-ASCII and has a test that says
//! so, built from a code point.

use std::collections::BTreeSet;

use sutura_domain::catalog::{Definitions, DimensionValue};
use sutura_domain::knowledge::{
    Absence, Capability, Caveat, Example, GlossaryEntry, Knowledge, KnowledgeCapabilities, KnowledgeInput, NoteBody, NoteName,
    Phrase, Referent,
};
use sutura_domain::model::{DimensionName, Grain, MetricName};
use sutura_domain::query::{Filter, Query};

use super::june;

/// What a body is replaced by on both sides of the comparison.
///
/// Not the empty string, because a body cannot be one: `NoteBody::parse` refuses it, deliberately, so
/// a note with a heading and no reason under it cannot load. This is the placeholder that plays the
/// same role.
const PLACEHOLDER: &str = "Prose lives in the markdown.";

fn phrase(raw: &str) -> Phrase {
    Phrase::parse(raw).expect("a corpus phrase is a phrase")
}

fn phrases(raw: &[&str]) -> BTreeSet<Phrase> {
    raw.iter().map(|word| phrase(word)).collect()
}

fn note_name(raw: &str) -> NoteName {
    NoteName::parse(raw).expect("a corpus note name is a name")
}

fn metric(raw: &str) -> MetricName {
    MetricName::parse(raw).expect("a corpus metric is a metric")
}

fn dimension(raw: &str) -> DimensionName {
    DimensionName::parse(raw).expect("a corpus dimension is a dimension")
}

fn body() -> NoteBody {
    NoteBody::parse(PLACEHOLDER).expect("the placeholder is a body")
}

/// The whole metric.
fn about(raw: &str) -> Referent {
    Referent::Metric { metric: metric(raw) }
}

/// One of a metric's dimensions.
fn about_dimension(raw: &str, name: &str) -> Referent {
    Referent::Dimension {
        metric: metric(raw),
        dimension: dimension(name),
    }
}

/// One declared value of one of a metric's dimensions.
fn about_value(raw: &str, name: &str, value: &str) -> Referent {
    Referent::Value {
        metric: metric(raw),
        dimension: dimension(name),
        value: DimensionValue::parse(value).expect("a corpus value is a value"),
    }
}

fn entry(term: &str, synonyms: &[&str], means: Referent) -> GlossaryEntry {
    GlossaryEntry::new(phrase(term), phrases(synonyms), means, body())
}

fn caveat(name: &str, about: Vec<Referent>) -> Caveat {
    Caveat::new(note_name(name), about, body())
}

fn absence(term: &str, synonyms: &[&str]) -> Absence {
    Absence::new(phrase(term), phrases(synonyms), body())
}

fn worked(name: &str, asked: &[&str], question: Query) -> Example {
    Example::new(
        note_name(name),
        asked.iter().map(|word| phrase(word)).collect(),
        question,
        body(),
    )
}

// ------------------------------------------------------------------------------- the glossary ---

/// The nine glossary entries.
///
/// **Five of them resolve to a metric and four to something inside one**, which is the split worth
/// keeping: a phrase that means a whole metric is the easy case, and a phrase that means one declared
/// VALUE is the case that produces a wrong refusal rather than a wrong number when the catalog and the
/// allowlist disagree.
fn glossary() -> Vec<GlossaryEntry> {
    vec![
        // "always the ACTIVE figure" - the metric whose definitional filter is part of its name.
        entry(
            "monthly recurring revenue",
            &["MRR", "recurring revenue", "monatlicher Umsatz", "wiederkehrender Umsatz"],
            about("recurring_revenue"),
        ),
        // "the share of a month's subscriptions that terminated inside it" - the share, not the count.
        entry(
            "churn rate",
            &["Abwanderungsrate", "cancellation rate", "attrition rate"],
            about("churn_rate"),
        ),
        // "How many subscriptions were active at the end of the month" - the filtered count, not the
        // base. `Anschluss` and `Vertrag` are the German words for one subscription.
        entry(
            "subscriber count",
            &["subscribers", "active lines", "Anschluss", "Vertrag"],
            about("active_subscriptions"),
        ),
        // "the U is a USER, and this catalog has two candidates" - this one is per CUSTOMER.
        entry(
            "average revenue per customer",
            &["ARPU", "revenue per customer", "Umsatz pro Kunde"],
            about("revenue_per_customer"),
        ),
        // "per subscription that USED the network" - the ratio, not the raw volume.
        entry(
            "data usage",
            &["data volume", "Datenvolumen", "gigabytes per subscription"],
            about("data_per_subscription"),
        ),
        // "the value is `business`, in the singular and in lower case" - a VALUE referent, and the
        // spelling is the whole reason the entry exists.
        entry(
            "business customers",
            &["B2B", "Firmenkunden", "enterprise customers"],
            about_value("recurring_revenue", "segment", "business"),
        ),
        // "Private households, as opposed to companies. The value is `consumer`."
        entry(
            "consumer customers",
            &["B2C", "Privatkunden", "residential customers"],
            about_value("recurring_revenue", "segment", "consumer"),
        ),
        // "the individual product ... by its own name rather than by its kind", and the dimension it
        // names is the one that can be grouped by and not filtered.
        entry(
            "tariff",
            &["Tarif", "plan", "product name"],
            about_dimension("recurring_revenue", "product_name"),
        ),
        // "rolling month-to-month arrangement or a committed one" - the dimension, not either value.
        entry(
            "contract term",
            &["Vertragslaufzeit", "commitment", "term"],
            about_dimension("recurring_revenue", "contract_term"),
        ),
    ]
}

// -------------------------------------------------------------------------------- the caveats ---

/// The five caveats, each scoped to every metric its prose is about.
///
/// The scope is the part to transcribe carefully: a caveat attached to three of the four metrics it
/// warns about is a warning that does not appear on the fourth, and nothing but this comparison would
/// say so.
fn caveats() -> Vec<Caveat> {
    vec![
        // "Every one of these counts SUBSCRIPTIONS" - the four counting metrics, and it names all
        // four in its own first line.
        caveat(
            "subscriber_is_not_customer",
            vec![
                about("active_subscriptions"),
                about("subscription_base"),
                about("subscription_months_billed"),
                about("subscriptions_churned"),
            ],
        ),
        // "an event on a SUBSCRIPTION inside a month" - the rate and the count of the same event.
        caveat(
            "churn_is_subscription_level",
            vec![about("churn_rate"), about("subscriptions_churned")],
        ),
        // "Neither of these can be broken down at all" - the two metrics on the usage model.
        caveat(
            "usage_reaches_no_snapshot",
            vec![about("data_per_subscription"), about("voice_minutes")],
        ),
        // "These two both sound like 'average revenue' and they average different things."
        caveat(
            "two_averages_of_the_same_revenue",
            vec![about("mean_subscription_mrr"), about("revenue_per_customer")],
        ),
        // "Every revenue figure in this catalog is in MINOR UNITS" - all four that report money.
        caveat(
            "revenue_is_in_minor_units",
            vec![
                about("recurring_revenue"),
                about("mean_subscription_mrr"),
                about("revenue_per_customer"),
                about("revenue_per_churned_subscription"),
            ],
        ),
    ]
}

// ------------------------------------------------------------------------------- the absences ---

/// The three terms recorded as deliberately undefined.
///
/// None of them names a metric this catalog defines, and that is not a coincidence to be relied on:
/// `Knowledge::assemble` refuses an absence whose phrase is the name of a defined metric, which is
/// what stops this list becoming a lie the day somebody certifies one of them.
fn absences() -> Vec<Absence> {
    vec![
        // "needs three things this catalog does not hold: an expected lifetime, a discount rate, and
        // a decision about which costs to net off".
        absence("customer lifetime value", &["CLV", "CLTV", "Kundenwert"]),
        // "Churn in this catalog is subscription-level. Customer-level churn has no definition here."
        absence(
            "customer churn",
            &["customer churn rate", "Kundenabwanderung", "customer attrition"],
        ),
        // "Nothing here forecasts anything."
        absence("revenue forecast", &["forecast", "Umsatzprognose", "projected revenue"]),
    ]
}

// ------------------------------------------------------------------------------- the examples ---

/// The three worked questions.
///
/// The questions themselves are the part that has to be exact: an example is what an agent copies, so
/// a grain or a filter value transcribed loosely here would be a difference from the document that
/// this comparison is the only thing able to catch.
fn examples() -> Vec<Example> {
    let half_year = sutura_domain::calendar::TimeRange::new(
        sutura_domain::calendar::Date::parse("2026-01-01").expect("a corpus date is a date"),
        sutura_domain::calendar::Date::parse("2026-07-01").expect("a corpus date is a date"),
    )
    .expect("the first half of 2026 is a range");
    vec![
        // "one metric, one period with both ends given, one dimension to break it down by".
        worked(
            "revenue_by_segment_in_june",
            &[
                "how much recurring revenue did each segment bring in June",
                "wie viel wiederkehrender Umsatz kam im Juni pro Segment",
            ],
            Query::new(
                metric("recurring_revenue"),
                Grain::Month,
                june(),
                vec![dimension("segment")],
                Vec::new(),
            ),
        ),
        // "one question covering the whole trend, not six questions covering a month each", and
        // "Nothing to group by here."
        worked(
            "churn_rate_over_the_half_year",
            &[
                "how has the churn rate developed this year",
                "wie hat sich die Abwanderungsrate entwickelt",
            ],
            Query::new(metric("churn_rate"), Grain::Month, half_year, Vec::new(), Vec::new()),
        ),
        // "Two filters and nothing to group by: this asks for one number rather than a breakdown."
        // The order of the two is the document's own, and it is content: filters are a list.
        worked(
            "business_revenue_in_the_north",
            &[
                "what are business customers in the north worth per month",
                "Umsatz der Firmenkunden im Norden",
            ],
            Query::new(
                metric("recurring_revenue"),
                Grain::Month,
                june(),
                Vec::new(),
                vec![
                    Filter::new(
                        dimension("segment"),
                        DimensionValue::parse("business").expect("a corpus value is a value"),
                    ),
                    Filter::new(
                        dimension("region"),
                        DimensionValue::parse("north").expect("a corpus value is a value"),
                    ),
                ],
            ),
        ),
    ]
}

/// Everything above, checked against the definitions it is about.
///
/// **Declares every capability**, which is what the markdown adapter declares and for the same
/// reason: a reviewed first-party catalog can carry all four kinds, so a kind with nothing in it is an
/// empty list rather than a concept the provider does not have.
pub(super) fn stated(definitions: &Definitions) -> Knowledge {
    Knowledge::assemble(
        definitions,
        KnowledgeInput::new(KnowledgeCapabilities::all(), glossary(), caveats(), absences(), examples()),
    )
    .expect("the hand-written knowledge holds together with the hand-written definitions")
}

/// The same notes with every body replaced by one placeholder.
///
/// The comparison the oracle actually makes, and the counterpart of `without_descriptions` next door:
/// prose lives in the markdown and nowhere else, so comparing it would be comparing one
/// implementation against a copy of itself. Everything that decides what the prompt SAYS is compared -
/// the declaration, the phrases, the referents, the recorded absences and the questions.
pub(super) fn without_bodies(definitions: &Definitions, knowledge: &Knowledge) -> Knowledge {
    let glossary = knowledge
        .glossary()
        .values()
        .map(|note| GlossaryEntry::new(note.term().clone(), note.synonyms().clone(), note.means().clone(), body()))
        .collect();
    let caveats = knowledge
        .caveats()
        .values()
        .map(|note| Caveat::new(note.name().clone(), note.about().to_vec(), body()))
        .collect();
    let absences = knowledge
        .absences()
        .values()
        .map(|note| Absence::new(note.phrase().clone(), note.synonyms().clone(), body()))
        .collect();
    let examples = knowledge
        .examples()
        .values()
        .map(|note| Example::new(note.name().clone(), note.asked().to_vec(), note.question().clone(), body()))
        .collect();
    // The declaration is carried over rather than replaced by `all()`, because it is one of the things
    // being compared: a provider that stopped declaring a capability has changed what the prompt may
    // claim, and blanking it here would be blanking the finding.
    let declares = KnowledgeCapabilities::of(Capability::every().filter(|capability| knowledge.supports(*capability)));
    Knowledge::assemble(
        definitions,
        KnowledgeInput::new(declares, glossary, caveats, absences, examples),
    )
    .expect("blanking prose cannot break consistency")
}

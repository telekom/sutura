//! Every way a set of notes can fail to hold together, provoked once each.
//!
//! A check nobody has seen fire is a check nobody knows works, and these are checks that run at load
//! and never again - so the only evidence they do anything is a test that reaches each one.
//!
//! The fixture bundle is deliberately small and deliberately awkward in two places: `segment` is
//! filterable and `product_name` is not, so the difference between "not declared" and "declares no
//! values" is reachable; and `voice_minutes` declares two grains where `recurring_revenue` declares
//! one, so a grain an example gets wrong is a grain that exists somewhere in the same bundle.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Absence, Capability, Caveat, Example, GlossaryEntry, InconsistentKnowledge, InvalidNoteBody, InvalidPhrase, InvalidReferent,
    Knowledge, KnowledgeCapabilities, KnowledgeInput, MAX_NOTE_BODY_BYTES, MAX_NOTE_LINES, MAX_PHRASE_CHARS, NoteBody, NoteName,
    Phrase, Referent,
};
use crate::calendar::{Date, TimeRange};
use crate::catalog::{Definitions, Description, Dimension, DimensionValue, Metric, Model};
use crate::measure::{AggregatedColumn, Measure, Term};
use crate::model::{
    Aggregate, ColumnName, DimensionName, Grain, InvalidIdentifier, MetricName, ModelName, SourceName, TableName,
};
use crate::query::{Filter, Query};

// ------------------------------------------------------------------------------- the fixture ---

pub(super) fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

pub(super) fn metric_name(raw: &str) -> MetricName {
    MetricName::parse(raw).expect("a test metric is a metric")
}

pub(super) fn dimension_name(raw: &str) -> DimensionName {
    DimensionName::parse(raw).expect("a test dimension is a dimension")
}

pub(super) fn note_name(raw: &str) -> NoteName {
    NoteName::parse(raw).expect("a test note name is a name")
}

pub(super) fn declared_value(raw: &str) -> DimensionValue {
    DimensionValue::parse(raw).expect("a test value is a value")
}

pub(super) fn phrase(raw: &str) -> Phrase {
    Phrase::parse(raw).expect("a test phrase is a phrase")
}

pub(super) fn body() -> NoteBody {
    NoteBody::parse("Why this note exists.").expect("a short body is a body")
}

pub(super) fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range")
}

/// Two metrics: one with a filterable dimension and a group-by-only one, one with two grains.
pub(super) fn definitions() -> Definitions {
    let model = Model::new(
        ModelName::parse("subscriptions").expect("a test model is a model"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("fct_subscription_monthly").expect("a test table is a table"),
        BTreeSet::from([
            column("month"),
            column("mrr_cents"),
            column("segment"),
            column("product_name"),
        ]),
        Description::default(),
    );
    let segment = Dimension::new(
        dimension_name("segment"),
        column("segment"),
        None,
        Some(BTreeSet::from([declared_value("business"), declared_value("consumer")])),
        Description::default(),
    );
    let product_name = Dimension::new(
        dimension_name("product_name"),
        column("product_name"),
        None,
        None,
        Description::default(),
    );
    let revenue = Metric::new(
        metric_name("recurring_revenue"),
        ModelName::parse("subscriptions").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_cents")))),
        Vec::new(),
        column("month"),
        BTreeSet::from([Grain::Month]),
        BTreeMap::from([
            (dimension_name("segment"), segment),
            (dimension_name("product_name"), product_name),
        ]),
        None,
        Description::default(),
    );
    let minutes = Metric::new(
        metric_name("voice_minutes"),
        ModelName::parse("subscriptions").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_cents")))),
        Vec::new(),
        column("month"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::new(),
        None,
        Description::default(),
    );
    Definitions::assemble(vec![model], vec![], vec![revenue, minutes]).expect("the test bundle is consistent")
}

/// The referent every well-formed note in this file points at.
pub(super) fn revenue() -> Referent {
    Referent::Metric {
        metric: metric_name("recurring_revenue"),
    }
}

pub(super) fn glossary_entry(term: &str, synonyms: &[&str], means: Referent) -> GlossaryEntry {
    GlossaryEntry::new(phrase(term), synonyms.iter().map(|s| phrase(s)).collect(), means, body())
}

pub(super) fn absence(term: &str, synonyms: &[&str]) -> Absence {
    Absence::new(phrase(term), synonyms.iter().map(|s| phrase(s)).collect(), body())
}

pub(super) fn caveat(name: &str, about: Vec<Referent>) -> Caveat {
    Caveat::new(note_name(name), about, body())
}

pub(super) fn example(name: &str, question: Query) -> Example {
    Example::new(note_name(name), vec![phrase("how much revenue")], question, body())
}

pub(super) fn question(grain: Grain, dimensions: Vec<DimensionName>, filters: Vec<Filter>) -> Query {
    Query::new(metric_name("recurring_revenue"), grain, june(), dimensions, filters)
}

/// The one input shape every failing test varies one field of.
///
/// Each declares only the capability it carries content for, which is both what a real adapter with
/// that one capability would do and what keeps these tests from passing because everything is always
/// declared.
pub(super) fn only_glossary(entries: Vec<GlossaryEntry>) -> KnowledgeInput {
    KnowledgeInput::new(
        KnowledgeCapabilities::of([Capability::Glossary]),
        entries,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

pub(super) fn only_caveats(notes: Vec<Caveat>) -> KnowledgeInput {
    KnowledgeInput::new(
        KnowledgeCapabilities::of([Capability::Caveats]),
        Vec::new(),
        notes,
        Vec::new(),
        Vec::new(),
    )
}

pub(super) fn only_absences(notes: Vec<Absence>) -> KnowledgeInput {
    KnowledgeInput::new(
        KnowledgeCapabilities::of([Capability::Absences]),
        Vec::new(),
        Vec::new(),
        notes,
        Vec::new(),
    )
}

pub(super) fn only_examples(notes: Vec<Example>) -> KnowledgeInput {
    KnowledgeInput::new(
        KnowledgeCapabilities::of([Capability::Examples]),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        notes,
    )
}

pub(super) fn refuses(input: KnowledgeInput) -> InconsistentKnowledge {
    Knowledge::assemble(&definitions(), input).expect_err("this set of notes must not load")
}

pub(super) fn accepts(input: KnowledgeInput) -> Knowledge {
    Knowledge::assemble(&definitions(), input).expect("this set of notes is consistent")
}

// ------------------------------------------------------------------------------ the newtypes ---

#[test]
fn a_phrase_is_prose_and_not_an_identifier() {
    // The whole reason this is not a sixth `identifier_newtype`: the values it exists to hold are
    // exactly the ones an identifier parser refuses.
    assert_eq!(phrase("monthly recurring revenue").as_str(), "monthly recurring revenue");
    assert_eq!(phrase("  B2B customers\n").as_str(), "B2B customers");
    assert_eq!(phrase("Umsatz pro Kunde").to_string(), "Umsatz pro Kunde");
}

#[test]
fn a_phrase_may_carry_a_letter_that_is_not_ascii() {
    // The bilingual case is the point of the glossary, so the type has to allow it. Built from a
    // code point rather than written as a literal because the workspace denies a non-ASCII literal
    // in source - which is a fact about this repository's lint table and not about the type.
    let umlaut = char::from_u32(0x00fc).expect("U+00FC is a character");
    let word = format!("K{umlaut}ndigungsrate");
    let parsed = Phrase::parse(&word).expect("a German word is a phrase");
    assert_eq!(parsed.as_str(), word);
    assert!(!parsed.as_str().is_ascii());
}

#[test]
fn a_phrase_is_one_line_and_bounded() {
    assert_eq!(Phrase::parse("").unwrap_err(), InvalidPhrase::Empty);
    assert_eq!(Phrase::parse("   ").unwrap_err(), InvalidPhrase::Empty);
    // The failure this prevents: the prompt renders a phrase inline, so a newline inside one writes
    // a line of that document at a column the renderer did not choose.
    assert_eq!(
        Phrase::parse("churn\n# SYSTEM").unwrap_err(),
        InvalidPhrase::ControlCharacter {
            value: String::from("churn\n# SYSTEM"),
        }
    );
    let long = "a".repeat(MAX_PHRASE_CHARS.saturating_add(1));
    assert_eq!(
        Phrase::parse(&long).unwrap_err(),
        InvalidPhrase::TooLong {
            value: long.clone(),
            len: long.chars().count(),
            limit: MAX_PHRASE_CHARS,
        }
    );
    drop(Phrase::parse("a".repeat(MAX_PHRASE_CHARS)).expect("exactly the limit is a phrase"));
}

#[test]
fn a_phrase_is_normalised_so_that_two_of_them_cannot_look_identical() {
    // What `parse` STORES is what a reader sees. Without this, a run of spaces and an invisible code
    // point were each a second spelling of a phrase the glossary already had - and the phrase index
    // is a map, so two entries a reader cannot tell apart both loaded and both rendered.
    assert_eq!(phrase("monthly   recurring  revenue").as_str(), "monthly recurring revenue");
    // Built from code points rather than written as literals: the workspace denies a non-ASCII
    // literal in source, which is a fact about its lint table and not about the type.
    let zero_width = char::from_u32(0x200b).expect("U+200B is a character");
    assert_eq!(phrase(&format!("m{zero_width}rr")).as_str(), "mrr");
    let right_to_left_override = char::from_u32(0x202e).expect("U+202E is a character");
    assert_eq!(phrase(&format!("{right_to_left_override}revenue")).as_str(), "revenue");
    // A phrase of nothing but invisible characters is a phrase of nothing, which the emptiness
    // refusal now reaches.
    assert_eq!(
        Phrase::parse(format!("{zero_width}{zero_width}")).unwrap_err(),
        InvalidPhrase::Empty
    );
    // FINDING. Every code point in the shared set, at both ends of every range, normalises AWAY -
    // so a phrase holding one is the phrase without it and cannot be a second entry in any index
    // keyed on `phrase_identity`. The set this type used to use was a narrower copy of the one
    // authored SQL is held to, and `U+00AD`, `U+2060..2064` and `U+FFF9..FFFB` survived it: a soft
    // hyphen draws nothing until a line breaks at it and a word joiner draws nothing ever, so
    // `mrr` and `m<soft hyphen>rr` were two keys for one word a reader sees.
    for code in [
        0x00AD_u32, 0x200B, 0x200F, 0x202A, 0x202E, 0x2060, 0x2064, 0x2066, 0x2069, 0xFEFF, 0xFFF9, 0xFFFB,
    ] {
        let offending = char::from_u32(code).expect("a listed code point is a character");
        assert_eq!(phrase(&format!("m{offending}rr")).as_str(), "mrr", "{code:#06x}");
        assert_eq!(
            super::phrase_identity(&phrase(&format!("m{offending}rr"))),
            super::phrase_identity(&phrase("mrr")),
            "{code:#06x}"
        );
    }
    // And the neighbour of each bound is KEPT, so the normalisation is the set written down rather
    // than a sweep that would silently edit a phrase somebody wrote on purpose.
    for code in [0x00AC_u32, 0x00AE, 0x2010, 0x2065, 0x206A, 0xFEFE, 0xFF00, 0xFFFC] {
        let benign = char::from_u32(code).expect("a listed code point is a character");
        let written = format!("m{benign}rr");
        assert_eq!(phrase(&written).as_str(), written, "{code:#06x}");
    }
    // Case is NOT folded here: it is meaning, and the prompt renders the spelling an author chose.
    // What folds case is `phrase_identity`, which is what every index in this module is keyed on -
    // so "MRR" and "mrr" are one phrase for the purpose of noticing that two documents disagree and
    // two phrases for the purpose of rendering them.
    assert_eq!(phrase("MRR").as_str(), "MRR");
    assert_eq!(super::phrase_identity(&phrase("MRR")), "mrr");
    assert_eq!(
        super::phrase_identity(&phrase("Monthly  Recurring Revenue")),
        "monthly recurring revenue"
    );
}

#[test]
fn a_phrase_with_no_letter_and_no_digit_is_not_a_phrase() {
    // What a YAML scalar that is not text arrives as: a null reaches this constructor as the one
    // character it prints as, and without this clause the prompt renders a glossary entry for a
    // tilde. The placeholders somebody meant to fill in later go the same way.
    for raw in ["~", "--", "...", "?!"] {
        assert_eq!(
            Phrase::parse(raw).unwrap_err(),
            InvalidPhrase::NotAWord {
                value: String::from(raw)
            },
            "{raw:?} carries no word"
        );
    }
    // A digit on its own is a word, though: "5G" is a phrase somebody says.
    assert_eq!(phrase("5G").as_str(), "5G");
}

#[test]
fn a_note_body_is_refused_over_the_cap_rather_than_cut_to_fit() {
    // The decision this asserts: over the cap the DOCUMENT does not load. Truncating instead would
    // make the rendered prompt say something the author did not write, about a bundle whose digest
    // certifies the text as it stands - and nothing downstream could tell.
    assert_eq!(NoteBody::parse("  \n ").unwrap_err(), InvalidNoteBody::Empty);
    let long = "x".repeat(MAX_NOTE_BODY_BYTES.saturating_add(1));
    assert_eq!(
        NoteBody::parse(&long).unwrap_err(),
        InvalidNoteBody::TooLong {
            len: long.len(),
            limit: MAX_NOTE_BODY_BYTES,
        }
    );
    // The line cap is a separate bound rather than a proxy for the byte one: this body is a tenth of
    // the byte budget and would render as a thousand lines of a document.
    let tall = "a\n".repeat(MAX_NOTE_LINES.saturating_add(1));
    assert_eq!(
        NoteBody::parse(&tall).unwrap_err(),
        InvalidNoteBody::TooManyLines {
            len: MAX_NOTE_LINES.saturating_add(1),
            limit: MAX_NOTE_LINES,
        }
    );
    let kept = NoteBody::parse("  Two lines.\nBoth of them.  ").expect("a body is a body");
    assert_eq!(kept.as_str(), "Two lines.\nBoth of them.");
}

#[test]
fn a_note_body_with_an_invisible_character_mixed_into_prose_is_refused() {
    // FINDING. The check this file already had covered a body made ENTIRELY of these characters -
    // that one is `Empty`, and still is. What it did not cover, and what a note body is actually FOR,
    // is one of them in the middle of a sentence: the body reads as `status = 'active'` in every
    // terminal, every diff and every pull request, says something else, and reaches an agent's
    // context under a digest taken over text nobody read. Trojan Source, CVE-2021-42574, with the
    // authored SQL fragment replaced by the paragraph next to it.
    let right_to_left_override = char::from_u32(0x202e).expect("U+202E is a character");
    assert_eq!(
        NoteBody::parse(format!(
            "Counts rows where status = '{right_to_left_override}evitca', which is the certified rule."
        )),
        Err(InvalidNoteBody::InvisibleCharacter { code: 0x202E })
    );
    // The narrower set this type used to be held to did not include the soft hyphen, and a soft
    // hyphen draws nothing until a line happens to break at it.
    let soft_hyphen = char::from_u32(0x00ad).expect("U+00AD is a character");
    assert_eq!(
        NoteBody::parse(format!("Excludes the m{soft_hyphen}rr of cancelled subscriptions.")),
        Err(InvalidNoteBody::InvisibleCharacter { code: 0x00AD })
    );
    // Both ends of every range, in prose, so a typo in one of the bounds fails here.
    for code in [
        0x00AD_u32, 0x200B, 0x200F, 0x202A, 0x202E, 0x2060, 0x2064, 0x2066, 0x2069, 0xFEFF, 0xFFF9, 0xFFFB,
    ] {
        let offending = char::from_u32(code).expect("a listed code point is a character");
        assert_eq!(
            NoteBody::parse(format!("Revenue{offending} recognised monthly.")),
            Err(InvalidNoteBody::InvisibleCharacter { code }),
            "{code:#06x}"
        );
    }
    // And the neighbour of each bound loads, so this is the set written down rather than a sweep
    // that would refuse a reviewed paragraph for a character that draws.
    for code in [
        0x00AC_u32, 0x00AE, 0x200A, 0x2010, 0x2029, 0x202F, 0x205F, 0x2065, 0x206A, 0xFEFE, 0xFF00, 0xFFFC,
    ] {
        let benign = char::from_u32(code).expect("a listed code point is a character");
        let body = format!("Revenue{benign} recognised monthly.");
        assert_eq!(
            NoteBody::parse(&body)
                .expect("a body of ordinary characters is a body")
                .as_str(),
            body,
            "{code:#06x}"
        );
    }
    // Emptiness still wins over this refusal, and the order is the point: a body of nothing but
    // these characters is best described as the blank note it is.
    let zero_width = char::from_u32(0x200b).expect("U+200B is a character");
    assert_eq!(
        NoteBody::parse(format!("{zero_width}{zero_width}")),
        Err(InvalidNoteBody::Empty)
    );
}

/// Deserialize without a format crate: the boundary gate allowlists none for this crate, and this
/// tests the wiring rather than a YAML parser.
fn deserialize_phrase(raw: &str) -> Result<Phrase, serde::de::value::Error> {
    use serde::Deserialize as _;
    use serde::de::IntoDeserializer as _;

    Phrase::deserialize(String::from(raw).into_deserializer())
}

fn deserialize_body(raw: &str) -> Result<NoteBody, serde::de::value::Error> {
    use serde::Deserialize as _;
    use serde::de::IntoDeserializer as _;

    NoteBody::deserialize(String::from(raw).into_deserializer())
}

#[test]
fn deserialization_goes_through_the_constructors() {
    // The hole a derived `Deserialize` leaves open, and the one path that carries authored input: a
    // catalog document. Without `try_from` the derive writes into the private field and every check
    // above is decoration.
    drop(deserialize_phrase("two\nlines").expect_err("a newline must not deserialize into a phrase"));
    drop(deserialize_phrase("").expect_err("empty must not deserialize into a phrase"));
    assert_eq!(
        deserialize_phrase("recurring revenue")
            .expect("a real phrase still deserializes")
            .as_str(),
        "recurring revenue"
    );
    drop(
        deserialize_body(&"x".repeat(MAX_NOTE_BODY_BYTES.saturating_add(1))).expect_err("an oversized body must not deserialize"),
    );
    assert_eq!(
        deserialize_body("Prose.").expect("a real body still deserializes").as_str(),
        "Prose."
    );
}

#[test]
fn a_note_name_shares_the_one_identifier_parser() {
    // The macro is `crate::model`'s, and this is what says so: the same input produces the same
    // error as it does for a metric name, so there is no second parser to drift.
    assert_eq!(note_name("usage_join_grain").as_str(), "usage_join_grain");
    let expected = InvalidIdentifier::IllegalCharacter {
        value: String::from("a b"),
        offending: ' ',
    };
    assert_eq!(NoteName::parse("a b").unwrap_err(), expected);
    assert_eq!(MetricName::parse("a b").unwrap_err(), expected);
}

#[test]
fn a_referent_names_a_metric_and_never_a_column() {
    // The absence is the property: there is no variant to put a model, a table or a column in, so
    // the prompt sections rendered from this type cannot leak one whatever an author writes.
    let value = Referent::Value {
        metric: metric_name("recurring_revenue"),
        dimension: dimension_name("segment"),
        value: declared_value("business"),
    };
    assert_eq!(value.metric(), &metric_name("recurring_revenue"));
    assert_eq!(value.dimension(), Some(&dimension_name("segment")));
    assert_eq!(value.value(), Some(&declared_value("business")));
    assert_eq!(revenue().dimension(), None);
    assert_eq!(revenue().value(), None);
}

/// One on-disk referent, as a value rather than as text.
///
/// The representation is private to the parent module, which a child module can still reach - so the
/// conversion is testable here without a format parser. What is NOT testable here is the format
/// itself: `Option` fields need a self-describing deserializer, and `serde_norway` would have to join
/// `ALLOWED_IN_DOMAIN` in `xtask/src/boundaries.rs` for that. Widening the boundary allowlist to
/// reach a test is exactly the trade the gate exists to make visible, so those assertions live in
/// `sutura-catalog-local`, which has a parser - the same split `query::tests` records for
/// `deny_unknown_fields`.
fn repr(metric: &str, dimension: Option<&str>, value: Option<&str>) -> super::ReferentRepr {
    super::ReferentRepr {
        metric: metric_name(metric),
        dimension: dimension.map(dimension_name),
        value: value.map(declared_value),
    }
}

#[test]
fn a_referent_is_the_three_fields_it_has_written_and_no_others() {
    // The shape decision, asserted rather than described. Externally tagged, the commonest referent
    // would be written `metric: { metric: recurring_revenue }` - the tag word and the field word are
    // the same word - which is the spelling `measure::Term` refused for the same reason.
    assert_eq!(
        Referent::try_from(repr("recurring_revenue", None, None)).expect("a metric alone is a referent"),
        revenue()
    );
    assert_eq!(
        Referent::try_from(repr("recurring_revenue", Some("segment"), None)).expect("a metric and a dimension are one"),
        Referent::Dimension {
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("segment"),
        }
    );
    assert_eq!(
        Referent::try_from(repr("recurring_revenue", Some("segment"), Some("business"))).expect("all three are one"),
        Referent::Value {
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("segment"),
            value: declared_value("business"),
        }
    );
}

#[test]
fn a_value_with_no_dimension_is_not_a_referent() {
    // The one combination of the three fields that is not a referent, and the message names what is
    // missing rather than reporting that a mapping did not match a variant.
    let error =
        Referent::try_from(repr("recurring_revenue", None, Some("business"))).expect_err("a value belongs to a dimension");
    assert_eq!(
        error,
        InvalidReferent::ValueWithoutDimension {
            metric: metric_name("recurring_revenue"),
            value: declared_value("business"),
        }
    );
    assert!(error.to_string().contains("dimension"), "{error}");
}

#[test]
fn a_referent_serializes_as_what_a_catalog_wrote() {
    // Not cosmetic: the digest is taken over the serialized bundle, so a serialized shape that
    // differed from the on-disk one would make the bundle snapshot a description of serde. Asserted
    // through the round trip, which is the part that has to hold.
    for referent in [
        revenue(),
        Referent::Dimension {
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("segment"),
        },
        Referent::Value {
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("segment"),
            value: declared_value("business"),
        },
    ] {
        let written = super::ReferentRepr::from(referent.clone());
        assert_eq!(Referent::try_from(written).expect("what was written is a referent"), referent);
    }
}

// -------------------------------------------------------------------- capability, not content ---

#[test]
fn every_capability_there_is_comes_from_the_one_exhaustive_match() {
    // `Capability::every` is derived from `Capability::next`, which is a total match: a fifth variant
    // is a compile error there rather than a kind that quietly never appears in a list. This test is
    // the hand-written half of that pair - it is allowed to list the variants, because listing them
    // in a TEST is an assertion and listing them in the production path is the bug.
    let every: Vec<Capability> = Capability::every().collect();
    assert_eq!(
        every,
        vec![
            Capability::Glossary,
            Capability::Caveats,
            Capability::Absences,
            Capability::Examples,
        ]
    );
    // **The seed, which was the hole this pair did not cover.** `every` starts at `Glossary` by
    // name, so a variant declared ABOVE it left every match in this crate exhaustive and simply
    // never appeared - in `all()`, in the rendered declaration, or in `Knowledge::assemble`'s
    // undeclared-content guard, which walks this iterator. A review inserted one there and both of
    // the guard's own tests still passed. `Capability::previous` is the second match the compiler
    // forces, the const assertion beside the enum is what now fails to compile, and this is the same
    // fact asserted where a reader of the list above will see it.
    assert!(
        Capability::Glossary.previous().is_none(),
        "every() is seeded with Glossary, so no capability may be declared before it"
    );
    // And the two matches are each other's inverse over the whole list, so neither can be edited
    // into agreeing with a different order than the other.
    for capability in Capability::every() {
        if let Some(next) = capability.next() {
            assert_eq!(next.previous(), Some(capability), "{capability} and {next} disagree");
        }
    }
    assert_eq!(Capability::Examples.next(), None);
    // And `all()` is that same list, so an adapter declaring everything cannot declare less than
    // everything.
    let all = KnowledgeCapabilities::all();
    for capability in Capability::every() {
        assert!(all.declares(capability), "{capability} is not in all()");
    }
    assert_eq!(all.declared().len(), every.len());
    assert_eq!(Capability::Absences.as_str(), "absences");
    assert_eq!(Capability::Absences.to_string(), "absences");
}

/// A capability name, as a value a provider might have read from somewhere.
fn deserialize_capability(raw: &str) -> Result<Capability, serde::de::value::Error> {
    use serde::Deserialize as _;
    use serde::de::IntoDeserializer as _;

    Capability::deserialize(String::from(raw).into_deserializer())
}

#[test]
fn a_capability_is_a_closed_set_and_not_a_string() {
    // The reason it is an enum: a provider that declared "glossaries" must not silently declare
    // nothing. An unrecognised word is an error that names what was written and lists what exists.
    assert_eq!(
        deserialize_capability("glossary").expect("glossary is a capability"),
        Capability::Glossary
    );
    let wrong = deserialize_capability("glossaries").expect_err("glossaries is not a capability");
    let message = wrong.to_string();
    assert!(message.contains("glossaries"), "{message}");
    assert!(message.contains("glossary"), "{message}");
    drop(deserialize_capability("Glossary ").expect_err("a capability is not a trimmed, cased guess"));
}

#[test]
fn a_declared_and_empty_kind_is_not_the_same_as_an_undeclared_one() {
    // The distinction the whole declaration mechanism exists for, and the reason it is a declaration
    // rather than an inference from emptiness: both bundles below have no absences recorded, and only
    // one of them licenses a document to say that nothing is undefined here.
    let absent = Knowledge::none();
    assert!(!absent.supports(Capability::Absences));
    assert!(absent.absences().is_empty());

    let declared = accepts(only_absences(Vec::new()));
    assert!(
        declared.supports(Capability::Absences),
        "declared and empty is still declared"
    );
    assert!(declared.absences().is_empty());
    assert!(
        !declared.supports(Capability::Glossary),
        "declaring one capability declares no other"
    );

    // Not a formatting detail: the digest is taken over the serialized bundle, so the two have to be
    // two different values or provenance would certify a claim the prompt no longer makes.
    assert_ne!(absent, declared);
}

#[test]
fn content_for_a_capability_nobody_declared_does_not_load() {
    // An ADAPTER bug rather than a catalog one, refused where it happened: a glossary entry from a
    // provider that declares no glossary would otherwise render into a document that claims a
    // capability nobody said they had.
    let smuggled = KnowledgeInput::new(
        KnowledgeCapabilities::of([Capability::Caveats]),
        vec![glossary_entry("revenue", &[], revenue())],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(
        refuses(smuggled),
        InconsistentKnowledge::UndeclaredContent {
            capability: Capability::Glossary,
            supplied: 1,
        }
    );
    // The check is over `Capability::every`, so it holds for each of the four rather than for the one
    // somebody remembered to write a branch for.
    for input in [
        only_glossary(vec![glossary_entry("revenue", &[], revenue())]),
        only_caveats(vec![caveat("grain", vec![revenue()])]),
        only_absences(vec![absence("customer lifetime value", &[])]),
        only_examples(vec![example("in_june", question(Grain::Month, Vec::new(), Vec::new()))]),
    ] {
        drop(accepts(input));
    }
}

#[test]
fn every_kind_can_be_populated_at_once() {
    let knowledge = accepts(KnowledgeInput::new(
        KnowledgeCapabilities::all(),
        vec![glossary_entry(
            "monthly recurring revenue",
            &["monatlicher Umsatz"],
            revenue(),
        )],
        vec![caveat("month_grain_only", vec![revenue()])],
        vec![absence("customer lifetime value", &["CLV"])],
        vec![example("revenue_in_june", question(Grain::Month, Vec::new(), Vec::new()))],
    ));
    assert_eq!(knowledge.glossary().len(), 1);
    assert_eq!(knowledge.caveats().len(), 1);
    assert_eq!(knowledge.absences().len(), 1);
    assert_eq!(knowledge.examples().len(), 1);
    // A synonym is indexed as a claim and is reachable from the entry, which is what the prompt
    // renders the glossary line from.
    let entry = knowledge
        .glossary()
        .get(&phrase("monthly recurring revenue"))
        .expect("the term is the key");
    assert!(entry.synonyms().contains(&phrase("monatlicher Umsatz")));
    assert_eq!(entry.phrases().count(), 2);
}

#[test]
fn a_caveat_is_found_by_the_metric_it_is_about_however_it_is_scoped() {
    // A caveat about a dimension of a metric is a caveat about that metric: the prompt renders it
    // inside that metric's block, so this is the lookup the rendering is built on.
    let knowledge = accepts(only_caveats(vec![
        caveat("about_the_metric", vec![revenue()]),
        caveat(
            "about_a_value",
            vec![Referent::Value {
                metric: metric_name("recurring_revenue"),
                dimension: dimension_name("segment"),
                value: declared_value("business"),
            }],
        ),
        caveat(
            "about_something_else",
            vec![Referent::Metric {
                metric: metric_name("voice_minutes"),
            }],
        ),
    ]));
    let names: Vec<&str> = knowledge
        .caveats_about(&metric_name("recurring_revenue"))
        .map(|note| note.name().as_str())
        .collect();
    assert_eq!(names, vec!["about_a_value", "about_the_metric"]);
    assert_eq!(knowledge.caveats_about(&metric_name("voice_minutes")).count(), 1);
    // And a provider with no caveats at all answers nothing rather than failing.
    assert_eq!(Knowledge::none().caveats_about(&metric_name("recurring_revenue")).count(), 0);
}

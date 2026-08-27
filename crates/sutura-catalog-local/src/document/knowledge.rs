//! The on-disk shape of the four knowledge documents: the glossary, the caveats, the terms
//! deliberately left undefined, and the worked examples.
//!
//! Same rules as the definition documents next door, and one addition worth stating on its own.
//!
//! **`example`'s `question:` field deserializes `sutura_domain::query::Query` DIRECTLY, and that is
//! the most useful line in this file.** `Query` carries `deny_unknown_fields` and has no field for
//! SQL, so an author who copies a verified-query file out of a reference implementation - `nl:` plus
//! `sql:` is exactly the shape those files have - gets an error naming `sql`, at load, in the
//! document. `document.rs`'s `a_document_carrying_sql_is_refused_by_name` therefore extends to this
//! kind for nothing, and an example cannot describe a request the surface would not accept, because
//! the thing in the document IS the request.
//!
//! **Prose is the markdown body**, as for a model and a metric, and it becomes a
//! [`NoteBody`] - which is where the size caps are. A document over the cap does not load; nothing
//! anywhere truncates it.
//!
//! **Nothing here reads the directory a document sits in.** The `kind:` tag in the frontmatter is
//! what says what a file is - `crate::document::DocumentKind` argues why - so
//! `catalog/knowledge/glossary/` is a convenience for whoever browses the tree and not a fact the
//! loader depends on. That is also the part of this adapter a metadata-service adapter can borrow: a
//! document kind is a concept, not a path.

use std::collections::BTreeSet;

use sutura_domain::knowledge::{Absence, Caveat, Example, GlossaryEntry, NoteBody, NoteName, Phrase, Referent};
use sutura_domain::query::Query;

use crate::document::DocumentKind;

/// One glossary entry: the words, and the one thing they mean.
///
/// `means` needs no `singleton_map` adapter, unlike `MetricDoc`'s `measure`. A [`Referent`] is read
/// through a flat `try_from` representation rather than by an external tag - `sutura_domain::knowledge`
/// gives the argument - so what an author writes is one mapping of plain keys, which is what YAML is
/// good at.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlossaryDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    term: Phrase,
    /// The other ways people say it. A set, so the same synonym written twice is one synonym - which
    /// is the one duplication in this format that carries no information at all.
    #[serde(default)]
    synonyms: BTreeSet<Phrase>,
    means: Referent,
}

impl GlossaryDoc {
    pub fn into_domain(self, body: NoteBody) -> GlossaryEntry {
        GlossaryEntry::new(self.term, self.synonyms, self.means, body)
    }
}

/// One caveat, and everything it is about.
///
/// `about` has no default. A caveat scoped to nothing is refused by
/// `sutura_domain::knowledge::InconsistentKnowledge::CaveatAboutNothing`, and that refusal is what
/// keeps the catalog from having an unscoped channel into the prompt - so the field being required
/// here means the author is told about the missing key rather than about the empty list.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaveatDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    name: NoteName,
    about: Vec<Referent>,
}

impl CaveatDoc {
    pub fn into_domain(self, body: NoteBody) -> Caveat {
        Caveat::new(self.name, self.about, body)
    }
}

/// One term this catalog deliberately does not define.
///
/// `NotDefinedDoc` rather than `AbsenceDoc`, because the two names are for two audiences: `not_defined`
/// is the word an author writes in `kind:`, and it says what they are doing; `Absence` is what the
/// domain calls the thing once it exists. Naming the document after the word in the file is what makes
/// an error about it findable.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotDefinedDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    phrase: Phrase,
    #[serde(default)]
    synonyms: BTreeSet<Phrase>,
}

impl NotDefinedDoc {
    pub fn into_domain(self, body: NoteBody) -> Absence {
        Absence::new(self.phrase, self.synonyms, body)
    }
}

/// One worked question: how it was asked, and what to send.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExampleDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    name: NoteName,
    /// The ways somebody put the question. A list rather than a set, because the order is the
    /// author's: the first is the phrasing a reader is meant to recognise.
    asked: Vec<Phrase>,
    /// **The domain's `Query`, deserialized directly.** See this module's own documentation: it is
    /// what makes `sql:` in an example an error naming the field, and what keeps an example from
    /// describing a request the surface would refuse.
    question: Query,
}

impl ExampleDoc {
    pub fn into_domain(self, body: NoteBody) -> Example {
        Example::new(self.name, self.asked, self.question, body)
    }
}

#[cfg(test)]
mod tests {
    use super::{CaveatDoc, DocumentKind, ExampleDoc, GlossaryDoc, NotDefinedDoc};
    use sutura_domain::knowledge::{Capability, KnowledgeCapabilities, NoteBody, Phrase, Referent};
    use sutura_domain::model::{DimensionName, Grain, MetricName};

    const GLOSSARY: &str = "
kind: glossary
term: monthly recurring revenue
synonyms: [MRR, monatlicher Umsatz]
means: { metric: recurring_revenue }
";

    const EXAMPLE: &str = "
kind: example
name: revenue_by_segment_in_june
asked:
  - how much recurring revenue did each segment bring in June
question:
  metric: recurring_revenue
  grain: month
  range: { start: 2026-06-01, end: 2026-07-01 }
  dimensions: [segment]
";

    fn body() -> NoteBody {
        NoteBody::parse("Prose.").expect("a short body is a body")
    }

    fn metric(raw: &str) -> MetricName {
        MetricName::parse(raw).expect("a test metric is a metric")
    }

    #[test]
    fn a_glossary_document_parses_into_its_phrases_and_its_one_meaning() {
        let doc: GlossaryDoc = serde_norway::from_str(GLOSSARY).expect("a glossary document is a document");
        let entry = doc.into_domain(body());
        assert_eq!(entry.term().as_str(), "monthly recurring revenue");
        assert_eq!(entry.synonyms().len(), 2);
        assert_eq!(
            entry.means(),
            &Referent::Metric {
                metric: metric("recurring_revenue"),
            }
        );
        assert_eq!(entry.body().as_str(), "Prose.");
    }

    #[test]
    fn a_referent_is_a_flat_mapping_and_a_misspelled_key_in_one_is_refused_by_name() {
        // The shape assertion that needs a real parser, which is why it is here and not in
        // `sutura-domain`: the boundary gate allowlists no format crate for that crate, and widening
        // the allowlist to reach a test is the trade the gate exists to make visible.
        //
        // What is being asserted is that a referent is written the way the thing reads - one mapping
        // of plain keys - rather than as a tag around its own field name. Externally tagged, this
        // line would have to be `means: { metric: { metric: recurring_revenue } }`.
        let scoped = GLOSSARY.replace(
            "means: { metric: recurring_revenue }",
            "means: { metric: recurring_revenue, dimension: segment, value: business }",
        );
        let doc: GlossaryDoc = serde_norway::from_str(&scoped).expect("all three keys are a referent");
        assert_eq!(
            doc.into_domain(body()).means(),
            &Referent::Value {
                metric: metric("recurring_revenue"),
                dimension: DimensionName::parse("segment").expect("a name"),
                value: String::from("business"),
            }
        );

        // And the reason `deny_unknown_fields` is on the representation: without it `dimesion:` is
        // dropped in silence and a note about one dimension becomes a note about a whole metric,
        // which loads, renders, and says something the author did not write.
        let typo = GLOSSARY.replace(
            "means: { metric: recurring_revenue }",
            "means: { metric: recurring_revenue, dimesion: segment }",
        );
        let err = serde_norway::from_str::<GlossaryDoc>(&typo).expect_err("a misspelled key is not a field");
        assert!(err.to_string().contains("dimesion"), "{err}");
    }

    #[test]
    fn a_value_with_no_dimension_is_refused_by_what_is_missing() {
        let orphan = GLOSSARY.replace(
            "means: { metric: recurring_revenue }",
            "means: { metric: recurring_revenue, value: business }",
        );
        let err = serde_norway::from_str::<GlossaryDoc>(&orphan).expect_err("a value belongs to a dimension");
        assert!(err.to_string().contains("dimension"), "{err}");
    }

    #[test]
    fn a_phrase_that_is_not_one_is_refused_at_the_document() {
        // The newtype's parsing applies through the document, which is the whole reason
        // `Deserialize` on it is routed through `parse`. A phrase carrying a newline would otherwise
        // reach the prompt and write a line of it.
        let multiline = "kind: glossary\nterm: \"a\\nb\"\nmeans: { metric: recurring_revenue }\n";
        drop(serde_norway::from_str::<GlossaryDoc>(multiline).expect_err("a phrase is one line"));
        let empty = "kind: glossary\nterm: \"\"\nmeans: { metric: recurring_revenue }\n";
        drop(serde_norway::from_str::<GlossaryDoc>(empty).expect_err("a phrase is not nothing"));
    }

    #[test]
    fn a_caveat_document_carries_every_referent_it_is_about() {
        let yaml = "
kind: caveat
name: usage_reaches_no_snapshot
about:
  - { metric: data_per_subscription }
  - { metric: voice_minutes }
";
        let doc: CaveatDoc = serde_norway::from_str(yaml).expect("a caveat document is a document");
        let note = doc.into_domain(body());
        assert_eq!(note.name().as_str(), "usage_reaches_no_snapshot");
        assert_eq!(note.about().len(), 2);
    }

    #[test]
    fn a_caveat_with_no_about_key_is_refused_by_the_missing_field() {
        // Required rather than defaulted to empty: the empty list is refused by the domain anyway -
        // an unscoped caveat is the one shape that would put a paragraph about the deployment at
        // large into the prompt - so making the key required means the author is told which key they
        // left out instead of being told their list is empty.
        let yaml = "kind: caveat\nname: read_me_first\n";
        let err = serde_norway::from_str::<CaveatDoc>(yaml).expect_err("a caveat is about something");
        assert!(err.to_string().contains("about"), "{err}");
    }

    #[test]
    fn a_not_defined_document_names_the_term_and_its_synonyms() {
        let yaml = "
kind: not_defined
phrase: customer lifetime value
synonyms: [CLV, Kundenwert]
";
        let doc: NotDefinedDoc = serde_norway::from_str(yaml).expect("a not_defined document is a document");
        let note = doc.into_domain(body());
        assert_eq!(note.phrase().as_str(), "customer lifetime value");
        assert!(note.synonyms().contains(&Phrase::parse("CLV").expect("a phrase")));
        assert_eq!(note.phrases().count(), 3);
    }

    #[test]
    fn an_example_document_carries_a_real_question() {
        let doc: ExampleDoc = serde_norway::from_str(EXAMPLE).expect("an example document is a document");
        let note = doc.into_domain(body());
        assert_eq!(note.name().as_str(), "revenue_by_segment_in_june");
        assert_eq!(note.asked().len(), 1);
        assert_eq!(note.question().metric(), &metric("recurring_revenue"));
        assert_eq!(note.question().grain(), Grain::Month);
        assert_eq!(
            note.question().dimensions(),
            [DimensionName::parse("segment").expect("a name")]
        );
    }

    #[test]
    fn an_example_carrying_sql_is_refused_by_name() {
        // **The point of deserializing `Query` directly.** A reference implementation's
        // verified-query documents are `nl:` plus `sql:`, so this is the paste somebody will make -
        // and `deny_unknown_fields` on `Query` turns it into an error naming the field rather than a
        // document that loads with the statement silently dropped and an example that claims to be
        // one thing while carrying another.
        let with_sql = EXAMPLE.replace("  dimensions: [segment]", "  sql: SELECT 1\n");
        let err = serde_norway::from_str::<ExampleDoc>(&with_sql).expect_err("sql is not a question field");
        assert!(err.to_string().contains("sql"), "{err}");

        // The same from the other direction: the reference implementation's own key for the natural
        // language is `nl:`, and it is not this format's word either.
        let with_nl = EXAMPLE.replace("asked:", "nl:");
        let err = serde_norway::from_str::<ExampleDoc>(&with_nl).expect_err("nl is not a field here");
        assert!(err.to_string().contains("nl"), "{err}");
    }

    #[test]
    fn an_example_question_with_no_end_to_its_period_does_not_load() {
        // The domain's own parsing, applied through the document: a `Query` has no unbounded range,
        // so an example cannot show one being asked for.
        let open = EXAMPLE.replace(
            "range: { start: 2026-06-01, end: 2026-07-01 }",
            "range: { start: 2026-06-01 }",
        );
        drop(serde_norway::from_str::<ExampleDoc>(&open).expect_err("a period has both ends"));
    }

    /// The document kind this adapter reads for one knowledge capability.
    ///
    /// **A total match, and it exists in order not to compile.** `Collected::assemble` declares
    /// `KnowledgeCapabilities::all()` - "this provider supports whatever kinds exist, including ones
    /// added later" - and a capability with no `DocumentKind` beside it is one this adapter DECLARES
    /// and cannot read. That is not a cosmetic drift: the declaration goes under the definition
    /// digest, so the rendered prompt would claim a capability with nothing behind it and the
    /// provenance would certify the claim. A fifth capability in the domain is a compile error here,
    /// in the adapter that made the claim, rather than a kind nothing reads.
    ///
    /// In the test target on purpose. Nothing on the read path needs this mapping - the walk
    /// dispatches on the `kind:` tag it found in a file, not on a capability - so a non-test copy
    /// would be dead code, which this workspace denies. The gates compile the test targets, so the
    /// compile error still lands in CI. `sutura_app::prompt::tests::guide_for` is the same trade for
    /// the same reason.
    const fn reads(capability: Capability) -> DocumentKind {
        match capability {
            Capability::Glossary => DocumentKind::Glossary,
            Capability::Caveats => DocumentKind::Caveat,
            Capability::Absences => DocumentKind::NotDefined,
            Capability::Examples => DocumentKind::Example,
        }
    }

    #[test]
    fn every_capability_this_adapter_declares_is_one_it_has_a_document_for() {
        // The declaration and the document shapes, checked against each other rather than kept in
        // step by whoever remembers both. `reads` above is what stops compiling when they part
        // company; this is the half that says what the pairing has to be.
        let declared = KnowledgeCapabilities::all();
        for capability in Capability::every() {
            assert!(
                declared.declares(capability),
                "all() has to declare {capability} for this adapter to read it"
            );
        }
        assert_eq!(declared.declared().len(), Capability::every().count());
        // Written out, because listing the variants in a TEST is an assertion and listing them in a
        // production path is the bug. The last pairing is the one worth spelling out: an author writes
        // `kind: not_defined`, which says what they are doing, and the domain calls the thing an
        // absence, which says what it is.
        assert_eq!(reads(Capability::Glossary), DocumentKind::Glossary);
        assert_eq!(reads(Capability::Caveats), DocumentKind::Caveat);
        assert_eq!(reads(Capability::Absences), DocumentKind::NotDefined);
        assert_eq!(reads(Capability::Examples), DocumentKind::Example);
        assert_eq!(reads(Capability::Absences).as_str(), "not_defined");
    }
}

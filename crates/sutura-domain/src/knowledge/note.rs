//! The four kinds of note, as records.
//!
//! Split out of `knowledge.rs` because that file is over the thousand-line limit
//! `cargo xtask max-lines` enforces without it, and `devco/max-lines-ignore` refuses any pattern
//! under `crates/` on purpose - the answer there is to split the file. The seam is the one the
//! module already had: the parent holds the vocabulary a note is written in and the bundle they sit
//! in, and this holds the four records themselves.
//!
//! Nothing here validates anything. A note is a record, `Knowledge::assemble` is where a set of them
//! is checked against the definitions it is about, and that split is the whole design - see
//! `super`'s own documentation for why the check cannot live in an adapter.

use std::collections::BTreeSet;

use super::{NoteBody, NoteName, Phrase, Referent, phrase_bytes, sum_bytes};
use crate::query::Query;

/// One entry of the business glossary: the words people use, and the one thing they mean.
///
/// `means` is a [`Referent`] and not prose, which is what lets the glossary be rendered from the
/// STRUCTURE rather than from the body: every token of the rendered line is a phrase this type
/// parsed or a name the bundle already declares.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GlossaryEntry {
    term: Phrase,
    synonyms: BTreeSet<Phrase>,
    means: Referent,
    body: NoteBody,
}

impl GlossaryEntry {
    pub const fn new(term: Phrase, synonyms: BTreeSet<Phrase>, means: Referent, body: NoteBody) -> Self {
        Self {
            term,
            synonyms,
            means,
            body,
        }
    }

    #[inline]
    pub const fn term(&self) -> &Phrase {
        &self.term
    }

    #[inline]
    pub const fn synonyms(&self) -> &BTreeSet<Phrase> {
        &self.synonyms
    }

    #[inline]
    pub const fn means(&self) -> &Referent {
        &self.means
    }

    #[inline]
    pub const fn body(&self) -> &NoteBody {
        &self.body
    }

    /// Every phrase this entry claims: the term first, then its synonyms.
    pub fn phrases(&self) -> impl Iterator<Item = &Phrase> {
        core::iter::once(&self.term).chain(self.synonyms.iter())
    }

    pub(super) fn authored_bytes(&self) -> usize {
        phrase_bytes(self.phrases())
            .saturating_add(self.body.as_str().len())
            .saturating_add(self.means.authored_bytes())
    }
}

/// Something a reader has to know before trusting a number: a grain trap, a base that is not what it
/// sounds like, a value that means less than it appears to.
///
/// **Scoped, and a caveat about nothing does not load.** That check -
/// [`super::InconsistentKnowledge::CaveatAboutNothing`] - is what stops this kind from becoming the unscoped
/// text channel the module documentation refuses. The prompt renders each caveat inside the block of
/// the metric it is about rather than as a preamble, so it is read by whoever is about to ask that
/// question rather than by whoever is skimming the top of the document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Caveat {
    name: NoteName,
    about: Vec<Referent>,
    body: NoteBody,
}

impl Caveat {
    pub const fn new(name: NoteName, about: Vec<Referent>, body: NoteBody) -> Self {
        Self { name, about, body }
    }

    #[inline]
    pub const fn name(&self) -> &NoteName {
        &self.name
    }

    #[inline]
    pub fn about(&self) -> &[Referent] {
        &self.about
    }

    #[inline]
    pub const fn body(&self) -> &NoteBody {
        &self.body
    }

    pub(super) fn authored_bytes(&self) -> usize {
        sum_bytes(self.about.iter().map(Referent::authored_bytes))
            .saturating_add(self.name.as_str().len())
            .saturating_add(self.body.as_str().len())
    }
}

/// A term people ask about that this catalog deliberately does NOT define.
///
/// The half of a certified-metrics document that the pinned bundle cannot render for itself. A metric
/// list says what exists; it cannot say "customer lifetime value was considered and has no agreed
/// definition here", which is the sentence that stops an agent computing one out of the parts.
///
/// **It rots into a lie the moment somebody defines the thing**, which is why
/// [`super::InconsistentKnowledge::AbsenceNamesADefinedMetric`] exists: the load fails rather than the
/// prompt telling an agent to decline a question the bundle would now answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Absence {
    phrase: Phrase,
    synonyms: BTreeSet<Phrase>,
    body: NoteBody,
}

impl Absence {
    pub const fn new(phrase: Phrase, synonyms: BTreeSet<Phrase>, body: NoteBody) -> Self {
        Self { phrase, synonyms, body }
    }

    #[inline]
    pub const fn phrase(&self) -> &Phrase {
        &self.phrase
    }

    #[inline]
    pub const fn synonyms(&self) -> &BTreeSet<Phrase> {
        &self.synonyms
    }

    #[inline]
    pub const fn body(&self) -> &NoteBody {
        &self.body
    }

    /// Every phrase this note declares undefined.
    pub fn phrases(&self) -> impl Iterator<Item = &Phrase> {
        core::iter::once(&self.phrase).chain(self.synonyms.iter())
    }

    pub(super) fn authored_bytes(&self) -> usize {
        phrase_bytes(self.phrases()).saturating_add(self.body.as_str().len())
    }
}

/// A question somebody asked, and the [`Query`] that answers it.
///
/// **`question` is a real [`Query`] and not a rendering of one**, which is the most useful move in
/// this module. `Query` carries `deny_unknown_fields` and has no field for SQL, so an author who
/// pastes a `sql:` line out of a reference implementation's verified-query file gets an error naming
/// the field - and `sutura_catalog_local`'s existing test that a document carrying SQL is refused by
/// name extends to this kind without a line being written for it. It also means an example cannot
/// drift from what the surface accepts: the thing in the document is the thing you send.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Example {
    name: NoteName,
    asked: Vec<Phrase>,
    question: Query,
    body: NoteBody,
}

impl Example {
    pub const fn new(name: NoteName, asked: Vec<Phrase>, question: Query, body: NoteBody) -> Self {
        Self {
            name,
            asked,
            question,
            body,
        }
    }

    #[inline]
    pub const fn name(&self) -> &NoteName {
        &self.name
    }

    /// The ways the question was put in words. A list rather than a set because the order is the
    /// author's: the first is the one a reader is meant to recognise.
    #[inline]
    pub fn asked(&self) -> &[Phrase] {
        &self.asked
    }

    #[inline]
    pub const fn question(&self) -> &Query {
        &self.question
    }

    #[inline]
    pub const fn body(&self) -> &NoteBody {
        &self.body
    }

    /// The authored bytes of this example, the rendered question included.
    ///
    /// **The question is counted, and leaving it out was a hole rather than a rounding.** The
    /// aggregate cap bounds the size of one prompt, and `sutura_app::prompt` renders every dimension
    /// and every filter of this question into that prompt - so a question carrying ten thousand
    /// filters on one permitted value is inside every per-note bound, passes
    /// `Knowledge::assemble`'s per-question checks, and is not an example of anything. The grain and
    /// the period are fixed-width and are the one part not counted; nothing an author writes can make
    /// either of them longer.
    pub(super) fn authored_bytes(&self) -> usize {
        phrase_bytes(self.asked.iter())
            .saturating_add(self.name.as_str().len())
            .saturating_add(self.body.as_str().len())
            .saturating_add(question_bytes(&self.question))
    }
}

/// The authored bytes of one worked question: every name and value of it that reaches the prompt.
fn question_bytes(question: &Query) -> usize {
    let dimensions = sum_bytes(question.dimensions().iter().map(|name| name.as_str().len()));
    let filters = sum_bytes(question.filters().iter().map(|filter| {
        filter
            .dimension()
            .as_str()
            .len()
            .saturating_add(filter.value().as_str().len())
    }));
    question
        .metric()
        .as_str()
        .len()
        .saturating_add(dimensions)
        .saturating_add(filters)
}

//! The bundle of notes, and everything that decides whether it holds together.
//!
//! Split out of `knowledge.rs` for a limit rather than a preference: `cargo xtask max-lines` fails at
//! a thousand lines under `crates/`, and `devco/max-lines-ignore` refuses any pattern there on
//! purpose - the answer is to split the file. The seam is a real one. The parent holds the vocabulary
//! a note is written in, `note.rs` holds the four records, `referent.rs` holds the walks from a
//! note's names to what the definitions declare, and this holds the value they are collected into
//! plus the checks that run once, when a bundle is loaded, and never again.
//!
//! **One `impl Knowledge` block, and that is forced rather than chosen.** The workspace enables
//! clippy's whole `restriction` category, so `multiple_inherent_impl` is an error: a type's inherent
//! methods live in one place. That is why the accessors sit here beside the checks rather than next to
//! the struct they read.
//!
//! **The checks live here, once, for every adapter**, which is the argument
//! [`crate::catalog::Definitions::assemble`] already makes and the reason [`Knowledge::assemble`]
//! takes the definitions rather than being handed a value somebody else checked. A glossary check
//! inside the markdown adapter is a check a metadata-service adapter would not have.
//!
//! # One phrase, and what that means
//!
//! Every index here is keyed on [`super::phrase_identity`] rather than on the authored [`Phrase`],
//! and the authored phrase is the value. Two documents whose phrases differ only in case, only in a
//! run of spaces or only in an invisible code point are two documents about one phrase as far as
//! whoever reads the rendered prompt is concerned, so they are refused rather than both rendered.
//! Byte equality in one check beside a case-folded comparison in the next is how a glossary line and
//! an absence line came to be rendered, three sections apart, about the same word.

use std::collections::{BTreeMap, BTreeSet};

use super::referent::{caveat_fault, declared_as, fault_in, glossary_fault, reached_through, visible};
use super::{
    Absence, Absences, Capability, Caveat, Caveats, Example, Examples, Glossary, GlossaryEntry, KnowledgeCapabilities,
    MAX_KNOWLEDGE_BYTES, NoteName, Phrase, Referent, phrase_identity, sum_bytes,
};
use crate::catalog::{Definitions, DimensionValue};
use crate::model::{ColumnName, DimensionName, Grain, InvalidIdentifier, MetricName, ModelName, RelationshipName};
use crate::pinned::view::ScopedView;
use crate::query::{MAX_DIMENSIONS, MAX_RANGE_DAYS};

/// Everything a catalog said about what it defines, checked against the definitions it is about.
///
/// `BTreeMap` throughout, for the same reason [`crate::catalog::Definitions`] uses them: the digest is taken over the
/// serialized form of the bundle this sits in, and an unordered map serializes in whatever order its
/// hasher chose this run.
///
/// `declares` comes FIRST, and not for reading order: it is what the four collections mean. An empty
/// glossary beside a declaration of one is "nothing recorded yet"; the same empty glossary beside no
/// declaration is "this provider has no glossary", and only the first licenses the prompt to say
/// anything about it. The module documentation argues it at length, and `sutura_app::prompt` is where
/// the difference becomes a sentence an agent reads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Knowledge {
    declares: KnowledgeCapabilities,
    glossary: Glossary,
    caveats: Caveats,
    absences: Absences,
    examples: Examples,
}

/// What one adapter read, before it is checked.
///
/// Vectors rather than maps, for the reason [`crate::catalog::Definitions::assemble`] gives: a caller that built a map
/// first has already silently dropped one of a duplicated pair, and a directory of documents is
/// exactly where two files claim one name.
///
/// **The declaration is a separate argument from the content**, so an adapter says what it supports
/// rather than having it inferred from what it happened to find. Content for an undeclared capability
/// is refused by [`Knowledge::assemble`] rather than tolerated: it is an adapter bug, and the load is
/// where it is cheap to notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeInput {
    declares: KnowledgeCapabilities,
    glossary: Vec<GlossaryEntry>,
    caveats: Vec<Caveat>,
    absences: Vec<Absence>,
    examples: Vec<Example>,
}

impl KnowledgeInput {
    /// What this provider supports, and what it read.
    pub const fn new(
        declares: KnowledgeCapabilities,
        glossary: Vec<GlossaryEntry>,
        caveats: Vec<Caveat>,
        absences: Vec<Absence>,
        examples: Vec<Example>,
    ) -> Self {
        Self {
            declares,
            glossary,
            caveats,
            absences,
            examples,
        }
    }

    /// A provider with nothing to say: no capability declared and no content.
    ///
    /// It exists so a bundle built in code - a fixture, a test, a transport's own sample - does not
    /// have to write four empty vectors to say nothing, and so that saying nothing is one recognisable
    /// call rather than five arguments a reader has to add up.
    #[must_use]
    pub const fn none() -> Self {
        Self::new(KnowledgeCapabilities::none(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
    }

    /// How many notes arrived for one capability. What the undeclared-content check counts.
    const fn supplied(&self, capability: Capability) -> usize {
        match capability {
            Capability::Glossary => self.glossary.len(),
            Capability::Caveats => self.caveats.len(),
            Capability::Absences => self.absences.len(),
            Capability::Examples => self.examples.len(),
        }
    }

    /// The authored bytes across every note, before any of it is indexed.
    ///
    /// **Read from the four vectors directly, and not from a [`Knowledge`] built first.**
    /// [`Knowledge::assemble`] used to sum this over the assembled bundle, after building four
    /// indices - each phrase's identity computed and cloned into a map, each referent cloned, the
    /// claim map walked for a collision. An oversized input paid for all of that before it was
    /// refused. Measuring the input costs one pass over `Vec`s the caller already allocated, and it
    /// is the same number for every note but one kind: nothing here drops a note before the cap is
    /// checked, so summing before indexing and summing after are the same total by construction. The
    /// exception is a caveat written about relationships, which indexing EXPANDS into one caveat per
    /// metric - counted here as written, and re-counted as expanded by `index_caveats`, which checks
    /// the cap again on what the bundle will hold.
    fn authored_bytes(&self) -> usize {
        let glossary = sum_bytes(self.glossary.iter().map(GlossaryEntry::authored_bytes));
        let caveats = sum_bytes(self.caveats.iter().map(Caveat::authored_bytes));
        let absences = sum_bytes(self.absences.iter().map(Absence::authored_bytes));
        let examples = sum_bytes(self.examples.iter().map(Example::authored_bytes));
        glossary
            .saturating_add(caveats)
            .saturating_add(absences)
            .saturating_add(examples)
    }
}

/// Why a set of notes does not hold together with the definitions it is about.
///
/// **The checks live here, once, and that is the argument [`Definitions::assemble`] already makes.** A
/// glossary check inside the markdown adapter is a check a metadata-service adapter would not have,
/// and the failure it misses is not cosmetic: an entry saying "business customers means segment
/// business" while the bundle's allowlist says `b2b` makes every agent that reads it produce a
/// question refused as `DimensionValueNotAllowed`, for a reason the agent cannot see.
///
/// Every variant has a test that provokes it. A check nobody has seen fire is a check nobody knows
/// works, and `docs/crap.md`'s gate scores this crate, so that discipline is paid for where it is
/// measured.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InconsistentKnowledge {
    /// Notes arrived for a capability the provider did not declare.
    ///
    /// **An adapter bug rather than a catalog one**, and the reason it fails the load is where it
    /// would otherwise surface: a glossary entry from a provider that declared no glossary would
    /// render into a document that then claims a capability nobody said they had. Named at the
    /// boundary where it happened instead.
    #[error("{supplied} {capability} note(s) arrived from a provider that does not declare {capability}")]
    UndeclaredContent { capability: Capability, supplied: usize },
    /// Two caveat documents with one name. Refused rather than resolved, because a map would keep the
    /// second and the first would vanish without a word.
    #[error("caveat {name} is declared twice")]
    DuplicateCaveat { name: NoteName },
    #[error("example {name} is declared twice")]
    DuplicateExample { name: NoteName },
    #[error("the phrase {phrase} is declared undefined twice")]
    DuplicateAbsence { phrase: Phrase },
    #[error("glossary entry {term} means metric {metric}, which is not defined")]
    GlossaryUnknownMetric { term: Phrase, metric: MetricName },
    #[error("glossary entry {term} means dimension {dimension}, which metric {metric} does not declare")]
    GlossaryUnknownDimension {
        term: Phrase,
        metric: MetricName,
        dimension: DimensionName,
    },
    /// The one that produces a wrong REFUSAL rather than a wrong number: the glossary says a phrase
    /// means a value the dimension's allowlist does not carry, so every question derived from it is
    /// declined for a reason that names the dimension and not the glossary.
    #[error(
        "glossary entry {term} means value {:?} of dimension {dimension}, which metric {metric} does not permit",
        .value.as_str()
    )]
    GlossaryValueNotAllowed {
        term: Phrase,
        metric: MetricName,
        dimension: DimensionName,
        value: DimensionValue,
    },
    #[error("glossary entry {term} means model {model}, which is not defined")]
    GlossaryUnknownModel { term: Phrase, model: ModelName },
    #[error("glossary entry {term} means column {column}, which model {model} does not declare")]
    GlossaryUnknownColumn {
        term: Phrase,
        model: ModelName,
        column: ColumnName,
    },
    #[error("caveat {name} is about metric {metric}, which is not defined")]
    CaveatUnknownMetric { name: NoteName, metric: MetricName },
    #[error("caveat {name} is about dimension {dimension}, which metric {metric} does not declare")]
    CaveatUnknownDimension {
        name: NoteName,
        metric: MetricName,
        dimension: DimensionName,
    },
    #[error(
        "caveat {name} is about value {:?} of dimension {dimension}, which metric {metric} does not permit",
        .value.as_str()
    )]
    CaveatValueNotAllowed {
        name: NoteName,
        metric: MetricName,
        dimension: DimensionName,
        value: DimensionValue,
    },
    /// A caveat scoped to nothing. **This is the check that removes the global text channel**: with
    /// it, every note in this module is attached to something the bundle declares, so there is no
    /// shape a catalog author can use to put a paragraph about the deployment at large into the
    /// prompt's preamble.
    #[error("caveat {name} is about nothing, so there is no question it would be shown beside")]
    CaveatAboutNothing { name: NoteName },
    /// A caveat about a model or one of its columns. A caveat is printed under the metric it is
    /// about, and a model has no block of its own to print it in, so it would load and be read by
    /// nobody. Only a glossary entry may mean a model or a column.
    #[error("caveat {name} is about model {model}, and a caveat is printed under a metric: name the metrics it warns about")]
    CaveatAboutAModel { name: NoteName, model: ModelName },
    #[error("caveat {name} is about relationship {relationship}, which is not defined")]
    CaveatUnknownRelationship { name: NoteName, relationship: RelationshipName },
    /// A caveat about a relationship no dimension is reached through. It would expand into no caveat
    /// at all and load read by nobody, which is [`Self::CaveatAboutAModel`]'s argument one step
    /// along. A cross-model ratio's hop to its shared calendar does not count as reaching: nothing
    /// names it in a `via`.
    #[error("caveat {name} is about relationship {relationship}, and no metric reaches a dimension through it")]
    CaveatRelationshipReachesNoMetric { name: NoteName, relationship: RelationshipName },
    /// A caveat naming both `about` and `relationships`. The second is expanded into one caveat per
    /// metric under a name derived from this one, and the first would be printed under this name,
    /// so one document would become notes that warn about one metric twice under two names.
    #[error("caveat {name} names metrics and relationships: a caveat is about one or the other")]
    CaveatAboutAndThroughRelationships { name: NoteName },
    /// The name a relationship caveat is printed under for one metric, `<caveat>__<metric>`, is not
    /// a note name - which, both halves being names already, means it is past the 63 characters an
    /// identifier may have. Refused rather than shortened: two metrics sharing a prefix would shorten
    /// to one name.
    #[error("caveat {caveat} would be printed under metric {metric} as {caveat}__{metric}, which is not a note name")]
    DerivedCaveatNotAName {
        caveat: NoteName,
        metric: MetricName,
        #[source]
        cause: InvalidIdentifier,
    },
    /// The name a relationship caveat is printed under for one metric is already a caveat's: an
    /// authored one, or another relationship caveat's under another metric.
    #[error("caveat {caveat} would be printed under metric {metric} as {name}, which is already a caveat")]
    DerivedCaveatNameTaken {
        name: NoteName,
        caveat: NoteName,
        metric: MetricName,
    },
    /// One phrase, claimed by TWO glossary entries. A phrase resolves to at most one thing across the
    /// whole glossary, and the check is on the CLAIM rather than on what it resolves to: two entries
    /// claiming one phrase are two bodies for it, and a map would keep the second silently.
    ///
    /// It names the two TERMS rather than the two referents, which is the fix to something the author
    /// can act on: the referents are what the entries mean, and the terms are which documents to open.
    /// It also keeps this variant inside the size the workspace holds an error type to - the
    /// `result_large_err` lint is deliberately not allowed here, and two referents were 160 bytes of
    /// it.
    ///
    /// Two DOCUMENTS, because one document claiming one phrase twice would make `first` and `second`
    /// the same term and send an author looking for a second file that does not exist. That case is
    /// [`Self::TermIsItsOwnSynonym`].
    #[error("the phrase {phrase} is claimed by two glossary entries, {first} and {second}")]
    AmbiguousPhrase { phrase: Phrase, first: Phrase, second: Phrase },
    /// One glossary entry claiming one phrase twice: a synonym that is the term again, or two
    /// synonyms that are one phrase once case, spacing and invisible code points are set aside.
    ///
    /// Its own variant because the remedy is different. [`Self::AmbiguousPhrase`] says which two
    /// documents disagree; here there is one document, and the line to delete is inside it.
    #[error(
        "glossary entry {term} claims the phrase {phrase} twice: a term is not its own synonym, and two synonyms are not one phrase"
    )]
    TermIsItsOwnSynonym { term: Phrase, phrase: Phrase },
    #[error("the phrase {phrase} is both given a meaning by the glossary and declared undefined")]
    PhraseBothDefinedAndNot { phrase: Phrase },
    /// **What stops the absence list rotting into a lie.** A note saying a term has no definition
    /// here, beside a metric whose name is that term, is a document that would teach an agent to
    /// decline a question this bundle answers. The comparison is on the identifier shape of the
    /// phrase - lower-cased, with every run of non-alphanumeric characters as one underscore - so
    /// "Customer Lifetime Value" is recognised as naming a metric called `customer_lifetime_value`.
    #[error("the phrase {phrase} is declared undefined, and metric {metric} defines it")]
    AbsenceNamesADefinedMetric { phrase: Phrase, metric: MetricName },
    /// The same rot one level down. A note declaring "segment" undefined renders three sections above
    /// a metric block that lists `segment` among the things a question may group by and filter on, so
    /// the prompt tells an agent to decline a question the bundle answers - which is exactly what
    /// [`Self::AbsenceNamesADefinedMetric`] exists to prevent, and a metric name is not the only name
    /// a bundle declares.
    #[error("the phrase {phrase} is declared undefined, and metric {metric} declares a dimension called {dimension}")]
    AbsenceNamesADeclaredDimension {
        phrase: Phrase,
        metric: MetricName,
        dimension: DimensionName,
    },
    /// And one level down again: a phrase declared undefined that is a value the metric block prints
    /// as one a question may filter on.
    #[error(
        "the phrase {phrase} is declared undefined, and it is the value {:?} of dimension {dimension}, which metric {metric} permits",
        .value.as_str()
    )]
    AbsenceNamesADeclaredValue {
        phrase: Phrase,
        metric: MetricName,
        dimension: DimensionName,
        value: DimensionValue,
    },
    #[error("example {name} asks about metric {metric}, which is not defined")]
    ExampleUnknownMetric { name: NoteName, metric: MetricName },
    /// The example's question names more than one metric. `github.com/telekom/sutura#968` lets a
    /// question NAME a set, and a set whose every member shares model, time column, grain and
    /// dimensions now executes as one grouped statement - but loading it anyway would check only
    /// `crate::query::Query::metric`'s first name (this module reads the compiler's own numbers,
    /// not the compiler, and has no second-metric check to run, so it cannot certify the rest of
    /// the set) and then render it under `EXAMPLES_INTRO`'s promise that a worked question is "one
    /// this deployment answers", which is false for one this module cannot certify.
    #[error("example {name} names {requested} metrics, which this module cannot validate")]
    ExampleNamesMultipleMetrics { name: NoteName, requested: usize },
    #[error("example {name} asks metric {metric} at the {grain} grain, which it does not declare")]
    ExampleGrainNotSupported {
        name: NoteName,
        metric: MetricName,
        grain: Grain,
    },
    #[error("example {name} groups metric {metric} by {dimension}, which it does not declare")]
    ExampleDimensionNotPermitted {
        name: NoteName,
        metric: MetricName,
        dimension: DimensionName,
    },
    /// The example's filter names a value the dimension does not permit - which includes a dimension
    /// that permits no value at all, because [`crate::catalog::Dimension::permits`] answers `false`
    /// for one with no allowlist. Both are the same fact about the document: the question in it would
    /// be refused, so it is not an example of anything.
    #[error(
        "example {name} filters {dimension} to {:?}, which metric {metric} does not permit",
        .value.as_str()
    )]
    ExampleValueNotAllowed {
        name: NoteName,
        metric: MetricName,
        dimension: DimensionName,
        value: DimensionValue,
    },
    /// The example asks for a longer span of history than a request may.
    ///
    /// **The limit is READ from [`MAX_RANGE_DAYS`], not restated**, which is why checking it here is
    /// the same opinion the compiler holds rather than a second one. The prompt tells an agent that
    /// a worked question is "one this deployment answers rather than one it would decline"; without
    /// this check an example spanning twenty-six years loaded, rendered under that sentence, and was
    /// refused as `TimeRangeTooLong` the moment an agent copied it.
    #[error("example {name} asks for {days} days of history, and a request may ask for {limit}")]
    ExampleRangeTooLong { name: NoteName, days: i32, limit: i32 },
    /// The example groups by more dimensions than a request may. [`MAX_DIMENSIONS`]'s half of the
    /// same argument.
    #[error("example {name} groups by {requested} dimensions, and a request may group by {limit}")]
    ExampleTooManyDimensions { name: NoteName, requested: usize, limit: usize },
    /// The example was asked in a phrase this catalog records as deliberately undefined.
    ///
    /// The rendered document would say "a question about one of these terms is to be DECLINED" and
    /// then, further down, show the same wording as a worked question to copy. Narrow on purpose: it
    /// is the phrase index's `not_defined` claims only. A worked question OVERLAPPING the glossary is
    /// legitimate and common - an `asked` phrase is a sentence, and the glossary is what its words
    /// mean.
    #[error("example {name} is asked as {phrase}, which this catalog declares undefined")]
    ExampleAsksWhatIsDeclaredUndefined { name: NoteName, phrase: Phrase },
    /// The aggregate cap, so that N conforming notes cannot do what one oversized note cannot.
    #[error("the notes carry {bytes} bytes of authored text, and the limit is {limit}")]
    KnowledgeTooLarge { bytes: usize, limit: usize },
}

/// What a phrase was claimed to be, while the phrase index is being built.
///
/// Two claims on one phrase are three different errors depending on which pair they are, so the index
/// has to remember which kind of note made each one - and, for a glossary claim, WHICH entry made it,
/// because that is the document an author has to open.
///
/// Keyed by [`phrase_identity`] rather than by the phrase, so "MRR" and "mrr" are one claim.
enum Claim {
    Defined { term: Phrase },
    NotDefined,
}

impl Knowledge {
    /// A provider that declares nothing and carries nothing.
    ///
    /// **The only [`Knowledge`] obtainable without a [`crate::catalog::Definitions`] to check it against, and that is
    /// consistent rather than an exception:** it says nothing about any definition, so there is
    /// nothing for a check to disagree with. Every other value comes from [`Self::assemble`].
    #[must_use]
    pub const fn none() -> Self {
        Self {
            declares: KnowledgeCapabilities::none(),
            glossary: BTreeMap::new(),
            caveats: BTreeMap::new(),
            absences: BTreeMap::new(),
            examples: BTreeMap::new(),
        }
    }

    /// What this provider says it supports.
    ///
    /// **Read this before reading any of the four collections below.** An empty collection means
    /// "nothing recorded" when its capability is declared and "no such concept here" when it is not,
    /// and only the first licenses a document to say anything about it. The collections themselves
    /// cannot tell the two apart, which is why the declaration is a value rather than an inference.
    #[inline]
    pub const fn declares(&self) -> &KnowledgeCapabilities {
        &self.declares
    }

    /// Shorthand for [`KnowledgeCapabilities::declares`] on this bundle's own declaration.
    #[inline]
    pub fn supports(&self, capability: Capability) -> bool {
        self.declares.declares(capability)
    }

    /// The glossary. Empty when nothing is recorded AND when [`Capability::Glossary`] is not declared;
    /// [`Self::declares`] is what tells those apart.
    #[inline]
    pub const fn glossary(&self) -> &Glossary {
        &self.glossary
    }

    #[inline]
    pub const fn caveats(&self) -> &Caveats {
        &self.caveats
    }

    /// The terms this catalog records as deliberately undefined.
    ///
    /// The collection whose emptiness is most often misread, which is why the capability exists: empty
    /// with [`Capability::Absences`] declared is a reviewed list with nothing on it, and empty without
    /// it is a provider that cannot record such a thing at all.
    #[inline]
    pub const fn absences(&self) -> &Absences {
        &self.absences
    }

    #[inline]
    pub const fn examples(&self) -> &Examples {
        &self.examples
    }

    /// Every caveat scoped to this metric, however it is scoped to it.
    ///
    /// A caveat about one of a metric's dimensions is a caveat about that metric: whoever is about to
    /// ask the question is who needs to read it, and what they are looking at is the metric.
    ///
    /// `use<'a>` and a clone of the name, rather than borrowing it for as long as the iterator lives.
    /// Under the 2024 edition an `impl Trait` return captures every lifetime in scope, so the
    /// borrowed form makes `knowledge.caveats_about(&name_built_here())` a borrow-check error at the
    /// call site - which is an API that reads as though it wanted a temporary and does not.
    pub fn caveats_about<'a>(&'a self, metric: &MetricName) -> impl Iterator<Item = &'a Caveat> + use<'a> {
        let metric = metric.clone();
        self.caveats
            .values()
            .filter(move |note| note.about().iter().any(|referent| referent.metric() == Some(&metric)))
    }
    /// This bundle's knowledge, filtered down to what one caller may see - `docs/adr/0028`.
    ///
    /// Knowledge follows its structured referents rather than its prose: a glossary entry follows the
    /// metric or the model its `Referent` names, a caveat survives only when EVERY metric it refers to
    /// is visible, and a worked example follows the metric in its `Query`. The caller's own view
    /// supplies what is visible - a metric or a model is visible iff it is both declared and granted -
    /// so this is metadata access over the already-pinned bundle, never a new source of definitions.
    ///
    /// **It takes a [`ScopedView`] rather than a predicate, and that is where the ADR rule lives.**
    /// `docs/adr/0028`'s all-referents rule is exactly "a note survives when every metric it refers
    /// to is visible to this caller's grant", which is a fact about the caller's view - so a hand-
    /// written closure at a call site (with the `|_| true` that reads as "drop nothing") cannot exist
    /// here. The view is the only way to state a caller's grant, and this method is the only reader
    /// of it for knowledge.
    ///
    /// **The terms recorded as undefined have no metric to follow**, and the ADR names them
    /// separately: an unscoped absence needs an explicit catalog-wide audience and is withheld when
    /// none is granted. Dropping the CONTENT while keeping the declaration would render "kept and
    /// empty" to a caller from whom it is withheld - a false negative that invites an agent to
    /// approximate a term it should decline. So the declaration of `Absences` is withdrawn with the
    /// content, and the prompt then falls back to its "cannot record such a thing - infer NOTHING"
    /// claim, which is the honest rendering of a withheld list. The deployment's own view
    /// (`ScopedView::everything`) is the one caller for whom nothing is withheld: it returns the
    /// whole bundle unchanged, so the operator-facing surfaces keep absences and their declaration.
    #[must_use]
    pub fn scoped(&self, view: &ScopedView<'_>) -> Self {
        // The deployment's own view keeps the whole bundle, absences and their declaration included.
        // Only a caller-scoped view withholds them. `catalog_knowledge` renders the deployment's own
        // view by borrowing the bundle directly, but stating that here makes the function correct for
        // both and keeps the caller-scoped rule one branch old from silently applying to the
        // deployment.
        if view.is_everything() {
            return self.clone();
        }
        let mut declares = self.declares.clone();
        // Withheld, with its declaration, because an invisible absence must not read as a
        // reviewed-but-empty list - see the doc above.
        declares.withdraw(Capability::Absences);
        Self {
            declares,
            glossary: self
                .glossary
                .iter()
                .filter(|(_, entry)| visible(view, entry.means()))
                .map(|(term, entry)| (term.clone(), entry.clone()))
                .collect(),
            caveats: self
                .caveats
                .iter()
                .filter(|(_, note)| note.about().iter().all(|referent| visible(view, referent)))
                .map(|(name, note)| (name.clone(), note.clone()))
                .collect(),
            // Withheld from any caller-scoped view: no catalog-wide audience is declared, and the
            // ADR withholds an unscoped absence when none is granted. The deployment's own view keeps
            // them (handled by the `is_everything` branch above).
            absences: BTreeMap::new(),
            examples: self
                .examples
                .iter()
                .filter(|(_, note)| view.metric(note.question().metric()).is_some())
                .map(|(name, note)| (name.clone(), note.clone()))
                .collect(),
        }
    }

    /// Checks a set of notes against the definitions they are about.
    ///
    /// **It TAKES the [`Definitions`], so a `Knowledge` that was never checked against a bundle cannot
    /// exist** - the same shape [`crate::pinned::AnchorReport::verdict`] uses, and for the same
    /// reason: the rule lives beside the thing it is a rule about, so every adapter gets it rather
    /// than the first one that thought of it.
    ///
    /// **An example's question is checked against the bounds a REQUEST is held to, and that is not a
    /// second opinion.** This function used to say it was, and left [`MAX_RANGE_DAYS`] and
    /// [`MAX_DIMENSIONS`] to "the compiler" - but both are `pub const` in this same crate, so reading
    /// them here is the same opinion read from the same place. What the argument was really about is
    /// still true and is a different thing: nothing here CALLS the compiler, which lives in a crate
    /// above this one. The cost of having believed otherwise was concrete: an example asking for
    /// twenty-six years of history loaded, rendered under a sentence promising it was a question this
    /// deployment answers, and was refused the moment an agent copied it.
    pub fn assemble(definitions: &Definitions, input: KnowledgeInput) -> Result<Self, InconsistentKnowledge> {
        // The declaration first, because it is what the content is allowed to be. Walked over
        // `Capability::every` rather than over four hand-written checks, so a capability added to the
        // enum is checked here without this function being edited.
        for capability in Capability::every() {
            let supplied = input.supplied(capability);
            if supplied > 0 && !input.declares.declares(capability) {
                return Err(InconsistentKnowledge::UndeclaredContent { capability, supplied });
            }
        }
        // **Before any of the four indices are built, and on the INPUT rather than on what indexing
        // produces.** This used to run last, over the assembled bundle, on the argument that what
        // the cap bounds is the size of the rendered prompt and the input is "a different thing,
        // equal to it only for as long as indexing never drops a note." That property holds, but
        // only because four separate duplicate checks refuse rather than overwrite, not because of
        // the code's shape: a fifth index that overwrote instead of refusing would make the input
        // sum and the assembled sum diverge, and this cap would then bound the wrong one - silently,
        // with every test still green. Computing the cap first still means an oversized input is
        // refused before it pays for what indexing costs: parsing every phrase's identity, cloning
        // every referent into a map, walking the claim index for a collision. `KnowledgeInput` owns
        // its `Vec`s, so this reads memory the caller already allocated rather than allocating more -
        // the check itself is not what was expensive. The one index that GROWS its input - a caveat
        // written about relationships, expanded per metric - re-checks this cap on what it produces.
        let bytes = input.authored_bytes();
        if bytes > MAX_KNOWLEDGE_BYTES {
            return Err(InconsistentKnowledge::KnowledgeTooLarge {
                bytes,
                limit: MAX_KNOWLEDGE_BYTES,
            });
        }
        // One index across the glossary, the absences and the worked questions, because the
        // interesting failures are the ones that span them: a phrase cannot be given a meaning and
        // declared meaningless at once, and a phrase recorded as undefined cannot also be the way a
        // worked question was asked.
        let mut claims: BTreeMap<String, Claim> = BTreeMap::new();
        let glossary = Self::index_glossary(definitions, input.glossary, &mut claims)?;
        let caveats = Self::index_caveats(definitions, input.caveats, bytes)?;
        let absences = Self::index_absences(definitions, input.absences, &mut claims)?;
        let examples = Self::index_examples(definitions, input.examples, &claims)?;
        Ok(Self {
            declares: input.declares,
            glossary,
            caveats,
            absences,
            examples,
        })
    }

    fn index_glossary(
        definitions: &Definitions,
        entries: Vec<GlossaryEntry>,
        claims: &mut BTreeMap<String, Claim>,
    ) -> Result<Glossary, InconsistentKnowledge> {
        let mut indexed: Glossary = BTreeMap::new();
        for entry in entries {
            if let Some(fault) = fault_in(definitions, entry.means()) {
                return Err(glossary_fault(&fault, entry.term()));
            }
            // One entry against itself first. A synonym that is the term again, or two synonyms that
            // are one phrase, is an ambiguity inside a single document - and reporting it as
            // `AmbiguousPhrase` would name that document twice and send an author looking for a
            // second file.
            let mut mine: BTreeSet<String> = BTreeSet::new();
            for phrase in entry.phrases() {
                if !mine.insert(phrase_identity(phrase)) {
                    return Err(InconsistentKnowledge::TermIsItsOwnSynonym {
                        term: entry.term().clone(),
                        phrase: phrase.clone(),
                    });
                }
            }
            for phrase in entry.phrases() {
                let claim = Claim::Defined {
                    term: entry.term().clone(),
                };
                if let Some(Claim::Defined { term: first }) = claims.insert(phrase_identity(phrase), claim) {
                    return Err(InconsistentKnowledge::AmbiguousPhrase {
                        phrase: phrase.clone(),
                        first,
                        second: entry.term().clone(),
                    });
                }
            }
            // The term is one of the phrases just indexed, so a second entry claiming it has already
            // been refused as ambiguous. There is nothing here for this insert to displace.
            drop(indexed.insert(entry.term().clone(), entry));
        }
        Ok(indexed)
    }

    /// The caveats, with every one written about relationships expanded into one per metric.
    ///
    /// **Two passes, and the order is what makes a collision a named refusal.** Every authored name
    /// is claimed in the first, so a derived name that is already a note is refused as
    /// [`InconsistentKnowledge::DerivedCaveatNameTaken`] whichever document arrived first, rather
    /// than as a duplicate of whichever was indexed second.
    ///
    /// `bytes` is what the input was measured at. A relationship caveat is counted there as written;
    /// here its own count is taken back and each caveat it expands into is added and checked as it is
    /// built - so the cap bounds what the bundle holds, an expansion stops at the first caveat over
    /// it, and a composed bundle re-assembling these derived caveats as authored ones arrives at the
    /// same total.
    fn index_caveats(definitions: &Definitions, notes: Vec<Caveat>, bytes: usize) -> Result<Caveats, InconsistentKnowledge> {
        let mut indexed: Caveats = BTreeMap::new();
        let mut authored: BTreeSet<NoteName> = BTreeSet::new();
        let mut written_through: Vec<Caveat> = Vec::new();
        for note in notes {
            if !authored.insert(note.name().clone()) {
                return Err(InconsistentKnowledge::DuplicateCaveat {
                    name: note.name().clone(),
                });
            }
            if !note.relationships().is_empty() {
                if !note.about().is_empty() {
                    return Err(InconsistentKnowledge::CaveatAboutAndThroughRelationships {
                        name: note.name().clone(),
                    });
                }
                written_through.push(note);
                continue;
            }
            if note.about().is_empty() {
                return Err(InconsistentKnowledge::CaveatAboutNothing {
                    name: note.name().clone(),
                });
            }
            for referent in note.about() {
                if let Referent::Model { ref model } | Referent::Column { ref model, .. } = *referent {
                    return Err(InconsistentKnowledge::CaveatAboutAModel {
                        name: note.name().clone(),
                        model: model.clone(),
                    });
                }
                if let Some(fault) = fault_in(definitions, referent) {
                    return Err(caveat_fault(&fault, note.name()));
                }
            }
            // `authored` has already refused a second note with this name.
            drop(indexed.insert(note.name().clone(), note));
        }
        let mut bytes = bytes;
        for note in written_through {
            bytes = bytes.saturating_sub(note.authored_bytes());
            for (metric, dimensions) in reached_through(definitions, note.name(), note.relationships())? {
                let name = NoteName::parse(format!("{}__{metric}", note.name())).map_err(|cause| {
                    InconsistentKnowledge::DerivedCaveatNotAName {
                        caveat: note.name().clone(),
                        metric: metric.clone(),
                        cause,
                    }
                })?;
                if indexed.contains_key(&name) {
                    return Err(InconsistentKnowledge::DerivedCaveatNameTaken {
                        name,
                        caveat: note.name().clone(),
                        metric: metric.clone(),
                    });
                }
                let about = dimensions
                    .into_iter()
                    .map(|dimension| Referent::Dimension {
                        metric: metric.clone(),
                        dimension: dimension.clone(),
                    })
                    .collect();
                let derived = Caveat::new(name, about, note.body().clone());
                bytes = bytes.saturating_add(derived.authored_bytes());
                if bytes > MAX_KNOWLEDGE_BYTES {
                    return Err(InconsistentKnowledge::KnowledgeTooLarge {
                        bytes,
                        limit: MAX_KNOWLEDGE_BYTES,
                    });
                }
                drop(indexed.insert(derived.name().clone(), derived));
            }
        }
        Ok(indexed)
    }

    fn index_absences(
        definitions: &Definitions,
        notes: Vec<Absence>,
        claims: &mut BTreeMap<String, Claim>,
    ) -> Result<Absences, InconsistentKnowledge> {
        let mut indexed: Absences = BTreeMap::new();
        for note in notes {
            for phrase in note.phrases() {
                if let Some(fault) = declared_as(definitions, phrase) {
                    return Err(fault);
                }
                match claims.insert(phrase_identity(phrase), Claim::NotDefined) {
                    None => {}
                    Some(Claim::Defined { .. }) => {
                        return Err(InconsistentKnowledge::PhraseBothDefinedAndNot { phrase: phrase.clone() });
                    }
                    Some(Claim::NotDefined) => {
                        return Err(InconsistentKnowledge::DuplicateAbsence { phrase: phrase.clone() });
                    }
                }
            }
            // As in the glossary: the note's own phrase was just indexed, so a second note claiming it
            // has already been refused as a duplicate.
            drop(indexed.insert(note.phrase().clone(), note));
        }
        Ok(indexed)
    }

    /// The worked questions, and the one thing they are checked against the phrase index for.
    ///
    /// `claims` is read and not written. A worked question's phrasing is a sentence, not a claim about
    /// what a word means, so it makes no entry - the only thing being caught is the pair that makes
    /// the rendered document contradict itself: a phrase the same bundle records as deliberately
    /// undefined, shown as a question to copy.
    fn index_examples(
        definitions: &Definitions,
        notes: Vec<Example>,
        claims: &BTreeMap<String, Claim>,
    ) -> Result<Examples, InconsistentKnowledge> {
        let mut indexed: Examples = BTreeMap::new();
        for note in notes {
            Self::check_question(definitions, &note)?;
            for phrase in note.asked() {
                if matches!(claims.get(&phrase_identity(phrase)), Some(&Claim::NotDefined)) {
                    return Err(InconsistentKnowledge::ExampleAsksWhatIsDeclaredUndefined {
                        name: note.name().clone(),
                        phrase: phrase.clone(),
                    });
                }
            }
            if let Some(existing) = indexed.insert(note.name().clone(), note) {
                return Err(InconsistentKnowledge::DuplicateExample {
                    name: existing.name().clone(),
                });
            }
        }
        Ok(indexed)
    }

    /// The question in an example, checked the way the compiler would check it.
    ///
    /// Not by calling the compiler, which lives in a crate above this one - but against the same
    /// numbers, which are `pub const` in this crate and are read rather than restated. The six things
    /// checked are the six an author gets wrong by hand: a metric that has been renamed, a grain the
    /// metric never declared, a dimension it does not have, a filter value outside its allowlist, a
    /// period longer than a request may ask for, and more group-by keys than a request may carry.
    fn check_question(definitions: &Definitions, note: &Example) -> Result<(), InconsistentKnowledge> {
        let question = note.question();
        let name = note.name().clone();
        if question.metrics().len() > 1 {
            return Err(InconsistentKnowledge::ExampleNamesMultipleMetrics {
                name,
                requested: question.metrics().len(),
            });
        }
        let metric_name = question.metric().clone();
        let Some(metric) = definitions.metric(&metric_name) else {
            return Err(InconsistentKnowledge::ExampleUnknownMetric {
                name,
                metric: metric_name,
            });
        };
        if !metric.supports_grain(question.grain()) {
            return Err(InconsistentKnowledge::ExampleGrainNotSupported {
                name,
                metric: metric_name,
                grain: question.grain(),
            });
        }
        // The two request bounds, in the order `sutura_semantic::resolve` checks them: the span
        // first, because it is the one about cost.
        let days = question.range().days();
        if days > MAX_RANGE_DAYS {
            return Err(InconsistentKnowledge::ExampleRangeTooLong {
                name,
                days,
                limit: MAX_RANGE_DAYS,
            });
        }
        if question.dimensions().len() > MAX_DIMENSIONS {
            return Err(InconsistentKnowledge::ExampleTooManyDimensions {
                name,
                requested: question.dimensions().len(),
                limit: MAX_DIMENSIONS,
            });
        }
        for dimension in question.dimensions() {
            if metric.dimension(dimension).is_none() {
                return Err(InconsistentKnowledge::ExampleDimensionNotPermitted {
                    name,
                    metric: metric_name,
                    dimension: dimension.clone(),
                });
            }
        }
        for filter in question.filters() {
            let declared = metric.dimension(filter.dimension());
            for value in filter.values() {
                let permitted = declared.is_some_and(|declared| declared.permits(value));
                if !permitted {
                    return Err(InconsistentKnowledge::ExampleValueNotAllowed {
                        name,
                        metric: metric_name,
                        dimension: filter.dimension().clone(),
                        value: value.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

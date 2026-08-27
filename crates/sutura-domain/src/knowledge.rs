//! What a catalog says ABOUT what it defines: the words a question arrives in, the traps a reader
//! has to be warned about, the things deliberately NOT defined, and worked questions.
//!
//! [`crate::catalog`] holds what executes. This holds what a person needs in order to choose from it,
//! and the two are separate types on purpose: `docs/architecture.md` and `docs/concepts.md` have both
//! promised a glossary behind `SemanticCatalog` since before there was one, and the gap that promise
//! covered is the largest single difference between this repository and the reference implementation
//! it is measured against. A metric named `recurring_revenue` is unreachable to somebody who asks
//! about "monatlicher Umsatz", and a refusal naming a metric they never heard of is not an answer to
//! that.
//!
//! # The governance invariant, stated first because it is the one that can be lost
//!
//! `AGENTS.md`: *"Reading from the catalog at request time - descriptive content only, nothing that
//! selects, widens or parameterizes what executes."* Knowledge is descriptive **only while the
//! prompt is its only consumer**, and that is a property of where it is read rather than of what it
//! contains.
//!
//! So: **never add server-side phrase resolution.** [`crate::query::Query`] keeps a
//! [`MetricName`] and never gains a phrase, the glossary renders into the agent-facing prompt, and
//! the AGENT does the resolving - which puts the resolution in the agent's own transcript, where it
//! is auditable, instead of inside a service whose answer would then depend on a synonym table
//! nobody saw. "Let sutura resolve the synonyms" is the plausible next feature, it looks like a
//! convenience, and it moves the choice of what executes from a name a caller sent to a phrase match
//! a caller did not.
//!
//! There is also **no new [`crate::query::RefusalReason`] variant**, and the absence is structural
//! rather than an omission. Knowledge is checked when the bundle is loaded: a note naming a metric
//! that does not exist fails the load, so no request can reach a state where the knowledge is wrong.
//! A `PhraseNotDefined` refusal in particular would be unreachable - there is no field a caller
//! could put a phrase in - and a variant no test can provoke is exactly what
//! [`crate::query::RefusalReason`]'s own documentation refuses to carry.
//!
//! # Four kinds, and the two that were deliberately not adopted
//!
//! [`GlossaryEntry`], [`Caveat`], [`Absence`] and [`Example`]. The reference implementation keeps two
//! more, and neither is here:
//!
//! * **A `rules` kind.** Five of its seven rules are already enforced by types in this crate or are
//!   unrepresentable here - certified metrics only, no DML, a bounded time range, a join that cannot
//!   duplicate rows, one data system - so restating them in the prompt is exactly what
//!   `sutura_app::prompt`'s module documentation argues against: text teaching an agent to attempt
//!   what the surface refuses by construction. The other two are a glossary entry and a caveat, which
//!   this module has. And a `rules` kind is the only shape among the five that would be scoped to
//!   NOTHING - a body of prose about the deployment at large - which is an unscoped global text
//!   channel from the catalog into the prompt. The injection answer depends on that channel not
//!   existing: every note here is attached to something the bundle declares, and
//!   [`InconsistentKnowledge::CaveatAboutNothing`] is the check that keeps it so.
//! * **A `certified-metrics` table.** Its definitional half is already generated from the pinned
//!   bundle by `sutura_app::prompt`, so a second copy would break "one owner per artefact" and drift
//!   the first time a measure changed. The reference implementation needs the prose table because its
//!   measures are SQL strings a reader cannot check; here a measure is a closed vocabulary that
//!   renders itself. Only the ABSENCE half of that document is information the bundle does not
//!   already carry - "customer lifetime value has no definition here" - and that half is [`Absence`].
//!
//! # Capability asymmetry: an adapter DECLARES what it supports
//!
//! Not every provider has all four concepts, and the asymmetry is permanent rather than a gap to be
//! filled. A metadata service has glossary terms with synonyms and has nowhere at all to put a
//! reviewed list of what is deliberately undefined or a worked question somebody signed off; those
//! two are things only a reviewed first-party catalog can supply. That is the honest reason such a
//! catalog is not merely a degraded metadata service.
//!
//! **So the adapter states its capabilities, in [`KnowledgeCapabilities`], and the content is then
//! just content.** Declaring is the mechanism; emptiness is not evidence of anything. Two facts that
//! an empty collection cannot tell apart:
//!
//! * `not_defined` declared, nothing in it - nothing is recorded as deliberately undefined here, and
//!   the record is one somebody keeps.
//! * `not_defined` not declared - this provider has no way to record that, so the absence of an entry
//!   says nothing at all.
//!
//! The difference decides what the rendered prompt may CLAIM, which is the whole point of carrying
//! it: with `not_defined` declared, the prompt may say the list is authoritative and an agent may
//! decline a question on the strength of it; without it, the prompt must imply nothing whatever about
//! what is undefined. `sutura_app::prompt` renders that difference explicitly rather than by
//! omission.
//!
//! Two consequences follow, and both are checks rather than intentions:
//!
//! * [`Knowledge::assemble`] REFUSES content for a kind the adapter did not declare. A glossary entry
//!   arriving from a provider that declared no glossary is an adapter bug, and it fails the load at
//!   the boundary where it happened instead of rendering into a document that then claims a
//!   capability nobody said they had.
//! * The declaration is under the definition digest, because it changes what the agent is told. A
//!   deployment that silently stopped declaring `not_defined` has changed what its prompt claims, and
//!   provenance that did not move would certify the old claim.
//!
//! **The port does not grow a method per kind.** [`crate::pinned::SemanticCatalog::load`] returns one
//! bundle carrying the declaration; a provider with no glossary declares none and writes no other
//! code. A trait method per kind would mean every adapter implementing four functions, most of them
//! returning nothing, which is the "a port arrives with its implementor" rule inverted.
//!
//! # The bounds, and the measurement behind them
//!
//! Every note is authored prose, and prose reaches the agent-facing prompt. So it is bounded, and
//! bounded **at load** rather than truncated at render: a description cut to fit makes the prompt say
//! something the author did not write, about a bundle whose digest certifies the text as it stands.
//!
//! What was measured before the numbers were chosen. The reference implementation's entire knowledge
//! tree is 6231 bytes across seven files, and its largest single note is 1566 bytes. The largest prose
//! body already in this repository's own example catalog is 3513 bytes over 51 lines
//! (`revenue_per_churned_subscription.md`), and that document is at the far end of how much argument
//! one definition has ever needed here.
//!
//! * [`MAX_NOTE_BODY_BYTES`] is 4 KiB - a sixth more than the longest body this repository has ever
//!   written, and two and a half times the longest note the reference implementation has. A note that
//!   does not fit is not a note, it is a document.
//! * [`MAX_NOTE_LINES`] is 200, about four times that same body's 51. It exists beside the byte cap
//!   rather than instead of it because the two bound different things: four thousand newlines are
//!   four thousand lines of a rendered prompt and well inside the byte budget.
//! * [`MAX_KNOWLEDGE_BYTES`] is 32 KiB of authored text across every note - five times the reference
//!   implementation's whole tree, and eight notes at the per-note cap. It exists because per-note
//!   caps alone let N conforming notes do what one oversized note cannot, and the thing being bounded
//!   is the size of one prompt rather than the size of one file.
//!
//! Argued the way [`crate::query::MAX_RANGE_DAYS`] is argued, and with the same honesty about what is
//! not bounded: this caps the text, not the persuasiveness of it. Nothing here can catch prose that
//! misleads without escaping. What bounds that is the same thing that bounds a metric description - a
//! catalog is reviewed, authored content whose digest moves when a word of it changes.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{DimensionName, MetricName, identifier_newtype};
use crate::text::{first_invisible, is_invisible};

// Two splits rather than one file, because `cargo xtask max-lines` fails at a thousand lines under
// `crates/` and cannot be exempted. The seams are real ones: `note` holds the four records, `check`
// holds everything that runs once when a bundle is loaded, and this file holds the vocabulary a note
// is written in and the bundle they sit in. The names stay where they were - a caller still writes
// `sutura_domain::knowledge::GlossaryEntry` - because the module is the unit of API and the files are
// not.
mod bundle;
mod note;

pub use bundle::{InconsistentKnowledge, Knowledge, KnowledgeInput};
pub use note::{Absence, Caveat, Example, GlossaryEntry};

/// The longest phrase a note may claim, in characters.
///
/// Long enough for the longest thing a person says instead of a metric name - "average revenue per
/// customer per month" is 40 characters - and short enough that a sentence cannot be smuggled in as a
/// synonym. Counted in characters rather than bytes because a phrase is prose: the German half of a
/// bilingual glossary spends two bytes on a letter, and a limit in bytes would make one phrase legal
/// in one language and not in the other.
const MAX_PHRASE_CHARS: usize = 120;

/// The longest note body, in bytes.
pub const MAX_NOTE_BODY_BYTES: usize = 4 * 1024;

/// The most lines one note body may have.
pub const MAX_NOTE_LINES: usize = 200;

/// The most authored text a whole [`Knowledge`] may carry, in bytes.
pub const MAX_KNOWLEDGE_BYTES: usize = 32 * 1024;

/// Does this text carry anything a reader would see?
///
/// A character reaches the rendered document when `sutura_app::prompt::quote` keeps it - a newline, a
/// tab, or anything that is not a control character - and it draws something when it is neither
/// whitespace nor one of the invisible code points [`crate::text`] names. The invisible half is the
/// interesting one: those are not control characters, so the renderer keeps them, and they draw
/// nothing.
fn carries_prose(raw: &str) -> bool {
    raw.chars()
        .any(|character| !character.is_control() && !character.is_whitespace() && !is_invisible(character))
}

/// One line of text with its invisible characters removed and every run of whitespace as one space.
///
/// Leading and trailing whitespace goes with them, so this trims as well. Read by [`Phrase::parse`],
/// so that what is stored is what a reader sees, and by [`phrase_identity`], so that two phrases are
/// compared the way they are read.
fn collapse_spacing(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut pending = false;
    for character in raw.chars() {
        if is_invisible(character) {
            continue;
        }
        if character.is_whitespace() {
            pending = !out.is_empty();
            continue;
        }
        if pending {
            out.push(' ');
            pending = false;
        }
        out.push(character);
    }
    out
}

/// The form two phrases are compared in when the question is whether they are one phrase.
///
/// **Lower-cased, and that is the whole difference from what [`Phrase::parse`] stores.** Case is
/// meaning inside a phrase - "MRR" is how somebody writes it - and case is NOT identity: a glossary
/// that gives "MRR" a meaning while an absence declares "mrr" undefined renders both lines, three
/// sections apart, about what a reader sees as one word. So the authored spelling is kept as the
/// value everywhere, and this is what the index is keyed on.
///
/// A free function rather than a method, for the reason [`super::bundle::identifier_shape`] gives:
/// the only legitimate use of the result is to notice that two documents disagree, and a phrase
/// should not offer to fold its own case for anybody else.
pub(super) fn phrase_identity(phrase: &Phrase) -> String {
    collapse_spacing(&phrase.as_str().to_lowercase())
}

/// A natural-language phrase: what somebody says instead of a metric name.
///
/// **Not an identifier, and the difference is the reason for the type.** Spaces are legitimate,
/// non-ASCII letters are legitimate, and mixed case is meaning rather than noise - "monatlicher
/// Umsatz" and "monthly recurring revenue" are the values this exists to hold. So it cannot reuse
/// [`crate::model::InvalidIdentifier`]'s parser, whose whole job is to refuse those.
///
/// What it refuses is what makes a phrase unusable as one: nothing, a newline or any other control
/// character - a phrase is one line, and the prompt renders it inline, so a newline in one writes a
/// line of that document - a run of punctuation with no letter or digit in it, and anything long
/// enough to be a sentence.
///
/// **What it NORMALISES is the other half, and it is there so that two phrases a reader cannot tell
/// apart cannot both exist.** Runs of whitespace collapse to one space and the invisible code points
/// `crate::text::is_invisible` names are dropped, so "monthly  revenue" and "monthly revenue" are one
/// value, and neither a zero-width space nor a soft hyphen inside "mrr" is a second spelling of it -
/// the second of those is the one that was getting through, because the set here used to be a
/// narrower copy of the set authored SQL is held to. Case is deliberately
/// NOT folded here - it is meaning, and the prompt renders the spelling an author chose - which is
/// why identity is [`phrase_identity`] and not this type's `Eq`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
// Without this the derived `Deserialize` writes straight into the private field, and the one path
// that carries a catalog file bypasses every check in `parse`.
#[serde(try_from = "String")]
pub struct Phrase(String);

/// Why a phrase was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidPhrase {
    /// Empty or whitespace-only, invisible code points included. A synonym for nothing resolves
    /// everything.
    #[error("a phrase must not be empty")]
    Empty,
    /// Holds a control character, a newline included. The prompt renders a phrase inline, so a
    /// newline here writes a line of a document nobody authored.
    #[error("a phrase must be one line and must not contain control characters: {value:?}")]
    ControlCharacter { value: String },
    /// No letter and no digit. **The clause that catches a scalar that is not text**: a `term: ~` in
    /// a YAML document reaches this constructor as the one-character string it prints as, and would
    /// otherwise render into the prompt as a glossary entry for a tilde. It refuses the placeholders
    /// somebody meant to fill in later for the same reason - a row of hyphens or of full stops.
    #[error("a phrase must contain a letter or a digit: {value:?} has neither")]
    NotAWord { value: String },
    #[error("a phrase may be at most {limit} characters, {value:?} has {len}")]
    TooLong { value: String, len: usize, limit: usize },
}

impl Phrase {
    /// Parses a phrase, refusing anything that is not one and normalising what is.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidPhrase> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidPhrase::Empty);
        }
        // Before the normalisation, and reported as the author wrote it. A newline collapsed to a
        // space first would be accepted in silence, and the value in the message is what somebody
        // has to find in a file.
        if trimmed.chars().any(char::is_control) {
            return Err(InvalidPhrase::ControlCharacter {
                value: String::from(trimmed),
            });
        }
        let phrase = collapse_spacing(trimmed);
        // Reachable only now that the invisible code points are dropped: a phrase of nothing but
        // zero-width spaces normalises to nothing.
        if phrase.is_empty() {
            return Err(InvalidPhrase::Empty);
        }
        if !phrase.chars().any(char::is_alphanumeric) {
            return Err(InvalidPhrase::NotAWord { value: phrase });
        }
        // Counted after normalising, because the bound is on what a reader has to read.
        let len = phrase.chars().count();
        if len > MAX_PHRASE_CHARS {
            return Err(InvalidPhrase::TooLong {
                value: phrase,
                len,
                limit: MAX_PHRASE_CHARS,
            });
        }
        Ok(Self(phrase))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Delegates to [`Phrase::parse`]: one constructor is the source of truth, and `serde(try_from)`
/// above is what makes this the deserialization path.
impl TryFrom<String> for Phrase {
    type Error = InvalidPhrase;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for Phrase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Bounded authored prose: the body of one note.
///
/// **Refused at load when it is over the cap, and never truncated at render.** A truncated
/// description makes the rendered prompt say something the author did not write, about a definition
/// whose digest certifies the text as it stands - and it would do so silently, because nothing
/// downstream of the render can tell a cut body from a short one. The module documentation records
/// what was measured to choose the numbers.
///
/// Newlines and tabs are content here, where [`Phrase`] refuses them: a body is a markdown block and
/// its paragraph breaks are the author's. Other control characters survive parsing and are dropped by
/// the renderer, which is the one place that knows what it is rendering into - so the emptiness check
/// is made on what the renderer will keep and a body that would draw nothing is refused rather than
/// rendered as a heading over blank space.
///
/// **What does NOT survive parsing is an invisible or direction-changing code point, and unlike a
/// [`Phrase`] a body is not normalised** - it is refused, naming the character. The two types differ
/// because what they are is different: a phrase is a key, so two spellings that read as one word have
/// to become one value, and a body is prose a person reviewed, so silently editing it would make the
/// rendered document differ from the text the definition digest certifies. This is the same argument
/// [`crate::expression::InvalidFragment::InvisibleCharacter`] makes for authored SQL, at the one
/// remaining channel that carried reviewed prose into an agent's context verbatim: a body reading
/// `status = 'active'` in every terminal and every diff, saying something else, under a digest taken
/// over text nobody read - CVE-2021-42574 with the fragment replaced by a paragraph.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct NoteBody(String);

/// Why a note body was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidNoteBody {
    /// Nothing a reader would see. A note with no body is a claim with no reason attached, and the
    /// prompt would render a heading over empty space - so this covers whitespace, control
    /// characters the renderer drops, and the zero-width code points that draw nothing, as well as
    /// the empty string.
    #[error("a note body must not be empty")]
    Empty,
    /// One of them, mixed into prose. **Separate from [`Self::Empty`], because a body made ENTIRELY
    /// of these characters was already refused and a body with one in the middle of a sentence was
    /// not** - and the second is the dangerous one: the first renders as a blank heading somebody
    /// notices, the second renders as a paragraph that reads correctly and is not what it says.
    ///
    /// It is a second refusal beside the emptiness check rather than a widening of it for the reason
    /// [`crate::expression::InvalidFragment::InvisibleCharacter`] gives: `char::is_control` is false
    /// for every one of these - general category `Cf`, not `Cc` - so nothing that tests for a control
    /// character can see one. The code is reported because an author cannot find the character by
    /// looking at the file.
    #[error("a note body may not contain the invisible or direction-changing character {code:#06x}")]
    InvisibleCharacter { code: u32 },
    /// Over [`MAX_NOTE_BODY_BYTES`]. The document does not load; it is not shortened.
    #[error("a note body may be at most {limit} bytes, this one has {len}")]
    TooLong { len: usize, limit: usize },
    /// Over [`MAX_NOTE_LINES`]. Separate from the byte cap because four thousand newlines are four
    /// thousand lines of a rendered prompt and eight kilobytes of nothing.
    #[error("a note body may be at most {limit} lines, this one has {len}")]
    TooManyLines { len: usize, limit: usize },
}

impl NoteBody {
    /// Parses a body, rejecting anything too large to be one.
    ///
    /// The lengths are reported without the offending text, which is the opposite of what
    /// [`InvalidPhrase`] does and is deliberate: a phrase is short enough to name in a message and a
    /// four-kilobyte body is not, so the error says how much there was and where the limit is.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidNoteBody> {
        let trimmed = raw.as_ref().trim();
        // Emptiness is decided on what a READER will see, not on what the file holds.
        // `sutura_app::prompt::quote` drops every control character other than a newline or a tab,
        // and an invisible code point draws nothing at all - so a body of bell characters, of
        // zero-width spaces, or of one byte-order mark used to pass this check and then render as a
        // heading over empty space, which is the exact state the check exists to make unreachable.
        if !carries_prose(trimmed) {
            return Err(InvalidNoteBody::Empty);
        }
        // Second, and in this order deliberately: a body made of nothing but these characters is
        // EMPTY, which is the more accurate thing to tell its author, and a body with one mixed into
        // a sentence is the defect this refusal exists for. Reversing the two would report a blank
        // note as a hidden character.
        if let Some(offending) = first_invisible(trimmed) {
            return Err(InvalidNoteBody::InvisibleCharacter {
                code: u32::from(offending),
            });
        }
        if trimmed.len() > MAX_NOTE_BODY_BYTES {
            return Err(InvalidNoteBody::TooLong {
                len: trimmed.len(),
                limit: MAX_NOTE_BODY_BYTES,
            });
        }
        let lines = trimmed.lines().count();
        if lines > MAX_NOTE_LINES {
            return Err(InvalidNoteBody::TooManyLines {
                len: lines,
                limit: MAX_NOTE_LINES,
            });
        }
        Ok(Self(String::from(trimmed)))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NoteBody {
    type Error = InvalidNoteBody;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

identifier_newtype! {
    /// The name of one note: the handle a reviewer, a log line or an error uses for it.
    ///
    /// An identifier and not a [`Phrase`], because it names a document rather than saying anything: a
    /// caveat is referred to by name in a review the way a metric is, and the parser that keeps a
    /// metric name spellable keeps this one greppable. The macro is [`crate::model`]'s, so there is
    /// one identifier parser in this crate and nothing for a second one to drift from.
    NoteName
}

/// What one note is about: something the pinned bundle declares.
///
/// **There is deliberately no variant for a model, a table or a column, and that absence is load
/// bearing rather than tidy.** A caller cannot ask about any of the three - [`crate::query::Query`] has no field
/// for one - and a name in an agent's context is a name it will eventually try to use. Every
/// STRUCTURED rendering `sutura_app::prompt` builds out of a note - the glossary line, a caveat's
/// scope, the request in a worked question - is rendered from a `Referent`, so none of them CAN name
/// a model, a table or a column, whatever an author writes. That is the claim the type holds up, and
/// it is worth stating at its real width:
///
/// * **The structured renderings cannot name one.** There is no variant to put it in, so this half
///   is a property of the type rather than a review of each rendering.
/// * **[`Phrase`] and [`NoteBody`] are free text, and both reach the rendered document.** Nothing
///   here stops an author writing a column name into a glossary term or a note body, and the
///   pre-existing metric-description channel already carries such names into the prompt - the
///   shipped example catalog's own prose names `mrr_cents` and `status`, and
///   `sutura-cli`'s prompt snapshot records that it does. Prose is bounded, authored, reviewed
///   content whose digest moves when a word of it changes; it is not mechanically constrained, and
///   claiming otherwise would be claiming the wrong mechanism.
///
/// A load-time scan of every phrase and body for the bundle's own model, table and column names
/// would close the second half. It is not here: it is a larger change than the type-level property
/// needs, it would make an authored note refuse for naming a column in a sentence about why the
/// column is not the thing being asked for, and the honest statement of what holds is the cheaper
/// half of it.
///
/// It carries `Deserialize` as well as `Serialize`, for the same reason [`crate::measure::Measure`]
/// does: this IS the on-disk shape, and a mirror of it in the adapter would be a second place to
/// forget the next variant.
///
/// **Flat on disk, and read through [`ReferentRepr`] rather than by an external tag**, which is
/// [`crate::measure::Term`]'s decision and its argument applies here unchanged. The one-key mapping
/// the rest of this format uses would spell the commonest referent
/// `means: { metric: { metric: recurring_revenue } }`: the tag word and the field word are the same
/// word, so the nesting says nothing. `#[serde(untagged)]` is not the way out either - it reports
/// "data did not match any variant", which names nothing. So a referent is one flat mapping with a
/// `deny_unknown_fields` struct behind it, a misspelled key is an error naming the typo, and the one
/// combination that is not a referent - a value with no dimension - is an [`InvalidReferent`] that
/// says so.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "ReferentRepr", into = "ReferentRepr")]
pub enum Referent {
    /// The metric as a whole.
    Metric { metric: MetricName },
    /// One dimension of one metric. Scoped to the metric because a dimension is: two metrics may
    /// declare `segment` over the same column and still not permit the same questions about it.
    Dimension { metric: MetricName, dimension: DimensionName },
    /// One declared value of one dimension of one metric.
    Value {
        metric: MetricName,
        dimension: DimensionName,
        value: String,
    },
}

impl Referent {
    /// The metric every referent is scoped to.
    #[inline]
    pub const fn metric(&self) -> &MetricName {
        match *self {
            Self::Metric { ref metric } | Self::Dimension { ref metric, .. } | Self::Value { ref metric, .. } => metric,
        }
    }

    /// The dimension, when this referent names one.
    #[inline]
    pub const fn dimension(&self) -> Option<&DimensionName> {
        match *self {
            Self::Metric { .. } => None,
            Self::Dimension { ref dimension, .. } | Self::Value { ref dimension, .. } => Some(dimension),
        }
    }

    /// The value, when this referent names one.
    #[inline]
    pub fn value(&self) -> Option<&str> {
        match *self {
            Self::Metric { .. } | Self::Dimension { .. } => None,
            Self::Value { ref value, .. } => Some(value),
        }
    }

    /// The authored bytes of this referent.
    ///
    /// Counted rather than treated as free, because a referent's names are bounded individually by
    /// the bundle and a `Vec` of them is not: a caveat listing one referent ten thousand times is
    /// inside every per-note cap and is not a note.
    fn authored_bytes(&self) -> usize {
        self.dimension()
            .map_or(0, |d| d.as_str().len())
            .saturating_add(self.value().unwrap_or_default().len())
            .saturating_add(self.metric().as_str().len())
    }
}

/// The on-disk shape of a [`Referent`], and the only place that shape is decided.
///
/// It exists to be converted, the way [`crate::measure::Term`]'s does. `deny_unknown_fields` is the
/// most useful line in it: without it `dimesion:` is dropped in silence and a note that meant to be
/// about one dimension becomes one about a whole metric, with nothing anywhere saying so.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferentRepr {
    metric: MetricName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dimension: Option<DimensionName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

/// Why a referent was rejected.
///
/// One variant, because there is one combination of the three fields that is not a referent. A
/// missing `metric` is a serde missing-field error naming the field, which is a better message than
/// anything this enum could produce for it.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidReferent {
    #[error("a value belongs to a dimension: `value: {value:?}` on metric {metric} has no `dimension` beside it")]
    ValueWithoutDimension { metric: MetricName, value: String },
}

impl TryFrom<ReferentRepr> for Referent {
    type Error = InvalidReferent;

    fn try_from(repr: ReferentRepr) -> Result<Self, Self::Error> {
        match (repr.dimension, repr.value) {
            (None, None) => Ok(Self::Metric { metric: repr.metric }),
            (Some(dimension), None) => Ok(Self::Dimension {
                metric: repr.metric,
                dimension,
            }),
            (Some(dimension), Some(value)) => Ok(Self::Value {
                metric: repr.metric,
                dimension,
                value,
            }),
            (None, Some(value)) => Err(InvalidReferent::ValueWithoutDimension {
                metric: repr.metric,
                value,
            }),
        }
    }
}

/// The other direction, so that what a digest is taken over and what a catalog wrote are the same
/// text. Not a convenience: a serialized shape that differed from the on-disk one would make the
/// bundle snapshot a description of serde rather than of the catalog.
impl From<Referent> for ReferentRepr {
    fn from(referent: Referent) -> Self {
        match referent {
            Referent::Metric { metric } => Self {
                metric,
                dimension: None,
                value: None,
            },
            Referent::Dimension { metric, dimension } => Self {
                metric,
                dimension: Some(dimension),
                value: None,
            },
            Referent::Value {
                metric,
                dimension,
                value,
            } => Self {
                metric,
                dimension: Some(dimension),
                value: Some(value),
            },
        }
    }
}

/// The authored bytes of a run of phrases.
fn phrase_bytes<'a>(phrases: impl Iterator<Item = &'a Phrase>) -> usize {
    sum_bytes(phrases.map(|phrase| phrase.as_str().len()))
}

/// Saturating, because a count of authored bytes must not be able to wrap into a small number and
/// pass the cap it exists to fail.
fn sum_bytes(counts: impl Iterator<Item = usize>) -> usize {
    counts.fold(0, usize::saturating_add)
}

/// One thing a provider can declare it supports.
///
/// A closed set, for the reason [`crate::model::Aggregate`] is one: the alternative is a string, and a
/// provider that declared `"glossaries"` or `"Glossary "` would silently declare nothing at all. With
/// an enum, an unrecognised capability is a parse error naming what was written and listing what
/// exists.
///
/// **The variants are named after the domain concepts rather than after the words a document writes.**
/// `Absences` here is `kind: not_defined` in a markdown file; the format's word says what an author is
/// doing, and this one says what the thing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// The business glossary: phrases, and the one thing each of them means.
    Glossary,
    /// Notes a reader has to see before trusting a number.
    Caveats,
    /// A reviewed list of terms that are deliberately not defined.
    Absences,
    /// Worked questions: how somebody asked, and what to send.
    Examples,
}

// `Capability::every` is seeded with `Glossary`, and this is what makes that a derived fact rather
// than a hand-written guess. A variant declared above `Glossary` answers `Some(..)` to
// `Capability::previous` and this file stops compiling - which is the failure the walk itself cannot
// produce: `every` would simply start one variant late, and `Knowledge::assemble`'s
// undeclared-content guard would never look at the new kind.
//
// `const _` rather than a named constant: a name would be an item nothing reads, and `dead_code` is
// denied in this workspace.
const _: () = assert!(
    Capability::Glossary.previous().is_none(),
    "Capability::every is seeded with Glossary, so no capability may be declared before it"
);

impl Capability {
    /// The next capability in declaration order, or `None` at the end.
    ///
    /// **This function exists in order not to compile.** It is one of the two exhaustive matches over
    /// this enum in this crate, and [`Self::every`] is derived from it - so a fifth capability is a
    /// compile error here rather than a variant that quietly never appears in a list. A hand-written
    /// `ALL` array is the classic version of that bug: it type-checks with a variant missing, and the
    /// missing one is then invisible everywhere the array is walked.
    ///
    /// The other place a `match` on this enum has to be total is `sutura_app::prompt::knowledge`, where
    /// each capability decides what the rendered document may CLAIM. That one is deliberately not
    /// derived from this: what a capability licenses a prompt to say is not something the domain gets
    /// to know.
    const fn next(self) -> Option<Self> {
        match self {
            Self::Glossary => Some(Self::Caveats),
            Self::Caveats => Some(Self::Absences),
            Self::Absences => Some(Self::Examples),
            Self::Examples => None,
        }
    }

    /// The previous capability in declaration order, or `None` at the start.
    ///
    /// **The other half of [`Self::next`], and it exists because the SEED was the hole.** [`Self::every`]
    /// starts at `Glossary`, and that name was written by hand - so a variant declared BEFORE
    /// `Glossary` left every match in this crate exhaustive and was invisible to `every`, to
    /// [`KnowledgeCapabilities::all`], to the rendered declaration, and to [`Knowledge::assemble`]'s
    /// undeclared-content guard, which walks `every`. A review inserted one there, made every match
    /// total, and both of the guard's own tests still passed: content for the new capability was never
    /// looked at, because the walk never reached it.
    ///
    /// So the seed is checked rather than trusted. The const assertion below this `impl` block reads
    /// this function, so a variant declared before `Glossary` fails to compile in this file rather than
    /// disappearing out of a list. One more match the compiler forces, which is the same trade
    /// [`Self::next`] already makes.
    const fn previous(self) -> Option<Self> {
        match self {
            Self::Glossary => None,
            Self::Caveats => Some(Self::Glossary),
            Self::Absences => Some(Self::Caveats),
            Self::Examples => Some(Self::Absences),
        }
    }

    /// Every capability there is, in declaration order.
    ///
    /// Derived from [`Self::next`] rather than listed, and seeded by the one variant
    /// [`Self::previous`] answers `None` for - which is asserted where this enum is declared rather
    /// than assumed by whoever reads the line.
    pub fn every() -> impl Iterator<Item = Self> {
        core::iter::successors(Some(Self::Glossary), |current| current.next())
    }

    /// The word this capability answers to, in a message and in a declaration.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Glossary => "glossary",
            Self::Caveats => "caveats",
            Self::Absences => "absences",
            Self::Examples => "examples",
        }
    }
}

impl core::fmt::Display for Capability {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one provider declares it supports.
///
/// **The declaration is the mechanism, and emptiness is not evidence.** A metadata service has
/// glossary terms and no way to record that a term is deliberately undefined; a reviewed first-party
/// catalog has both. Which of the two is talking decides what the rendered prompt may claim, and an
/// empty collection cannot say - so the adapter says, here, and the content is then just content.
///
/// A `BTreeSet` rather than four booleans: the order is content order, which the digest needs, and
/// adding a capability does not add a field to every construction site. Wrapped in a newtype with the
/// usual treatment - private field, one constructor, accessors, no `Deref` - because
/// `xtask check-boundaries` fails a `pub` field on a `pub struct` and the rule does not make an
/// exception for a type with no invariant to protect.
///
/// The constructor is infallible, for the reason [`crate::identity::Secret::new`] gives: any set of
/// capabilities is a legitimate declaration, and a `Result` here would be inventing an invariant. What
/// is NOT legitimate is content for a capability that was not declared, and that is
/// [`Knowledge::assemble`]'s to refuse.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeCapabilities(BTreeSet<Capability>);

impl KnowledgeCapabilities {
    /// The capabilities a provider says it has.
    pub fn of(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self(capabilities.into_iter().collect())
    }

    /// A provider with none of them.
    #[must_use]
    pub const fn none() -> Self {
        Self(BTreeSet::new())
    }

    /// Every capability there is.
    ///
    /// **What a REFERENCE adapter declares, and it means more than "all four today".** A provider
    /// calling this is saying it supports whatever kinds exist, including ones added later - which is
    /// true of a reviewed first-party catalog, whose format grows with the domain, and is not true of
    /// anything that maps a fixed external schema. Such an adapter lists its capabilities with
    /// [`Self::of`], so a new kind leaves its declaration alone.
    #[must_use]
    pub fn all() -> Self {
        Self::of(Capability::every())
    }

    /// Does this provider support that kind at all?
    #[inline]
    pub fn declares(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }

    /// Everything declared, in a deterministic order.
    #[inline]
    pub const fn declared(&self) -> &BTreeSet<Capability> {
        &self.0
    }

    /// Is nothing at all declared?
    ///
    /// Read by the prompt: a provider that declares nothing gets no section about what it records,
    /// because there is no distinction to draw and four lines saying "not recorded" is noise rather
    /// than information.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The glossary, keyed by the term each entry defines.
///
/// An alias rather than the map written out at every use site, and not only for reading: the
/// workspace's `type_complexity` threshold is 100 against clippy's default 250, so
/// `Result<BTreeMap<Phrase, GlossaryEntry>, InconsistentKnowledge>` is a lint. Naming the four
/// collections is the fix the lint asks for and the one that makes the accessors read as what they
/// return.
pub type Glossary = BTreeMap<Phrase, GlossaryEntry>;

/// The caveats, keyed by name.
pub type Caveats = BTreeMap<NoteName, Caveat>;

/// The terms declared undefined, keyed by the phrase each note is about.
pub type Absences = BTreeMap<Phrase, Absence>;

/// The worked examples, keyed by name.
pub type Examples = BTreeMap<NoteName, Example>;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod refusal_tests;

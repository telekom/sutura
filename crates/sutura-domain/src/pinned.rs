//! The snapshot a question is answered against, and the port it arrives through.
//!
//! Definitions do not arrive live. They arrive as a [`PinnedDefinitions`]: a whole
//! [`crate::catalog::Definitions`], the [`crate::knowledge::Knowledge`] written about it, a version,
//! and a digest over the canonical form of both. Two consequences follow, and both are the reason
//! this type exists rather than passing `Definitions` around directly.
//!
//! A catalog edit cannot change what a question means between two invocations, because the bundle a
//! request resolves against was fixed before the request arrived. It changes the digest instead, and
//! the digest travels with the answer.
//!
//! And the catalog cannot see who is asking. [`SemanticCatalog::load`] takes no request context, so
//! there is nothing for an implementation to branch on. A trait that accepted one could return a
//! different definition to different callers, which would make the pinning meaningless and the
//! provenance a lie.
//!
//! **One thing this module deliberately does NOT hold, and one it now does.** It cannot execute a
//! statement, so it holds [`AnchorReport`] - the evidence - and [`AnchorReport::verdict`] - the rule -
//! but not the proof. The proof is `sutura_app::Validated`, whose only constructor is
//! `sutura_app::verify_and_validate` and therefore cannot be reached without a `Warehouse` having
//! been called. A report is public data anybody can build, and nothing anybody builds here turns
//! into a bundle the service will serve.
//!
//! What it does hold is the hashing. [`PinnedDefinitions::pin`] takes a version, a set of definitions
//! and the knowledge about them, and nothing else: the digest is computed here, from the values being
//! stored, by `DefinitionDigest::of`. The previous shape took the hash *function* from its
//! caller, on the argument that the domain could not hash - and that left the hole intact, because a
//! function handed the definitions is not a function that read them. `crate::definitions` says what
//! the twelve allowlisted crates bought.

use std::collections::BTreeMap;

use crate::capabilities::MetadataCapabilities;
use crate::catalog::{Anchor, Definitions};
use crate::definitions::{DefinitionDigest, NotDigestible};
use crate::knowledge::Knowledge;
use crate::model::{MetricName, SourceName};
use crate::query::RefusalReason;
use crate::source::ExecutedAs;
use crate::text::first_invisible;

/// The longest version label we accept. Long enough for a commit id plus a tag, short enough that
/// it cannot be used to smuggle a paragraph into an audit record.
const MAX_VERSION_LEN: usize = 128;

/// Which snapshot of the definitions this is.
///
/// Free-form on purpose, because what identifies a snapshot differs per catalog: a commit id, a
/// build number, an export timestamp. What is *not* free-form is its shape, because it is echoed
/// into provenance and into an audit record, and a value with a newline in it can forge a second
/// record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct DefinitionVersion(String);

/// Why a version label was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidVersion {
    /// Empty or whitespace-only. An unversioned snapshot must not be able to claim it is one.
    #[error("a definition version must not be empty")]
    Empty,
    /// Holds a control character. This is the one that matters: provenance is written to an audit
    /// sink line by line, so a newline here appends a record nobody wrote.
    #[error("a definition version must not contain control characters: {value:?}")]
    ControlCharacter { value: String },
    /// Holds an invisible or direction-changing code point. **The second half of the reason the
    /// variant above exists**, and it was missing: `char::is_control` is false for every one of
    /// these (general category `Cf`, not `Cc`), so the check that refuses a newline could not see a
    /// right-to-left override, and a version label carrying one passed.
    ///
    /// That matters here for exactly the reason the control-character refusal states. A version
    /// travels in [`Provenance`], and provenance is written to an audit sink line by line, echoed
    /// into the body of an answer, and printed by the compile command - so a label that renders as
    /// `v2.1` in every one of those places and is a different string is a provenance record that
    /// cannot be matched against the bundle it names. The digest is what makes an answer checkable;
    /// two labels a reader cannot tell apart is the one way to make the version half of it useless.
    ///
    /// The code is reported rather than the value, which is the opposite of the variant above: a
    /// label whose only defect is a character that draws nothing would print as though it were
    /// correct, so the message names the character instead.
    #[error("a definition version must not contain the invisible or direction-changing character {code:#06x}")]
    InvisibleCharacter { code: u32 },
    #[error("a definition version may be at most {limit} characters, {value:?} has {len}")]
    TooLong { value: String, len: usize, limit: usize },
}

impl DefinitionVersion {
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidVersion> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidVersion::Empty);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(InvalidVersion::ControlCharacter {
                value: String::from(trimmed),
            });
        }
        // Beside the control-character check rather than folded into it, because it is a second
        // character class the first one provably cannot see. `crate::text` owns the set, so this
        // refusal and the one an authored SQL fragment gets are the same refusal.
        if let Some(offending) = first_invisible(trimmed) {
            return Err(InvalidVersion::InvisibleCharacter {
                code: u32::from(offending),
            });
        }
        if trimmed.chars().count() > MAX_VERSION_LEN {
            return Err(InvalidVersion::TooLong {
                value: String::from(trimmed),
                len: trimmed.chars().count(),
                limit: MAX_VERSION_LEN,
            });
        }
        Ok(Self(String::from(trimmed)))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DefinitionVersion {
    type Error = InvalidVersion;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for DefinitionVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What defined an answer, and what each of its legs executed as - travelling with it.
///
/// A result cannot be separated from what defined it, so this is a typed field a caller reads
/// deliberately rather than a sentence concatenated into a channel that also carries instructions.
///
/// # Two halves with two different owners, and the posture is BESIDE the digest rather than under it
///
/// The version and the digest identify the *authored content*: two deployments serving the same
/// catalog certify the same numbers, which is the one property the digest exists to have. The
/// posture per leg is *deployment configuration* - the same bundle may be served by a deployment that
/// impersonates and one that does not - so hashing it in would make one catalog produce two digests
/// in two deployments. That is why [`crate::source::ExecutedAs`] is a field here and not an input to
/// [`PinnedDefinitions::pin`], and it is the opposite of the knowledge declaration, which *is* under
/// the digest because it is content a catalog author wrote.
///
/// **Recording is not a control.** Provenance is read by whoever holds the answer, after the rows
/// were served, so it cannot prevent a disclosure and does not attempt to. It makes one attributable
/// and it makes a misconfiguration visible to whoever reads an answer; the thing that keeps a shared
/// source from being served unnoticed is a boot refusal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Provenance {
    version: DefinitionVersion,
    digest: DefinitionDigest,
    executed_as: ExecutedAs,
}

impl Provenance {
    /// Private, and that is the second half of the fix in this file.
    ///
    /// [`PinnedDefinitions::provenance`] is the only way to obtain one, so a `Provenance` is always a
    /// bundle's own and never a pair of values somebody chose. With a public constructor here, the
    /// digest computed inside `pin` could be bypassed one level further out: `ToolOutcome::Answer`
    /// carries a `Provenance` beside its rows, and an enum variant is always constructible by
    /// whoever can build its fields. Nothing outside this crate built one - checked before narrowing
    /// it - so this costs no caller.
    const fn new(version: DefinitionVersion, digest: DefinitionDigest, executed_as: ExecutedAs) -> Self {
        Self {
            version,
            digest,
            executed_as,
        }
    }

    #[inline]
    pub const fn version(&self) -> &DefinitionVersion {
        &self.version
    }

    #[inline]
    pub const fn digest(&self) -> &DefinitionDigest {
        &self.digest
    }

    /// What each leg of this answer ran as.
    ///
    /// Read off the posture the **adapter was handed**, never off a settings tree - see
    /// [`crate::source`]. Non-empty, because [`ExecutedAs`] has no empty form.
    #[inline]
    pub const fn executed_as(&self) -> &ExecutedAs {
        &self.executed_as
    }
}

/// An immutable, hashed snapshot of everything a catalog said.
///
/// **Two halves, and the split between them is a governance boundary rather than a filing decision.**
/// [`Definitions`] is exactly what the compiler reads - `sutura_semantic` resolves a question against
/// it and nothing else - and [`Knowledge`] is what a person reads: the glossary, the caveats, the
/// terms deliberately undefined, the worked questions. Keeping the second out of the first is what
/// makes "descriptive content only" checkable: the resolver is handed a bundle whose `definitions()`
/// carries no prose channel at all, so reaching the knowledge would mean naming
/// [`Self::knowledge`] in the query path, which is a one-line diff a reviewer sees rather than a
/// property somebody has to remember. `sutura_app::prompt` is the only thing in this workspace that
/// names it.
///
/// The digest covers both. A glossary decides which metric an agent asks about, so a bundle whose
/// glossary changed answers different questions from the same words - `crate::definitions` argues it
/// where the hashing is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PinnedDefinitions {
    version: DefinitionVersion,
    digest: DefinitionDigest,
    definitions: Definitions,
    knowledge: Knowledge,
}

impl PinnedDefinitions {
    /// Pins a set of definitions and the knowledge about them, computing the digest here, from both.
    ///
    /// **Three arguments, and the absence of a fourth is the mechanism.** This constructor has been
    /// wrong twice, and the second time is the more interesting one:
    ///
    /// * `new(version, digest, definitions)` took any syntactically valid digest next to any
    ///   [`Definitions`] and conceded in its own comment that the one need not describe the other.
    /// * `pin(version, definitions, digest_fn)` then took the hash *function* from its caller, on
    ///   the argument that the domain could not hash. A review re-tested it and it was still
    ///   forgeable: safe public code could pass `|_| Ok(elsewhere)`, and a unit test that inspected
    ///   `given.metrics().len()` inside the closure proved only that the closure had been handed the
    ///   definitions - not that the digest it returned described them. **Passing content to
    ///   untrusted code is not the same as that code having used it.**
    ///
    /// So the canonical digest operation is the constructor boundary now. `pin` calls
    /// `DefinitionDigest::of` on the values it is about to store, and there is no parameter, closure
    /// or trait through which a caller can influence what the digest is taken over.
    /// `crate::definitions` holds the canonical form, the hash, and the measured cost of the two
    /// dependencies that made it possible.
    ///
    /// The knowledge argument arrived after both of those corrections and did not reopen either: it is
    /// a third piece of CONTENT, hashed with the rest, and not a third opinion about the hashing.
    ///
    /// The forgery a caller could write before does not compile - there is no parameter to pass it
    /// as, and adding the knowledge did not add one:
    ///
    /// ```compile_fail
    /// use core::convert::Infallible;
    /// use sutura_domain::catalog::Definitions;
    /// use sutura_domain::definitions::DefinitionDigest;
    /// use sutura_domain::knowledge::Knowledge;
    /// use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};
    ///
    /// // The digest of some OTHER catalog, returned by a closure that ignores its argument.
    /// fn _forged(
    ///     version: DefinitionVersion,
    ///     definitions: Definitions,
    ///     knowledge: Knowledge,
    ///     elsewhere: DefinitionDigest,
    /// ) -> Result<PinnedDefinitions, Infallible> {
    ///     PinnedDefinitions::pin(version, definitions, knowledge, |_| Ok(elsewhere))
    /// }
    /// ```
    ///
    /// Nor is there a way past the constructor. The fields are private, so the struct literal that
    /// would pair them by hand is not a struct literal a caller can write:
    ///
    /// ```compile_fail
    /// use sutura_domain::catalog::Definitions;
    /// use sutura_domain::definitions::DefinitionDigest;
    /// use sutura_domain::knowledge::Knowledge;
    /// use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};
    ///
    /// fn _by_hand(
    ///     version: DefinitionVersion,
    ///     digest: DefinitionDigest,
    ///     definitions: Definitions,
    ///     knowledge: Knowledge,
    /// ) -> PinnedDefinitions {
    ///     PinnedDefinitions { version, digest, definitions, knowledge }
    /// }
    /// ```
    ///
    /// And the twin of both blocks, which pins the signature so that a rename cannot make either of
    /// them pass vacuously:
    ///
    /// ```
    /// use sutura_domain::catalog::Definitions;
    /// use sutura_domain::definitions::NotDigestible;
    /// use sutura_domain::knowledge::Knowledge;
    /// use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};
    ///
    /// fn _pin(
    ///     version: DefinitionVersion,
    ///     definitions: Definitions,
    ///     knowledge: Knowledge,
    /// ) -> Result<PinnedDefinitions, NotDigestible> {
    ///     PinnedDefinitions::pin(version, definitions, knowledge)
    /// }
    /// ```
    pub fn pin(version: DefinitionVersion, definitions: Definitions, knowledge: Knowledge) -> Result<Self, NotDigestible> {
        let digest = DefinitionDigest::of(&definitions, &knowledge)?;
        Ok(Self {
            version,
            digest,
            definitions,
            knowledge,
        })
    }

    #[inline]
    pub const fn definitions(&self) -> &Definitions {
        &self.definitions
    }

    /// What this bundle says ABOUT what it defines.
    ///
    /// **Read by the prompt renderer and by nothing on the query path**, which is the whole of the
    /// governance argument in [`crate::knowledge`]: a glossary is descriptive content while a person
    /// or an agent is the one resolving it, and a selecting input the moment the service does.
    #[inline]
    pub const fn knowledge(&self) -> &Knowledge {
        &self.knowledge
    }

    #[inline]
    pub const fn version(&self) -> &DefinitionVersion {
        &self.version
    }

    #[inline]
    pub const fn digest(&self) -> &DefinitionDigest {
        &self.digest
    }

    /// The provenance to attach to one answer produced from this bundle.
    ///
    /// **`executed_as` is a required argument and there is no second door that omits it.** An answer
    /// carries a `Provenance`, `Provenance::new` is private, and this is the only way to one - so an
    /// answer cannot be produced without saying which posture each of its legs ran under. That is the
    /// same shape [`Self::pin`] uses for the digest: the value is computed from what the caller
    /// already has rather than accepted as an optional decoration.
    ///
    /// A caller that only wants to *describe* this bundle - a catalog endpoint, the agent-facing
    /// prompt - reads [`Self::version`] and [`Self::digest`] instead. Nothing executed for it, and a
    /// `Provenance` with an empty execution record would be the one shape this argument exists to
    /// make unrepresentable.
    pub fn provenance(&self, executed_as: ExecutedAs) -> Provenance {
        Provenance::new(self.version.clone(), self.digest.clone(), executed_as)
    }

    /// The metrics that declare an anchor, and therefore have to be checked before serving.
    pub fn anchored_metrics(&self) -> impl Iterator<Item = (&MetricName, &Anchor)> {
        self.definitions
            .metrics()
            .iter()
            .filter_map(|(name, metric)| metric.anchor().map(|anchor| (name, anchor)))
    }
}

/// A message and every cause beneath it, on one line.
///
/// Used by the two variants of [`NotExecutedReason`] whose own cause is a type this crate cannot
/// name. Flattening the chain into `Display` is what keeps a driver's complaint reachable from
/// anything that only prints an error, which is every operator-facing surface there is.
fn flattened(message: &str, chain: &[String]) -> String {
    let mut out = String::from(message);
    for cause in chain {
        out.push_str(": ");
        out.push_str(cause);
    }
    out
}

/// Why one anchor produced no verdict at all.
///
/// Typed, one variant per branch of the check, because anchor verification is the readiness gate:
/// when it fails, this value is the whole of what an operator gets. A single `String` here was the
/// bug - `thiserror` prints only the outermost message, so an adapter error's `source` chain was
/// discarded on the way in and the operator was told "the anchor query failed" and nothing else.
///
/// Each variant sends a reader somewhere different. A missing grain is a catalog to fix, a refusal
/// is a governance outcome, a source mismatch is a composition root that opened the wrong data
/// system, and only [`NotExecutedReason::Failed`] is the data system's own fault.
///
/// Two variants carry text rather than a typed cause, and that is a boundary rather than a
/// shortcut: the `Warehouse` port's error is a generic parameter and the compiler's error lives in a
/// crate the domain must not depend on, so neither type can be stored here. Both are walked to
/// exhaustion at the call site and arrive as a message plus its chain, which is the lossless option
/// available at that boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, serde::Serialize)]
pub enum NotExecutedReason {
    /// The report names a metric the bundle does not define, so there was nothing to run.
    #[error("this bundle does not define the metric the check is about")]
    BundleMissingMetric,
    /// The metric declares no grain, so no single period - and therefore no single number - is
    /// available to compare the declared one against.
    #[error("the metric declares no grain, so an anchor range reduces to no single period")]
    NoGrain,
    /// The anchor's own question would not compile against the bundle that carries it.
    #[error("the anchor's own question would not compile: {}", flattened(.message, .chain))]
    NotCompiled { message: String, chain: Vec<String> },
    /// The anchor's own question was refused. A governance outcome, surfaced as one: an anchor a
    /// caller could not have asked for is not a failure of the data system.
    #[error("the anchor's own question was refused: {reason:?}")]
    Refused { reason: RefusalReason },
    /// The plan names a data system this process did not open. Not prose in a report field: it is
    /// the same condition the query path refuses, and it is a misconfigured composition root rather
    /// than an outage.
    ///
    /// **It used to be `SourceMismatch`, carrying the plan's source and the one warehouse's**, and
    /// that pair stopped being expressible when the service started holding a registry: a plan now
    /// SELECTS its warehouse rather than being compared against one, so the only failure left is that
    /// no data system is registered under the name. Renamed rather than kept with a second field
    /// nothing could fill, because a variant no code path can produce is one this enum refuses to
    /// carry.
    #[error("the metric reads from {plan}, and no data system is configured under that name")]
    SourceNotConfigured { plan: SourceName },
    /// The declared range covers more than one period at the metric's coarsest grain, so the result
    /// is several numbers and an anchor is one.
    #[error(
        "the anchor query returned {rows} rows, and an anchor is one number: the declared range \
         covers more than one period at the metric's coarsest grain"
    )]
    NotOneNumber { rows: usize },
    /// The result carries no column named after the metric, so there is nothing to compare.
    #[error("the result has no single column labelled {label:?}, so there is nothing to compare")]
    NoMeasureColumn { label: String },
    /// The result set was not the shape it reported.
    #[error("the result set was not the shape it reported")]
    ResultShapeMismatch,
    /// The plan the boot path compiled is not this anchor's own, so nothing executed it.
    ///
    /// **A defect in the boot path rather than anything about the catalog**, which is why it is one
    /// variant with the typed cause flattened into it rather than one per cause: whoever reads a
    /// report needs to know this anchor was not checked and why, and every way
    /// `sutura_domain::plan::AnchorPlan::of` refuses a plan is "the question compiled here was not the
    /// anchor's". Nothing in this workspace can provoke it - it is a SELF-CHECK on the boot path, not
    /// a barrier against a caller, and `AnchorPlan`'s own documentation is where that distinction is
    /// argued - and a check with no reportable outcome would have to be a panic instead.
    #[error("the plan compiled for this anchor is not the anchor's own: {}", flattened(.message, .chain))]
    NotAnAnchor { message: String, chain: Vec<String> },
    /// The data system failed the statement. `message` is the adapter's own, `chain` is every cause
    /// beneath it - the driver error included, which is the part that names a table, a column or a
    /// file and the part a single string used to throw away.
    #[error("the anchor query failed: {}", flattened(.message, .chain))]
    Failed { message: String, chain: Vec<String> },
}

/// What happened when one metric's anchor was checked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum AnchorCheck {
    /// The metric reproduced its declared number.
    Matched,
    /// It produced a different one. This is the interesting failure: the definition still runs, so
    /// nothing errors, and the number is simply not the one that was certified.
    Mismatch { expected: String, actual: String },
    /// The check could not run at all: the data system was unreachable, or the statement failed.
    ///
    /// Deliberately not merged with `Mismatch`. "It is wrong" and "we do not know" call for
    /// different operational responses, and collapsing them makes an outage look like a wrong
    /// number.
    NotExecuted { reason: NotExecutedReason },
}

/// The outcome of checking every anchor in a bundle.
///
/// Built by whoever can execute a statement, which is not this crate.
///
/// **This is evidence, and evidence is forgeable - deliberately so.** [`AnchorReport::new`] and
/// [`AnchorReport::record`] are public because an operator-facing surface has to be able to render
/// and serialize a report, and because the rule below is worth testing here, where the bundle's
/// shape lives. What used to be wrong is that this same public pair also reached the proof: a caller
/// could enumerate [`PinnedDefinitions::anchored_metrics`], record [`AnchorCheck::Matched`] for each
/// without ever opening a data system, and hand the result to a constructor that returned a bundle
/// the service would serve. So the wrapper attested to nothing but the caller's own assertion, and
/// read like proof.
///
/// The proof now lives one layer out, in `sutura_app::Validated`, whose only constructor is
/// `sutura_app::verify_and_validate` - which takes a `Warehouse` and calls it. [`Self::verdict`] is
/// the rule that constructor applies, and returns `Result<(), NotValidated>`: a verdict, not a
/// bundle. Nothing in this crate can turn a report into something servable.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct AnchorReport {
    checks: BTreeMap<MetricName, AnchorCheck>,
}

impl AnchorReport {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records what happened for one metric.
    ///
    /// Last write wins, because a re-check after a transient failure should replace it rather than
    /// accumulate. The coverage check below is on presence, so a replaced entry cannot hide one.
    pub fn record(&mut self, metric: MetricName, check: AnchorCheck) {
        drop(self.checks.insert(metric, check));
    }

    #[inline]
    pub const fn checks(&self) -> &BTreeMap<MetricName, AnchorCheck> {
        &self.checks
    }

    /// Whether this report shows every anchor `pinned` declares having matched.
    ///
    /// The rule, with no proof attached. It returns `Result<(), NotValidated>` rather than a
    /// validated bundle on purpose: this crate cannot tell whether the checks in the report ever
    /// reached a data system, so it is not the crate that gets to say a bundle is fit to serve.
    /// `sutura_app::verify_and_validate` runs the anchors and then applies this, and it is the only
    /// thing that mints the proof.
    ///
    /// The unknown-metric check is not tidiness: without it, a report built against a different
    /// bundle would satisfy the coverage check for whatever it happened to overlap, and a bundle
    /// would be "validated" by evidence about something else.
    pub fn verdict(&self, pinned: &PinnedDefinitions) -> Result<(), NotValidated> {
        for metric in self.checks.keys() {
            if pinned.definitions().metric(metric).is_none() {
                return Err(NotValidated::UnknownMetricChecked { metric: metric.clone() });
            }
        }
        for (metric, _) in pinned.anchored_metrics() {
            match self.checks.get(metric) {
                None => {
                    return Err(NotValidated::AnchorUnchecked { metric: metric.clone() });
                }
                Some(AnchorCheck::Matched) => {}
                Some(AnchorCheck::Mismatch { expected, actual }) => {
                    return Err(NotValidated::AnchorMismatch {
                        metric: metric.clone(),
                        expected: expected.clone(),
                        actual: actual.clone(),
                    });
                }
                Some(AnchorCheck::NotExecuted { reason }) => {
                    return Err(NotValidated::AnchorNotExecuted {
                        metric: metric.clone(),
                        reason: reason.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// Why a bundle is not validated.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NotValidated {
    #[error("metric {metric} was expected to produce {expected}, and produced {actual}")]
    AnchorMismatch {
        metric: MetricName,
        expected: String,
        actual: String,
    },
    /// The reason is the `source`, not the message, so whoever renders this walks the chain and
    /// gets the data system's own complaint. Interpolating it would have printed the outermost
    /// message and stopped, which is the whole of what was wrong before.
    #[error("metric {metric}'s anchor could not be checked")]
    AnchorNotExecuted {
        metric: MetricName,
        #[source]
        reason: NotExecutedReason,
    },
    #[error("metric {metric} declares an anchor and no check was recorded for it")]
    AnchorUnchecked { metric: MetricName },
    #[error("a check was recorded for {metric}, which this bundle does not define")]
    UnknownMetricChecked { metric: MetricName },
}

/// Where definitions come from.
///
/// One trait, implemented once per catalog. A directory of files in git and a metadata service over
/// HTTP are two adapters behind it, and swapping one for the other does not touch the query path.
///
/// **`load` takes no request context, and that is the whole design of this port.** A catalog that
/// could see the caller could return a different definition per caller, and then the digest that
/// travels with an answer would describe something other than what produced it.
///
/// # The declaration, and why an adapter cannot be silent
///
/// [`Self::capabilities`] says which kinds of thing this adapter can supply at all - and, by
/// omission from its own lists, which it cannot. **An absence has to be declared rather than
/// inferred from silence**, because a bundle with no metrics in it is two entirely different facts: a
/// reviewed catalog that has not certified one yet, and a source that holds a measure this
/// repository will not execute. An empty collection cannot tell those apart, and a caller deciding
/// what to trust needs to know which it is looking at.
///
/// [`crate::warehouse::Warehouse::IMPERSONATION`] is the shape this copies and its argument carries
/// over word for word: a defaulted capability would mean an adapter that said nothing got the benefit
/// of the doubt in whichever direction the default pointed, and both directions are wrong. Defaulted
/// to *supplies*, a narrow source would silently claim measures it does not have. Defaulted to *does
/// not*, a complete adapter that forgot the line would be reported as narrow and somebody would fix
/// that by deleting the check.
///
/// The rule that decides required from defaulted is that trait's, demonstrated there twice:
/// **required with no default where the absence changes what a caller may believe, defaulted with a
/// stated reason where it is a missed optimisation.** This one is the first case - `dry_run` next
/// door is the second, and says so in its own words.
///
/// **An adapter that declares no capabilities does not compile:**
///
/// ```compile_fail
/// use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog};
///
/// struct Undeclared;
///
/// // No `fn capabilities`, so this impl is incomplete: the trait declares it with no default.
/// impl SemanticCatalog for Undeclared {
///     type Error = core::fmt::Error;
///
///     fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
///         Err(core::fmt::Error)
///     }
/// }
/// ```
///
/// The compiling twin, so the block above cannot be passing on a typo - the only difference between
/// the two is the declaration:
///
/// ```
/// use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
/// use sutura_domain::knowledge::{Capability, KnowledgeCapabilities};
/// use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog};
///
/// struct Declared;
///
/// impl SemanticCatalog for Declared {
///     type Error = core::fmt::Error;
///
///     fn capabilities() -> MetadataCapabilities {
///         MetadataCapabilities::of(
///             DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Descriptions]),
///             KnowledgeCapabilities::of([Capability::Glossary]),
///         )
///     }
///
///     fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
///         Err(core::fmt::Error)
///     }
/// }
///
/// assert!(
///     !<Declared as SemanticCatalog>::capabilities()
///         .definitions()
///         .declares(DefinitionKind::Metrics),
///     "this adapter declares no metrics, and the declaration is what says so"
/// );
/// ```
pub trait SemanticCatalog {
    /// Why this catalog could not be read. Typed per adapter, because "the directory does not
    /// exist" and "the service returned 503" are not the same thing to anybody responding to it.
    type Error: core::error::Error + 'static;

    /// What this adapter can supply, and - by what its lists omit - what it cannot.
    ///
    /// **Required, with no default, and that is the whole mechanism.** The trait doc above argues why
    /// both possible defaults are wrong; what this line does is make the argument unnecessary, since
    /// an adapter that omits it does not build.
    ///
    /// **An associated function taking no `self`, rather than an associated constant, and the
    /// difference is worth stating because the precedent is a constant.** What
    /// [`crate::warehouse::Warehouse::IMPERSONATION`]'s constness buys is that the declaration cannot
    /// vary per instance: it is a fact about what was linked, and nothing at run time can widen it.
    /// Taking no `self` buys exactly that, and it is what a caller can rely on here - two instances
    /// of one adapter declare the same thing, because there is no instance in the signature. What it
    /// does NOT buy is const evaluation, and the reason it is given up is representational:
    /// [`crate::knowledge::KnowledgeCapabilities`] is a `BTreeSet` newtype that no `const` expression
    /// can build, and this declaration reuses it verbatim rather than growing a second vocabulary
    /// over the same four kinds. The const-friendly alternative is a boolean per kind, which
    /// `crate::knowledge` already argues against: it makes adding a kind a field at every
    /// construction site. Nothing needs this value in a const context, and if something ever does,
    /// that is the point at which the representation is worth revisiting.
    ///
    /// **The cost, stated where the decision is:** an associated function with no `self` makes this
    /// trait not object-safe, exactly as the `Warehouse` constant does to that one. Nothing in the
    /// workspace holds a `dyn SemanticCatalog` - the golden suite is generic with a bound on this
    /// trait, which it has to be anyway because the port carries an associated error type - and a
    /// heterogeneous set of adapters behind one port wants a closed enum over the registered ones
    /// rather than dynamic dispatch. If that changes it is an architecture decision, not a signature
    /// tweak.
    ///
    /// **What holds it honest, and what does not.** Nothing here refuses a load whose content
    /// disagrees with this declaration: the declaration is a property of the code, so it is not under
    /// the definition digest and no composition root reads it yet.
    /// [`MetadataCapabilities::checked_against`] is the check, and a conformance suite is what runs
    /// it - which is the assertion a *declaring* adapter gets in place of the golden adapters'
    /// agreement with the hand-written oracle.
    fn capabilities() -> MetadataCapabilities;

    /// Reads the whole catalog and pins it.
    fn load(&self) -> Result<PinnedDefinitions, Self::Error>;
}

#[cfg(test)]
mod tests;

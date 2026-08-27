//! The snapshot a question is answered against, and the port it arrives through.
//!
//! Definitions do not arrive live. They arrive as a [`PinnedDefinitions`]: a whole
//! [`crate::catalog::Definitions`] with a version and a digest over its canonical form. Two
//! consequences follow, and both are the reason this type exists rather than passing
//! `Definitions` around directly.
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
//! What it does hold is the hashing. [`PinnedDefinitions::pin`] takes a version and a set of
//! definitions and nothing else: the digest is computed here, from the value being stored, by
//! `DefinitionDigest::of`. The previous shape took the hash *function* from its
//! caller, on the argument that the domain could not hash - and that left the hole intact, because a
//! function handed the definitions is not a function that read them. `crate::definitions` says what
//! the twelve allowlisted crates bought.

use std::collections::BTreeMap;

use crate::catalog::{Anchor, Definitions};
use crate::definitions::{DefinitionDigest, NotDigestible};
use crate::model::{MetricName, SourceName};
use crate::query::RefusalReason;

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

/// What defined an answer, travelling with it.
///
/// A result cannot be separated from what defined it, so this is a typed field a caller reads
/// deliberately rather than a sentence concatenated into a channel that also carries instructions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Provenance {
    version: DefinitionVersion,
    digest: DefinitionDigest,
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
    const fn new(version: DefinitionVersion, digest: DefinitionDigest) -> Self {
        Self { version, digest }
    }

    #[inline]
    pub const fn version(&self) -> &DefinitionVersion {
        &self.version
    }

    #[inline]
    pub const fn digest(&self) -> &DefinitionDigest {
        &self.digest
    }
}

/// An immutable, hashed snapshot of everything a catalog said.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PinnedDefinitions {
    version: DefinitionVersion,
    digest: DefinitionDigest,
    definitions: Definitions,
}

impl PinnedDefinitions {
    /// Pins a set of definitions, computing the digest here, from them.
    ///
    /// **Two arguments, and the absence of a third is the mechanism.** This constructor has been
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
    /// `DefinitionDigest::of` on the value it is about to store, and there is no parameter, closure
    /// or trait through which a caller can influence what the digest is taken over.
    /// `crate::definitions` holds the canonical form, the hash, and the measured cost of the two
    /// dependencies that made it possible.
    ///
    /// The forgery a caller could write before does not compile - there is no third parameter to
    /// pass it as:
    ///
    /// ```compile_fail
    /// use core::convert::Infallible;
    /// use sutura_domain::catalog::Definitions;
    /// use sutura_domain::definitions::DefinitionDigest;
    /// use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};
    ///
    /// // The digest of some OTHER catalog, returned by a closure that ignores its argument.
    /// fn _forged(
    ///     version: DefinitionVersion,
    ///     definitions: Definitions,
    ///     elsewhere: DefinitionDigest,
    /// ) -> Result<PinnedDefinitions, Infallible> {
    ///     PinnedDefinitions::pin(version, definitions, |_| Ok(elsewhere))
    /// }
    /// ```
    ///
    /// Nor is there a way past the constructor. The fields are private, so the struct literal that
    /// would pair them by hand is not a struct literal a caller can write:
    ///
    /// ```compile_fail
    /// use sutura_domain::catalog::Definitions;
    /// use sutura_domain::definitions::DefinitionDigest;
    /// use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};
    ///
    /// fn _by_hand(
    ///     version: DefinitionVersion,
    ///     digest: DefinitionDigest,
    ///     definitions: Definitions,
    /// ) -> PinnedDefinitions {
    ///     PinnedDefinitions { version, digest, definitions }
    /// }
    /// ```
    ///
    /// And the twin of both blocks, which pins the signature so that a rename cannot make either of
    /// them pass vacuously:
    ///
    /// ```
    /// use sutura_domain::catalog::Definitions;
    /// use sutura_domain::definitions::NotDigestible;
    /// use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};
    ///
    /// fn _pin(
    ///     version: DefinitionVersion,
    ///     definitions: Definitions,
    /// ) -> Result<PinnedDefinitions, NotDigestible> {
    ///     PinnedDefinitions::pin(version, definitions)
    /// }
    /// ```
    pub fn pin(version: DefinitionVersion, definitions: Definitions) -> Result<Self, NotDigestible> {
        let digest = DefinitionDigest::of(&definitions)?;
        Ok(Self {
            version,
            digest,
            definitions,
        })
    }

    #[inline]
    pub const fn definitions(&self) -> &Definitions {
        &self.definitions
    }

    #[inline]
    pub const fn version(&self) -> &DefinitionVersion {
        &self.version
    }

    #[inline]
    pub const fn digest(&self) -> &DefinitionDigest {
        &self.digest
    }

    /// The provenance to attach to any answer produced from this bundle.
    pub fn provenance(&self) -> Provenance {
        Provenance::new(self.version.clone(), self.digest.clone())
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
    #[error("the metric reads from {plan}, and the data system opened here is {warehouse}")]
    SourceMismatch { plan: SourceName, warehouse: SourceName },
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
pub trait SemanticCatalog {
    /// Why this catalog could not be read. Typed per adapter, because "the directory does not
    /// exist" and "the service returned 503" are not the same thing to anybody responding to it.
    type Error: core::error::Error + 'static;

    /// Reads the whole catalog and pins it.
    fn load(&self) -> Result<PinnedDefinitions, Self::Error>;
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        AnchorCheck, AnchorReport, DefinitionVersion, InvalidVersion, MAX_VERSION_LEN, NotExecutedReason, NotValidated,
        PinnedDefinitions,
    };
    use crate::calendar::{Date, TimeRange};
    use crate::catalog::{Anchor, Definitions, Metric, Model};
    use crate::definitions::DefinitionDigest;
    use crate::measure::{AggregatedColumn, Measure, Term};
    use crate::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};

    fn metric_name(raw: &str) -> MetricName {
        MetricName::parse(raw).expect("a test metric name is a name")
    }

    /// A one-model, one-metric bundle, with an anchor when `anchor` is set.
    fn bundle(anchor: Option<Anchor>) -> PinnedDefinitions {
        pin(definitions(anchor))
    }

    /// The definitions [`bundle`] pins, before they are pinned.
    fn definitions(anchor: Option<Anchor>) -> Definitions {
        let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
        let model = Model::new(
            ModelName::parse("orders").expect("a test model is a model"),
            SourceName::parse("local").expect("a test source is a source"),
            TableName::parse("orders").expect("a test table is a table"),
            BTreeSet::from([column("amount_cents"), column("order_date")]),
            String::new(),
        );
        let metric = Metric::new(
            metric_name("revenue"),
            ModelName::parse("orders").expect("a test model is a model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            BTreeMap::new(),
            anchor,
            String::new(),
        );
        Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent")
    }

    /// Pins definitions, hashing them for real.
    ///
    /// No stub hasher any more, and that is the point of the change these tests came with: the
    /// hashing is this crate's own, so a test does not have to hand one in and therefore cannot hand
    /// in one that lies.
    fn pin(definitions: Definitions) -> PinnedDefinitions {
        PinnedDefinitions::pin(
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
            definitions,
        )
        .expect("the test definitions hash")
    }

    fn june() -> TimeRange {
        TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range")
    }

    #[test]
    fn a_version_label_may_not_carry_a_newline() {
        // The bug this prevents: provenance is written to an append-only audit sink a line at a
        // time, so a newline in a version label appends a second record that nobody wrote and that
        // reads exactly like a real one.
        assert_eq!(
            DefinitionVersion::parse("v1\nv2").unwrap_err(),
            InvalidVersion::ControlCharacter {
                value: String::from("v1\nv2")
            }
        );
        assert_eq!(DefinitionVersion::parse("").unwrap_err(), InvalidVersion::Empty);
        let long = "v".repeat(MAX_VERSION_LEN + 1);
        assert_eq!(
            DefinitionVersion::parse(&long).unwrap_err(),
            InvalidVersion::TooLong {
                value: long.clone(),
                len: long.chars().count(),
                limit: MAX_VERSION_LEN,
            }
        );
    }

    #[test]
    fn a_bundle_with_no_anchors_validates_with_an_empty_report() {
        // Not a loophole: a bundle that certifies nothing has nothing to check. The loophole would
        // be an anchored bundle passing on an empty report, which the next test rules out.
        let plain = bundle(None);
        AnchorReport::new()
            .verdict(&plain)
            .expect("nothing declared means nothing to check");
        assert!(plain.definitions().metric(&metric_name("revenue")).is_some());
    }

    #[test]
    fn an_anchored_metric_with_no_recorded_check_does_not_validate() {
        // The rule, and it is only half of what makes it mean anything: this says an unchecked
        // anchor fails the verdict. What says the checks in a report actually ran is that the report
        // cannot become a servable bundle here at all - `sutura_app::verify_and_validate` is the
        // only thing that mints one, and it calls a `Warehouse`.
        let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
        assert_eq!(
            AnchorReport::new().verdict(&anchored).unwrap_err(),
            NotValidated::AnchorUnchecked {
                metric: metric_name("revenue")
            }
        );
    }

    #[test]
    fn a_mismatched_anchor_does_not_validate() {
        let mut report = AnchorReport::new();
        report.record(
            metric_name("revenue"),
            AnchorCheck::Mismatch {
                expected: String::from("197122"),
                actual: String::from("197100"),
            },
        );
        let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
        assert_eq!(
            report.verdict(&anchored).unwrap_err(),
            NotValidated::AnchorMismatch {
                metric: metric_name("revenue"),
                expected: String::from("197122"),
                actual: String::from("197100"),
            }
        );
    }

    #[test]
    fn an_anchor_that_could_not_run_is_not_the_same_as_a_wrong_one() {
        // Kept separate because the response differs: one is a definition to fix, the other is a
        // data system to page somebody about. Collapsing them makes an outage look like a wrong
        // number, which is the more expensive of the two mistakes.
        let mut report = AnchorReport::new();
        let reason = NotExecutedReason::Failed {
            message: String::from("connection refused"),
            chain: Vec::new(),
        };
        report.record(metric_name("revenue"), AnchorCheck::NotExecuted { reason: reason.clone() });
        let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
        assert_eq!(
            report.verdict(&anchored).unwrap_err(),
            NotValidated::AnchorNotExecuted {
                metric: metric_name("revenue"),
                reason,
            }
        );
    }

    #[test]
    fn a_failed_check_carries_its_causes_all_the_way_out() {
        // The bug this prevents, and it shipped: the reason was one `String`, so an adapter error's
        // own `source` chain was flattened away before it got here and the operator was told "the
        // anchor query failed" with no table, no column and no driver error in it. Anchor
        // verification is the readiness gate, so that message is the whole of what a failed
        // deployment says.
        //
        // Asserted through `Display` and through `source`, because those are the two ways anything
        // renders this: a chain that is only reachable by matching on the variant is a chain no log
        // line would ever show.
        let mut report = AnchorReport::new();
        report.record(
            metric_name("revenue"),
            AnchorCheck::NotExecuted {
                reason: NotExecutedReason::Failed {
                    message: String::from("the statement was rejected"),
                    chain: vec![String::from("IO error"), String::from("no such file: orders.parquet")],
                },
            },
        );
        let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
        let error = report.verdict(&anchored).unwrap_err();
        let cause = core::error::Error::source(&error).expect("the reason is the source, not the message");
        let rendered = cause.to_string();
        assert!(rendered.contains("no such file: orders.parquet"), "{rendered}");
        assert_eq!(
            rendered,
            "the anchor query failed: the statement was rejected: IO error: no such file: orders.parquet"
        );
    }

    #[test]
    fn a_source_mismatch_names_both_data_systems() {
        // It used to be a sentence in a report field. It is a governance condition - the plan names
        // a data system this process did not open - and naming both halves is what tells an operator
        // whether the catalog or the deployment is wrong.
        let reason = NotExecutedReason::SourceMismatch {
            plan: SourceName::parse("elsewhere").expect("a test source is a source"),
            warehouse: SourceName::parse("local").expect("a test source is a source"),
        };
        assert_eq!(
            reason.to_string(),
            "the metric reads from elsewhere, and the data system opened here is local"
        );
    }

    #[test]
    fn a_report_about_a_different_bundle_does_not_validate_this_one() {
        // Without this check, evidence gathered against another bundle satisfies the coverage
        // check for whatever names happen to overlap, and a bundle becomes "validated" by a run
        // that never touched it.
        let mut report = AnchorReport::new();
        report.record(metric_name("churn"), AnchorCheck::Matched);
        assert_eq!(
            report.verdict(&bundle(None)).unwrap_err(),
            NotValidated::UnknownMetricChecked {
                metric: metric_name("churn")
            }
        );
    }

    #[test]
    fn a_matched_anchor_passes_the_verdict_and_the_bundle_is_untouched() {
        let mut report = AnchorReport::new();
        report.record(metric_name("revenue"), AnchorCheck::Matched);
        let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
        let expected_provenance = anchored.provenance();
        // A verdict, and nothing else. It does NOT hand back a bundle the service would take: that
        // is `sutura_app::verify_and_validate`, and getting one from it means a `Warehouse` was
        // called. The whole reason this returns `()` is that this crate cannot know whether it was.
        report
            .verdict(&anchored)
            .expect("a matched anchor is what the verdict is about");
        assert_eq!(anchored.provenance(), expected_provenance);
        assert_eq!(anchored.version().as_str(), "test-1");
    }

    #[test]
    fn the_digest_a_bundle_carries_is_computed_from_the_definitions_it_holds() {
        // The bug this closes, in two steps, because the first fix did not close it.
        //
        // Originally the constructor took a digest BESIDE a set of definitions, and its own comment
        // conceded the two need not be related - so an answer could carry provenance for content
        // that did not produce it. That was replaced by a constructor taking the hash FUNCTION, and
        // a review showed the hole was still open: `|_| Ok(elsewhere)` is safe public code, and the
        // test that shipped with that change asserted only that the closure was HANDED the
        // definitions - which is not evidence it read them.
        //
        // There is no parameter to lie with now. The assertion is therefore about the values: the
        // same definitions hash the same way twice, and different definitions do not collide. That
        // is a property of the stored bundle rather than of what a caller passed in.
        let pinned = pin(definitions(None));
        let again = pin(definitions(None));
        assert_eq!(
            pinned.digest(),
            again.digest(),
            "the same definitions must pin to the same digest, or provenance is not reproducible"
        );

        let anchored = pin(definitions(Some(Anchor::new(june(), String::from("197122")))));
        assert_ne!(
            pinned.digest(),
            anchored.digest(),
            "declaring an anchor changes what the bundle means, so it must change the digest"
        );

        // And the digest is the one the hashing function computes for exactly these definitions -
        // asserted against an independent call rather than against a constant, so the test cannot
        // pass by pinning whatever the implementation currently happens to emit.
        let independent = DefinitionDigest::of(pinned.definitions()).expect("the definitions hash");
        assert_eq!(*pinned.digest(), independent);
    }

    #[test]
    fn anchored_metrics_lists_only_the_metrics_that_declare_one() {
        let plain = bundle(None);
        assert_eq!(plain.anchored_metrics().count(), 0);
        let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
        let listed: Vec<&MetricName> = anchored.anchored_metrics().map(|(name, _)| name).collect();
        assert_eq!(listed, vec![&metric_name("revenue")]);
    }
}

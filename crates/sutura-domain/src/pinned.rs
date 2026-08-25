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

use std::collections::BTreeMap;

use crate::catalog::{Anchor, Definitions};
use crate::definitions::DefinitionDigest;
use crate::model::MetricName;

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
    pub const fn new(version: DefinitionVersion, digest: DefinitionDigest) -> Self {
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
    /// Pins a set of definitions.
    ///
    /// The digest is computed by the adapter rather than here, because `sutura-domain` cannot hash:
    /// `sha2` is not on its allowlisted dependency tree, and a domain that could hash would be a
    /// domain that could re-derive what it is supposed to accept as given. What this type
    /// guarantees is that the three travel together, not that the third describes the first.
    pub const fn new(version: DefinitionVersion, digest: DefinitionDigest, definitions: Definitions) -> Self {
        Self {
            version,
            digest,
            definitions,
        }
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
    NotExecuted { reason: String },
}

/// The outcome of checking every anchor in a bundle.
///
/// Built by whoever can execute a statement, which is not this crate. It is the evidence
/// [`Validated::new`] demands, and its shape is what makes that demand mean something: a caller
/// cannot claim a bundle is validated without having recorded an outcome for every anchored metric
/// in it.
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
    #[error("metric {metric}'s anchor could not be checked: {reason}")]
    AnchorNotExecuted { metric: MetricName, reason: String },
    #[error("metric {metric} declares an anchor and no check was recorded for it")]
    AnchorUnchecked { metric: MetricName },
    #[error("a check was recorded for {metric}, which this bundle does not define")]
    UnknownMetricChecked { metric: MetricName },
}

/// A `T` that has been shown to hold up.
///
/// **The service accepts only this, so an unvalidated bundle is unrepresentable rather than merely
/// refused.** The field is private and [`Validated::new`] is the only way in.
///
/// Generic in the type it wraps, but constructible only for [`PinnedDefinitions`], and that
/// asymmetry is the point: validating means checking every anchor the bundle declares, so the
/// constructor has to be able to enumerate them. A blanket `Validated::new` for any `T` would be a
/// wrapper that proves nothing, which is worse than no wrapper because it reads like proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated<T>(T);

impl<T> Validated<T> {
    #[inline]
    pub const fn get(&self) -> &T {
        &self.0
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl Validated<PinnedDefinitions> {
    /// Accepts a bundle if, and only if, every anchor it declares was checked and matched.
    ///
    /// The unknown-metric check is not tidiness: without it, a report built against a different
    /// bundle would satisfy the coverage check for whatever it happened to overlap, and a bundle
    /// would be "validated" by evidence about something else.
    pub fn new(pinned: PinnedDefinitions, report: &AnchorReport) -> Result<Self, NotValidated> {
        for metric in report.checks().keys() {
            if pinned.definitions().metric(metric).is_none() {
                return Err(NotValidated::UnknownMetricChecked { metric: metric.clone() });
            }
        }
        for (metric, _) in pinned.anchored_metrics() {
            match report.checks().get(metric) {
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
        Ok(Self(pinned))
    }
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
        AnchorCheck, AnchorReport, DefinitionVersion, InvalidVersion, MAX_VERSION_LEN, NotValidated, PinnedDefinitions, Validated,
    };
    use crate::calendar::{Date, TimeRange};
    use crate::catalog::{Anchor, Definitions, Metric, Model};
    use crate::definitions::DefinitionDigest;
    use crate::model::{Aggregate, ColumnName, Grain, Measure, MetricName, ModelName, SourceName, TableName};

    const DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn metric_name(raw: &str) -> MetricName {
        MetricName::parse(raw).expect("a test metric name is a name")
    }

    /// A one-model, one-metric bundle, with an anchor when `anchor` is set.
    fn bundle(anchor: Option<Anchor>) -> PinnedDefinitions {
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
            Measure::new(Aggregate::Sum, column("amount_cents")),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            BTreeMap::new(),
            anchor,
            String::new(),
        );
        let definitions = Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent");
        PinnedDefinitions::new(
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
            DefinitionDigest::parse(DIGEST).expect("a real digest is a digest"),
            definitions,
        )
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
        let validated = Validated::new(bundle(None), &AnchorReport::new()).expect("nothing declared means nothing to check");
        assert!(validated.get().definitions().metric(&metric_name("revenue")).is_some());
    }

    #[test]
    fn an_anchored_metric_with_no_recorded_check_does_not_validate() {
        // This is the whole point of the type. Before it, "we checked the anchors" was something a
        // caller asserted by having called a function; now a bundle that was never checked cannot
        // be handed to anything that serves.
        let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
        assert_eq!(
            Validated::new(anchored, &AnchorReport::new()).unwrap_err(),
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
            Validated::new(anchored, &report).unwrap_err(),
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
        report.record(
            metric_name("revenue"),
            AnchorCheck::NotExecuted {
                reason: String::from("connection refused"),
            },
        );
        let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
        assert_eq!(
            Validated::new(anchored, &report).unwrap_err(),
            NotValidated::AnchorNotExecuted {
                metric: metric_name("revenue"),
                reason: String::from("connection refused"),
            }
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
            Validated::new(bundle(None), &report).unwrap_err(),
            NotValidated::UnknownMetricChecked {
                metric: metric_name("churn")
            }
        );
    }

    #[test]
    fn a_matched_anchor_validates_and_the_bundle_survives_intact() {
        let mut report = AnchorReport::new();
        report.record(metric_name("revenue"), AnchorCheck::Matched);
        let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
        let expected_provenance = anchored.provenance();
        let validated = Validated::new(anchored, &report).expect("a matched anchor is what validation means");
        assert_eq!(validated.get().provenance(), expected_provenance);
        assert_eq!(validated.into_inner().version().as_str(), "test-1");
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

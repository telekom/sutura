//! One instance of every refusal variant, so the prompt's guidance can be checked against it.
//!
//! A submodule of [`super`] rather than a fixture inside it, and the reason is the mechanical one
//! [`super::column_zero`] gives: `cargo xtask max-lines` fails at a thousand lines under `crates/`
//! and cannot be exempted, and that file was one line under it. The seam is a real one - this is a
//! corpus rather than a helper, sixty lines of literals that no test reads except by asking for the
//! whole list.
//!
//! **It names every variant, which is exactly why `cargo xtask check-refusal-coverage` counts it
//! for none of them.** That gate treats a file naming every variant as a census; the provocations
//! live where the refusals are produced. What this list buys is the second net the function below
//! documents.

use sutura_domain::model::{Aggregate, Grain, SourceName, TableName};
use sutura_domain::query::{MAX_DIMENSIONS, MAX_RANGE_DAYS, RefusalReason, ResultBound};

use super::{dimension_name, metric_name};

/// One instance of every refusal variant.
///
/// The second net rather than the first: what forces an author to write a guide is `guide_for` in
/// `prompt.rs` failing to compile. What this list adds is that once they have, the set of guides the
/// prompt renders and the set `guide_for` can return are asserted to be the same.
pub(super) fn every_refusal() -> Vec<RefusalReason> {
    vec![
        RefusalReason::MetricUnknown {
            metric: metric_name("revenue"),
        },
        RefusalReason::GrainNotSupported {
            metric: metric_name("revenue"),
            grain: Grain::Year,
        },
        RefusalReason::DimensionNotPermitted {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
        },
        RefusalReason::DimensionNotFilterable {
            metric: metric_name("revenue"),
            dimension: dimension_name("tariff"),
        },
        RefusalReason::DimensionValueNotAllowed {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
        },
        RefusalReason::DuplicateDimension {
            dimension: dimension_name("region"),
        },
        RefusalReason::TooManyDimensions {
            requested: 9,
            limit: MAX_DIMENSIONS,
        },
        RefusalReason::ResultTooLarge {
            bound: ResultBound::Rows { limit: 10_000 },
        },
        RefusalReason::TimeRangeTooLong {
            days: 99_999,
            limit: MAX_RANGE_DAYS,
        },
        // A gibibyte, which is the provisional default `sutura-config` writes. Any number would
        // exercise the guide; this one is the one an operator will actually read in a log.
        RefusalReason::ResourcesExhausted {
            ceiling_bytes: 1024 * 1024 * 1024,
        },
        RefusalReason::PlanSpansTooManySources { sources: 3, limit: 2 },
        RefusalReason::FederationNotExecutable,
        RefusalReason::FederationLinkAmbiguous {
            source: SourceName::parse("geo").expect("a test source is a source"),
        },
        RefusalReason::MeasureDoesNotFederate {
            metric: metric_name("active_subscriptions"),
            aggregate: Aggregate::CountDistinct,
        },
        RefusalReason::PlanTablesShareAnIdentifier {
            table: TableName::parse("orders").expect("a test table is a table"),
        },
        RefusalReason::SourceUnavailable {
            source: SourceName::parse("elsewhere").expect("a test source is a source"),
        },
        RefusalReason::SourceRefused {
            source: SourceName::parse("warehouse").expect("a test source is a source"),
        },
        RefusalReason::CredentialUnavailable {
            source: SourceName::parse("warehouse").expect("a test source is a source"),
        },
        // Both labels, off `SourcePosture::NAMES` rather than spelled here: the closed set is the
        // whole of what this refusal may carry, and a literal beside it would be a second copy of
        // it. Never a `SourcePosture` value - that one carries an operator's acknowledgement prose.
        RefusalReason::LegsDecideIdentityDifferently {
            postures: sutura_domain::source::SourcePosture::NAMES.iter().copied().collect(),
        },
    ]
}

/// G4 - the witness pairing, driven by every variant in this corpus.
///
/// In the corpus file because these are the tests that READ it, and `prompt/tests.rs` is one line
/// off the thousand-line ceiling. The pairing the set test in `prompt/tests.rs` deliberately does
/// NOT make: it compares two SETS of reason strings, so swapping any two `Guide.reason` consts
/// stays green. Asserting the pairing per variant - a guide's `reason` must be the variant's OWN
/// machine code - reddens exactly that mutation, which is the witness G4 exists to hold.
use super::super::refusal::guide_for;

/// A guide's `reason` is the domain's canonical `code()` for the variant it was matched from.
#[test]
fn guide_key_carries_the_variant_it_was_matched_from() {
    for reason in every_refusal() {
        assert_eq!(
            guide_for(&reason).reason,
            reason.code(),
            "guide_for returned another variant's key"
        );
    }
}

/// Every variant's key is supplied to a no-wildcard match, so none can drift.
#[test]
fn guide_key_carries_every_variant() {
    for reason in every_refusal() {
        let key = guide_for(&reason).reason;
        // ONE arm, an or-pattern: a new `RefusalReason` variant fails to compile here until named.
        match &reason {
            RefusalReason::MetricUnknown { .. }
            | RefusalReason::GrainNotSupported { .. }
            | RefusalReason::DimensionNotPermitted { .. }
            | RefusalReason::DimensionNotFilterable { .. }
            | RefusalReason::DimensionValueNotAllowed { .. }
            | RefusalReason::DuplicateDimension { .. }
            | RefusalReason::TooManyDimensions { .. }
            | RefusalReason::ResultTooLarge { .. }
            | RefusalReason::TimeRangeTooLong { .. }
            | RefusalReason::PlanSpansTooManySources { .. }
            | RefusalReason::FederationNotExecutable
            | RefusalReason::FederationLinkAmbiguous { .. }
            | RefusalReason::MeasureDoesNotFederate { .. }
            | RefusalReason::PlanTablesShareAnIdentifier { .. }
            | RefusalReason::SourceUnavailable { .. }
            | RefusalReason::SourceRefused { .. }
            | RefusalReason::ResourcesExhausted { .. }
            | RefusalReason::CredentialUnavailable { .. }
            | RefusalReason::LegsDecideIdentityDifferently { .. } => {}
        }
        assert_eq!(
            key,
            reason.code(),
            "the exhaustive match gave a key that is not this variant's code"
        );
    }
}

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
        RefusalReason::CredentialUnavailable {
            source: SourceName::parse("warehouse").expect("a test source is a source"),
        },
    ]
}

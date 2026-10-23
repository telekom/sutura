//! One instance of every [`RefusalReason`], listed beside the enum so a consumer names no variant.
//!
//! A test file that spelled out every variant had to change in the same diff as any new variant,
//! and `just causality` keeps a changed test file at HEAD while it rebuilds the base tree: the base
//! build named a variant it did not have and no verdict came out. The list lives here instead, in a
//! file the base run reverts with the enum.
//!
//! Limit: the match below forces a new variant to be named in this file, not to be given an
//! instance in the list. That second half is a run-time check - the prompt's set test fails for a
//! variant with a guide and no instance here.

#![allow(
    clippy::expect_used,
    reason = "fixture literals: each is a constant that is valid by construction, and a typo should \
              stop the build of a test, not ship a refusal"
)]

use super::{MAX_DIMENSIONS, MAX_FILTERS, MAX_METRICS, MAX_RANGE_DAYS, RefusalReason, ResultBound};
use crate::catalog::ColumnType;
use crate::model::{DimensionName, Grain, MetricName, ModelName, RelationshipName, SourceName, TableName};
use crate::plan::{FederatedAnswerRefusal, QuotedColumnType};

fn metric(raw: &str) -> MetricName {
    MetricName::parse(raw).expect("a fixture metric is a metric")
}

fn dimension(raw: &str) -> DimensionName {
    DimensionName::parse(raw).expect("a fixture dimension is a dimension")
}

fn source(raw: &str) -> SourceName {
    SourceName::parse(raw).expect("a fixture source is a source")
}

fn model(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a fixture model is a model")
}

fn quoted(raw: &str) -> QuotedColumnType {
    QuotedColumnType::from(&ColumnType::parse(raw).expect("a fixture type is a type"))
}

/// One instance of every variant.
#[must_use]
pub fn every_refusal() -> Vec<RefusalReason> {
    let all = vec![
        RefusalReason::MetricUnknown {
            metric: metric("revenue"),
        },
        RefusalReason::TooManyMetrics {
            requested: 9,
            limit: MAX_METRICS,
        },
        RefusalReason::DimensionNotConformed {
            dimension: dimension("region"),
            first: metric("revenue"),
            other: metric("margin"),
        },
        RefusalReason::QuestionNeedsTooManyLegs { legs: 8, limit: 6 },
        RefusalReason::DuplicateMetricName {
            metric: metric("revenue"),
        },
        RefusalReason::MultiMetricTopNotExecutable {
            metrics: vec![metric("revenue"), metric("margin")],
        },
        RefusalReason::GrainNotSupported {
            metric: metric("revenue"),
            grain: Grain::Year,
        },
        RefusalReason::DimensionNotPermitted {
            metric: metric("revenue"),
            dimension: dimension("region"),
        },
        RefusalReason::DimensionNotFilterable {
            metric: metric("revenue"),
            dimension: dimension("tariff"),
        },
        RefusalReason::DimensionValueNotAllowed {
            metric: metric("revenue"),
            dimension: dimension("region"),
        },
        RefusalReason::DuplicateDimension {
            dimension: dimension("region"),
        },
        RefusalReason::TooManyDimensions {
            requested: 9,
            limit: MAX_DIMENSIONS,
        },
        RefusalReason::TooManyFilters {
            requested: MAX_FILTERS + 1,
            limit: MAX_FILTERS,
        },
        RefusalReason::ResultTooLarge {
            bound: ResultBound::Rows { limit: 10_000 },
        },
        RefusalReason::TimeRangeTooLong {
            days: 99_999,
            limit: MAX_RANGE_DAYS,
        },
        // A gibibyte, the provisional default `sutura-config` writes: the number an operator
        // will actually read in a log.
        RefusalReason::ResourcesExhausted {
            ceiling_bytes: 1024 * 1024 * 1024,
        },
        RefusalReason::PlanSpansTooManySources { sources: 5, limit: 4 },
        RefusalReason::PlanNeedsTooManyLookupLegs { legs: 4, limit: 3 },
        RefusalReason::FederationNotExecutable,
        RefusalReason::FederationLinkTypeMismatch {
            relationship: RelationshipName::parse("orders_customer").expect("a fixture relationship is a relationship"),
            fact_type: quoted("DATE"),
            lookup_type: quoted("BIGINT"),
        },
        RefusalReason::FederatedAnswerNotWellFormed {
            federated: FederatedAnswerRefusal::AmbiguousLink,
        },
        RefusalReason::PlanTablesShareAnIdentifier {
            table: TableName::parse("orders").expect("a fixture table is a table"),
        },
        RefusalReason::SourceUnavailable {
            source: source("elsewhere"),
        },
        RefusalReason::SourceRefused {
            source: source("warehouse"),
        },
        RefusalReason::BillToNotAllowed {
            source: source("warehouse"),
        },
        RefusalReason::NoBillingProject {
            source: source("warehouse"),
        },
        RefusalReason::CredentialUnavailable {
            source: source("warehouse"),
        },
        RefusalReason::AddressOutsideDomains {
            source: source("warehouse"),
        },
        RefusalReason::DeadlineExceeded { budget_seconds: 29 },
        RefusalReason::BudgetExhausted { reset_after_seconds: 41 },
        RefusalReason::TopOverUncertifiedRows { ceiling: 10_000 },
        RefusalReason::CrossModelRatioWithoutSharedCalendar {
            metric: metric("revenue_per_customer"),
            model: model("customers"),
        },
        RefusalReason::CrossModelRatioSpansSources {
            metric: metric("revenue_per_customer"),
            model: model("customers"),
        },
        RefusalReason::CrossModelRatioWithoutSharedDimension {
            metric: metric("revenue_per_customer"),
            model: model("customers"),
        },
        RefusalReason::CrossModelRatioNeedsOneLookupLeg {
            metric: metric("revenue_per_customer"),
        },
    ];
    for reason in &all {
        // No wildcard: a variant added to the enum does not compile until it is named here.
        match reason {
            RefusalReason::MetricUnknown { .. }
            | RefusalReason::TooManyMetrics { .. }
            | RefusalReason::DimensionNotConformed { .. }
            | RefusalReason::QuestionNeedsTooManyLegs { .. }
            | RefusalReason::DuplicateMetricName { .. }
            | RefusalReason::MultiMetricTopNotExecutable { .. }
            | RefusalReason::GrainNotSupported { .. }
            | RefusalReason::DimensionNotPermitted { .. }
            | RefusalReason::DimensionNotFilterable { .. }
            | RefusalReason::DimensionValueNotAllowed { .. }
            | RefusalReason::DuplicateDimension { .. }
            | RefusalReason::TooManyDimensions { .. }
            | RefusalReason::TooManyFilters { .. }
            | RefusalReason::ResultTooLarge { .. }
            | RefusalReason::TimeRangeTooLong { .. }
            | RefusalReason::PlanNeedsTooManyLookupLegs { .. }
            | RefusalReason::PlanSpansTooManySources { .. }
            | RefusalReason::FederationNotExecutable
            | RefusalReason::FederationLinkTypeMismatch { .. }
            | RefusalReason::FederatedAnswerNotWellFormed { .. }
            | RefusalReason::PlanTablesShareAnIdentifier { .. }
            | RefusalReason::SourceUnavailable { .. }
            | RefusalReason::SourceRefused { .. }
            | RefusalReason::BillToNotAllowed { .. }
            | RefusalReason::NoBillingProject { .. }
            | RefusalReason::ResourcesExhausted { .. }
            | RefusalReason::CredentialUnavailable { .. }
            | RefusalReason::AddressOutsideDomains { .. }
            | RefusalReason::DeadlineExceeded { .. }
            | RefusalReason::BudgetExhausted { .. }
            | RefusalReason::TopOverUncertifiedRows { .. }
            | RefusalReason::CrossModelRatioWithoutSharedCalendar { .. }
            | RefusalReason::CrossModelRatioSpansSources { .. }
            | RefusalReason::CrossModelRatioWithoutSharedDimension { .. }
            | RefusalReason::CrossModelRatioNeedsOneLookupLeg { .. } => {}
        }
    }
    all
}

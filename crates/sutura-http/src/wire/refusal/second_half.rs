//! The rest of `every_reason`'s fixture list, split into its own file for `cargo xtask
//! max-lines`'s per-file cap - a sibling module of `refusal::tests`, reading `super::` for the
//! same private items that module does.

use axum::http::StatusCode;
use sutura_domain::model::{MetricName, SourceName, TableName};
use sutura_domain::query::RefusalReason;

use super::tests::{Expected, metric};

pub(super) fn every_reason_the_second_half() -> Vec<Expected> {
    vec![
        (
            RefusalReason::FederationNotExecutable,
            StatusCode::CONFLICT,
            "federation_not_executable",
        ),
        (
            RefusalReason::FederationLinkAmbiguous {
                source: SourceName::parse("warehouse").expect("a test source is a source"),
            },
            StatusCode::CONFLICT,
            "federation_link_ambiguous",
        ),
        (
            RefusalReason::FederationLinkCompound {
                source: SourceName::parse("warehouse").expect("a test source is a source"),
                relationship: sutura_domain::model::RelationshipName::parse("usage_subscription")
                    .expect("a test relationship is a relationship"),
            },
            StatusCode::CONFLICT,
            "federation_link_compound",
        ),
        (
            RefusalReason::FederationLinkTypeMismatch {
                relationship: sutura_domain::model::RelationshipName::parse("usage_subscription")
                    .expect("a test relationship is a relationship"),
                fact_type: sutura_domain::plan::QuotedColumnType::from(
                    &sutura_domain::catalog::ColumnType::parse("BIGINT").expect("a test type is a type"),
                ),
                lookup_type: sutura_domain::plan::QuotedColumnType::from(
                    &sutura_domain::catalog::ColumnType::parse("VARCHAR").expect("a test type is a type"),
                ),
            },
            StatusCode::CONFLICT,
            "federation_link_type_mismatch",
        ),
        (
            RefusalReason::FederatedAnswerNotWellFormed {
                federated: sutura_domain::plan::FederatedAnswerRefusal::AmbiguousLink,
            },
            StatusCode::CONFLICT,
            "federated_answer_not_well_formed",
        ),
        (
            RefusalReason::PlanTablesShareAnIdentifier {
                table: TableName::parse("orders").expect("a test table is a table"),
            },
            StatusCode::CONFLICT,
            "plan_tables_share_an_identifier",
        ),
        (
            RefusalReason::MultiMetricFederationNotExecutable {
                metrics: vec![metric(), MetricName::parse("margin").expect("a test metric is a metric")],
            },
            StatusCode::CONFLICT,
            "multi_metric_federation_not_executable",
        ),
        (
            RefusalReason::MultiMetricTopNotExecutable {
                metrics: vec![metric(), MetricName::parse("margin").expect("a test metric is a metric")],
            },
            StatusCode::CONFLICT,
            "multi_metric_top_not_executable",
        ),
        (
            RefusalReason::SourceUnavailable {
                source: sutura_domain::model::SourceName::parse("local").expect("a test source is a source"),
            },
            StatusCode::SERVICE_UNAVAILABLE,
            "source_unavailable",
        ),
        (
            RefusalReason::CredentialUnavailable {
                source: sutura_domain::model::SourceName::parse("warehouse").expect("a test source is a source"),
            },
            StatusCode::FORBIDDEN,
            "credential_unavailable",
        ),
        (
            RefusalReason::SourceRefused {
                source: sutura_domain::model::SourceName::parse("warehouse").expect("a test source is a source"),
            },
            StatusCode::FORBIDDEN,
            "source_refused",
        ),
        (
            RefusalReason::DeadlineExceeded { budget_seconds: 29 },
            StatusCode::UNPROCESSABLE_ENTITY,
            "deadline_exceeded",
        ),
        (
            RefusalReason::BudgetExhausted { reset_after_seconds: 41 },
            StatusCode::TOO_MANY_REQUESTS,
            "budget_exhausted",
        ),
        (
            RefusalReason::TopOverUncertifiedRows { ceiling: 10_000 },
            StatusCode::UNPROCESSABLE_ENTITY,
            "top_over_uncertified_rows",
        ),
        (
            RefusalReason::CrossModelRatioWithoutSharedCalendar {
                metric: metric(),
                model: sutura_domain::model::ModelName::parse("customers").expect("a test model is a model"),
            },
            StatusCode::CONFLICT,
            "cross_model_ratio_without_shared_calendar",
        ),
        (
            RefusalReason::CrossModelRatioSpansSources {
                metric: metric(),
                model: sutura_domain::model::ModelName::parse("calendar").expect("a test model is a model"),
            },
            StatusCode::CONFLICT,
            "cross_model_ratio_spans_sources",
        ),
        (
            RefusalReason::CrossModelRatioWithoutSharedDimension {
                metric: metric(),
                model: sutura_domain::model::ModelName::parse("visits").expect("a test model is a model"),
            },
            StatusCode::UNPROCESSABLE_ENTITY,
            "cross_model_ratio_without_shared_dimension",
        ),
    ]
}

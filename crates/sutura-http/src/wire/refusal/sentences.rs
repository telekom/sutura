//! Sentences of `refusal.rs`, in a file of their own for its line cap and for `clippy::too_many_lines` on `refused`.

use sutura_domain::model::{DimensionName, MetricName, SourceName};

/// The metrics a refusal names, comma-joined - catalog-authored names, safe to echo back.
fn joined_metric_names(metrics: &[MetricName]) -> String {
    metrics.iter().map(MetricName::as_str).collect::<Vec<_>>().join(", ")
}

pub(super) fn multi_metric_top_not_executable(metrics: &[MetricName]) -> String {
    format!(
        "{} were asked together with `top`, which names no metric to rank by",
        joined_metric_names(metrics)
    )
}

pub(super) fn dimension_not_conformed(dimension: &DimensionName, first: &MetricName, other: &MetricName) -> String {
    format!(
        "`{dimension}` does not end on the same model and column for `{first}` and `{other}`, so it \
         cannot be used for both; ask for them separately, or leave `{dimension}` out"
    )
}

pub(super) fn too_many_legs(legs: usize, limit: usize) -> String {
    format!(
        "answering this would run {legs} queries against the data systems and one question runs \
         {limit} at most; ask for fewer metrics at a time"
    )
}

pub(super) fn too_many_lookup_legs(legs: usize, limit: usize) -> String {
    format!("answering this would read {legs} lookup legs, and a plan reads {limit} at most")
}

pub(super) fn resources_exhausted(ceiling_bytes: u64) -> String {
    format!(
        "answering this needed more working memory than this deployment's ceiling of \
         {ceiling_bytes} bytes, and it was refused rather than allowed to exhaust the \
         process; narrow the period, group by fewer dimensions or add a filter and ask \
         again. Retrying it unchanged returns this same refusal"
    )
}

pub(super) fn credential_unavailable(source: &SourceName) -> String {
    format!(
        "you have no credential at the data system `{source}`, so this deployment will not \
         read it on your behalf - and it will not read it as itself instead. This is not \
         about the token you presented to this service: the missing grant is at that data \
         system. Asking the same question again returns this same refusal"
    )
}

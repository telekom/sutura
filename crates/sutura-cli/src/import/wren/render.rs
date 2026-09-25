//! Spelling a [`super::plan`] value as the exact document text `sutura-catalog-local` reads.
//!
//! Hand-formatted rather than run through a YAML writer, and deliberately: these are documents for
//! a human to review before committing, in the byte shape `sutura-catalog-local/src/document.rs`
//! parses and every example in `examples/` already uses, not a machine's canonical form of a
//! `serde` value the reader-side types do not even derive `Serialize` to produce.
//!
//! Every `write_*` function returns [`core::fmt::Result`] and propagates with `?`; each public
//! entry point resolves that Result exactly once, the way `sutura_runtime::metrics::Registry`
//! already does for the identical fact that writing to a `String` cannot fail.

use std::fmt::Write as _;

use super::convert::Converted;
use super::plan::{PlannedComputation, PlannedMetric, PlannedModel, PlannedRelationship};

/// The audience every imported metric is written with.
///
/// Wren has no concept of who may see a metric - `docs/adr/0028` is a sutura-only channel - so
/// every imported metric defaults to the least restrictive value a person reviewing the output can
/// then narrow. Never widened silently the other way: `open` is `MetricDoc::audience`'s own
/// required key, and there is no default this converter could pick that hides a metric a wren
/// project made visible.
const AUDIENCE: &str = "open";

/// The grain every imported metric is written with, for the reason [`AUDIENCE`] gives: a `WrenAI`
/// time dimension carries no grain, and `day` is the finest one `sutura_domain::model::Grain` has,
/// so a question this catalog answers is never coarser than what the source data can support.
const GRAIN: &str = "day";

fn write_model(model: &PlannedModel, out: &mut String) -> core::fmt::Result {
    writeln!(out, "---")?;
    writeln!(out, "kind: model")?;
    writeln!(out, "name: {}", model.name)?;
    writeln!(out, "source: wren")?;
    writeln!(out, "table: {}", model.table)?;
    writeln!(out, "columns:")?;
    for column in &model.columns {
        writeln!(out, "  - name: {}", column.name)?;
        writeln!(out, "    type: {}", column.data_type)?;
        writeln!(out, "    nullable: {}", column.nullable)?;
    }
    if let Some(primary_key) = &model.primary_key {
        writeln!(out, "primary_key: [{primary_key}]")?;
    }
    writeln!(out, "---")?;
    writeln!(out)?;
    writeln!(out, "Imported from the WrenAI model `{}`.", model.name)
}

/// `kind: model` document text - frontmatter, then the model's own name as a one-line description.
///
/// The prose is intentionally thin: wren's `Model` carries nothing that reads as a description, so
/// inventing one would be this converter's own words presented as if a person had written them.
pub(crate) fn model_document(model: &PlannedModel) -> String {
    let mut out = String::new();
    #[expect(clippy::expect_used, reason = "writing to a String cannot fail")]
    write_model(model, &mut out).expect("writing to a String cannot fail");
    out
}

fn write_relationship(relationship: &PlannedRelationship, out: &mut String) -> core::fmt::Result {
    writeln!(out, "---")?;
    writeln!(out, "kind: relationship")?;
    writeln!(out, "name: {}", relationship.name)?;
    writeln!(out, "origin:")?;
    writeln!(out, "  model: {}", relationship.origin_model)?;
    writeln!(out, "  column: {}", relationship.origin_column)?;
    writeln!(out, "target:")?;
    writeln!(out, "  model: {}", relationship.target_model)?;
    writeln!(out, "  column: {}", relationship.target_column)?;
    writeln!(out, "join_type: {}", relationship.join_type)?;
    writeln!(out, "---")?;
    writeln!(out)?;
    writeln!(out, "Imported from the WrenAI relationship `{}`.", relationship.name)
}

/// `kind: relationship` document text.
pub(crate) fn relationship_document(relationship: &PlannedRelationship) -> String {
    let mut out = String::new();
    #[expect(clippy::expect_used, reason = "writing to a String cannot fail")]
    write_relationship(relationship, &mut out).expect("writing to a String cannot fail");
    out
}

/// `measure:` written the way `MetricDoc` reads it - `{ aggregate: sum, column: x }`, or the ratio
/// shape with both terms plus a `zero_denominator`.
fn write_computation(computation: &PlannedComputation, out: &mut String) -> core::fmt::Result {
    match computation {
        PlannedComputation::Simple(term) => {
            writeln!(
                out,
                "measure:\n  simple: {{ aggregate: {}, column: {} }}",
                term.aggregate, term.column
            )
        }
        PlannedComputation::Ratio { numerator, denominator } => writeln!(
            out,
            "measure:\n  ratio:\n    numerator: {{ aggregate: {}, column: {} }}\n    denominator: {{ aggregate: {}, column: {} \
             }}\n    zero_denominator: yields_null",
            numerator.aggregate, numerator.column, denominator.aggregate, denominator.column
        ),
    }
}

fn write_metric(metric: &PlannedMetric, out: &mut String) -> core::fmt::Result {
    writeln!(out, "---")?;
    writeln!(out, "kind: metric")?;
    writeln!(out, "name: {}", metric.name)?;
    writeln!(out, "model: {}", metric.model)?;
    write_computation(&metric.computation, out)?;
    writeln!(out, "time_column: {}", metric.time_column)?;
    writeln!(out, "grains: [{GRAIN}]")?;
    if !metric.dimensions.is_empty() {
        writeln!(out, "dimensions:")?;
        for dimension in &metric.dimensions {
            writeln!(out, "  - name: {}", dimension.name)?;
            writeln!(out, "    column: {}", dimension.column)?;
        }
    }
    writeln!(out, "audience: {AUDIENCE}")?;
    writeln!(out, "---")?;
    writeln!(out)?;
    writeln!(
        out,
        "Imported from the WrenAI cube `{}`'s measure `{}`. The grain and the ratio's zero-denominator policy are this \
         converter's own defaults - wren declares neither - and are the first two things to review before committing.",
        metric.cube, metric.measure
    )
}

/// `kind: metric` document text.
pub(crate) fn metric_document(metric: &PlannedMetric) -> String {
    let mut out = String::new();
    #[expect(clippy::expect_used, reason = "writing to a String cannot fail")]
    write_metric(metric, &mut out).expect("writing to a String cannot fail");
    out
}

fn write_report_body(converted: &Converted, out: &mut String) -> core::fmt::Result {
    writeln!(out, "sutura import wren")?;
    writeln!(out)?;
    writeln!(out, "mapped")?;
    writeln!(out, "  models         {}", converted.models.len())?;
    writeln!(out, "  relationships  {}", converted.relationships.len())?;
    writeln!(out, "  metrics        {}", converted.metrics.len())?;
    writeln!(out)?;
    writeln!(out, "refused {}", converted.refusals.len())?;
    for refusal in &converted.refusals {
        writeln!(out, "  {:<28} {:<30} {}", refusal.kind, refusal.name, refusal.reason)?;
    }
    if !converted.notes.is_empty() {
        writeln!(out)?;
        writeln!(out, "notes")?;
        for note in &converted.notes {
            writeln!(out, "  {note}")?;
        }
    }
    Ok(())
}

/// The plain-text refusal report - `report.txt`, plain text so `sutura-catalog-local`'s walk never
/// mistakes it for a document (see [`super::write_report`]).
pub(crate) fn report(converted: &Converted) -> String {
    let mut out = String::new();
    #[expect(clippy::expect_used, reason = "writing to a String cannot fail")]
    write_report_body(converted, &mut out).expect("writing to a String cannot fail");
    out
}

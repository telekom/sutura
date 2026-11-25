//! Descriptive physical structure, visible only after an operator opts in.

use sutura_domain::pinned::view::ScopedView;

use super::{CatalogProse, quote};
use crate::catalog_budget::{MAX_CATALOG_LISTING_BYTES, MAX_OMITTED_NAMES, fill_to_budget};

/// The model and column names this view may list. Never selects a query plan.
///
/// Held to [`MAX_CATALOG_LISTING_BYTES`]: a model's full block (table, columns, descriptions) is
/// included whole or not at all, and a model left out is still named, so an agent always knows
/// `describe_catalog(model: <name>)` can read the rest - it just does not land unasked in this
/// prompt. Relationships are rendered in full regardless, unbounded: they are one line per
/// relationship with no prose, so even a few hundred stay far under this same budget on their own.
pub fn physical_schema(view: &ScopedView<'_>, prose: CatalogProse) -> String {
    let models: Vec<_> = view.models().collect();
    if models.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        String::from("## Physical schema (descriptive only)"),
        String::from("Tables and columns describe structure. They do not certify a metric or permit a query."),
    ];
    let fragments: Vec<(String, String)> = models
        .into_iter()
        .map(|model| (model.name().to_string(), model_fragment(model, prose)))
        .collect();
    let (included, omitted_names, omitted_count) = fill_to_budget(
        fragments,
        MAX_CATALOG_LISTING_BYTES,
        MAX_OMITTED_NAMES,
        |(_, fragment)| fragment.len(),
        |(name, _)| name.clone(),
    );
    for (_, fragment) in included {
        lines.push(fragment);
    }
    if omitted_count > 0 {
        lines.push(String::new());
        lines.push(omission_note(&omitted_names, omitted_count));
    }
    for relationship in view.relationships() {
        lines.push(format!(
            "- Relationship `{}`: `{}` -> `{}` (`{}`)",
            relationship.name(),
            relationship.origin_model(),
            relationship.target_model(),
            relationship.join_type().as_str()
        ));
        for key in relationship.keys().iter() {
            let grain = key
                .grain()
                .map_or_else(String::new, |grain| format!(" (origin truncated to {})", grain.as_str()));
            lines.push(format!("  - Key: `{}` = `{}`{grain}", key.origin(), key.target()));
        }
        if let Some(comparison) = relationship.comparison() {
            lines.push(format!("  - Comparison as {}", comparison.word()));
        }
    }
    lines.join("\n")
}

fn model_fragment(model: &sutura_domain::catalog::Model, prose: CatalogProse) -> String {
    let mut lines = vec![
        format!("\n### Model `{}`", model.name()),
        format!("- Table: `{}`", model.table()),
    ];
    if prose.is_quoted() && !model.description().is_empty() {
        lines.push(String::from("- Description (untrusted catalog prose):"));
        lines.push(quote(model.description()));
    }
    for column in model.columns() {
        lines.push(format!("- Column: `{}`", column.name()));
        if prose.is_quoted() && !column.description().is_empty() {
            lines.push(quote(column.description()));
        }
    }
    lines.join("\n")
}

fn omission_note(names: &[String], total: usize) -> String {
    let more = total.saturating_sub(names.len());
    let suffix = if more > 0 {
        format!(" (+{more} more not named)")
    } else {
        String::new()
    };
    format!(
        "Left out of this listing over its {MAX_CATALOG_LISTING_BYTES}-byte bound ({total} model(s)): \
         {}{suffix}. Call `describe_catalog(model: <name>)` to read one of those in full.",
        names.join(", ")
    )
}

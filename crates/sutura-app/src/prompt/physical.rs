//! Descriptive physical structure, visible only after an operator opts in.

use sutura_domain::pinned::view::ScopedView;

use super::{CatalogProse, quote};
use crate::catalog_budget::{MAX_CATALOG_LISTING_BYTES, MAX_OMITTED_NAMES, fill_to_budget, omission_reserve};

/// The model and column names this view may list. Never selects a query plan.
///
/// `budget` is this pool's SHARE of the whole prompt's [`MAX_CATALOG_LISTING_BYTES`] - see
/// [`super::metrics::metrics`]'s own doc for the spend order. A model's full block (table, columns,
/// descriptions) is included whole or not at all, and a model left out is still named, so an agent
/// always knows `describe_catalog(model: <name>)` can read the rest - it just does not land unasked
/// in this prompt. Relationships are rendered in full regardless - one line per relationship with no
/// prose - but their bytes are now reserved out of `budget` BEFORE the model pool is chosen, so a
/// bundle with an unusual number of relationships still cannot push the models-plus-relationships
/// total past this section's share.
pub fn physical_schema(view: &ScopedView<'_>, prose: CatalogProse, budget: usize) -> String {
    let models: Vec<_> = view.models().collect();
    if models.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        String::from("## Physical schema (descriptive only)"),
        String::from("Tables and columns describe structure. They do not certify a metric or permit a query."),
    ];
    // Reserved BEFORE `fill_to_budget` runs: this header's own bytes, which render unconditionally,
    // plus one join separator - same reason `metrics`'s own header is reserved up front.
    let header_reserve = lines.join("\n").len() + 1;
    let mut relationship_lines = Vec::new();
    for relationship in view.relationships() {
        relationship_lines.push(format!(
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
            relationship_lines.push(format!("  - Key: `{}` = `{}`{grain}", key.origin(), key.target()));
        }
        if let Some(comparison) = relationship.comparison() {
            relationship_lines.push(format!("  - Comparison as {}", comparison.word()));
        }
    }
    let relationships_text = relationship_lines.join("\n");
    let fragments: Vec<(String, String)> = models
        .into_iter()
        .map(|model| (model.name().to_string(), model_fragment(model, prose)))
        .collect();
    // Each fragment and each omission entry gets the same "+2 join separators" overcount
    // `metrics`'s own fragment/entry costs do - see that module for why. Sized from the REAL
    // candidates, not a worst-case identifier length - see `omission_reserve`'s own doc.
    let fragment_cost = |(_, fragment): &(String, String)| fragment.len() + 2;
    let omission_entry_cost = |(name, _): &(String, String)| name.len() + 2;
    let model_budget = budget
        .saturating_sub(header_reserve)
        .saturating_sub(relationships_text.len() + 1)
        .saturating_sub(omission_reserve(&fragments, omission_entry_cost));
    let (included, omitted_names, omitted_count) =
        fill_to_budget(fragments, model_budget, MAX_OMITTED_NAMES, fragment_cost, |(name, _)| {
            name.clone()
        });
    for (_, fragment) in included {
        lines.push(fragment);
    }
    if omitted_count > 0 {
        lines.push(String::new());
        lines.push(omission_note(&omitted_names, omitted_count));
    }
    if !relationships_text.is_empty() {
        lines.push(relationships_text);
    }
    lines.join("\n")
}

fn model_fragment(model: &sutura_domain::catalog::Model, prose: CatalogProse) -> String {
    let mut lines = vec![
        format!("\n### Model `{}`", model.name()),
        format!("- Table: `{}`", model.table()),
        format!("- Source: `{}`", model.source()),
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
        "Left out of this listing over its shared {MAX_CATALOG_LISTING_BYTES}-byte bound ({total} model(s)): \
         {}{suffix}. Call `describe_catalog(model: <name>)` to read one of those in full.",
        names.join(", ")
    )
}

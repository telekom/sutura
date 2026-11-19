//! Descriptive physical structure, visible only after an operator opts in.

use sutura_domain::pinned::view::ScopedView;

use super::{CatalogProse, quote};

/// The model and column names this view may list. Never selects a query plan.
pub fn physical_schema(view: &ScopedView<'_>, prose: CatalogProse) -> String {
    let models: Vec<_> = view.models().collect();
    if models.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        String::from("## Physical schema (descriptive only)"),
        String::from("Tables and columns describe structure. They do not certify a metric or permit a query."),
    ];
    for model in models {
        lines.push(format!("\n### Model `{}`", model.name()));
        lines.push(format!("- Table: `{}`", model.table()));
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

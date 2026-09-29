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
    lines.join("\n")
}

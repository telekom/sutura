use std::collections::BTreeMap;

use sutura_app::catalog_budget::{MAX_CATALOG_LISTING_BYTES, paged_names};
use sutura_app::prompt::CatalogProse;
use sutura_domain::model::ModelName;
use sutura_domain::pinned::view::ScopedView;

use super::{ColumnContent, JoinKeyContent, ModelContent, RelationshipContent, json_string_cost};
use crate::wire::prose::Carried;
use crate::wire::{ProvenanceContent, bundle_content};

/// One model's name and table, as a `names: true` index entry - nothing else, since this index
/// exists only to make a name reachable, not to describe what it holds.
#[derive(Debug, serde::Serialize)]
pub(super) struct ModelNameEntry {
    name: String,
    table: String,
}

/// A names-only, paged index of every model this caller may see - `describe_catalog(names: true)`'s
/// answer, `github.com/telekom/sutura#2083`'s first ask.
///
/// **Why this exists beside [`CatalogContent::omitted_models`]**: that field is itself capped at
/// [`MAX_OMITTED_NAMES`], so a catalog with more omissions than that cap left the rest of them with
/// no argument that could ever name them. This index has no such cap on how many PAGES it takes to
/// walk it - only [`Self::next`] decides whether another page remains, via [`paged_names`], so every
/// visible model is reachable, eventually, by repeating the call with `after` set to the cursor each
/// page returns.
#[derive(Debug, serde::Serialize)]
pub struct NamesIndexContent {
    /// Which snapshot this index describes - see [`CatalogContent::provenance`].
    provenance: ProvenanceContent,
    /// This page's names, in the same order the full listing's models would appear in - a `BTreeMap`
    /// walk, so repeating `after` from one page always means the same resume point.
    model_names: Vec<ModelNameEntry>,
    /// The cursor to pass as `after` to continue this index - absent once every name matching
    /// `model_prefix` (if any) has been returned, i.e. the index is exhausted.
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<String>,
}

impl NamesIndexContent {
    /// `after` and `model_prefix` are raw strings here, not yet the newtypes the rest of this module
    /// parses `model`/`metric` into - a cursor or a prefix is not itself required to be a valid
    /// `ModelName`, only a prefix or exact value one happens to start with, so there is nothing to
    /// parse into.
    pub fn of(view: &ScopedView<'_>, after: Option<&str>, model_prefix: Option<&str>) -> Self {
        // A small fixed reserve for the provenance trailer and this result's own JSON scaffolding -
        // the same role `OMISSION_HEADER_RESERVE` plays for a bounded pool, sized generously rather
        // than measured, since this index carries nothing else fixed to measure.
        const FIXED_RESERVE: usize = 256;
        let provenance = bundle_content(view.pinned());
        let fixed_cost =
            json_string_cost(&provenance.definition_version) + json_string_cost(&provenance.definition_digest) + FIXED_RESERVE;
        let budget = MAX_CATALOG_LISTING_BYTES.saturating_sub(fixed_cost);
        let (page, next) = paged_names(
            view.models(),
            after,
            model_prefix,
            budget,
            |model| {
                let table = model.table().to_string();
                json_string_cost(model.name().as_str())
                    + json_string_cost(&table)
                    + model.name().as_str().len()
                    + table.len()
                    + 16
            },
            |model| String::from(model.name().as_str()),
        );
        let model_names = page
            .into_iter()
            .map(|model| ModelNameEntry {
                name: String::from(model.name().as_str()),
                table: model.table().to_string(),
            })
            .collect();
        Self {
            provenance,
            model_names,
            next,
        }
    }

    /// The same index as text, for the content block beside the structured one - see
    /// [`CatalogContent::as_text`] for why both are sent.
    pub(crate) fn as_text(&self) -> String {
        let mut out = String::from("model names (paged index; names only, nothing else):");
        if self.model_names.is_empty() {
            out.push_str("\n  no visible models matched");
        }
        for entry in &self.model_names {
            out.push_str("\n  ");
            out.push_str(&entry.name);
            out.push_str(" (table ");
            out.push_str(&entry.table);
            out.push(')');
        }
        match &self.next {
            Some(cursor) => {
                out.push_str("\n\nmore names remain - call describe_catalog(names: true, after: \"");
                out.push_str(cursor);
                out.push_str("\") to continue.");
            }
            None => out.push_str("\n\nindex complete - every visible model name has been returned."),
        }
        out.push_str("\ndefinitions: ");
        out.push_str(&self.provenance.definition_version);
        out.push_str(" (digest ");
        out.push_str(&self.provenance.definition_digest);
        out.push(')');
        out
    }
}

/// Every model this listing may include, filtered to the caller's `view`, narrowed to one `model`
/// when given, or to names starting with `model_prefix` when given - factored out of
/// [`CatalogContent::of_with_physical_schema`] so that function stays under `cargo xtask`'s line cap.
pub(super) fn physical_schema_candidates(
    view: &ScopedView<'_>,
    prose: CatalogProse,
    model: Option<&ModelName>,
    model_prefix: Option<&str>,
) -> Vec<ModelContent> {
    fn content_of(
        model: &sutura_domain::catalog::Model,
        relationships: &mut BTreeMap<&ModelName, Vec<RelationshipContent>>,
        prose: CatalogProse,
    ) -> ModelContent {
        ModelContent {
            name: String::from(model.name().as_str()),
            table: model.table().to_string(),
            relationships: relationships.remove(model.name()).unwrap_or_default(),
            description: Carried::under(prose, model.description()),
            columns: model
                .columns()
                .map(|column| ColumnContent {
                    name: String::from(column.name().as_str()),
                    description: Carried::under(prose, column.description()),
                })
                .collect(),
        }
    }
    let mut relationships = BTreeMap::<&ModelName, Vec<RelationshipContent>>::new();
    for relationship in view.relationships() {
        relationships
            .entry(relationship.origin_model())
            .or_default()
            .push(RelationshipContent {
                name: relationship.name().as_str().to_owned(),
                from: relationship.origin_model().as_str().to_owned(),
                to: relationship.target_model().as_str().to_owned(),
                cardinality: relationship.join_type().as_str(),
                keys: relationship
                    .keys()
                    .iter()
                    .map(|key| JoinKeyContent {
                        from: key.origin().as_str().to_owned(),
                        to: key.target().as_str().to_owned(),
                        grain: key.grain().map(|grain| grain.as_str().to_owned()),
                    })
                    .collect(),
                comparison: relationship.comparison().map(sutura_domain::plan::LinkKind::word),
            });
    }
    // Unified for a `model`-filtered and an unfiltered request alike: at most one model ever flows
    // through this iterator when `model` is `Some`, so the bound a caller's fold applies downstream
    // only ever matters for the unfiltered case in practice - but both cases spend the same budget
    // rather than one path silently exempting a targeted request from a limit the other obeys.
    match model {
        Some(name) => view
            .model(name)
            .into_iter()
            .map(|model| content_of(model, &mut relationships, prose))
            .collect(),
        None => view
            .models()
            .filter(|model| model_prefix.is_none_or(|prefix| model.name().as_str().starts_with(prefix)))
            .map(|model| content_of(model, &mut relationships, prose))
            .collect(),
    }
}

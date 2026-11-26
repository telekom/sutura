use std::collections::BTreeMap;

use sutura_app::catalog_budget::{MAX_CATALOG_LISTING_BYTES, paged_names};
use sutura_app::prompt::CatalogProse;
use sutura_domain::model::ModelName;
use sutura_domain::pinned::view::ScopedView;

use super::{ColumnContent, JoinKeyContent, ModelContent, RelationshipContent};
use crate::wire::prose::Carried;
use crate::wire::{ProvenanceContent, bundle_content};

/// One model's name and table, as a `names: true` index entry - nothing else, since this index
/// exists only to make a name reachable, not to describe what it holds.
#[derive(Debug, serde::Serialize)]
pub(super) struct ModelNameEntry {
    name: String,
    table: String,
}

/// One metric's name and owning model, as a `names: true` index entry - the metrics-side sibling of
/// [`ModelNameEntry`], carrying `model` for the same reason [`OmittedMetric`](super::OmittedMetric)
/// does: `describe_catalog(metric: ...)` is this metric's own read-back, and `model` is the only
/// value that narrows the full listing to this metric's physical schema.
#[derive(Debug, serde::Serialize)]
pub(super) struct MetricNameEntry {
    name: String,
    model: String,
}

/// The real rendering cost of one [`ModelNameEntry`] - derived, not guessed: the structured half
/// serializes the entry itself (plus one byte for the list separator `,`), and the text half is
/// exactly the line [`NamesIndexContent::as_text`] pushes per model - `\n  <name> (table <table>)`,
/// i.e. `name.len() + table.len() + 12`.
fn model_entry_cost(model: &sutura_domain::catalog::Model) -> usize {
    let table = model.table().to_string();
    let entry = ModelNameEntry {
        name: String::from(model.name().as_str()),
        table: table.clone(),
    };
    let structured = serde_json::to_vec(&entry).map_or(0, |bytes| bytes.len()) + 1;
    let text = model.name().as_str().len() + table.len() + 12;
    structured + text
}

/// The real rendering cost of one [`MetricNameEntry`] - the metrics-side sibling of
/// [`model_entry_cost`], same derivation: `\n  <name> (model <model>)` is `name.len() + model.len() +
/// 12`.
fn metric_entry_cost(metric: &sutura_domain::catalog::Metric) -> usize {
    let model = metric.model().as_str().to_owned();
    let entry = MetricNameEntry {
        name: String::from(metric.name().as_str()),
        model: model.clone(),
    };
    let structured = serde_json::to_vec(&entry).map_or(0, |bytes| bytes.len()) + 1;
    let text = metric.name().as_str().len() + model.len() + 12;
    structured + text
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
    /// This page's model names, in the same order the full listing's models would appear in - a
    /// `BTreeMap` walk, so repeating `after` from one page always means the same resume point.
    model_names: Vec<ModelNameEntry>,
    /// The cursor to pass as `after` to continue the model half of this index - absent once every
    /// model name matching `model_prefix` (if any) has been returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<String>,
    /// This page's metric names, paged and cursored the same way `model_names` is - the metrics-side
    /// sibling closing the same gap [`CatalogContent::omitted_metrics`] leaves once it crosses
    /// `MAX_OMITTED_NAMES`.
    metric_names: Vec<MetricNameEntry>,
    /// The cursor to pass as `after_metric` to continue the metric half of this index.
    #[serde(skip_serializing_if = "Option::is_none")]
    next_metric: Option<String>,
}

/// The real worst-case byte cost of this index's fixed envelope - the provenance trailer plus both
/// halves' header and "more remain" sentences, each carrying the longest cursor a name can produce
/// ([`sutura_domain::model::MAX_IDENTIFIER_LEN`]) - measured by rendering an actual empty-entries
/// skeleton rather than guessed, since a guess drifts the moment either sentence's wording changes.
fn envelope_reserve(view: &ScopedView<'_>) -> usize {
    let cursor = "a".repeat(sutura_domain::model::MAX_IDENTIFIER_LEN);
    let skeleton = NamesIndexContent {
        provenance: bundle_content(view.pinned()),
        model_names: Vec::new(),
        next: Some(cursor.clone()),
        metric_names: Vec::new(),
        next_metric: Some(cursor),
    };
    let text_len = skeleton.as_text().len();
    let json_len = serde_json::to_vec(&skeleton).map_or(0, |bytes| bytes.len());
    text_len + json_len
}

impl NamesIndexContent {
    /// `after`, `after_metric` and `model_prefix` are raw strings here, not yet the newtypes the
    /// rest of this module parses `model`/`metric` into - a cursor or a prefix is not itself
    /// required to be a valid `ModelName`, only a prefix or exact value one happens to start with,
    /// so there is nothing to parse into.
    ///
    /// **One shared budget, models first then metrics** - the same "spend in sequence" rule
    /// [`CatalogContent::of_with_physical_schema`] follows, so the whole page (both halves) never
    /// exceeds [`MAX_CATALOG_LISTING_BYTES`]. `model_prefix` narrows metrics by their OWN model
    /// (`Metric::model`), matching how it narrows the full listing's metrics pool.
    pub fn of(view: &ScopedView<'_>, after: Option<&str>, after_metric: Option<&str>, model_prefix: Option<&str>) -> Self {
        let provenance = bundle_content(view.pinned());
        let budget = MAX_CATALOG_LISTING_BYTES.saturating_sub(envelope_reserve(view));

        // A caller only ever sends `after_metric` without `after` once it has already seen `next:
        // None` for the model half - so treat the model half as already finished and give metrics the
        // whole budget, instead of re-walking (and re-spending the budget on) models every call. Without
        // this, a caller paging `after_metric` to exhaustion would restart the model pool each time,
        // starving metrics to roughly one entry per call - `github.com/telekom/sutura#2083`'s review.
        let skip_models = after.is_none() && after_metric.is_some();

        let (model_names, next, models_spent) = if skip_models {
            (Vec::<ModelNameEntry>::new(), None::<String>, 0_usize)
        } else {
            let (page, next) = paged_names(
                view.models(),
                after,
                model_prefix,
                budget,
                |model| model_entry_cost(model),
                |model| String::from(model.name().as_str()),
            );
            let spent: usize = page.iter().map(|model| model_entry_cost(model)).sum();
            let names = page
                .into_iter()
                .map(|model| ModelNameEntry {
                    name: String::from(model.name().as_str()),
                    table: model.table().to_string(),
                })
                .collect();
            (names, next, spent)
        };

        let metric_budget = budget.saturating_sub(models_spent);
        let metric_candidates: Vec<&sutura_domain::catalog::Metric> = view
            .metrics()
            .filter(|metric| model_prefix.is_none_or(|prefix| metric.model().as_str().starts_with(prefix)))
            .collect();

        // The model half already spent part of the shared budget this call - never force the first
        // metric candidate in over budget the way `fill_to_budget` would for an otherwise-empty page;
        // a sentinel empty-string cursor defers the whole metric half to a call that (paired with no
        // `after`) hits `skip_models` above and gets the full budget instead.
        let starved = models_spent > 0
            && metric_candidates
                .first()
                .is_some_and(|metric| metric_entry_cost(metric) > metric_budget);

        let (metric_names, next_metric) = if starved {
            (Vec::new(), Some(String::new()))
        } else {
            let (page, next) = paged_names(
                metric_candidates.into_iter(),
                after_metric,
                None,
                metric_budget,
                |metric| metric_entry_cost(metric),
                |metric| String::from(metric.name().as_str()),
            );
            let names = page
                .into_iter()
                .map(|metric| MetricNameEntry {
                    name: String::from(metric.name().as_str()),
                    model: metric.model().as_str().to_owned(),
                })
                .collect();
            (names, next)
        };

        Self {
            provenance,
            model_names,
            next,
            metric_names,
            next_metric,
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
                out.push_str("\n\nmore model names remain - call describe_catalog(names: true, after: \"");
                out.push_str(cursor);
                out.push_str("\") to continue.");
            }
            None => out.push_str("\n\nmodel index complete - every visible model name has been returned."),
        }
        out.push_str("\n\nmetric names (paged index; names only, nothing else):");
        if self.metric_names.is_empty() {
            out.push_str("\n  no visible metrics matched");
        }
        for entry in &self.metric_names {
            out.push_str("\n  ");
            out.push_str(&entry.name);
            out.push_str(" (model ");
            out.push_str(&entry.model);
            out.push(')');
        }
        match &self.next_metric {
            Some(cursor) => {
                out.push_str("\n\nmore metric names remain - call describe_catalog(names: true, after_metric: \"");
                out.push_str(cursor);
                out.push_str("\") to continue.");
            }
            None => out.push_str("\n\nmetric index complete - every visible metric name has been returned."),
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

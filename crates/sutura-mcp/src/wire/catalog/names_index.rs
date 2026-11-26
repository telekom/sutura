use std::collections::BTreeMap;

use sutura_app::catalog_budget::{MAX_CATALOG_LISTING_BYTES, MalformedNamesCursor, NamesCursor, paged_names};
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
    /// This page's metric names, paged and cursored the same way `model_names` is - the metrics-side
    /// sibling closing the same gap [`CatalogContent::omitted_metrics`] leaves once it crosses
    /// `MAX_OMITTED_NAMES`.
    metric_names: Vec<MetricNameEntry>,
    /// The cursor to pass as `after` to continue this index - ONE sequential walk, models then
    /// metrics: absent only once every visible model AND every visible metric has been returned.
    /// Format it exactly as received here - it round-trips through [`NamesCursor`]'s own
    /// `Display`/`parse`, never a value this caller constructs itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<String>,
}

/// The real worst-case byte cost of this index's fixed envelope - the provenance trailer plus both
/// halves' header and the single "more remain" sentence, carrying the longest cursor this index can
/// produce ([`NamesCursor::Metric`] over [`sutura_domain::model::MAX_IDENTIFIER_LEN`] bytes, the
/// longer of its two variants) - measured by rendering an actual empty-entries skeleton rather than
/// guessed, since a guess drifts the moment the sentence's wording changes.
fn envelope_reserve(view: &ScopedView<'_>) -> usize {
    let cursor = NamesCursor::Metric("a".repeat(sutura_domain::model::MAX_IDENTIFIER_LEN)).to_string();
    let skeleton = NamesIndexContent {
        provenance: bundle_content(view.pinned()),
        model_names: Vec::new(),
        metric_names: Vec::new(),
        next: Some(cursor),
    };
    let text_len = skeleton.as_text().len();
    let json_len = serde_json::to_vec(&skeleton).map_or(0, |bytes| bytes.len());
    text_len + json_len
}

impl NamesIndexContent {
    /// `after` is [`NamesCursor`]'s own wire form here - parsed up front, so a malformed value is
    /// refused before any pool is walked, not read as "start from the beginning". `model_prefix` is
    /// a raw string, not yet the `ModelName` newtype `model`/`metric` get parsed into elsewhere: a
    /// prefix is not itself required to be a valid name, only a prefix one happens to start with.
    ///
    /// **ONE sequential cursor, models then metrics - `github.com/telekom/sutura#2083`'s round 3.**
    /// Two independent cursors (`after`/`after_metric`) let a caller address either half without the
    /// other, which is exactly what went wrong: a caller that only ever followed the metric cursor
    /// could make the model half look finished without ever having walked it, and the metric half's
    /// "first candidate always admitted" rule (`fill_to_budget`'s own, shared by every caller of it)
    /// could fire on every single call rather than once. A `NamesCursor::Model` resumes the model
    /// half; `NamesCursor::Metric` means the model half is ALREADY exhausted, so this call skips it
    /// and resumes metrics directly - there is no separate "did I already finish models" question to
    /// get wrong, because only the cursor the previous page actually returned can ask for it. If the
    /// model half finishes within this very call, the leftover budget carries straight into metrics
    /// in the same page - the one place `fill_to_budget`'s admit-the-first-item rule can fire, which
    /// happens at most once across the whole walk rather than on a caller's every repeated call.
    pub fn of(view: &ScopedView<'_>, after: Option<&str>, model_prefix: Option<&str>) -> Result<Self, MalformedNamesCursor> {
        let cursor = after.map(NamesCursor::parse).transpose()?;
        let provenance = bundle_content(view.pinned());
        let budget = MAX_CATALOG_LISTING_BYTES.saturating_sub(envelope_reserve(view));

        let model_after = match &cursor {
            Some(NamesCursor::Model(name)) => Some(name.as_str()),
            _ => None,
        };
        let walk_models = !matches!(cursor, Some(NamesCursor::Metric(_)));

        let (model_names, models_next, models_spent) = if walk_models {
            let (page, next) = paged_names(
                view.models(),
                model_after,
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
        } else {
            (Vec::new(), None, 0_usize)
        };

        // The model half is walked (not skipped) only until IT returns no further cursor - which
        // covers both "there was no model cursor to begin with" and "this very call's model page
        // reached the end" - so metrics only start once models are truly exhausted, in or across
        // calls, never before.
        let walk_metrics = models_next.is_none();
        let metric_after = match &cursor {
            Some(NamesCursor::Metric(name)) => Some(name.as_str()),
            _ => None,
        };
        let metric_budget = budget.saturating_sub(models_spent);
        let (metric_names, metrics_next) = if walk_metrics {
            let metric_candidates = view
                .metrics()
                .filter(|metric| model_prefix.is_none_or(|prefix| metric.model().as_str().starts_with(prefix)));
            let (page, next) = paged_names(
                metric_candidates,
                metric_after,
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
        } else {
            (Vec::new(), None)
        };

        let next = match (models_next, metrics_next) {
            (Some(name), _) => Some(NamesCursor::Model(name)),
            (None, Some(name)) => Some(NamesCursor::Metric(name)),
            (None, None) => None,
        }
        .map(|cursor| cursor.to_string());

        Ok(Self {
            provenance,
            model_names,
            metric_names,
            next,
        })
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
        match &self.next {
            Some(cursor) => {
                out.push_str("\n\nmore names remain - call describe_catalog(names: true, after: \"");
                out.push_str(cursor);
                out.push_str("\") to continue.");
            }
            None => out.push_str("\n\nindex complete - every visible model and metric name has been returned."),
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

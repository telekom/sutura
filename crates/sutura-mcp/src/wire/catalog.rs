//! The catalog tool's wire shape: what `describe_catalog` takes, what it answers, and the text half
//! of that answer.
//!
//! **Its own module because `wire.rs` was twenty-one lines under the thousand-line limit
//! `cargo xtask max-lines` enforces and cannot exempt**, at the banner that file already carried -
//! and the seam is one whole TOOL rather than a share of lines. `wire.rs` keeps the question and the
//! answer of `ask`; everything here belongs to the other tool a caller can reach, and the two share
//! nothing but `prose`, which is where the `Option` behind a description stays private.
//!
//! Why the reasoning about a wire type is a plain comment rather than a doc comment is stated at
//! `DescribeCatalogArgs` - `schemars` puts a root doc comment into the schema's `description`, which
//! is text a MODEL reads.

use sutura_app::catalog_budget::{MAX_CATALOG_LISTING_BYTES, MAX_OMITTED_NAMES, fill_to_budget, omission_reserve};
use sutura_app::prompt::CatalogProse;
use sutura_domain::model::{Grain, MetricName, ModelName};
use sutura_domain::pinned::view::ScopedView;

use super::prose::{self, Carried};
use super::{ProvenanceContent, bundle_content};

// `deny_unknown_fields` is why this type exists rather than accepting any object: `schemars` reads
// the same attribute serde does, so the advertised schema says `additionalProperties: false` and an
// arguments object carrying `sql` or anything else this type does not declare is a named parse
// error rather than a key dropped on the floor. A tool that accepted any object would be a tool
// whose surface a caller could guess at, and a caller that sent an unknown field would believe it
// had narrowed a listing it did not.
//
// It is also what keeps this half of the surface under the same drift guard as the other: the
// generated schema is snapshotted in `crate::tool`, so a field added here lands in a reviewer's diff.
//
// **A plain comment and not a doc comment, deliberately.** `schemars` puts a root doc comment into
// the schema's `description`, which is text a MODEL reads before it calls the tool - so the doc
// comment on a wire type is caller-facing prose and the reasoning about the type goes here. The one
// below is written for that reader.
//
// **`model` is parsed, never matched against anything here.** `server::describe` resolves it to a
// `ModelName` and then to a `&Model` through the caller's own `ScopedView`, exactly as the catalog
// itself narrows by caller identity - a name that does not parse, or one that parses but names no
// model this caller may see, is refused rather than answered with an empty listing
// (`github.com/telekom/sutura#2071`'s second ask).
//
// **`names`/`after`/`model_prefix` close `#2083`**: the byte-bounded listing's `omitted_models` (and
// `omitted_metrics`) list is itself capped at `MAX_OMITTED_NAMES` entries, so a catalog with more
// omissions than that cap left the rest of them genuinely unreachable - no argument existed to ask
// for them. `names: true` switches this tool to a names-only paged index instead of the full
// listing: every visible model's name and table, then every visible metric's name and owning model,
// walked as ONE sequential stream in the same budget, with `after` a single opaque cursor to resume
// from once a page does not finish the index - round 3 of this review collapsed an earlier
// `after`/`after_metric` pair into this one cursor, because two independent ones let a caller
// desynchronise them (follow only the metric cursor and the model half never gets walked at all).
// `model_prefix` narrows either mode (the full listing or the names index) to names starting with a
// prefix, which is how an agent that only has part of a name reaches it without already knowing
// where it sits in the full list.
/// This tool returns the catalog of what this deployment measures, narrowed to what the calling
/// principal may see - the metrics, grains, dimensions and permitted values.
///
/// `model` is optional and narrows the answer to one named model: only the metrics tied to it, and -
/// where the operator exposes physical schema - only that model's own columns and relationships.
/// `metric` is optional and narrows the answer to that one metric alone, naming nothing about its
/// model's physical schema - the way to read back a single metric a previous, byte-bounded listing
/// named but left out. Give at most one of `model` and `metric`; both together is refused. Omit
/// both for the whole catalog. An unfiltered catalog whose metrics or physical schema would be
/// large is itself capped at a byte bound shared across the whole result; where it is, every item
/// left out is named explicitly up to [`sutura_app::catalog_budget::MAX_OMITTED_NAMES`] names,
/// along with how to read it back - call this tool again with `model` or `metric` set to one of
/// those names.
///
/// `model_prefix` narrows the listing (or the names index below) to model names starting with that
/// prefix, for when only part of a name is known. Mutually exclusive with `model` and `metric`.
///
/// `names: true` switches this tool to a names-only index of every visible model AND every visible
/// metric - name and table for a model, name and owning model for a metric, nothing else - walked as
/// one sequential stream (every model, in order, then every metric) under the same byte bound and
/// NOT subject to the omission list's own cap on how many names it carries. Once a page does not
/// cover every remaining name, the result carries a `next` cursor; call this tool again with
/// `after` set to that cursor, verbatim, to continue. `next` is absent only once BOTH halves are
/// exhausted. This is how every model or metric past that cap is reached, not only the first page
/// of them. Mutually exclusive with `model` and `metric`; `after` is only meaningful alongside
/// `names: true`, and a value this tool did not itself return is refused.
///
/// This tool also returns the catalog's own knowledge (the glossary, caveats, worked examples and
/// terms recorded as undefined it carries) and the deployment operator's instructions - neither is
/// scoped to one model or metric, so none of these arguments narrow them.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeCatalogArgs {
    /// The model's own name - the same identifier its physical schema prints as `model <name>`.
    /// Omit it for the whole catalog. Mutually exclusive with `metric`, `model_prefix` and `names`.
    model: Option<String>,
    /// One metric's own name, to read back exactly that metric's entry - nothing about its model's
    /// physical schema. Omit it for the whole catalog. Mutually exclusive with `model`,
    /// `model_prefix` and `names`.
    metric: Option<String>,
    /// Narrows the listing, or the `names` index, to model names starting with this prefix.
    /// Mutually exclusive with `model` and `metric`. Refused if given empty.
    model_prefix: Option<String>,
    /// `true` switches this tool to a names-only, paged index of every visible model AND every
    /// visible metric - see this type's own doc for how to page through it with `after`. Mutually
    /// exclusive with `model` and `metric`.
    names: Option<bool>,
    /// Resumes a `names: true` index at the point the previous call left off - pass the `next`
    /// cursor that call returned, verbatim. Refused unless `names` is also `true`, and refused if it
    /// does not parse as a cursor this tool itself produced.
    after: Option<String>,
}

/// [`DescribeCatalogArgs`], parsed rather than left as five independent `Option`s - the fields this
/// tool's handler actually reads are never each other's business, but their MUTUAL EXCLUSION is, and
/// a struct whose fields are already proven consistent is harder to misuse than five parameters a
/// caller could pass in any combination.
pub(crate) struct DescribeCatalogRequest {
    pub(crate) model: Option<String>,
    pub(crate) metric: Option<String>,
    pub(crate) model_prefix: Option<String>,
    pub(crate) names: bool,
    pub(crate) after: Option<String>,
}

impl DescribeCatalogArgs {
    /// The five raw fields, taken rather than borrowed - `server::describe` is the only caller and
    /// it is the last read this value gets, so there is nothing left to hand a borrow to.
    pub(crate) fn into_request(self) -> DescribeCatalogRequest {
        DescribeCatalogRequest {
            model: self.model,
            metric: self.metric,
            model_prefix: self.model_prefix,
            names: self.names.unwrap_or(false),
            after: self.after,
        }
    }
}

// **Why this half reads the prose setting, and a plain comment for the reason `DescribeCatalogArgs`
// states**: reasoning about a wire type is not caller-facing prose. Not to escape anything - `serde`
// owns the field boundary here, so a description cannot cross one whatever it spells.
// `prompt.catalog_prose` decides **who may put words in front of an agent**, which is a property of
// the deployment rather than of one field on one surface. This half shipped every description while
// the text block beside it withheld them: `docs/adr/0022`'s second amendment.
/// What this deployment measures, as the catalog tool's structured content.
///
/// **A second wire type beside `sutura_http::wire::CatalogBody` for the metric half, and the same
/// deliberate cost [`super::AskArgs`] already pays.** An adapter never calls another adapter, so
/// this crate cannot import that shape; what keeps the metric halves equal is review plus the fact
/// that both are built from the one `sutura_domain::pinned::PinnedDefinitions` accessor set. The
/// knowledge text is not a second copy: both transports call
/// `sutura_app::prompt::catalog_knowledge` over the caller's view.
/// **The whole type is still WIDER than `CatalogBody`**: it also carries the operator's
/// instructions and, where the operator enabled them, the physical models, which the HTTP
/// `/v1/catalog` surface does not.
///
/// **What narrows this listing is the CALLER's identity - `docs/adr/0028` - and the caller sends
/// nothing that WIDENS it.** `sutura_domain::pinned::SemanticCatalog::load` takes no request context
/// and cannot be given one, so the caller's mapped audiences (which `describe` reads and this
/// constructor takes as a [`ScopedView`]) decide which metrics and which of their knowledge the
/// listing may ever hold; `model`/`metric`, the two mutually exclusive arguments this tool takes,
/// only ever narrow within that - one named model's own slice, or one named metric alone, of what
/// the view already permits, never a wider one. Invisible means absent, and it is the transport's
/// job to build the view, never this type's.
#[derive(Debug, serde::Serialize)]
pub struct CatalogContent {
    /// Which snapshot this listing describes. The same version and digest an answer carries, so a
    /// model can tell that the metric it read about is the metric it measured.
    provenance: ProvenanceContent,
    /// Which way the operator's `prompt.catalog_prose` setting points - the operator's own spelling,
    /// echoed rather than translated, as `sutura_http::wire::CatalogBody` echoes it - so an absent
    /// description is a fact a client can read rather than one it has to infer.
    catalog_prose: &'static str,
    // The trust boundary the text half names. Resolved by `prose::notice` at construction rather
    // than decided again in `as_text`, so the two halves of one reply cannot answer the operator's
    // question differently. `skip`: this crate's own text, not a field a client reads.
    #[serde(skip)]
    notice: &'static str,
    /// Held to this result's shared [`MAX_CATALOG_LISTING_BYTES`] budget - see
    /// [`Self::of_with_physical_schema`] for the spend order - a metric's whole entry (its
    /// dimensions and allowed values included) is either carried whole or left out and named, with
    /// its model, in [`Self::omitted_metrics`], never truncated mid-metric. Bounded even when
    /// `model` narrowed the request: a model with an unusually large metric vocabulary of its own
    /// still should not unbound this listing.
    metrics: Vec<MetricContent>,
    /// Metrics that exist and are visible, but were left out of [`Self::metrics`] to hold this
    /// result under its shared budget. Each entry carries both the metric AND its model, as a
    /// structured [`OmittedMetric`] rather than a joined string, so it is reachable with
    /// `describe_catalog(metric: <name>)` without a client having to parse which part of one string
    /// is the model. Capped at [`MAX_OMITTED_NAMES`] entries; [`Self::omitted_metrics_more`] carries
    /// the true count past that cap.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    omitted_metrics: Vec<OmittedMetric>,
    /// How many more metrics were left out of [`Self::omitted_metrics`] than that list names, once
    /// the [`MAX_OMITTED_NAMES`] cap on names is reached. A separate field rather than a trailing
    /// `"...and N more"` string in the name list, so a client reading names does not have to
    /// recognize a sentinel entry as something other than a name.
    #[serde(skip_serializing_if = "is_zero")]
    omitted_metrics_more: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    models: Option<Vec<ModelContent>>,
    /// Physical models that exist and are visible, but were left out of [`Self::models`] to hold the
    /// result under its shared budget - bounded the same way for a `model`-filtered request as for
    /// an unfiltered one, though a filtered request names at most one model and so essentially never
    /// triggers it. Capped at [`MAX_OMITTED_NAMES`] names the same way [`Self::omitted_metrics`] is;
    /// [`Self::omitted_models_more`] carries the true count past that cap. Read each one back with
    /// `describe_catalog(model: <name>)`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    omitted_models: Vec<String>,
    /// How many more models were left out of [`Self::omitted_models`] than that list names. See
    /// [`Self::omitted_metrics_more`] for why this is its own field.
    #[serde(skip_serializing_if = "is_zero")]
    omitted_models_more: usize,
    /// The knowledge sections - glossary, caveats, terms recorded as undefined, worked examples -
    /// audience-scoped through the same [`ScopedView`] that narrowed the metrics, descriptive only,
    /// bounded by the bundle's own knowledge cap (`MAX_KNOWLEDGE_BYTES`). `catalog_prose` decides
    /// whether note bodies are quoted in it.
    ///
    /// A caller that renders only the structured half of the listing still needs the glossary it is
    /// scoped to read: a phrase-resolution surface has no `initialize.instructions` to deliver this
    /// through, which is the whole of issue #971.
    knowledge: String,
    /// The operator's own instructions, in their own section and NOT part of [`Self::knowledge`].
    ///
    /// Deployment-wide and the same for every caller - never audience-scoped - so it sits outside
    /// the untrusted-catalog notice and under its own "operator's instructions" heading. An operator
    /// who names a restricted metric in it discloses that metric to every caller with `catalog.read`.
    /// Read once at startup, by the CLI composition root under `prompt.instructions_max_bytes`, and
    /// repeated in full on every call. This wire type does not check that cap itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
}

/// One metric left out of [`CatalogContent::metrics`] to hold the result under its shared budget.
///
/// Both names together, so `describe_catalog(metric: <name>)` is reachable without a client first
/// discovering, by some other means, which model this metric belongs to.
#[derive(Debug, serde::Serialize)]
pub struct OmittedMetric {
    metric: String,
    model: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ModelContent {
    name: String,
    table: String,
    /// The listed sutura source this table lives at - what `run_sql` takes as the prefix in
    /// `source.table` or `source.schema.table` to say which source a path two of them share
    /// should resolve against.
    source: String,
    #[serde(skip_serializing_if = "Carried::is_absent")]
    description: Carried,
    columns: Vec<ColumnContent>,
    relationships: Vec<RelationshipContent>,
}

#[derive(Debug, serde::Serialize)]
pub struct ColumnContent {
    name: String,
    #[serde(skip_serializing_if = "Carried::is_absent")]
    description: Carried,
}

#[derive(Debug, serde::Serialize)]
pub struct RelationshipContent {
    name: String,
    from: String,
    to: String,
    cardinality: &'static str,
    keys: Vec<JoinKeyContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    comparison: Option<&'static str>,
}
#[derive(Debug, serde::Serialize)]
pub struct JoinKeyContent {
    from: String,
    to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    grain: Option<String>,
}

/// One metric, as much of it as a caller needs to ask a valid question.
#[derive(Debug, serde::Serialize)]
pub struct MetricContent {
    name: String,
    /// The model this metric is defined over - the value `describe_catalog(model: ...)` takes to
    /// read that model's own physical detail, where the operator exposes physical schema at all. An
    /// agent reading this listing with `list_physical_schema` off still needs a valid `model` to
    /// narrow by, and this is the only place that name is ever carried next to a metric.
    model: String,
    /// The author's own prose, or absent where the operator omitted it. See [`Carried`].
    #[serde(skip_serializing_if = "Carried::is_absent")]
    description: Carried,
    /// Coarsest first, which is the order an anchor is checked at.
    grains: Vec<String>,
    dimensions: Vec<DimensionContent>,
}

/// One dimension of one metric.
#[derive(Debug, serde::Serialize)]
pub struct DimensionContent {
    name: String,
    /// The author's own prose, or absent where the operator omitted it. See [`Carried`].
    #[serde(skip_serializing_if = "Carried::is_absent")]
    description: Carried,
    /// Whether this dimension can be filtered on as well as grouped by.
    filterable: bool,
    /// The values a filter may use, where the catalog declares a set. Absent means groupable and not
    /// filterable.
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_values: Option<Vec<String>>,
}

/// `serde(skip_serializing_if)` for the `_more` count fields - zero means every omitted item (if
/// any) is already named, so the field is dropped rather than serialized as a bare `0`.
///
/// `serde`'s `skip_serializing_if` calls this with `&usize`, so neither the signature nor its
/// by-reference argument is this function's choice to make.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    clippy::missing_const_for_fn,
    reason = "signature is fixed by serde's skip_serializing_if contract"
)]
fn is_zero(count: &usize) -> bool {
    *count == 0
}

impl CatalogContent {
    /// The reader's view of one pinned bundle, under the prose setting this deployment was started
    /// with and under the caller this read is for.
    ///
    /// **A named constructor rather than a `From`, and the argument is the reason.** A conversion
    /// reachable without the setting fails OPEN - it ships the prose of a deployment that asked for
    /// none, which is the defect this function exists to close, and it is how that defect arrived
    /// here. A second argument cannot be left out.
    ///
    /// **The view is the second reason a `From` would be wrong, and it is the one `docs/adr/0028`
    /// exists to close.** A caller may see only the metrics its granted audiences name; rendering
    /// from a bare `&PinnedDefinitions` would hand every caller the whole bundle again, which is
    /// the defect this surface shipped until it took the view. `ScopedView` borrows the bundle, so
    /// this builder cannot reach `SemanticCatalog::load` - the knowledge sections and the metrics
    /// are filtered by the caller's own grant, never by a call the renderer omits.
    pub fn of(view: &ScopedView<'_>, prose: CatalogProse, instructions: Option<&str>) -> Self {
        Self::of_with_physical_schema(view, prose, instructions, false, None, None, None)
    }

    /// Include physical models only when the operator enabled them for this caller view, narrow to
    /// one model when `model` names one, narrow to one metric's own entry when `metric` names one, or
    /// narrow to model names starting with `model_prefix` when given (mutually exclusive with
    /// `model`/`metric`, enforced by `server::describe` before this is reached).
    ///
    /// **`model` and `metric` are already resolved to something existing and visible by the
    /// caller, and never both at once** - `server::describe` is the one place that turns a raw
    /// argument into either, refusing an unknown or invisible name, and refusing the two together,
    /// before this constructor is ever reached. This function cannot fail: every input it takes
    /// already named something real.
    ///
    /// **One shared [`MAX_CATALOG_LISTING_BYTES`] budget for the whole result, spent in sequence -
    /// see that constant's own doc for why.** The fixed parts (the notice, the knowledge section, the
    /// operator's instructions, the provenance trailer) are measured first, counted against BOTH the
    /// text half ([`Self::as_text`]) and the structured half this type serializes as, since a client
    /// reading either half still pays for them. What remains goes to the metrics pool, then whatever
    /// metrics did not spend goes to the physical-models pool. A `metric` request is the one
    /// exception: it names one metric's own entry rather than a listing, so that entry is emitted
    /// whole regardless of budget and no physical schema is attached - a metric has none of its own.
    pub fn of_with_physical_schema(
        view: &ScopedView<'_>,
        prose: CatalogProse,
        instructions: Option<&str>,
        list_physical_schema: bool,
        model: Option<&ModelName>,
        metric: Option<&MetricName>,
        model_prefix: Option<&str>,
    ) -> Self {
        let notice = prose::notice(prose);
        // The knowledge sections ride the same caller's view as the metrics: a caller that may not
        // see a metric does not see that metric's glossary entry, caveat or worked example either.
        // `catalog_knowledge` quotes note bodies under the same `prose` the descriptions follow, so
        // a deployment that withholds catalog prose withholds it from the knowledge tool too.
        //
        // NOT narrowed by `model`/`metric`: the glossary, caveats and worked examples are
        // catalog-wide prose, not one model's or metric's own description, so a narrowed request
        // still needs the terms it uses.
        let knowledge = sutura_app::prompt::catalog_knowledge(view, prose);
        // The operator's own text is DEPLOYMENT-WIDE - the same for every caller - so it gets its
        // own section and heading, outside the untrusted-catalog notice, and a preamble written for
        // a tool that returns no refusal rules. Rendered here rather than by `catalog_knowledge`
        // precisely so it does not sit under the notice that says to ignore catalog prose.
        let instructions = instructions.and_then(|raw| {
            let text = raw.trim();
            (!text.is_empty()).then(|| sutura_app::prompt::tool_operator_instructions(text))
        });
        let provenance = bundle_content(view.pinned());
        // Fixed parts, spent before either pool gets a byte. The text half pays each fixed part's
        // raw length; the structured half never carries `notice` (`#[serde(skip)]`) and pays every
        // other part its JSON-ESCAPED length, not its raw one - a quote or backslash costs real
        // bytes there a raw `.len()` would miss.
        let text_fixed_cost = notice.len()
            + knowledge.len()
            + instructions.as_deref().map_or(0, str::len)
            + provenance.definition_version.len()
            + provenance.definition_digest.len();
        let structured_fixed_cost = json_string_cost(&knowledge)
            + instructions.as_deref().map_or(0, json_string_cost)
            + json_string_cost(&provenance.definition_version)
            + json_string_cost(&provenance.definition_digest);
        let fixed_cost = text_fixed_cost + structured_fixed_cost;

        let metric_cost =
            |content: &MetricContent| serde_json::to_vec(content).map_or(0, |bytes| bytes.len()) + metric_text(content).len() + 2;
        let selected_metrics: Vec<MetricContent> = view
            .metrics()
            .filter(|m| model.is_none_or(|name| m.model() == name))
            .filter(|m| metric.is_none_or(|name| m.name() == name))
            .filter(|m| model_prefix.is_none_or(|prefix| m.model().as_str().starts_with(prefix)))
            .map(|m| MetricContent {
                name: String::from(m.name().as_str()),
                model: String::from(m.model().as_str()),
                description: Carried::under(prose, m.description()),
                grains: {
                    let mut grains: Vec<Grain> = m.grains().iter().copied().collect();
                    // Reversed, because `Grain`'s own ordering runs fine to coarse and a reader wants
                    // the coarsest first - the same order the HTTP surface renders.
                    grains.sort_unstable_by(|left, right| right.cmp(left));
                    grains.into_iter().map(|grain| String::from(grain.as_str())).collect()
                },
                dimensions: m
                    .dimensions()
                    .values()
                    .map(|dimension| DimensionContent {
                        name: String::from(dimension.name().as_str()),
                        description: Carried::under(prose, dimension.description()),
                        filterable: dimension.is_filterable(),
                        allowed_values: dimension
                            .allowed_values()
                            .map(|values| values.iter().map(|value| String::from(value.as_str())).collect()),
                    })
                    .collect(),
            })
            .collect();
        // Sized from the REAL metric candidates about to be weighed, not a worst-case identifier
        // length - see `omission_reserve`'s own doc. Skipped when `metric` narrows to one entry:
        // that path never runs `fill_to_budget` and so never produces an omission note.
        let metrics_omission_reserve = if metric.is_some() {
            0
        } else {
            omission_reserve(&selected_metrics, |content| {
                let text = format!("{} (model {})", content.name, content.model).len() + 2;
                let structured = serde_json::to_vec(&OmittedMetric {
                    metric: content.name.clone(),
                    model: content.model.clone(),
                })
                .map_or(0, |bytes| bytes.len())
                    + 1;
                text + structured
            })
        };

        // The model pool's own candidates, built eagerly - before `remaining` is even known - only
        // when a models section will actually be attached: `metric` skips physical schema entirely
        // regardless of `list_physical_schema`, a metric has none of its own.
        let model_contents: Option<Vec<ModelContent>> = (metric.is_none() && list_physical_schema)
            .then(|| names_index::physical_schema_candidates(view, prose, model, model_prefix));
        // Reserved only when models will actually be listed - see `model_contents` above - and
        // sized from those real candidates, the same way the metrics reserve is.
        let models_omission_reserve = model_contents.as_ref().map_or(0, |contents| {
            omission_reserve(contents, |content| {
                let text = content.name.len() + 2;
                let structured = json_string_cost(&content.name) + 1;
                text + structured
            })
        });

        let remaining = MAX_CATALOG_LISTING_BYTES
            .saturating_sub(fixed_cost)
            .saturating_sub(metrics_omission_reserve)
            .saturating_sub(models_omission_reserve);

        // `metric` names exactly one entry to read back, not a listing - it is emitted whole rather
        // than run through the shared budget, matching how an unfiltered listing's omission note
        // tells a caller to come back with precisely this argument.
        let (metrics, omitted_metrics, omitted_metrics_more, metrics_spent) = if metric.is_some() {
            let spent = selected_metrics.iter().map(metric_cost).sum();
            (selected_metrics, Vec::new(), 0, spent)
        } else {
            let (included, omitted_names, omitted_count) =
                fill_to_budget(selected_metrics, remaining, MAX_OMITTED_NAMES, metric_cost, |content| {
                    OmittedMetric {
                        metric: content.name.clone(),
                        model: content.model.clone(),
                    }
                });
            let spent = included.iter().map(metric_cost).sum();
            let more = omitted_count.saturating_sub(omitted_names.len());
            (included, omitted_names, more, spent)
        };

        let (models, omitted_models, omitted_models_more) = model_contents.map_or_else(
            || (None, Vec::new(), 0),
            |contents| {
                let model_budget = remaining.saturating_sub(metrics_spent);
                let (included, omitted_names, omitted_count) = fill_to_budget(
                    contents,
                    model_budget,
                    MAX_OMITTED_NAMES,
                    |content| serde_json::to_vec(content).map_or(0, |bytes| bytes.len()) + model_text(content).len() + 2,
                    |content| content.name.clone(),
                );
                let more = omitted_count.saturating_sub(omitted_names.len());
                (Some(included), omitted_names, more)
            },
        );
        Self {
            provenance,
            catalog_prose: prose.as_str(),
            notice,
            metrics,
            omitted_metrics,
            omitted_metrics_more,
            models,
            omitted_models,
            omitted_models_more,
            knowledge,
            instructions,
        }
    }

    /// The same listing as text, for the content block beside the structured one.
    ///
    /// Both are sent for the reason
    /// [`OutcomeContent::as_text`](crate::wire::OutcomeContent::as_text) gives: a client that
    /// renders only content blocks - which is most of what a person actually looks at - would
    /// otherwise be shown nothing.
    ///
    /// One metric per line, then its grains and its dimensions, because a model reads that back
    /// without being told how. The metrics and the knowledge are bounded at load: the bundle by the
    /// catalog's own parses, and [`Self::knowledge`] additionally by
    /// `sutura_domain::knowledge::MAX_KNOWLEDGE_BYTES`. The CLI composition root reads the operator's
    /// instructions under `prompt.instructions_max_bytes` at startup. This wire type does not check
    /// that limit itself; a direct caller can still construct it with longer text.
    pub(crate) fn as_text(&self) -> String {
        let mut out = String::from(self.notice);
        out.push('\n');
        for metric in &self.metrics {
            out.push_str(&metric_text(metric));
        }
        if !self.omitted_metrics.is_empty() {
            out.push_str("\n\nomitted metrics (over this result's shared ");
            out.push_str(&MAX_CATALOG_LISTING_BYTES.to_string());
            out.push_str("-byte limit, each named with its own model): ");
            let names: Vec<String> = self
                .omitted_metrics
                .iter()
                .map(|omitted| format!("{} (model {})", omitted.metric, omitted.model))
                .collect();
            out.push_str(&names.join(", "));
            if self.omitted_metrics_more > 0 {
                out.push_str(" (+");
                out.push_str(&self.omitted_metrics_more.to_string());
                out.push_str(" more not named)");
            }
            out.push_str(
                "\ncall describe_catalog(metric: <name>) with one of the names above, or describe_catalog(model: <model>), to read it.",
            );
        }
        if let Some(models) = &self.models {
            out.push_str("\n\nPhysical schema (descriptive only; no metric certified):");
            if models.is_empty() {
                out.push_str("\n  no visible models");
            }
            for model in models {
                out.push_str(&model_text(model));
            }
            if !self.omitted_models.is_empty() {
                out.push_str("\n\nomitted models (over this result's shared ");
                out.push_str(&MAX_CATALOG_LISTING_BYTES.to_string());
                out.push_str("-byte limit): ");
                out.push_str(&self.omitted_models.join(", "));
                if self.omitted_models_more > 0 {
                    out.push_str(" (+");
                    out.push_str(&self.omitted_models_more.to_string());
                    out.push_str(" more not named)");
                }
                out.push_str("\ncall describe_catalog(model: <name>) with one of the names above to read that model.");
            }
        }
        if !self.knowledge.is_empty() {
            out.push_str("\n\n");
            out.push_str(&self.knowledge);
        }
        // The operator's instructions come AFTER the knowledge and the notice, in their own section
        // with their own heading - outside the "ignore catalog prose" notice's scope, exactly as the
        // knowledge sections above are the catalog prose that notice governs.
        if let Some(instructions) = &self.instructions {
            out.push_str("\n\n");
            out.push_str(instructions);
        }
        out.push_str("\ndefinitions: ");
        out.push_str(&self.provenance.definition_version);
        out.push_str(" (digest ");
        out.push_str(&self.provenance.definition_digest);
        out.push(')');
        out
    }
}

mod names_index;
pub use names_index::NamesIndexContent;

/// `s`'s cost as a JSON string VALUE - the quotes and any escaping included - rather than its raw
/// byte length, which undercounts whenever a value needs escaping (a quote, a backslash, a control
/// character).
fn json_string_cost(s: &str) -> usize {
    serde_json::to_string(s).map_or_else(|_| s.len() + 2, |encoded| encoded.len())
}

/// One metric's text block, exactly as [`CatalogContent::as_text`] renders it - factored out so the
/// byte cost `fill_to_budget` weighs a metric by is the same text this produces, not an estimate of
/// it.
fn metric_text(metric: &MetricContent) -> String {
    let mut out = String::from("\n");
    out.push_str(&metric.name);
    out.push_str("\n  model: ");
    out.push_str(&metric.model);
    if let Some(description) = metric.description.words() {
        push_prose(&mut out, description, "  description:");
    }
    out.push_str("\n  grains: ");
    out.push_str(&metric.grains.join(", "));
    for dimension in &metric.dimensions {
        out.push_str("\n  dimension ");
        out.push_str(&dimension.name);
        out.push_str(" (");
        out.push_str(if dimension.filterable {
            "groupable, filterable"
        } else {
            "groupable"
        });
        match dimension.allowed_values {
            Some(ref values) => {
                out.push_str(", values: ");
                out.push_str(&values.join(", "));
            }
            None => out.push_str(", any value"),
        }
        out.push(')');
        if let Some(description) = dimension.description.words() {
            push_prose(&mut out, description, "    description:");
        }
    }
    out
}

/// One model's text block, exactly as [`CatalogContent::as_text`] renders it - the same reason
/// [`metric_text`] is factored out.
fn model_text(model: &ModelContent) -> String {
    let mut out = String::from("\n\nmodel ");
    out.push_str(&model.name);
    out.push_str(" (table ");
    out.push_str(&model.table);
    out.push_str(", source ");
    out.push_str(&model.source);
    out.push(')');
    if let Some(description) = model.description.words() {
        push_prose(&mut out, description, "  description:");
    }
    for column in &model.columns {
        out.push_str("\n  column ");
        out.push_str(&column.name);
        if let Some(description) = column.description.words() {
            push_prose(&mut out, description, "    description:");
        }
    }
    for relationship in &model.relationships {
        out.push_str("\n  relationship ");
        out.push_str(&relationship.name);
        out.push_str(": ");
        out.push_str(&relationship.from);
        out.push_str(" -> ");
        out.push_str(&relationship.to);
        out.push_str(" (");
        out.push_str(relationship.cardinality);
        out.push(')');
        for key in &relationship.keys {
            out.push_str("\n    key ");
            out.push_str(&key.from);
            out.push_str(" = ");
            out.push_str(&key.to);
            if let Some(grain) = &key.grain {
                out.push_str(" (origin truncated to ");
                out.push_str(grain);
                out.push(')');
            }
        }
        if let Some(comparison) = relationship.comparison {
            out.push_str("\n    comparison as ");
            out.push_str(comparison);
        }
    }
    out
}

/// Appends a heading followed by `heading`'s prose, each line quoted with `> `.
///
/// A quoted line cannot start a line the encoder did not write, which is the one property this whole
/// function exists to provide: an embedded `\ndefinitions:` in a description is content inside the
/// block, not a trailer the tool produced.
fn push_prose(out: &mut String, prose: &str, heading: &str) {
    let trimmed = prose.trim();
    if trimmed.is_empty() {
        return;
    }
    out.push('\n');
    out.push_str(heading);
    out.push('\n');
    for line in trimmed.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            out.push('>');
        } else {
            out.push_str("> ");
            out.push_str(line);
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use sutura_app::catalog_budget::{MAX_CATALOG_LISTING_BYTES, MAX_OMITTED_NAMES};
    use sutura_app::prompt::CatalogProse;
    use sutura_domain::pinned::view::ScopedView;

    use super::CatalogContent;

    /// Note bodies - the glossary, caveat, worked-example and undefined-term prose - follow the same
    /// `prompt.catalog_prose` setting the metric and dimension descriptions do, on both halves of
    /// one result.
    ///
    /// The omission cells over descriptions read a bundle that holds no note, so a
    /// `describe_catalog` that quoted every note body under `omitted` stayed green until this cell.
    /// Both settings are read here so the omission is a control and not an empty fixture.
    #[test]
    fn note_bodies_reach_the_mcp_catalog_only_under_the_quoted_prose_setting() {
        // The glossary, caveat, undefined-term and worked-example bodies of the fixture's four notes.
        const BODIES: [&str; 4] = [
            "the finance-only meaning of capex",
            "only a finance-granted caller should trust this number",
            "CLV is deliberately undefined here",
            "ask exactly this",
        ];
        let bundle = crate::testing::bundle_with_restricted_metric_and_glossary();
        let view = ScopedView::everything(&bundle);

        for (prose, carried) in [(CatalogProse::Quoted, true), (CatalogProse::Omitted, false)] {
            let content = CatalogContent::of(&view, prose, None);
            let rendered = serde_json::to_value(&content).expect("the catalog serializes");
            let structured = rendered["knowledge"]
                .as_str()
                .expect("the catalog carries a knowledge section");
            let text = content.as_text();
            for half in [structured, text.as_str()] {
                for body in BODIES {
                    assert_eq!(
                        half.contains(body),
                        carried,
                        "under `{}` a note body must be carried: {carried}; {body}: {half}",
                        prose.as_str()
                    );
                }
            }
        }
    }

    /// `model` narrows the physical schema to that model's own columns, and the metrics to that
    /// model's own - never the whole catalog's, and never an empty listing for a model that is real
    /// and visible but owns no metric (`github.com/telekom/sutura#2071`'s second ask).
    #[test]
    fn describe_catalog_model_narrows_physical_schema_and_metrics_to_one_model() {
        use sutura_domain::model::ModelName;

        let bundle = crate::testing::bundle_with_two_models();
        let view = ScopedView::everything(&bundle);
        let orders = ModelName::parse("orders").expect("a test model is a model");
        let customers = ModelName::parse("customers").expect("a test model is a model");

        let for_orders =
            CatalogContent::of_with_physical_schema(&view, CatalogProse::Quoted, None, true, Some(&orders), None, None);
        let json = serde_json::to_value(&for_orders).expect("the catalog serializes");
        assert_eq!(json["models"].as_array().expect("models is a list").len(), 1);
        assert_eq!(json["models"][0]["name"], "orders");
        assert_eq!(json["metrics"][0]["name"], "revenue");
        assert_eq!(json["metrics"][0]["model"], "orders");
        assert!(for_orders.as_text().contains("model: orders"), "{}", for_orders.as_text());

        let for_customers =
            CatalogContent::of_with_physical_schema(&view, CatalogProse::Quoted, None, true, Some(&customers), None, None);
        let json = serde_json::to_value(&for_customers).expect("the catalog serializes");
        assert_eq!(json["models"].as_array().expect("models is a list").len(), 1);
        assert_eq!(json["models"][0]["name"], "customers");
        assert_eq!(
            json["metrics"].as_array().expect("metrics is a list").len(),
            0,
            "customers owns no metric: {json}"
        );
    }

    /// An unfiltered, physical-schema listing under [`sutura_app::catalog_budget::MAX_CATALOG_LISTING_BYTES`] carries
    /// every model and omits none - the bound's absence is the ordinary case, not a special one.
    #[test]
    fn an_under_bound_catalog_lists_every_model_with_nothing_omitted() {
        let bundle = crate::testing::bundle();
        let view = ScopedView::everything(&bundle);
        let content = CatalogContent::of_with_physical_schema(&view, CatalogProse::Quoted, None, true, None, None, None);
        let json = serde_json::to_value(&content).expect("the catalog serializes");
        assert_eq!(json["models"].as_array().expect("models is a list").len(), 1);
        assert!(json.get("omitted_models").is_none(), "{json}");
        assert!(!content.as_text().contains("omitted models"), "{}", content.as_text());
    }

    /// An unfiltered, physical-schema listing whose models exceed [`sutura_app::catalog_budget::MAX_CATALOG_LISTING_BYTES`]
    /// carries whole models up to the bound plus an explicit, machine-readable list of the rest -
    /// never a model truncated mid-way.
    #[test]
    fn an_over_bound_catalog_carries_whole_models_and_names_the_rest() {
        const MODEL_COUNT: usize = 20;
        let bundle = crate::testing::bundle_with_many_models(MODEL_COUNT);
        let view = ScopedView::everything(&bundle);
        let content = CatalogContent::of_with_physical_schema(&view, CatalogProse::Quoted, None, true, None, None, None);
        let json = serde_json::to_value(&content).expect("the catalog serializes");
        let included = json["models"].as_array().expect("models is a list");
        let omitted = json["omitted_models"].as_array().expect("some model was omitted");
        assert!(!included.is_empty(), "{json}");
        assert!(!omitted.is_empty(), "{json}");
        assert_eq!(included.len() + omitted.len(), MODEL_COUNT, "{json}");
        // Every included model is whole: its own description and column survived serialization, not
        // a partial object.
        for model in included {
            assert!(model["description"].is_string(), "{model}");
            assert!(!model["columns"].as_array().expect("columns is a list").is_empty(), "{model}");
        }
        let text = content.as_text();
        assert!(text.contains("omitted models"), "{text}");
        assert!(text.contains("describe_catalog(model: <name>)"), "{text}");
        for name in omitted {
            let name = name.as_str().expect("an omitted model's name is a string");
            assert!(text.contains(name), "{text}");
        }
    }

    /// The metric pool's own version of [`an_over_bound_catalog_carries_whole_models_and_names_the_rest`]:
    /// whole metrics up to the bound, the rest named - and named WITH their model, since a metric's
    /// name alone does not say where to narrow a follow-up `describe_catalog(model: ...)` call to
    /// (`github.com/telekom/sutura#2071`'s second ask).
    #[test]
    fn an_over_bound_catalog_carries_whole_metrics_and_names_the_rest_with_model() {
        const METRIC_COUNT: usize = 40;
        let bundle = crate::testing::bundle_with_many_metrics(METRIC_COUNT, 4000);
        let view = ScopedView::everything(&bundle);
        let content = CatalogContent::of(&view, CatalogProse::Quoted, None);
        let json = serde_json::to_value(&content).expect("the catalog serializes");
        let included = json["metrics"].as_array().expect("metrics is a list");
        let omitted = json["omitted_metrics"].as_array().expect("some metric was omitted");
        assert!(!included.is_empty(), "{json}");
        assert!(!omitted.is_empty(), "every metric fit - make the fixture bigger: {json}");
        assert_eq!(included.len() + omitted.len(), METRIC_COUNT, "{json}");
        // Under the 200-name omission cap, so the count field stays at zero and unserialized.
        assert!(json.get("omitted_metrics_more").is_none(), "{json}");
        for entry in omitted {
            assert_eq!(
                entry["model"], "orders",
                "an omitted metric's structured entry must carry its model: {entry}"
            );
        }
        let text = content.as_text();
        for entry in omitted {
            let metric = entry["metric"]
                .as_str()
                .expect("an omitted metric's structured entry names it");
            assert!(text.contains(metric), "{text}");
        }
    }

    /// Both halves this type renders - the text `as_text()` reaches over MCP's text content, and the
    /// structured JSON `serde_json::to_value` reaches over MCP's structured content - stay under the
    /// shared bound even once their metrics cross it, because each half's own omission note is
    /// reserved for up front rather than charged after the fact. `fixed_cost` and the omission
    /// reserve are both doubled for exactly this reason - see `of_with_physical_schema`'s own doc.
    #[test]
    fn an_over_bound_catalog_stays_under_the_shared_bound_for_both_its_text_and_structured_halves() {
        const METRIC_COUNT: usize = 40;
        let bundle = crate::testing::bundle_with_many_metrics(METRIC_COUNT, 4000);
        let view = ScopedView::everything(&bundle);
        let content = CatalogContent::of(&view, CatalogProse::Quoted, None);
        let json = serde_json::to_value(&content).expect("the catalog serializes");
        let omitted = json["omitted_metrics"].as_array().expect("some metric was omitted");
        assert!(!omitted.is_empty(), "every metric fit - make the fixture bigger: {json}");

        let structured_bytes = serde_json::to_vec(&content).expect("the catalog serializes").len();
        assert!(
            structured_bytes <= MAX_CATALOG_LISTING_BYTES,
            "the structured half overshot the shared bound ({structured_bytes} bytes)"
        );
        let text_bytes = content.as_text().len();
        assert!(
            text_bytes <= MAX_CATALOG_LISTING_BYTES,
            "the text half overshot the shared bound ({text_bytes} bytes)"
        );
    }

    /// The model pool's own version of the halves check above - models, not metrics, cross the
    /// bound this time.
    #[test]
    fn an_over_bound_physical_schema_stays_under_the_shared_bound_for_both_its_text_and_structured_halves() {
        const MODEL_COUNT: usize = 20;
        let bundle = crate::testing::bundle_with_many_models(MODEL_COUNT);
        let view = ScopedView::everything(&bundle);
        let content = CatalogContent::of_with_physical_schema(&view, CatalogProse::Quoted, None, true, None, None, None);
        let json = serde_json::to_value(&content).expect("the catalog serializes");
        let omitted = json["omitted_models"].as_array().expect("some model was omitted");
        assert!(!omitted.is_empty(), "every model fit - make the fixture bigger: {json}");

        let structured_bytes = serde_json::to_vec(&content).expect("the catalog serializes").len();
        assert!(
            structured_bytes <= MAX_CATALOG_LISTING_BYTES,
            "the structured half overshot the shared bound ({structured_bytes} bytes)"
        );
        let text_bytes = content.as_text().len();
        assert!(
            text_bytes <= MAX_CATALOG_LISTING_BYTES,
            "the text half overshot the shared bound ({text_bytes} bytes)"
        );
    }

    /// Enough metrics that the number omitted crosses
    /// [`sutura_app::catalog_budget::MAX_OMITTED_NAMES`]'s own cap on the omission list itself, so
    /// the true count must appear as its own field rather than only being inferable by counting
    /// names that are no longer all there.
    #[test]
    fn an_over_bound_catalog_with_more_than_the_omission_cap_carries_a_separate_count() {
        const METRIC_COUNT: usize = 400;
        let bundle = crate::testing::bundle_with_many_metrics(METRIC_COUNT, 1000);
        let view = ScopedView::everything(&bundle);
        let content = CatalogContent::of(&view, CatalogProse::Quoted, None);
        let json = serde_json::to_value(&content).expect("the catalog serializes");
        let included = json["metrics"].as_array().expect("metrics is a list").len();
        let omitted = json["omitted_metrics"].as_array().expect("some metric was omitted");
        let truly_omitted = METRIC_COUNT - included;
        assert!(
            truly_omitted > MAX_OMITTED_NAMES,
            "make the fixture bigger - the omission list's own cap was never crossed: {included} included, {truly_omitted} omitted"
        );
        assert_eq!(
            omitted.len(),
            MAX_OMITTED_NAMES,
            "the omission list itself must stay capped: {json}"
        );
        let more = json["omitted_metrics_more"]
            .as_u64()
            .and_then(|more| usize::try_from(more).ok())
            .expect("the true omission count must be its own field once the name list is capped");
        assert_eq!(
            more,
            truly_omitted - MAX_OMITTED_NAMES,
            "the count field must carry what the name list could not: {json}"
        );
    }
}

#[cfg(test)]
mod relationships;

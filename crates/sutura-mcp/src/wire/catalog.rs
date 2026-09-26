//! The catalog tool's wire shape: what `describe_catalog` takes, what it answers, and the text half
//! of that answer.
//!
//! **Its own module because `wire.rs` was twenty-one lines under the thousand-line limit
//! `cargo xtask max-lines` enforces and cannot exempt**, at the banner that file already carried -
//! and the seam is one whole TOOL rather than a share of lines. `wire.rs` keeps the question and the
//! answer of `ask`; everything here belongs to the other tool a caller can reach, and the two share
//! nothing but `prose`, which is where the `Option` behind a description stays private.
//!
//! Why the arguments type has braces and no fields, and why the reasoning about a wire type is a
//! plain comment rather than a doc comment, are both stated at `DescribeCatalogArgs` - `schemars`
//! puts a root doc comment into the schema's `description`, which is text a MODEL reads.

#![expect(
    clippy::empty_enums,
    reason = "serde::Deserialize lowers the brace-form unit struct DescribeCatalogArgs to an empty \
              internal enum; that enum is a deserialization artifact of the derive, never constructed \
              by this crate. It cannot be suppressed on the struct itself (a derive artifact sits \
              outside the item's attributes), so it is scoped to the one module that derives it"
)]

use sutura_app::prompt::CatalogProse;
use sutura_domain::model::Grain;
use sutura_domain::pinned::view::ScopedView;

use super::prose::{self, Carried};
use super::{ProvenanceContent, bundle_content};

// **A type with no fields rather than no type at all**, and the reason is `deny_unknown_fields`.
// `schemars` reads the same attribute serde does, so the advertised schema says
// `additionalProperties: false` and an arguments object carrying `metric`, `sql` or anything else is
// a named parse error rather than a key dropped on the floor. A tool that accepted any object would
// be a tool whose surface a caller could guess at, and a caller that sent `metric` would believe it
// had narrowed a listing it did not.
//
// It is also what keeps this half of the surface under the same drift guard as the other: the
// generated schema is snapshotted in `crate::tool`, so a field added here lands in a reviewer's diff.
//
// **A plain comment and not a doc comment, deliberately.** `schemars` puts a root doc comment into
// the schema's `description`, which is text a MODEL reads before it calls the tool - so the doc
// comment on a wire type is caller-facing prose and the reasoning about the type goes here. The one
// below is written for that reader.
/// This tool takes no arguments. It returns the catalog of what this deployment measures, narrowed
/// to what the calling principal may see - the metrics, grains, dimensions and permitted values.
///
/// It also returns the catalog's own knowledge (the glossary, caveats, worked examples and terms
/// recorded as undefined it carries) and the deployment operator's instructions. There is nothing to
/// filter or select, so send an empty object.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "the braces are the behaviour, not the formatting: a UNIT struct's Deserialize accepts \
              `null` and REFUSES `{}` - measured, `invalid type: map, expected unit struct` - and `{}` \
              is what a client with no arguments sends. The lint's suggestion breaks the tool"
)]
pub struct DescribeCatalogArgs {}

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
/// that both are built from the one `sutura_domain::pinned::PinnedDefinitions` accessor set.
/// **The whole type is WIDER than `CatalogBody`**: it also carries the knowledge sections and the
/// operator's instructions, which the HTTP `/v1/catalog` surface has no equivalent of - that surface
/// is the structured half alone, rendered by a different reader.
///
/// **What narrows this listing is the CALLER's identity - `docs/adr/0028` - and nothing the caller
/// SENDS.** `sutura_domain::pinned::SemanticCatalog::load` takes no request context and cannot be
/// given one, so no argument selects, widens or parameterizes what this returns: the caller's mapped
/// audiences (which `describe` reads and this constructor takes as a [`ScopedView`]) decide which
/// metrics and which of their knowledge the listing holds, while the bundle underneath is the same
/// one every answer is computed from. Invisible means absent, and it is the transport's job to build
/// the view, never this type's.
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
    metrics: Vec<MetricContent>,
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
    /// **Unbounded, unlike [`Self::knowledge`]: there is no byte cap on the operator's own text**, and
    /// it is read from `prompt.instructions_file` fresh and repeated in full on every call.
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
}

/// One metric, as much of it as a caller needs to ask a valid question.
#[derive(Debug, serde::Serialize)]
pub struct MetricContent {
    name: String,
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
        let metrics = view
            .metrics()
            .map(|metric| MetricContent {
                name: String::from(metric.name().as_str()),
                description: Carried::under(prose, metric.description()),
                grains: {
                    let mut grains: Vec<Grain> = metric.grains().iter().copied().collect();
                    // Reversed, because `Grain`'s own ordering runs fine to coarse and a reader wants
                    // the coarsest first - the same order the HTTP surface renders.
                    grains.sort_unstable_by(|left, right| right.cmp(left));
                    grains.into_iter().map(|grain| String::from(grain.as_str())).collect()
                },
                dimensions: metric
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
        // The knowledge sections ride the same caller's view as the metrics: a caller that may not
        // see a metric does not see that metric's glossary entry, caveat or worked example either.
        // `catalog_knowledge` quotes note bodies under the same `prose` the descriptions follow, so
        // a deployment that withholds catalog prose withholds it from the knowledge tool too.
        let knowledge = sutura_app::prompt::catalog_knowledge(view, prose);
        // The operator's own text is DEPLOYMENT-WIDE - the same for every caller - so it gets its
        // own section and heading, outside the untrusted-catalog notice, and a preamble written for
        // a tool that returns no refusal rules. Rendered here rather than by `catalog_knowledge`
        // precisely so it does not sit under the notice that says to ignore catalog prose.
        let instructions = instructions.and_then(|raw| {
            let text = raw.trim();
            (!text.is_empty()).then(|| sutura_app::prompt::tool_operator_instructions(text))
        });
        Self {
            provenance: bundle_content(view.pinned()),
            catalog_prose: prose.as_str(),
            notice: prose::notice(prose),
            metrics,
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
            out.push('\n');
            out.push_str(&metric.name);
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

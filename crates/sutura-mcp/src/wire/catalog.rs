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
/// to what the calling principal may see: there is nothing to filter or select, so send an empty
/// object.
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
/// **A second wire type beside `sutura_http::wire::CatalogBody`, with the same fields, and that is
/// the same deliberate cost [`super::AskArgs`] already pays.** An adapter never calls another adapter, so
/// this crate cannot import that shape; what keeps the two equal is review plus the fact that both
/// are built from the one `sutura_domain::pinned::PinnedDefinitions` accessor set, which is where a
/// missing field would show up as a missing call rather than as a silent divergence.
///
/// **What narrows this listing is the CALLER's identity - `docs/adr/0028` - and nothing the caller
/// SENDS.** `sutura_domain::pinned::SemanticCatalog::load` takes no request context and cannot be
/// given one, so no argument selects, widens or parameterizes what this returns: the caller's mapped
/// audiences (which `describe` reads and this constructor takes as a [`ScopedView`]) decide which
/// metrics the listing holds, while the bundle underneath is the same one every answer is computed
/// from. Invisible means absent, and it is the transport's job to build the view, never this type's.
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
    // `github.com/telekom/sutura#971`: the same document `initialize.instructions` carries,
    // reachable through a TOOL rather than only through the handshake result - a gateway that
    // surfaces tools and drops everything else in `initialize` still lets a caller read the
    // knowledge sections (glossary, recorded absences, worked examples) and the operator's own
    // instructions, in `as_text` below. `skip`, matching `notice` above and for the identical
    // reason: this is prose for the TEXT content block a reader looks at, and putting it in the
    // structured half too would put the word "description" into `structured_content` even under
    // `catalog_prose: omitted` (its own OWN omission notice says the word "descriptions") - which
    // is exactly the leak shape `#266`'s H1 regression tests hold `structured_content` to having
    // none of. Descriptive prose only - no data row has ever been in this document - bounded the
    // same way it always was (`sutura_domain::knowledge::MAX_KNOWLEDGE_BYTES` at load) and quoted
    // under `catalog_prose` already, because it is `render`'s own output and `render` already
    // applies that setting to every description it touches.
    //
    // **The limit stated where the claim is, not smoothed over:** this is the SAME text every
    // verified caller's `initialize.instructions` already carries, rendered once over the WHOLE
    // bundle rather than through THIS door's `ScopedView` - `describe`'s own doc comment names
    // that gap already (an audience-restricted metric's name can appear in it), and reaching it
    // through a tool call does not widen or narrow that gap; it is the same content, one more way
    // to read it.
    #[serde(skip)]
    agent_instructions: String,
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
    /// this builder cannot reach `SemanticCatalog::load` - a per-caller filter stays off the
    /// request path as a property of the type, never a call the renderer happens to omit.
    ///
    /// It also asks nothing of the setting itself: [`Carried::under`] and `prose::notice` are the
    /// crate's only two readers of it, so this builder cannot fill a `description` or pick a notice
    /// without the operator's decision, and a third `CatalogProse` spelling is a compile error in
    /// both rather than an `else` arm here.
    ///
    /// `agent_instructions` is already-rendered text - the same document `initialize.instructions`
    /// carries - rather than something this builder derives: `sutura-mcp` never reads settings, so
    /// the operator's own section could not be assembled here even given the bundle, and the
    /// composition root is where the one rendering happens for both surfaces.
    #[must_use]
    pub fn of(view: &ScopedView<'_>, prose: CatalogProse, agent_instructions: &str) -> Self {
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
        Self {
            provenance: bundle_content(view.pinned()),
            catalog_prose: prose.as_str(),
            notice: prose::notice(prose),
            metrics,
            agent_instructions: String::from(agent_instructions),
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
    /// without being told how. Nothing here is truncated: the bundle is bounded at load by
    /// `sutura_domain::knowledge::MAX_KNOWLEDGE_BYTES` and by the catalog's own parses, and a listing
    /// that grew past what a context tolerates is a bundle nobody could ask about either way.
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
        out.push_str("\ndefinitions: ");
        out.push_str(&self.provenance.definition_version);
        out.push_str(" (digest ");
        out.push_str(&self.provenance.definition_digest);
        out.push(')');
        if !self.agent_instructions.is_empty() {
            out.push_str("\n\n");
            out.push_str(&self.agent_instructions);
        }
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

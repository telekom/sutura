//! What this catalog defines, on the wire.
//!
//! Split out of `wire.rs` when that file crossed the thousand-line gate `cargo xtask max-lines`
//! enforces and cannot exempt under `crates/`. Concept-named rather than `part2`: everything here
//! is the catalog-reading body, and `wire.rs` keeps the question and the answer.

use sutura_domain::model::Grain;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::pinned::view::ScopedView;

use super::ProvenanceBody;

/// The same two fields, read straight off a bundle nothing executed against.
///
/// The catalog endpoint describes a bundle rather than answering a question, so there is no
/// `Provenance` to build for it: `PinnedDefinitions::provenance` requires the execution record, and
/// inventing an empty one there would be a claim that a leg ran and produced nothing.
fn bundle_body(pinned: &PinnedDefinitions) -> ProvenanceBody {
    ProvenanceBody {
        definition_version: String::from(pinned.version().as_str()),
        definition_digest: String::from(pinned.digest().as_str()),
    }
}

/// What this catalog defines.
///
/// # Why a structured surface reads the prose setting at all
///
/// `prompt.catalog_prose: omitted` is not a mitigation for the forgery `docs/adr/0022` is about -
/// `serde` owns the field boundary here, so a description cannot cross one whatever it spells, and
/// this body escapes nothing. It is a decision about **who may put words in front of an agent**: an
/// operator whose catalog authors are not the people who decide what their agents are told drops the
/// prose, and a description that still reached an agent through a second transport would make that
/// setting a statement about one surface rather than about the deployment. So the omission is
/// honoured wherever the prose is carried, and the escaping stays where the delimiter is.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CatalogBody {
    provenance: ProvenanceBody,
    /// Which way the operator's `prompt.catalog_prose` setting points, so an absent description is a
    /// fact a client can read rather than one it has to infer.
    ///
    /// The operator's own spelling, echoed rather than translated, which is what keeps this body and
    /// the settings file from acquiring two vocabularies for one decision - the value is
    /// `sutura_config::CatalogProse::as_str`, so a third spelling cannot appear here without
    /// appearing in the settings parser too. It reads `quoted` on a surface that quotes nothing
    /// because the word names the SETTING and not this renderer.
    catalog_prose: &'static str,
    metrics: Vec<MetricBody>,
}

/// One metric, as much of it as a caller needs to ask a valid question.
///
/// Descriptive content only. Nothing here selects, widens or parameterizes what executes - the
/// catalog port takes no request context and cannot - so this is a reader's view of a pinned
/// bundle rather than an input to one.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct MetricBody {
    name: String,
    /// The author's own prose, or absent where the operator omitted it.
    ///
    /// `Option` rather than an empty string, because *this deployment ships no catalog prose* and
    /// *this metric's description is empty* are different facts, and a client rendering the second
    /// for the first would report an operator's decision as a catalog defect. `catalog_prose` on the
    /// body is what says which an absence is.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    /// Coarsest first, which is the order an anchor is checked at.
    grains: Vec<String>,
    dimensions: Vec<DimensionBody>,
}

/// One dimension of one metric.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct DimensionBody {
    name: String,
    /// The author's own prose, or absent where the operator omitted it. See [`MetricBody`].
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    /// Whether this dimension can be filtered on as well as grouped by.
    filterable: bool,
    /// The values a filter may use, when the catalog declares a set. Absent means the dimension is
    /// groupable and not filterable.
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_values: Option<Vec<String>>,
}

impl CatalogBody {
    /// The reader's view of a caller-scoped catalog, under the prose setting this deployment was
    /// started with.
    ///
    /// **Takes a [`ScopedView`], never a bare `&PinnedDefinitions`** - `docs/adr/0028`. A metric
    /// outside the view is not in `metrics` below, so advertisement and invocation cannot disagree
    /// about which metrics exist; the provenance still names the whole bundle's version and digest,
    /// because that is what `docs/adr/0028` says the digest continues to identify.
    ///
    /// **A named constructor rather than a `From`, and the argument is the reason.**
    /// `CatalogProse::default()` is `Quoted`, so a conversion reachable without the setting fails
    /// OPEN: it ships the prose of a deployment that asked for none, which is the defect this
    /// function exists to close. A second argument cannot be left out.
    #[must_use]
    pub fn of(view: &ScopedView<'_>, prose: sutura_config::CatalogProse) -> Self {
        // Exhaustive rather than `is_quoted()` in an `if`, which is what this line was: a question
        // asked of one variant reads every future spelling as the `else`, and on this setting the
        // `else` withholds prose nobody asked to withhold. `sutura-mcp`'s twin makes the same
        // decision unrepresentable with a field type; this surface has one carrier and no text half,
        // so the match is where the whole of it fits.
        let quoted = match prose {
            sutura_config::CatalogProse::Quoted => true,
            sutura_config::CatalogProse::Omitted => false,
        };
        let metrics = view
            .metrics()
            .map(|metric| MetricBody {
                name: String::from(metric.name().as_str()),
                description: quoted.then(|| String::from(metric.description())),
                grains: {
                    let mut grains: Vec<Grain> = metric.grains().iter().copied().collect();
                    grains.sort_unstable_by(|a, b| b.cmp(a));
                    grains.into_iter().map(|grain| String::from(grain.as_str())).collect()
                },
                dimensions: metric
                    .dimensions()
                    .values()
                    .map(|dimension| DimensionBody {
                        name: String::from(dimension.name().as_str()),
                        description: quoted.then(|| String::from(dimension.description())),
                        filterable: dimension.is_filterable(),
                        allowed_values: dimension
                            .allowed_values()
                            .map(|values| values.iter().map(|value| String::from(value.as_str())).collect()),
                    })
                    .collect(),
            })
            .collect();
        Self {
            provenance: bundle_body(view.pinned()),
            catalog_prose: prose.as_str(),
            metrics,
        }
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::pinned::view::ScopedView;

    use super::CatalogBody;

    #[test]
    fn the_catalog_view_lists_grains_coarsest_first_and_carries_the_digest() {
        let bundle = crate::testing::bundle();
        let body = CatalogBody::of(&ScopedView::everything(&bundle), sutura_config::CatalogProse::Quoted);
        let rendered = serde_json::to_string(&body).expect("the catalog serializes");
        assert!(rendered.contains(r#""definition_version":"test-1""#), "{rendered}");
        // Month before day: the coarsest grain is the one an anchor is checked at, so listing it
        // first is the order a reader needs.
        let month = rendered.find("month").expect("month is listed");
        let day = rendered.find(r#""day""#).expect("day is listed");
        assert!(month < day, "{rendered}");
        // A filterable dimension advertises its allowlist, which is what makes a valid filter
        // writable without guessing.
        assert!(rendered.contains(r#""filterable":true"#), "{rendered}");
        assert!(rendered.contains("north"), "{rendered}");
        // And the default setting really carries the prose, so the omission test below is a
        // difference rather than a fixture with nothing in it.
        assert!(rendered.contains("Revenue, in minor units."), "{rendered}");
        assert!(rendered.contains(r#""catalog_prose":"quoted""#), "{rendered}");
    }

    /// `prompt.catalog_prose: omitted` is honoured on the HTTP surface too.
    ///
    /// `#128` asked for `catalog_prose_omitted_omits_it_from_every_surface` and got it on the agent
    /// tool alone: this body carried every description whatever the operator had written down, so a
    /// deployment that had dropped its catalog prose from the prompt and from the MCP tool still
    /// served it here. The setting is not about the delimiter - `serde` owns the boundary on this
    /// surface and no cell or description can cross it - it is about whether a catalog author's
    /// words reach an agent at all, which is a property of the DEPLOYMENT and cannot be true of one
    /// transport and false of another.
    ///
    /// What must survive the omission is everything a caller needs in order to ask a valid question:
    /// the names, the grains, the dimensions and the allowlist. A caller that cannot form a question
    /// gets refusals instead of prose, which is a worse answer to the same worry.
    #[test]
    fn catalog_prose_omitted_omits_it_from_the_http_body() {
        let bundle = crate::testing::bundle();
        let omitted = serde_json::to_value(CatalogBody::of(
            &ScopedView::everything(&bundle),
            sutura_config::CatalogProse::Omitted,
        ))
        .expect("the catalog serializes");
        let rendered = omitted.to_string();
        // No description reaches the caller, on the metric or on the dimension.
        assert!(!rendered.contains("Revenue, in minor units."), "{rendered}");
        assert!(!rendered.contains("Sales region"), "{rendered}");
        assert!(!rendered.contains("description"), "{rendered}");
        // The omission is stated rather than left to be inferred from an absent field, so a client
        // can tell "this deployment ships no prose" from "this catalog has none".
        assert_eq!(omitted["catalog_prose"], "omitted");
        // And the structure a valid question needs is untouched.
        assert_eq!(omitted["metrics"][0]["name"], "revenue");
        assert_eq!(omitted["metrics"][0]["grains"][0], "month");
        assert_eq!(omitted["metrics"][0]["dimensions"][0]["name"], "region");
        assert_eq!(omitted["metrics"][0]["dimensions"][0]["allowed_values"][0], "north");
        assert_eq!(omitted["provenance"]["definition_version"], "test-1");
    }

    /// Every hostile description in the shared corpus stays one opaque JSON string here.
    ///
    /// The structured half of `#128`'s item 2, and the twin of
    /// `the_injection_corpus_cells_stay_single_opaque_json_strings_on_http`: the same corpus
    /// `sutura-app` and `sutura-mcp` walk, asserting the property THIS encoder provides rather than
    /// the one a text surface has to build. A description containing a heading, a fence or a fake
    /// `definitions:` trailer survives as exactly itself inside one field, and the document still
    /// parses - so nothing here needs escaping and nothing here does any.
    ///
    /// The corpus goes through a real `Description`, so what is proved is about prose a catalog can
    /// actually hold; and it is run under BOTH settings, because the interesting failure is a
    /// hostile description that the omission was believed to have dropped.
    #[test]
    fn the_injection_corpus_prose_stays_a_single_opaque_json_string_on_http() {
        for prose in sutura_app::untrusted::PROSE {
            let bundle = crate::testing::described_bundle(prose);
            let quoted = serde_json::to_value(CatalogBody::of(
                &ScopedView::everything(&bundle),
                sutura_config::CatalogProse::Quoted,
            ))
            .expect("the catalog serializes");
            let seen = quoted["metrics"][0]["description"]
                .as_str()
                .expect("the description survives as one string field");
            assert_eq!(seen, *prose, "a corpus description did not survive the field boundary");
            let omitted = serde_json::to_value(CatalogBody::of(
                &ScopedView::everything(&bundle),
                sutura_config::CatalogProse::Omitted,
            ))
            .expect("the catalog serializes");
            assert!(
                omitted["metrics"][0]["description"].is_null(),
                "a corpus description survived an omission: {omitted}"
            );
        }
    }
}

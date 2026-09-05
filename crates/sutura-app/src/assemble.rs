//! The metadata assembler: N catalog contributions become one pinned bundle.
//!
//! This is the *"metadata sources compose"* half of `docs/adr/0011` and of #115's step 4.
//! [`sutura_domain::pinned::SemanticCatalog::load`] reads one source; a deployment may declare
//! several. The assembler is where they stop being several:
//!
//! - **Application code in this crate, not an adapter over adapters** - ADR 0011's three reasons
//!   are recorded on the [`assemble`] function, and this crate is the one that owns the two driving
//!   ports.
//! - **One source may provide a given kind for a given entity.** Two sources defining one metric is
//!   refused, always, and refused naming both sources - "guessing which wins is how a metric
//!   silently means something different after a configuration change", which is the exact sentence
//!   ADR 0011 refuses.
//! - **The declaration-fidelity check runs per contributor, not per bundle.** `checked_against`
//!   on the merged result would say nothing once two sources are merged - a narrow source's
//!   undeclared kind could be hidden by what another source produced. Each contributor is held to
//!   its own declaration against its own content.
//! - **The contribution manifest is built from the contributors' own records.** Each source
//!   [`PinnedDefinitions`] carries a one-entry manifest naming itself and its declared capabilities;
//!   the composed manifest is those entries, keyed by name, so the digest of the composed bundle
//!   covers the composition.
//!
//! One limit is stated here because it decides what the wave-one deployment looks like: a metric
//! may reference only a model its own source also declares, because each contribution arrives
//! already assembled. The cross-source-reference case - the literal "`DataHub`'s model, metrics
//! certified here" - lands with a raw-content port, which is a separate decision recorded in
//! `docs/adr/0011`.

use std::collections::BTreeMap;

use sutura_domain::capabilities::{MetadataCapabilities, UnfaithfulDeclaration};
use sutura_domain::catalog::{Definitions, InconsistentDefinitions, Model, Relationship};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{
    Absence, Caveat, Example, GlossaryEntry, InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput, NoteName,
    Phrase,
};
use sutura_domain::model::{MetricName, ModelName, RelationshipName, SourceName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

/// Why N contributions will not compose.
///
/// Every collision variant names both sources and the entity, which is ADR 0011's *"refuses the
/// load, naming both and the entity"* - a precedence rule nobody stated is a precedence rule nobody
/// reviewed, so the refusal is what carries the names.
#[derive(Debug, thiserror::Error)]
pub enum CompositionError {
    /// Nothing was contributed; a deployment serves at least one metadata source.
    #[error("no contributions were assembled - a deployment serves at least one metadata source")]
    Empty,
    /// A contribution's own manifest did not name exactly one source, so this bundle cannot say who
    /// contributed it. The `count` is what a reader needs: the manifest is supposed to be the
    /// per-source record, and a value that failed to be one has nothing to merge under.
    #[error("a contribution manifest named {count} sources, and a contribution is one source")]
    NotASingleContribution { count: usize },
    /// Two contributors certify different snapshots. A bundle is one version, and `docs/adr/0011`'s
    /// amendment records the decision: two sources certified at different times is the "answers that
    /// differ across a refresh boundary" shape, refused rather than papered over.
    #[error("two contributors certify different snapshots: {first} and {second}")]
    VersionMismatch {
        first: DefinitionVersion,
        second: DefinitionVersion,
    },
    /// The one interpretation has no precedence, declared or otherwise: two definitions of one
    /// number is the failure this system exists to prevent.
    #[error("a metric can be defined by one source: {metric} is defined by both {first} and {second}")]
    MetricCollision {
        metric: MetricName,
        first: SourceName,
        second: SourceName,
    },
    /// Any other element two sources supply: a model, a relationship, a glossary term, a caveat,
    /// an absence or a worked example. `kind` is the closed vocabulary the message renders and the
    /// other fields are typed. It is not a [`CompositionError::MetricCollision`] because the rule
    /// for metrics is the stronger one - no precedence at all - while other elements could in
    /// principle be titled, and neither is today.
    #[error("{first} and {second} both provide {kind} {element}, and exactly one source may provide it")]
    ElementCollision {
        kind: &'static str,
        element: String,
        first: SourceName,
        second: SourceName,
    },
    /// A contributor's declaration disagrees with its own content. Per contributor, not per bundle:
    /// on the merged result this check would say nothing once two sources are merged.
    #[error("{source} declares kinds its own content does not supply")]
    Unfaithful {
        source: SourceName,
        #[source]
        cause: UnfaithfulDeclaration,
    },
    /// The composed definitions do not hold together - most often a metric naming a model some
    /// contributor should have provided and none did, and the domain's own refusal names it.
    #[error("the composed definitions do not hold together")]
    Definitions {
        #[source]
        cause: InconsistentDefinitions,
    },
    /// The composed knowledge does not hold together against the composed definitions.
    #[error("the composed knowledge does not hold together")]
    Knowledge {
        #[source]
        cause: InconsistentKnowledge,
    },
    /// The composed content could not be pinned.
    #[error("the composed bundle could not be pinned")]
    Digest {
        #[source]
        cause: NotDigestible,
    },
}

/// One source's contribution to a composition, taken apart for merging.
///
/// `name` is the manifest key the source's own load stamped and `capabilities` the declaration it
/// recorded there; everything else is what it read. Owning the values rather than borrowing the
/// source bundle keeps one contributor a single value through every check below.
struct Contributor {
    name: SourceName,
    capabilities: MetadataCapabilities,
    definitions: Definitions,
    knowledge: Knowledge,
    models: Vec<Model>,
    relationships: Vec<Relationship>,
    glossary: Vec<GlossaryEntry>,
    caveats: Vec<Caveat>,
    absences: Vec<Absence>,
    examples: Vec<Example>,
}

/// Composes N contributions into one bundle, refusing a composition ADR 0011 says cannot exist.
///
/// **Application code, not an adapter over adapters - the shape ADR 0011 picked, for three
/// reasons.** The rules being decided here are DOMAIN rules, not one adapter's; `load` stays free
/// of a request context in either shape, so that property does not choose between them; and an
/// assembling *adapter* implements the port over N others and would eventually depend on every one
/// of them, which is the crate-graph shape this avoids.
///
/// # Errors
///
/// [`CompositionError`], for any of: empty input, a contribution whose own manifest names more than
/// one source, two contributions certifying different versions, two sources providing the same
/// element, a contributor whose content disagrees with its declaration, or definitions/knowledge
/// that do not assemble once merged.
pub fn assemble(bundles: Vec<PinnedDefinitions>) -> Result<PinnedDefinitions, CompositionError> {
    let Some(first) = bundles.first() else {
        return Err(CompositionError::Empty);
    };
    let version = first.version().clone();
    let mut contributions = Vec::with_capacity(bundles.len());

    for bundle in bundles {
        if bundle.version() != &version {
            return Err(CompositionError::VersionMismatch {
                first: version,
                second: bundle.version().clone(),
            });
        }
        // A contribution is one source: its own manifest named itself, once. Anything else is a
        // bundle that failed to say who produced it, and there is nothing to merge it under.
        let mut entries = bundle.manifest().entries().iter();
        let (name, record) = match (entries.next(), entries.next()) {
            (Some((name, record)), None) => (name.clone(), record),
            _ => {
                return Err(CompositionError::NotASingleContribution {
                    count: bundle.manifest().entries().len(),
                });
            }
        };
        contributions.push(Contributor {
            name,
            capabilities: record.capabilities().clone(),
            definitions: bundle.definitions().clone(),
            knowledge: bundle.knowledge().clone(),
            models: bundle.definitions().models().values().cloned().collect(),
            relationships: bundle.definitions().relationships().values().cloned().collect(),
            glossary: bundle.knowledge().glossary().values().cloned().collect(),
            caveats: bundle.knowledge().caveats().values().cloned().collect(),
            absences: bundle.knowledge().absences().values().cloned().collect(),
            examples: bundle.knowledge().examples().values().cloned().collect(),
        });
    }

    // Exactly one source may provide a given kind for a given entity. Checked here, with both
    // sources named, because `Definitions::assemble` and `Knowledge::assemble` can only refuse the
    // element - a silent winner across sources is the failure ADR 0011 exists to refuse.
    check_no_metric_collisions(&contributions)?;
    check_no_element_collisions(&contributions)?;

    // The declaration-fidelity check, per contributor and against its OWN content: once two
    // sources are merged, a check over the whole bundle could not see that one of them supplied a
    // kind it never declared.
    for contribution in &contributions {
        let produced = MetadataCapabilities::produced(&contribution.definitions, &contribution.knowledge);
        contribution
            .capabilities
            .checked_against(&produced)
            .map_err(|cause| CompositionError::Unfaithful {
                source: contribution.name.clone(),
                cause,
            })?;
    }

    // One new-content knowledge declaration, the union of what each contributor declared, so the
    // merged bundle may carry what any of them licensed. A note for a capability nobody declared is
    // still refused by `Knowledge::assemble` as `UndeclaredContent` - the per-bundle net under the
    // per-contributor check above.
    let knowledge_declares =
        KnowledgeCapabilities::of(contributions.iter().flat_map(|c| c.knowledge.declares().declared()).copied());

    let models = contributions.iter().flat_map(|c| c.models.iter().cloned()).collect();
    let relationships = contributions.iter().flat_map(|c| c.relationships.iter().cloned()).collect();
    let metrics = contributions
        .iter()
        .flat_map(|c| c.definitions.metrics().values().cloned())
        .collect();
    let definitions =
        Definitions::assemble(models, relationships, metrics).map_err(|cause| CompositionError::Definitions { cause })?;

    let knowledge = Knowledge::assemble(
        &definitions,
        KnowledgeInput::new(
            knowledge_declares,
            contributions.iter().flat_map(|c| c.glossary.iter().cloned()).collect(),
            contributions.iter().flat_map(|c| c.caveats.iter().cloned()).collect(),
            contributions.iter().flat_map(|c| c.absences.iter().cloned()).collect(),
            contributions.iter().flat_map(|c| c.examples.iter().cloned()).collect(),
        ),
    )
    .map_err(|cause| CompositionError::Knowledge { cause })?;

    // The composed manifest is the contributors' own records, keyed by name - collection order is
    // content order, which the digest relies on.
    let manifest = ContributionManifest::of(
        contributions
            .iter()
            .map(|c| (c.name.clone(), Contribution::of(c.capabilities.clone()))),
    );

    PinnedDefinitions::pin(version, definitions, knowledge, manifest).map_err(|cause| CompositionError::Digest { cause })
}

/// No two sources may define one metric; the one element with no precedence at all.
fn check_no_metric_collisions(contributions: &[Contributor]) -> Result<(), CompositionError> {
    let mut by_name: BTreeMap<&MetricName, &SourceName> = BTreeMap::new();
    for contribution in contributions {
        for metric in contribution.definitions.metrics().keys() {
            if let Some(first) = by_name.insert(metric, &contribution.name) {
                return Err(CompositionError::MetricCollision {
                    metric: metric.clone(),
                    first: first.clone(),
                    second: contribution.name.clone(),
                });
            }
        }
    }
    Ok(())
}

/// No two sources may provide the same non-metric element: a model, a relationship, a glossary
/// term, a caveat, an absence or a worked example, each under its own identifier.
fn check_no_element_collisions(contributions: &[Contributor]) -> Result<(), CompositionError> {
    let mut models: BTreeMap<&ModelName, &SourceName> = BTreeMap::new();
    for contribution in contributions {
        for model in &contribution.models {
            if let Some(first) = models.insert(model.name(), &contribution.name) {
                return Err(CompositionError::ElementCollision {
                    kind: "model",
                    element: model.name().as_str().to_owned(),
                    first: first.clone(),
                    second: contribution.name.clone(),
                });
            }
        }
    }

    let mut relationships: BTreeMap<&RelationshipName, &SourceName> = BTreeMap::new();
    for contribution in contributions {
        for relationship in &contribution.relationships {
            if let Some(first) = relationships.insert(relationship.name(), &contribution.name) {
                return Err(CompositionError::ElementCollision {
                    kind: "relationship",
                    element: relationship.name().as_str().to_owned(),
                    first: first.clone(),
                    second: contribution.name.clone(),
                });
            }
        }
    }

    let mut glossary: BTreeMap<&Phrase, &SourceName> = BTreeMap::new();
    for contribution in contributions {
        for entry in &contribution.glossary {
            if let Some(first) = glossary.insert(entry.term(), &contribution.name) {
                return Err(CompositionError::ElementCollision {
                    kind: "glossary term",
                    element: entry.term().as_str().to_owned(),
                    first: first.clone(),
                    second: contribution.name.clone(),
                });
            }
        }
    }

    let mut caveats: BTreeMap<&NoteName, &SourceName> = BTreeMap::new();
    for contribution in contributions {
        for caveat in &contribution.caveats {
            if let Some(first) = caveats.insert(caveat.name(), &contribution.name) {
                return Err(CompositionError::ElementCollision {
                    kind: "caveat",
                    element: caveat.name().as_str().to_owned(),
                    first: first.clone(),
                    second: contribution.name.clone(),
                });
            }
        }
    }

    let mut absences: BTreeMap<&Phrase, &SourceName> = BTreeMap::new();
    for contribution in contributions {
        for absence in &contribution.absences {
            if let Some(first) = absences.insert(absence.phrase(), &contribution.name) {
                return Err(CompositionError::ElementCollision {
                    kind: "absence",
                    element: absence.phrase().as_str().to_owned(),
                    first: first.clone(),
                    second: contribution.name.clone(),
                });
            }
        }
    }

    let mut examples: BTreeMap<&NoteName, &SourceName> = BTreeMap::new();
    for contribution in contributions {
        for example in &contribution.examples {
            if let Some(first) = examples.insert(example.name(), &contribution.name) {
                return Err(CompositionError::ElementCollision {
                    kind: "worked example",
                    element: example.name().as_str().to_owned(),
                    first: first.clone(),
                    second: contribution.name.clone(),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
    use sutura_domain::catalog::{Definitions, Description, Metric, Model};
    use sutura_domain::identity::{PrincipalChain, RequestContext, Subject, SubjectId};
    use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
    use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
    use sutura_domain::query::{Query, ToolOutcome};
    use sutura_domain::warehouse::{RowSet, Value};

    use crate::tests_support::{FixedBroker, FixedWarehouse, shared_posture};
    use crate::{Warehouses, answer, verify_and_validate};

    use super::{CompositionError, assemble};

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test-1").expect("a test version is a version")
    }

    fn source(raw: &str) -> SourceName {
        SourceName::parse(raw).expect("a test source is a name")
    }

    fn column(raw: &str) -> ColumnName {
        ColumnName::parse(raw).expect("a test column is a name")
    }

    fn june() -> TimeRange {
        TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range")
    }

    /// The certified source: one measure over one model, declared for exactly what it carries.
    fn certified() -> PinnedDefinitions {
        let model = Model::new(
            ModelName::parse("subscriptions").expect("a test model is a model"),
            source("local"),
            TableName::parse("subscriptions").expect("a test table is a table"),
            BTreeSet::from([column("mrr_cents"), column("month"), column("status")]),
            Description::default(),
        );
        let metric = Metric::new(
            MetricName::parse("recurring_revenue").expect("a test metric is a name"),
            ModelName::parse("subscriptions").expect("a test model is a model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_cents")))),
            Vec::new(),
            column("month"),
            BTreeSet::from([Grain::Month]),
            Vec::new(),
            None,
            Description::default(),
        )
        .expect("no dimensions to duplicate");
        let definitions = Definitions::assemble(vec![model], vec![], vec![metric]).expect("the certified content holds together");
        let declared = MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Grains]),
            KnowledgeCapabilities::none(),
        );
        PinnedDefinitions::pin(
            version(),
            definitions,
            Knowledge::none(),
            ContributionManifest::single(source("certified"), Contribution::of(declared)),
        )
        .expect("the certified source pins")
    }

    /// The narrow source: structure and prose, no measures - the `DataHub` shape.
    fn narrow() -> PinnedDefinitions {
        let model = Model::new(
            ModelName::parse("geo").expect("a test model is a model"),
            source("local"),
            TableName::parse("geo").expect("a test table is a table"),
            BTreeSet::from([column("region")]),
            Description::parse("where the customer is").expect("a description is a description"),
        );
        let definitions = Definitions::assemble(vec![model], vec![], vec![]).expect("the narrow content holds together");
        let declared = MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Descriptions]),
            KnowledgeCapabilities::none(),
        );
        PinnedDefinitions::pin(
            version(),
            definitions,
            Knowledge::none(),
            ContributionManifest::single(source("datahub"), Contribution::of(declared)),
        )
        .expect("the narrow source pins")
    }

    fn asked_by_a_person() -> RequestContext {
        RequestContext::of(PrincipalChain::of(Subject::Verified {
            id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
        }))
    }

    #[test]
    fn a_bundle_composed_of_a_narrow_source_and_a_certified_one_answers_a_question() {
        // The wave-one deployment, and the requirement this issue exists for: DataHub's structure
        // and prose beside the metrics certified here, in ONE bundle, and a question the certified
        // source certifies answered against it.
        let composed = assemble(vec![certified(), narrow()]).expect("structure and measures compose");

        // The composed bundle carries BOTH contributors, records both in its manifest, and is a
        // different bundle than the certified source alone - the digest moved with the composition.
        assert_eq!(composed.manifest().count(), 2);
        assert_eq!(composed.manifest().entries().len(), 2);
        assert!(
            composed
                .definitions()
                .metric(&MetricName::parse("recurring_revenue").unwrap())
                .is_some()
        );
        assert!(composed.definitions().model(&ModelName::parse("geo").unwrap()).is_some());
        assert_ne!(composed.digest(), certified().digest());

        // And a question the certified source certifies is answered against the composed bundle.
        let question = Query::new(
            MetricName::parse("recurring_revenue").expect("a test metric is a name"),
            Grain::Month,
            june(),
            Vec::new(),
            Vec::new(),
        );
        let rows = RowSet::new(
            vec![String::from("period"), String::from("recurring_revenue")],
            vec![vec![Value::Text(String::from("2026-06")), Value::Integer(130_000_000)]],
        )
        .expect("one row and two columns is rectangular");
        let registry = Warehouses::of(FixedWarehouse::answering(source("local"), shared_posture(), rows));
        // No metric declares an anchor, so an empty report validates: the point is that the
        // composed bundle answered, not that the canned rows reproduced a certified number.
        let validated = verify_and_validate(composed, &registry).expect("a bundle with no anchors validates");
        let outcome = answer(
            &validated,
            &question,
            &asked_by_a_person(),
            &FixedBroker::GrantsShared,
            &registry,
            1 << 30,
        )
        .expect("the composed bundle answers")
        .into_outcome();
        assert!(
            matches!(outcome, ToolOutcome::Answer { .. }),
            "a certified question is answered, not {outcome:?}"
        );
    }

    #[test]
    fn two_sources_defining_one_metric_are_refused_naming_both() {
        // ADR 0011: for metrics there is no precedence at all, declared or otherwise. Two
        // definitions of one number is the failure this system exists to prevent - and the refusal
        // names both sources so nobody has to guess which won.
        let err = assemble(vec![certified(), certified()]).expect_err("two definitions of revenue must not compose");
        match err {
            CompositionError::MetricCollision { metric, first, second } => {
                assert_eq!(metric.as_str(), "recurring_revenue");
                assert_eq!(first.as_str(), "certified");
                assert_eq!(second.as_str(), "certified");
            }
            other => panic!("expected a metric collision, got {other:?}"),
        }
    }

    #[test]
    fn a_contributor_supplying_a_kind_it_did_not_declare_is_refused() {
        // The declaration-fidelity check runs per contributor: a source whose content carries
        // descriptions while it declared only structure is refused, naming the source - the
        // per-contributor half of `checked_against` that a check over the merged bundle could not
        // see, since the other contributor's descriptions would mask it.
        let model = Model::new(
            ModelName::parse("region").expect("a test model is a model"),
            source("local"),
            TableName::parse("region").expect("a test table is a table"),
            BTreeSet::from([column("id")]),
            Description::parse("a region, with prose").expect("a description is a description"),
        );
        let definitions = Definitions::assemble(vec![model], vec![], vec![]).expect("the lying content holds together");
        let lying = PinnedDefinitions::pin(
            version(),
            definitions,
            Knowledge::none(),
            ContributionManifest::single(
                source("lying"),
                Contribution::of(MetadataCapabilities::of(
                    DefinitionCapabilities::of([DefinitionKind::Structure]),
                    KnowledgeCapabilities::none(),
                )),
            ),
        )
        .expect("a lying declaration still pins");
        let err = assemble(vec![lying, narrow()]).expect_err("an undeclared kind must not compose");
        match err {
            CompositionError::Unfaithful { source, .. } => assert_eq!(source.as_str(), "lying"),
            other => panic!("expected an unfaithful declaration, got {other:?}"),
        }
    }
}

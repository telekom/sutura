//! Tests for the metadata capability declaration and for declaration fidelity.
//!
//! A file rather than an inline `mod tests`, which is this repository's usual shape for a module
//! whose declarations plus its cases would pass the thousand-line limit `cargo xtask max-lines`
//! enforces.
//!
//! **What is tested here is the MECHANISM**, over bundles built in this file: that the walk over the
//! vocabulary reaches every kind, that a bundle's content is observed through the consequence a
//! caller sees rather than through a field that is always present, and that each of the two fidelity
//! directions fails with the kind named. What is tested over a real adapter's real bundle is in
//! `sutura-app`'s golden suite, which is where an adapter is.

use std::collections::BTreeSet;

use super::{
    DeclarableKind, DefinitionCapabilities, DefinitionKind, MetadataCapabilities, UnfaithfulDeclaration, carried, recorded,
};
use crate::calendar::{Date, TimeRange};
use crate::catalog::{Anchor, AnchorValue, Definitions, Description, Dimension, DimensionValue, Metric, Model, Relationship};
use crate::knowledge::{Capability, GlossaryEntry, Knowledge, KnowledgeCapabilities, KnowledgeInput, NoteBody, Phrase, Referent};
use crate::measure::{AggregatedColumn, Measure, RequiredFilter, Term};
use crate::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn model_name(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model is a model")
}

fn metric_name(raw: &str) -> MetricName {
    MetricName::parse(raw).expect("a test metric is a metric")
}

fn relationship_name(raw: &str) -> RelationshipName {
    RelationshipName::parse(raw).expect("a test relationship is a relationship")
}

fn description(raw: &str) -> Description {
    Description::parse(raw).expect("a test description is a description")
}

/// What a bundle in this file is asked to carry, named in the vocabulary under test.
///
/// **A list of [`DefinitionKind`]s rather than a record of booleans**, and that is a clippy finding
/// turned into a better fixture: seven flags is `struct_excessive_bools`, and a case reading
/// `carrying(&[Metrics, Anchors])` says what it is about where `Carrying { metric: true, anchor: true,
/// .. }` needed the reader to add it up.
///
/// **It is keyed on the enum the observation reads, and the two halves are still independent.** This
/// function maps a kind to the domain values that constitute it; [`super::carried`] maps those values
/// back to the kind. Both mappings are written by hand, in different files, so a wrong one shows up as
/// a failing case rather than agreeing with itself.
///
/// **There is no `Grains` entry to switch off, and its absence is the interesting part.**
/// `Definitions::assemble` refuses a metric declaring no grain as `NoGrains`, so a bundle holding a
/// metric with no grain is unrepresentable and every metric here carries `Grain::Month`.
/// [`DefinitionKind::Grains`]'s own doc comment records what that costs the fidelity check.
type Carrying<'a> = &'a [DefinitionKind];

/// Every kind, which is the complete bundle a reference adapter's declaration is checked against.
fn everything() -> Vec<DefinitionKind> {
    DefinitionKind::every().collect()
}

/// Does this case ask for that kind?
fn asked_for(carrying: Carrying<'_>, kind: DefinitionKind) -> bool {
    carrying.contains(&kind)
}

/// Prose, or the empty description, depending on what was asked for.
fn prose(carrying: Carrying<'_>, raw: &str) -> Description {
    if asked_for(carrying, DefinitionKind::Descriptions) {
        description(raw)
    } else {
        Description::default()
    }
}

/// A bundle carrying exactly what `carrying` asks for.
///
/// `orders` and `customers` are always present, so [`DefinitionKind::Structure`] arrives whatever is
/// asked for: `Definitions::assemble` has nothing to hang a metric on without a model, and the
/// carries-nothing case is covered by comparing an empty request against
/// [`MetadataCapabilities::nothing`] instead.
fn definitions(carrying: Carrying<'_>) -> Definitions {
    let models = vec![
        Model::new(
            model_name("orders"),
            SourceName::parse("local").expect("a test source is a source"),
            TableName::parse("orders").expect("a test table is a table"),
            BTreeSet::from([column("amount_cents"), column("order_date"), column("customer_id")]),
            prose(carrying, "what this holds"),
        ),
        Model::new(
            model_name("customers"),
            SourceName::parse("local").expect("a test source is a source"),
            TableName::parse("customers").expect("a test table is a table"),
            BTreeSet::from([column("id"), column("region_code")]),
            Description::default(),
        ),
    ];
    // `Cardinality` is a dimension reached through a relationship, so asking for it asks for the
    // relationship too - which is the containment the vocabulary's own doc comment describes.
    let wants_join = asked_for(carrying, DefinitionKind::Relationships) || asked_for(carrying, DefinitionKind::Cardinality);
    let joins = if wants_join {
        vec![Relationship::new(
            relationship_name("orders_customer"),
            model_name("orders"),
            column("customer_id"),
            model_name("customers"),
            column("id"),
            // Many-to-one, which is the cardinality that licenses a join: `assemble` refuses a
            // dimension reached through one that may duplicate rows, so the `Cardinality` case
            // could not be built any other way.
            JoinType::ManyToOne,
        )]
    } else {
        Vec::new()
    };
    // Every other kind below `Metrics` in the walk lives ON a metric, so asking for one of those asks
    // for the metric.
    let wants_metric = [
        DefinitionKind::Cardinality,
        DefinitionKind::Metrics,
        DefinitionKind::RequiredFilters,
        DefinitionKind::Grains,
        DefinitionKind::AllowedValues,
        DefinitionKind::Anchors,
    ]
    .into_iter()
    .any(|kind| asked_for(carrying, kind));
    let metrics = if wants_metric { vec![metric(carrying)] } else { Vec::new() };
    Definitions::assemble(models, joins, metrics).expect("the fixture bundle holds together")
}

/// The one metric a bundle in this file carries, when it carries one.
fn metric(carrying: Carrying<'_>) -> Metric {
    let mut dimensions: Vec<Dimension> = Vec::new();
    if asked_for(carrying, DefinitionKind::Cardinality) {
        let dimension = Dimension::new(
            DimensionName::parse("region").expect("a test dimension is a dimension"),
            column("region_code"),
            Some(relationship_name("orders_customer")),
            None,
            Description::default(),
        );
        dimensions.push(dimension);
    }
    if asked_for(carrying, DefinitionKind::AllowedValues) {
        let dimension = Dimension::new(
            DimensionName::parse("segment").expect("a test dimension is a dimension"),
            column("customer_id"),
            None,
            Some(BTreeSet::from([
                DimensionValue::parse("business").expect("a test value is a value")
            ])),
            Description::default(),
        );
        dimensions.push(dimension);
    }
    let filters = if asked_for(carrying, DefinitionKind::RequiredFilters) {
        vec![RequiredFilter::IsNotNull {
            column: column("amount_cents"),
        }]
    } else {
        Vec::new()
    };
    Metric::new(
        metric_name("revenue"),
        model_name("orders"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        filters,
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        dimensions,
        asked_for(carrying, DefinitionKind::Anchors)
            .then(|| Anchor::new(june(), AnchorValue::parse("62").expect("a test anchor value is a value"))),
        prose(carrying, "revenue, in minor units"),
    )
    .expect("these fixture dimensions are distinct")
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-06-30").expect("a test date is a date"),
    )
    .expect("june is a range")
}

/// Knowledge carrying one glossary entry about the fixture metric, and nothing else.
fn one_glossary_entry(definitions: &Definitions) -> Knowledge {
    let entry = GlossaryEntry::new(
        Phrase::parse("top line").expect("a test phrase is a phrase"),
        BTreeSet::new(),
        Referent::Metric {
            metric: metric_name("revenue"),
        },
        NoteBody::parse("what the sales team calls revenue").expect("a test body is a body"),
    );
    Knowledge::assemble(
        definitions,
        KnowledgeInput::new(
            KnowledgeCapabilities::of([Capability::Glossary]),
            vec![entry],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
    )
    .expect("the fixture knowledge holds together")
}

// ------------------------------------------------------------------------------- the vocabulary ---

#[test]
fn the_walk_reaches_every_kind_there_is() {
    // Nine, and the number is written down so that adding a variant without extending `next` is a
    // failure here as well as at the exhaustive matches.
    assert_eq!(DefinitionKind::every().count(), 9);
    let walked: BTreeSet<DefinitionKind> = DefinitionKind::every().collect();
    assert_eq!(walked.len(), 9, "the walk yielded a kind twice");
}

#[test]
fn next_and_previous_are_inverses_over_the_whole_chain() {
    // The witness that `next` is a bijection rather than merely total: the seed itself is checked on
    // the discriminant where the enum is declared, which is a compile-time assertion and not this.
    for kind in DefinitionKind::every() {
        if let Some(following) = kind.next() {
            assert_eq!(following.previous(), Some(kind), "{kind} and {following} are not adjacent");
        }
        if let Some(preceding) = kind.previous() {
            assert_eq!(preceding.next(), Some(kind), "{preceding} and {kind} are not adjacent");
        }
    }
    assert_eq!(DefinitionKind::Structure.previous(), None);
    assert_eq!(DefinitionKind::Anchors.next(), None);
}

#[test]
fn every_declarable_kind_covers_both_halves() {
    let kinds: Vec<DeclarableKind> = MetadataCapabilities::every_kind().collect();
    assert_eq!(
        kinds.len(),
        DefinitionKind::every().count().saturating_add(Capability::every().count())
    );
    assert!(kinds.contains(&DeclarableKind::Definition(DefinitionKind::Anchors)));
    assert!(kinds.contains(&DeclarableKind::Knowledge(Capability::Absences)));
}

#[test]
fn a_kind_is_named_in_words_rather_than_in_a_variant_spelling() {
    // What a declared absence reads as, which is the whole reason the negative has a name: the
    // rendered fidelity failure has to be readable by whoever configured the deployment.
    assert_eq!(DefinitionKind::AllowedValues.to_string(), "allowed values");
    assert_eq!(
        DeclarableKind::Definition(DefinitionKind::Cardinality).to_string(),
        "cardinality"
    );
    assert_eq!(DeclarableKind::Knowledge(Capability::Examples).to_string(), "examples");
}

// ------------------------------------------------------------------------ observing what arrived ---

#[test]
fn a_complete_bundle_is_observed_as_carrying_every_kind() {
    let definitions = definitions(&everything());
    let knowledge = one_glossary_entry(&definitions);
    let produced = MetadataCapabilities::produced(&definitions, &knowledge);
    for kind in DefinitionKind::every() {
        assert!(
            produced.definitions().declares(kind),
            "the complete fixture bundle was not observed as carrying {kind}"
        );
    }
    assert!(produced.knowledge().declares(Capability::Glossary));
    assert!(
        !produced.knowledge().declares(Capability::Caveats),
        "the fixture records no caveat, so observing one would mean this reads the declaration"
    );
}

#[test]
fn cardinality_is_observed_through_a_dimension_and_not_through_the_join_type_field() {
    // The kind that would be unobservable if it were read off a field: every `Relationship` holds a
    // `JoinType` because the type has no other shape, so a bundle with a relationship and no
    // dimension reached through it carries `Relationships` and not `Cardinality`.
    let with_relationship_only = definitions(&[DefinitionKind::Relationships, DefinitionKind::Metrics]);
    assert!(carried(&with_relationship_only, DefinitionKind::Relationships));
    assert!(!carried(&with_relationship_only, DefinitionKind::Cardinality));

    let licensing_a_join = definitions(&[DefinitionKind::Relationships, DefinitionKind::Cardinality]);
    assert!(carried(&licensing_a_join, DefinitionKind::Cardinality));
}

#[test]
fn descriptions_are_observed_from_prose_rather_than_from_the_field() {
    // Every `Model` and `Metric` has a `Description`; `Description::default()` is empty. A bundle of
    // empty descriptions carries no prose whatever the fields are.
    let blank = definitions(&[DefinitionKind::Metrics]);
    assert!(!carried(&blank, DefinitionKind::Descriptions));
    let written = definitions(&[DefinitionKind::Descriptions, DefinitionKind::Metrics]);
    assert!(carried(&written, DefinitionKind::Descriptions));
}

#[test]
fn a_bundle_with_no_metric_carries_none_of_the_kinds_a_metric_holds() {
    // The narrow source 0016 measured: structure and prose, and no measure this repository executes.
    // It assembles, which is the part that makes such a source usable at all.
    let narrow = definitions(&[DefinitionKind::Descriptions, DefinitionKind::Relationships]);
    let produced = MetadataCapabilities::produced(&narrow, &Knowledge::none());
    assert_eq!(
        produced,
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Descriptions,
                DefinitionKind::Relationships,
            ]),
            KnowledgeCapabilities::none(),
        )
    );
}

#[test]
fn knowledge_is_observed_from_the_content_and_never_from_the_bundle_s_own_declaration() {
    // A bundle may declare a capability and record nothing under it - that is the legitimate
    // declared-and-empty state. Reading `Knowledge::declares` here would make the fidelity check
    // compare one claim against a copy of itself.
    let definitions = definitions(&[DefinitionKind::Metrics]);
    let declared_and_empty = Knowledge::assemble(
        &definitions,
        KnowledgeInput::new(KnowledgeCapabilities::all(), Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    )
    .expect("declaring everything and recording nothing is legitimate");
    assert!(declared_and_empty.declares().declares(Capability::Glossary));
    assert!(!recorded(&declared_and_empty, Capability::Glossary));
}

// --------------------------------------------------------------------------------------- fidelity ---

#[test]
fn the_reference_declaration_is_faithful_to_a_complete_bundle() {
    let definitions = definitions(&everything());
    let knowledge = Knowledge::assemble(
        &definitions,
        KnowledgeInput::new(
            KnowledgeCapabilities::of([Capability::Glossary]),
            vec![GlossaryEntry::new(
                Phrase::parse("top line").expect("a test phrase is a phrase"),
                BTreeSet::new(),
                Referent::Metric {
                    metric: metric_name("revenue"),
                },
                NoteBody::parse("what the sales team calls revenue").expect("a test body is a body"),
            )],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
    )
    .expect("the fixture knowledge holds together");
    let produced = MetadataCapabilities::produced(&definitions, &knowledge);
    // Not `everything()`: this bundle records one glossary entry and no other note, so a declaration
    // of all four knowledge capabilities would be aspirational in three of them. That the reference
    // declaration is checked against a bundle that really carries everything is the golden suite's
    // job, over the example corpus, and it is deliberately not faked here.
    let declared = MetadataCapabilities::of(
        DefinitionCapabilities::all(),
        KnowledgeCapabilities::of([Capability::Glossary]),
    );
    assert_eq!(declared.checked_against(&produced), Ok(()));
}

#[test]
fn content_of_an_undeclared_kind_names_the_kind() {
    let definitions = definitions(&[DefinitionKind::Metrics, DefinitionKind::Anchors]);
    let produced = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    let silent_about_anchors = MetadataCapabilities::of(
        DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Grains]),
        KnowledgeCapabilities::none(),
    );
    assert_eq!(
        silent_about_anchors.checked_against(&produced),
        Err(UnfaithfulDeclaration::Undeclared {
            kind: DeclarableKind::Definition(DefinitionKind::Anchors),
        })
    );
}

#[test]
fn a_declared_kind_the_bundle_does_not_carry_names_the_kind() {
    let definitions = definitions(&[DefinitionKind::Metrics]);
    let produced = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    assert_eq!(
        MetadataCapabilities::everything().checked_against(&produced),
        Err(UnfaithfulDeclaration::Unprovided {
            kind: DeclarableKind::Definition(DefinitionKind::Descriptions),
        })
    );
}

#[test]
fn the_undeclared_direction_is_reported_before_the_unprovided_one() {
    // A declaration wrong in both directions: it claims anchors the bundle has not got, and stays
    // silent about the grains the bundle has. The safety failure is the one a caller was misled by,
    // so it is the one reported.
    let definitions = definitions(&[DefinitionKind::Metrics]);
    let produced = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    let wrong_both_ways = MetadataCapabilities::of(
        DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Anchors]),
        KnowledgeCapabilities::none(),
    );
    assert_eq!(
        wrong_both_ways.checked_against(&produced),
        Err(UnfaithfulDeclaration::Undeclared {
            kind: DeclarableKind::Definition(DefinitionKind::Grains),
        })
    );
}

#[test]
fn a_declared_kind_with_no_content_is_not_the_same_as_an_undeclared_kind() {
    // **The whole reason the declaration exists.** One bundle, whose metric carries no anchor; two
    // declarations that an empty collection could not tell apart. The one that declares anchors is
    // aspirational and is reported; the one that does not is faithful, and a caller reading it knows
    // the absence is the source's rather than the corpus's.
    //
    // Anchors rather than grains, and the substitution is the point: `assemble` refuses a metric with
    // no grain, so a grainless bundle is unrepresentable and grains could not carry this case.
    let definitions = definitions(&[DefinitionKind::Metrics]);
    let produced = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    let base = [DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Grains];

    let declares_anchors = MetadataCapabilities::of(
        DefinitionCapabilities::of(base.into_iter().chain([DefinitionKind::Anchors])),
        KnowledgeCapabilities::none(),
    );
    let does_not = MetadataCapabilities::of(DefinitionCapabilities::of(base), KnowledgeCapabilities::none());

    assert_ne!(declares_anchors, does_not, "the two states are the same value");
    assert_eq!(
        declares_anchors.checked_against(&produced),
        Err(UnfaithfulDeclaration::Unprovided {
            kind: DeclarableKind::Definition(DefinitionKind::Anchors),
        })
    );
    assert_eq!(does_not.checked_against(&produced), Ok(()));
}

#[test]
fn a_conditional_kind_absent_from_the_bundle_is_faithful() {
    // `of_may_provide` is 0011's *declared-and-empty* state, and it is what a source whose content
    // the deployment authors writes (datahub's deployment-defined structured property is
    // the first). A kind may be
    // declared and lawfully absent from a given bundle - the deployment decided to author none of it
    // - so absence is faithful. The *undeclared* direction is untouched, so presence is still
    // covered by the declared half, and the unconditional form of the same kinds still fails an
    // absent bundle: the marking, not the kinds, is what this loosens.
    let no_metric = definitions(&[]);
    let produced_empty = MetadataCapabilities::produced(&no_metric, &Knowledge::none());
    let may_provide = MetadataCapabilities::of(
        DefinitionCapabilities::of_may_provide([
            DefinitionKind::Structure,
            DefinitionKind::Descriptions,
            DefinitionKind::Metrics,
            DefinitionKind::Grains,
        ]),
        KnowledgeCapabilities::none(),
    );
    assert_eq!(
        may_provide.checked_against(&produced_empty),
        Ok(()),
        "a declared kind a bundle is lawfully without is faithful"
    );

    // A metric is always carried with its grains (`assemble` has no grainless metric), so `produced`
    // observes both; the same may-provide declaration must cover them.
    let with_metric = definitions(&[DefinitionKind::Metrics]);
    let produced_with = MetadataCapabilities::produced(&with_metric, &Knowledge::none());
    assert_eq!(
        may_provide.checked_against(&produced_with),
        Ok(()),
        "a conditional kind the bundle carries is covered by the declared half"
    );

    let unconditional = MetadataCapabilities::of(
        DefinitionCapabilities::of([
            DefinitionKind::Structure,
            DefinitionKind::Descriptions,
            DefinitionKind::Metrics,
            DefinitionKind::Grains,
        ]),
        KnowledgeCapabilities::none(),
    );
    assert_eq!(
        unconditional.checked_against(&produced_empty),
        Err(UnfaithfulDeclaration::Unprovided {
            kind: DeclarableKind::Definition(DefinitionKind::Descriptions),
        }),
        "the same kinds, declared unconditionally, still fail an absent bundle"
    );
}

#[test]
fn declaring_nothing_is_faithful_only_to_a_bundle_that_carries_nothing() {
    // `nothing()` is a legitimate declaration and not a default: nothing reaches it by omission,
    // because the port requires the declaration.
    let definitions = definitions(&[]);
    let produced = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    assert_eq!(
        MetadataCapabilities::nothing().checked_against(&produced),
        Err(UnfaithfulDeclaration::Undeclared {
            kind: DeclarableKind::Definition(DefinitionKind::Structure),
        })
    );
}

#[test]
fn an_undeclared_knowledge_capability_with_content_is_named_too() {
    // The knowledge half of the same direction. `Knowledge::assemble` already refuses this at load,
    // which is why it has to be built by declaring the capability and then declaring otherwise here:
    // what this witnesses is that the fidelity check sees the knowledge half at all, so a regression
    // in that load guard does not leave this direction unwatched.
    let definitions = definitions(&[DefinitionKind::Metrics]);
    let knowledge = one_glossary_entry(&definitions);
    let produced = MetadataCapabilities::produced(&definitions, &knowledge);
    let silent_about_the_glossary = MetadataCapabilities::of(
        DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Grains]),
        KnowledgeCapabilities::none(),
    );
    assert_eq!(
        silent_about_the_glossary.checked_against(&produced),
        Err(UnfaithfulDeclaration::Undeclared {
            kind: DeclarableKind::Knowledge(Capability::Glossary),
        })
    );
}

#[test]
fn a_declaration_round_trips_through_serde() {
    // It is serializable because a deployment reporting what its sources declare is the obvious next
    // reader, and because the knowledge half already is. It is NOT under the definition digest:
    // `PinnedDefinitions::pin` hashes the bundle's own `Knowledge`, and this value is a property of
    // the code.
    let declared = MetadataCapabilities::of(
        DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Descriptions]),
        KnowledgeCapabilities::of([Capability::Glossary]),
    );
    let json = serde_json::to_string(&declared).expect("a declaration serializes");
    assert!(json.contains("structure"), "{json}");
    let read: MetadataCapabilities = serde_json::from_str(&json).expect("a declaration deserializes");
    assert_eq!(read, declared);
}

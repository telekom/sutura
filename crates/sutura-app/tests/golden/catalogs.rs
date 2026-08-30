//! What a `SemanticCatalog` decides, expanded over every registered one.
//!
//! The bodies are generic with a `CatalogUnderTest` bound and a cell is one `#[test]` each, so the
//! assertions are written once and monomorphised per registration. Generics rather than `dyn`,
//! which is the workspace rule and is also forced: the port carries an associated error type.

use sutura_domain::capabilities::{DeclarableKind, DefinitionKind, MetadataCapabilities, UnfaithfulDeclaration};
use sutura_domain::pinned::SemanticCatalog;
use sutura_semantic::{Compiled, PredicateOrigin, compile};

use crate::adapters::{CatalogUnderTest, load, questions, read_question, stem};
use crate::support::{executable_definitions, oracle_definitions, oracle_knowledge, stated_knowledge};

use crate::shared::{PROVOKED, question, settings};

/// The digest and the parsed content, in one snapshot.
///
/// It moves when a definition changes and when a description changes, because both are part of
/// what was certified; it does not move when a file is reformatted or when two documents swap
/// order, which is what makes it worth reading.
fn pins_the_whole_catalog<C>()
where
    C: CatalogUnderTest,
{
    let pinned = load::<C>();
    settings(C::NAME).bind(|| {
        insta::assert_snapshot!("catalog_digest", pinned.digest().as_str());
        insta::assert_yaml_snapshot!("catalog_definitions", pinned.definitions());
        // The other half of what the digest covers. Pinned separately from the definitions because it
        // is a separate artefact with a separate audience: this one is what the agent-facing prompt
        // is rendered from, so a change to it is a change to what an agent is told even when every
        // number stays the same. The declaration is the first field of it, deliberately - a
        // deployment that stopped declaring a capability is the change hardest to notice by reading
        // the content.
        insta::assert_yaml_snapshot!("catalog_knowledge", pinned.knowledge());
    });
}

/// The property the `SemanticCatalog` port exists to have.
///
/// The oracle is the same catalog written out in Rust by hand, so this is not one parser
/// checked against itself: an adapter reads bytes somebody wrote for it, and a disagreement
/// means one of the two is wrong. Compared over the machine-readable content, because prose
/// lives in the markdown and nowhere else.
fn agrees_with_the_oracle<C>()
where
    C: CatalogUnderTest,
{
    assert_eq!(
        executable_definitions::<C>(),
        oracle_definitions(),
        "the {} catalog and the hand-written statement of the same definitions disagree",
        C::NAME
    );
}

/// The same property, for what a catalog says ABOUT what it defines.
///
/// Separate from [`agrees_with_the_oracle`] because it is a separate claim and a separate failure: the
/// definitions deciding what executes and the notes deciding what an agent is TOLD are two things a
/// parser can get wrong independently. A glossary entry read against the wrong metric produces no
/// wrong number at all - it produces an agent that asks a question this deployment declines, for a
/// reason that names a dimension and never mentions the glossary.
///
/// Compared with the bodies blanked, for the reason the definitions are compared with the descriptions
/// blanked: prose lives in the markdown and nowhere else. Everything that decides what the prompt says
/// is compared, the declared capabilities included.
fn agrees_with_the_oracle_about_what_it_says<C>()
where
    C: CatalogUnderTest,
{
    assert_eq!(
        stated_knowledge::<C>(),
        oracle_knowledge(),
        "the {} catalog and the hand-written statement of the same notes disagree",
        C::NAME
    );
}

/// Declaration fidelity: what an adapter says it supplies is exactly what it supplied.
///
/// **This is a SECOND contract and not a weaker version of [`agrees_with_the_oracle`].** That one
/// compares a bundle against the hand-written statement of the same corpus and is the golden
/// adapters' own contract - a version of it that tolerated a missing measure would pass a golden
/// adapter that had silently stopped reading measures, which is the one thing it is for. This one
/// compares an adapter's *declaration* against the bundle it produced, in both directions, and it is
/// the assertion a **declaring** adapter gets in place of the oracle: 0016 decided that a source
/// supplying part of a model is measured against what it said rather than against the reference
/// bundle.
///
/// A registered catalog is expanded over this too, and for the reference adapter it is not a
/// formality: `sutura-catalog-local` declares every kind there is, so passing this asserts the
/// example corpus really carries all thirteen of them. A corpus trimmed to twelve would take a
/// declared capability with it and nothing else in this suite would notice.
fn provides_exactly_what_it_declares<C>()
where
    C: CatalogUnderTest,
{
    let pinned = load::<C>();
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert_eq!(
        C::capabilities().checked_against(&produced),
        Ok(()),
        "the {} catalog's declaration and its bundle disagree",
        C::NAME
    );
}

/// What the digest is FOR.
///
/// Taken over the canonical form of the parsed definitions, so whitespace and key order are
/// not part of it. Over the bytes on disk, a reformat would look like a changed definition and
/// nobody would trust it. The other half is asserted once, off the axis, in
/// `dropping_prose_moves_the_digest`.
fn reads_the_same_content_to_the_same_digest<C>()
where
    C: CatalogUnderTest,
{
    assert_eq!(load::<C>().digest(), load::<C>().digest());
}

/// The whole corpus compiled, and what came out.
///
/// **On the catalog axis and not the dialect one, and that is a correction.** A plan is a
/// function of the definitions; a refusal is decided before any rendering at all. Pinning
/// either per dialect produced three byte-identical copies of every plan and every refusal,
/// which read as coverage of three data systems and were one fact written down three times.
fn pins_every_plan_and_refusal<C>()
where
    C: CatalogUnderTest,
{
    let pinned = load::<C>();
    for path in questions() {
        let question = read_question(&path);
        let name = stem(&path);
        let compiled = compile(&question, &pinned).unwrap_or_else(|e| panic!("{name} would not compile: {e}"));
        settings(C::NAME).bind(|| match compiled {
            Compiled::Refused { ref reason } => {
                insta::assert_yaml_snapshot!(format!("{name}__refusal"), reason);
            }
            Compiled::Planned { ref plan } => {
                insta::assert_yaml_snapshot!(format!("{name}__plan"), plan);
            }
            // The golden corpus is one source, so no question in it federates.
            Compiled::Federated { .. } => {
                panic!("the question corpus spans one data system; no question federates")
            }
        });
    }
}

/// A refusal nobody has seen happen is a refusal nobody knows works.
///
/// Each fixture is named after the variant it exists to reach, and this asserts it reaches
/// that one and not another - a fixture that started refusing for a different reason would
/// otherwise still pass as "a refusal".
fn provokes_every_refusal<C>()
where
    C: CatalogUnderTest,
{
    let pinned = load::<C>();
    for &(fixture, expected) in PROVOKED {
        let asked = question(&format!("{fixture}.yaml"));
        let compiled = compile(&asked, &pinned).expect("a refusal is not an error");
        let reason = compiled
            .refusal()
            .unwrap_or_else(|| panic!("{fixture} was answered by the {} catalog", C::NAME));
        let rendered = format!("{reason:?}");
        assert!(
            rendered.starts_with(expected),
            "{fixture} was refused as {rendered}, and exists to provoke {expected}"
        );
    }
}

/// A required filter is what makes a metric mean what it says.
///
/// `recurring_revenue` is revenue from ACTIVE subscriptions, and a statement without that
/// predicate returns revenue including the terminated ones under that name. A caller cannot ask
/// for it and cannot turn it off, so nothing the caller does can make this pass or fail - which is
/// exactly why it has to be asserted here, and why it is a property of the CATALOG rather than of a
/// renderer.
fn keeps_every_definitional_filter<C>()
where
    C: CatalogUnderTest,
{
    let pinned = load::<C>();
    let mut seen = 0_usize;
    for path in questions() {
        let asked = read_question(&path);
        let Some(metric) = pinned.definitions().metric(asked.metric()) else {
            continue;
        };
        if metric.required_filters().is_empty() {
            continue;
        }
        let compiled = compile(&asked, &pinned).expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        for required in metric.required_filters() {
            let present = plan.filters().iter().any(|f| {
                matches!(f.origin(), PredicateOrigin::Definition) && f.predicate().column().column() == required.column()
            });
            assert!(
                present,
                "{}: the plan for {} dropped its required filter on {}",
                stem(&path),
                asked.metric(),
                required.column()
            );
        }
        seen = seen.saturating_add(1);
    }
    assert!(
        seen > 0,
        "no question asked about a metric with a required filter, so this proved nothing"
    );
}

/// One cell of the catalog axis.
macro_rules! cell {
    ($name:ident, $adapter:ty) => {
        mod $name {
            #[test]
            fn the_catalog_is_pinned_as_a_whole() {
                super::pins_the_whole_catalog::<$adapter>();
            }

            #[test]
            fn it_agrees_with_the_oracle_about_what_executes() {
                super::agrees_with_the_oracle::<$adapter>();
            }

            #[test]
            fn it_agrees_with_the_oracle_about_what_it_says() {
                super::agrees_with_the_oracle_about_what_it_says::<$adapter>();
            }

            #[test]
            fn it_provides_exactly_what_it_declares() {
                super::provides_exactly_what_it_declares::<$adapter>();
            }

            #[test]
            fn reformatting_a_document_does_not_move_the_digest() {
                super::reads_the_same_content_to_the_same_digest::<$adapter>();
            }

            #[test]
            fn the_whole_corpus_compiles_and_every_outcome_is_pinned() {
                super::pins_every_plan_and_refusal::<$adapter>();
            }

            #[test]
            fn every_refusal_a_question_can_provoke_is_provoked_by_a_fixture() {
                super::provokes_every_refusal::<$adapter>();
            }

            #[test]
            fn a_definitional_filter_is_in_every_plan_about_its_metric() {
                super::keeps_every_definitional_filter::<$adapter>();
            }
        }
    };
}

crate::adapters::registered!(catalogs: cell);

#[test]
fn dropping_prose_moves_the_digest() {
    // The other half of "a reformat does not move the digest", and it belongs off the axis because
    // it is a claim about the oracle rather than about an adapter: the hand-written catalog states
    // the same definitions with no descriptions, so if prose were outside what is certified the two
    // digests would be equal. Without this, a digest that stopped covering prose would look right
    // in every per-catalog snapshot.
    let from_markdown = crate::adapters::load::<crate::adapters::ReferenceCatalog>();
    let without_prose = crate::support::HandWrittenCatalog
        .load()
        .expect("the hand-written catalog cannot fail");
    assert_ne!(
        from_markdown.digest(),
        without_prose.digest(),
        "prose is part of what is certified, so dropping it must move the digest"
    );
}

// ----------------------------------------------------------- declaration fidelity, off the axis ---
//
// A **declaring** adapter - one that supplies part of the model this port defines - is measured
// against its own declaration rather than against the reference bundle. There is no such adapter in
// the registry, and there deliberately is not one: `tests/adapters/mod.rs` registers only what
// somebody could deploy, and inventing a narrow fake to register would put a cell in the matrix that
// cannot execute the corpus - which that module already argues at length.
//
// So the declaring case is exercised where the narrow catalogs already live. Both fakes below were
// narrow before this branch, for reasons written in their own doc comments: `HandWrittenCatalog`
// leaves prose out because prose lives in the markdown and nowhere else, and `TwoSourceCatalog`
// carries two models and one metric because that is the whole shape its refusal needs. Each now says
// so in a declaration, and these tests are what hold the declaration to the bundle.

/// **Everything it declared, it produced - and nothing of an undeclared kind appears.**
///
/// One assertion covering both directions, because `checked_against` is one function: the variant it
/// returns names which direction failed, so a reader is never left to infer it from a diff.
#[test]
fn a_declaring_adapter_provides_exactly_what_it_declares() {
    let hand_written = crate::support::HandWrittenCatalog
        .load()
        .expect("the hand-written catalog cannot fail");
    let two_source = crate::support::two_source_catalog()
        .load()
        .expect("the two-source catalog cannot fail");
    for (name, declared, pinned) in [
        (
            "the hand-written oracle",
            <crate::support::HandWrittenCatalog as SemanticCatalog>::capabilities(),
            &hand_written,
        ),
        (
            "the two-source catalog",
            <crate::support::TwoSourceCatalog as SemanticCatalog>::capabilities(),
            &two_source,
        ),
    ] {
        let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
        assert_eq!(
            declared.checked_against(&produced),
            Ok(()),
            "{name}'s declaration and its bundle disagree"
        );
    }
}

/// A declared absence is VISIBLY absent, which is the half nothing covered for definitions.
///
/// `Knowledge::assemble`'s `UndeclaredContent` guard already refuses content for an undeclared
/// knowledge capability at load. Nothing refuses the definition half, so what makes an absence
/// visible there is this comparison - and this asserts it bites: widen the oracle's declaration by
/// the one kind it deliberately does not supply, and the mismatch names that kind.
#[test]
fn a_declaration_wider_than_the_bundle_names_the_kind_it_over_claimed() {
    let pinned = crate::support::HandWrittenCatalog
        .load()
        .expect("the hand-written catalog cannot fail");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert_eq!(
        MetadataCapabilities::everything().checked_against(&produced),
        Err(UnfaithfulDeclaration::Unprovided {
            kind: DeclarableKind::Definition(DefinitionKind::Descriptions),
        }),
        "the oracle carries no prose, so a declaration of every kind has to be reported as \
         over-claiming exactly that one"
    );
}

/// The two states an empty collection cannot tell apart, over a real bundle.
///
/// The oracle carries no descriptions. Declaring them is an over-claim and is reported; not declaring
/// them is faithful. **Same bundle, same emptiness, two different verdicts** - which is the whole
/// reason the declaration is a value rather than an inference from what a source happened to hold.
#[test]
fn a_declared_kind_with_no_content_is_not_the_same_as_an_undeclared_kind() {
    let pinned = crate::support::HandWrittenCatalog
        .load()
        .expect("the hand-written catalog cannot fail");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert!(
        !produced.definitions().declares(DefinitionKind::Descriptions),
        "this test is about a kind the bundle does not carry"
    );
    assert_eq!(
        <crate::support::HandWrittenCatalog as SemanticCatalog>::capabilities().checked_against(&produced),
        Ok(())
    );
    assert!(MetadataCapabilities::everything().checked_against(&produced).is_err());
}

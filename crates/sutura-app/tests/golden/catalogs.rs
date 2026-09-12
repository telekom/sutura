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
/// what was certified, and it does not move when two documents swap order - which is what makes it
/// worth reading.
///
/// **It DOES move when a file is reformatted, and this comment claimed the opposite until it was
/// measured.** Pointing dprint's markdown plugin at the fixture inserted ONE BLANK LINE after each
/// file's YAML frontmatter - not a word changed - and the digest moved. The body is hashed as the
/// bytes it is, so leading whitespace is inside what was certified; a reformat is therefore
/// indistinguishable here from an edit to what an agent is told. `dprint.json` excludes the fixture
/// for that reason rather than re-pinning the snapshot.
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
    C: GoldenCatalog,
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
    C: GoldenCatalog,
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

/// **Repeat-load determinism, and that is all it is.**
///
/// It loads the SAME bytes twice and compares the two digests, so what it can see is an adapter that
/// answers differently on a second read - a hash-ordered collection, a timestamp, a source of
/// randomness. It is on the universal axis because every catalog owes that, documents or not.
///
/// **Its cell was called `reformatting_a_document_does_not_move_the_digest` and could not see a
/// reformat at all.** Reformatting is a claim about two DIFFERENT inputs; this compares one input
/// with itself, and would pass for an adapter that hashed raw source bytes - the one shape the claim
/// rules out. **The two mutations that measure it, both of which leave this cell GREEN while
/// reddening the cell that took the name:** `Description::parse` in
/// `crates/sutura-domain/src/catalog/authored.rs` no longer trimming its input, and
/// `frontmatter::split` in `crates/sutura-catalog-local/src/frontmatter.rs` handing the whole
/// document to the body. Naming them here is the point - a sentence citing a measurement a reader
/// cannot replay from the file that makes it is the shape `.agents/skills/sutura/gates` records
/// twice. That name now belongs to
/// [`reformatting_a_document_does_not_move_the_digest`], off the axis, which writes the same
/// definitions twice with different layout and puts both through the parser; the other direction is
/// [`changing_what_a_definition_means_moves_the_digest`] and `dropping_prose_moves_the_digest`
/// beside it.
fn reads_the_same_documents_twice_to_the_same_digest<C>()
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
    C: GoldenCatalog,
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
    C: GoldenCatalog,
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
    C: GoldenCatalog,
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

/// The marker that separates a **golden** catalog from a declaring one in this target.
///
/// Living here rather than in `tests/adapters/mod.rs` because that module is shared by every test
/// target and `dead_code` is `deny` - a marker only the golden cell functions use would be dead in
/// every other target that includes the registry. It is defined so a golden-only cell can be bound
/// on it (see the [`golden`] macro below): a cell bound on this marker compiles for an adapter only
/// if that adapter implements it, which is what makes a golden/oracle cell impossible to register
/// against a declaring adapter by accident. It is the ROUTING copy of [`SemanticCatalog::KIND`] -
/// `macro_rules!` cannot read an associated constant, so the same fact is stated here in the form a
/// registration can be bound on and in the domain where nothing else may vary - and [`cell!`]
/// asserts the two agree, so the marker and the domain constant cannot drift apart. The canonical
/// declaration of the kind is [`SemanticCatalog::KIND`] in the domain.
pub(crate) trait GoldenCatalog: CatalogUnderTest {}

impl GoldenCatalog for sutura_catalog_local::LocalCatalog {}

/// A catalog cell that holds for EVERY registered catalog, golden or declaring.
///
/// These three are what register successfully for a partial model: pinning whatever loads, that what
/// produced matches what the adapter declared, and that a second load of the same bytes gives the
/// same digest. They make no assumption about a complete model, which is what lets a declaring
/// adapter hold them - and the third one deliberately claims only determinism, because *the digest
/// is a function of CONTENT* is a claim about two different inputs and a declaring adapter reads no
/// document a reader could reformat.
macro_rules! universal {
    ($adapter:ty) => {
        #[test]
        fn the_catalog_is_pinned_as_a_whole() {
            super::pins_the_whole_catalog::<$adapter>();
        }

        #[test]
        fn it_provides_exactly_what_it_declares() {
            super::provides_exactly_what_it_declares::<$adapter>();
        }

        #[test]
        fn loading_the_same_documents_twice_gives_the_same_digest() {
            super::reads_the_same_documents_twice_to_the_same_digest::<$adapter>();
        }
    };
}

/// The catalog cells that additionally hold only for a GOLDEN adapter, each because it assumes the
/// whole model: the two oracle comparisons, and the three corpus cells a catalog that produces the
/// complete example can meet.
///
/// **This is where the classification lives, and it is in the type system rather than a comment.**
/// Every golden-only cell function is bound on [`GoldenCatalog`] rather than [`CatalogUnderTest`] -
/// so a cell written to require the whole model cannot be expanded for a declaring adapter (the call
/// does not typecheck), and a cell that stays bound on [`CatalogUnderTest`] claims it holds for any
/// adapter and belongs in [`universal`]. A cell added later MUST pick one bound, and this macro and
/// [`universal`] are where the two classes are collected.
macro_rules! golden {
    ($adapter:ty) => {
        universal!($adapter);

        #[test]
        fn it_agrees_with_the_oracle_about_what_executes() {
            super::agrees_with_the_oracle::<$adapter>();
        }

        #[test]
        fn it_agrees_with_the_oracle_about_what_it_says() {
            super::agrees_with_the_oracle_about_what_it_says::<$adapter>();
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
    };
}

/// One registration's worth of catalog cells, selected by the adapter's kind.
///
/// The kind is a literal tag beside the algorithm name in the registry - `golden` or `declaring` -
/// and it routes the cells: a `declaring` registration gets [`universal`] and no golden-only cell, a
/// `golden` one gets [`golden`], which is every cell. A golden tag on an adapter that does not
/// implement [`GoldenCatalog`] does not build, and a cell bound on [`GoldenCatalog`] cannot be
/// expanded for a declaring adapter, so the split holds by the compiler rather than by review.
///
/// **The tag is checked against the adapter's own [`SemanticCatalog::KIND`].** The assertion below
/// is what gives the domain constant a reader: tag an adapter against what its own declaration says
/// and the registration does not compile. The registry tag and the [`GoldenCatalog`] marker are this
/// target's routing copies of `KIND`; this arm is where the two are torn unless they agree.
macro_rules! cell {
    ($name:ident, declaring, $adapter:ty) => {
        mod $name {
            const _: () = assert!(
                matches!(
                    <$adapter as sutura_domain::pinned::SemanticCatalog>::KIND,
                    sutura_domain::pinned::CatalogKind::Declaring
                ),
                "a catalog registered `declaring` must declare a declaring kind"
            );
            universal!($adapter);
        }
    };
    ($name:ident, golden, $adapter:ty) => {
        mod $name {
            const _: () = assert!(
                matches!(
                    <$adapter as sutura_domain::pinned::SemanticCatalog>::KIND,
                    sutura_domain::pinned::CatalogKind::Golden
                ),
                "a catalog registered `golden` must declare a golden kind"
            );
            golden!($adapter);
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

// ----------------------------------------------- what a reformat is, at the parser boundary ---
//
// `canonical_form` in the domain states the property: *reformatting a document, reordering two files
// or rewording a comment must not move the digest; changing what a metric means must.* Nothing asked
// it. The universal cell above loads one directory twice, which is determinism, and it carried the
// reformatting NAME - a test that would pass for an adapter hashing raw bytes, which is the one thing
// the sentence rules out.
//
// So the question is asked here, with two catalogs that differ in LAYOUT and in nothing else, both
// read through `sutura-catalog-local` - a composition root's own adapter, so the frontmatter splitter
// and the YAML parser are in the path rather than bypassed by building `Definitions` in memory.
//
// Off the axis, and not for the usual reason: a DECLARING adapter reads recorded metadata and has no
// document anybody could reformat, so this cannot be a cell of a matrix that includes one. It is a
// claim about a document-backed catalog, which is what `sutura-catalog-local` is.

/// The same two definitions, as a person would write them.
///
/// A model and one metric over it, which is the smallest catalog that carries a measure, a grain and
/// prose - a metric alone fails `Definitions::assemble` for a reason this test is not about.
const AS_WRITTEN: [(&str, &str); 2] = [
    (
        "orders.md",
        "---\nkind: model\nname: orders\nsource: local\ntable: fct_order\ncolumns: [amount_cents, order_date, channel]\n---\nThe order fact table.\n",
    ),
    (
        "revenue.md",
        "---\nkind: metric\nname: revenue\nmodel: orders\nmeasure:\n  simple: { aggregate: sum, column: amount_cents }\ntime_column: order_date\ngrains: [month]\ndimensions:\n  - name: channel\n    column: channel\n    values: [online, retail]\n    description: Where the order was placed.\n---\nNet revenue, in minor units.\n",
    ),
];

/// The same catalog, REFORMATTED, and every difference here is layout.
///
/// Five kinds at once, because they are five ways for a digest to become formatting-sensitive and a
/// fixture that varied one would leave the other four unasked:
///
/// 1. **Key order** in the frontmatter is reversed.
/// 2. **Flow against block style** - `[a, b]` becomes a `-` list, and the inline `{ }` mapping
///    becomes a nested one with its own two keys swapped.
/// 3. **Quoting** - a plain scalar becomes a quoted one.
/// 4. **Whitespace around prose**, in both of the places prose arrives from - blank lines inside the
///    frontmatter and around the body, and a padded quoted `description:` scalar. Both are
///    identical after trimming, which is what makes this layout rather than content, and it is two
///    dimensions rather than one because two different trims absorb them: the frontmatter splitter's
///    for the body, `Description::parse`'s for the YAML scalar. A fixture with only the first cannot
///    see the second removed.
/// 5. **File names**, so the sorted walk reads the metric before the model rather than after. That is
///    `canonical_form`'s *reordering two files*, and it is the one dimension no single-document
///    fixture can carry.
const REFORMATTED: [(&str, &str); 2] = [
    (
        "a_revenue.md",
        "---\n\ndimensions:\n  - description: \"   Where the order was placed.   \"\n    values:\n      - online\n      - retail\n    column: channel\n    name: channel\ngrains:\n  - month\ntime_column: order_date\nmeasure:\n  simple:\n    column: amount_cents\n    aggregate: sum\nmodel: \"orders\"\nname: revenue\nkind: metric\n\n---\n\n\nNet revenue, in minor units.\n\n\n",
    ),
    (
        "z_orders.md",
        "---\ncolumns:\n  - amount_cents\n  - order_date\n  - channel\ntable: fct_order\nsource: \"local\"\nname: orders\nkind: model\n---\n\nThe order fact table.\n\n",
    ),
];

/// The same layout as [`AS_WRITTEN`], and one thing MEANT differently: the measure sums nothing, it
/// takes a minimum.
///
/// The control for the two above. Without it, a digest that had stopped covering measures at all
/// would satisfy them both.
const A_DIFFERENT_MEANING: [(&str, &str); 2] = [
    AS_WRITTEN[0],
    (
        "revenue.md",
        "---\nkind: metric\nname: revenue\nmodel: orders\nmeasure:\n  simple: { aggregate: min, column: amount_cents }\ntime_column: order_date\ngrains: [month]\ndimensions:\n  - name: channel\n    column: channel\n    values: [online, retail]\n    description: Where the order was placed.\n---\nNet revenue, in minor units.\n",
    ),
];

/// Those documents on disk, loaded through the local catalog adapter, and its digest.
///
/// `CARGO_TARGET_DIR`'s test scratch rather than the system temp directory, which is
/// `crates/sutura-cli/tests/declared_source.rs`'s reason and needs no dependency. Cleared on the way
/// IN, so a failing run leaves its documents to read.
fn digest_of(case: &str, documents: &[(&str, &str)]) -> String {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("reformatting-{case}"));
    drop(std::fs::remove_dir_all(&root));
    std::fs::create_dir_all(&root).expect("a scratch directory is creatable");
    for &(file, document) in documents {
        std::fs::write(root.join(file), document).expect("a scratch document is writable");
    }
    let name = sutura_domain::model::SourceName::parse("scratch").expect("a catalog name is a name");
    let version = sutura_domain::pinned::DefinitionVersion::parse("reformatting-1").expect("a version is a version");
    sutura_catalog_local::LocalCatalog::new(name, root, version)
        .load()
        .unwrap_or_else(|e| panic!("the {case} catalog does not load: {e}"))
        .digest()
        .as_str()
        .to_owned()
}

/// **The claim `canonical_form` makes, asked of the parser.**
///
/// Two catalogs that mean the same thing and are written differently, both through the frontmatter
/// splitter and the YAML parser, one digest. A digest sensitive to any of the five differences
/// [`REFORMATTED`] carries fails here - which is what the universal cell's old name promised and
/// could not deliver, because it read one directory twice.
#[test]
fn reformatting_a_document_does_not_move_the_digest() {
    assert_eq!(
        digest_of("as-written", &AS_WRITTEN),
        digest_of("reformatted", &REFORMATTED),
        "the same definitions written with different key order, style, quoting, blank lines and file \
         names have to pin to one digest, or a reformat reads as a changed definition"
    );
}

/// The other direction, and the reason the test above is not satisfied by a constant.
///
/// One aggregate changed, nothing else - same files, same layout, same prose. A digest that did not
/// move would mean the snapshot said nothing about what a metric measures.
#[test]
fn changing_what_a_definition_means_moves_the_digest() {
    assert_ne!(
        digest_of("as-written-control", &AS_WRITTEN),
        digest_of("a-different-meaning", &A_DIFFERENT_MEANING),
        "the measure is part of what was certified, so changing the aggregate must move the digest"
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

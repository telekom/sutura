use std::collections::BTreeSet;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::{DeclarableKind, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Definitions, Description, InvalidDescription, Metric, Model};
use sutura_domain::knowledge::{
    Capability, GlossaryEntry, InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput, NoteBody, Phrase,
    Referent,
};
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog};
use sutura_domain::query::{Query, RefusalReason};

use crate::fixture::FixtureReader;
use crate::{Dictionary, DictionaryReader, RdbmsCatalog, RdbmsError, Relationship, Table};

fn name() -> SourceName {
    SourceName::parse("local").expect("a test name is a name")
}

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("test").expect("a test version is a version")
}

fn over() -> RdbmsCatalog<FixtureReader> {
    crate::fixture::over_fixture_source(name(), version())
}

/// A question about a metric the bundle does not define.
fn question_about(metric: &str) -> Query {
    let start = Date::parse("2026-06-01").expect("the start is a date");
    let end = Date::parse("2026-07-01").expect("the end is a date");
    Query::new(
        MetricName::parse(metric).expect("a metric name is a name"),
        Grain::Month,
        TimeRange::new(start, end).expect("the range is a range"),
        Vec::new(),
        Vec::new(),
    )
}

/// The requirement that makes this adapter work alone at all.
///
/// A dictionary-only bundle - models, prose and a foreign-key relationship and zero metrics - must
/// LOAD and validate: `Definitions::assemble` has no minimum-metric refusal. And because there is no
/// metric, a question about any metric is refused as `MetricUnknown` - the bundle answers no
/// certified question. This is issue #115's shape.
#[test]
fn a_bundle_from_a_dictionary_loads_validates_and_answers_no_certified_question() {
    let pinned = over().load().expect("the recorded dictionary loads");
    assert_eq!(pinned.definitions().models().len(), 2);
    assert_eq!(pinned.definitions().relationships().len(), 1);
    assert!(pinned.definitions().metrics().is_empty());

    // A question about any metric is refused, because no metric is defined.
    let compiled = sutura_semantic::compile(&question_about("revenue"), &pinned).expect("a refusal is not an error");
    match compiled {
        sutura_semantic::Compiled::Refused { reason } => {
            assert!(matches!(reason, RefusalReason::MetricUnknown { .. }), "{reason:?}");
        }
        sutura_semantic::Compiled::Planned { .. } => panic!("a dictionary defines no metric, so a plan is impossible"),
        sutura_semantic::Compiled::Federated { .. } => panic!("a dictionary bundle is one source; nothing federates"),
    }
}

/// The issue #151 claim, held against the declaration.
///
/// `MetadataCapabilities::produced` reads what the bundle actually carries, and `checked_against`
/// compares it in BOTH directions. The declaration provides `Structure`, `Descriptions` and
/// `Relationships` and no measure, no grain, no definitional filter, no value allowlist and no
/// anchor - and the bundle, having no metric, carries none of those either. The pair agrees, which
/// is what proves the "provides no measures" half is a declaration the bundle honours rather than an
/// absence the bundle contradicts.
#[test]
fn it_declares_it_provides_no_measures_and_the_bundle_has_none() {
    let catalog = over();
    let pinned = catalog.load().expect("the recorded dictionary loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert_eq!(
        <RdbmsCatalog<FixtureReader> as SemanticCatalog>::capabilities().checked_against(&produced),
        Ok(())
    );
    // The declaration provides no measure...
    let declaration = <RdbmsCatalog<FixtureReader> as SemanticCatalog>::capabilities();
    assert!(
        !declaration.declares(DeclarableKind::Definition(DefinitionKind::Metrics)),
        "a dictionary declares no measures"
    );
    // ...and the bundle carries none.
    assert!(!produced.declares(DeclarableKind::Definition(DefinitionKind::Metrics)));
}

/// A foreign key licenses no dimension without a declared cardinality.
///
/// The fixture's foreign key is read into a relationship with the evidence-backed `ManyToOne`
/// direction (the referenced column is unique), yet no metric exists to reach a dimension `via` it -
/// so `produced` observes `Cardinality` absent, and the declaration agrees. A foreign key therefore
/// licences no dimension: the relationship is present as a `Relationships` contribution, but no
/// `Cardinality` is produced.
#[test]
fn a_foreign_key_licenses_no_dimension_without_a_declared_cardinality() {
    let pinned = over().load().expect("the recorded dictionary loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert!(
        !produced.declares(DeclarableKind::Definition(DefinitionKind::Cardinality)),
        "a foreign key may exist without producing a cardinality, so it licenses no dimension"
    );
    // The relationship is present in the bundle as a `Relationships` contribution.
    assert!(produced.declares(DeclarableKind::Definition(DefinitionKind::Relationships)));
}

/// A sparse dictionary does not over-claim its declaration.
///
/// A dictionary is whatever the database documents about itself. A schema with a foreign key but no
/// table or column comments carries `Relationships` and `Structure` but no `Descriptions`; a schema
/// with comments and no foreign key carries prose and no relationship. Both are FAITHFUL bundles,
/// and because the declaration marks all three kinds declared-and-conditional
/// ([`DefinitionCapabilities::and_may_provide`]), `checked_against`'s `Unprovided` direction
/// exempts the absent half rather than failing a source that underfed the adapter - the defect a
/// dictionary's thinness used to trip.
#[test]
fn a_sparse_dictionary_does_not_overclaim_its_declaration() {
    // A dictionary with the FK and no comments at all: Descriptions absent, lawfully.
    let fk_only = SparseReader(Dictionary::new(
        vec![
            Table::new(
                "orders".to_owned(),
                vec!["order_id".to_owned(), "customer_id".to_owned()],
                None,
            ),
            Table::new("customers".to_owned(), vec!["customer_id".to_owned()], None),
        ],
        vec![Relationship::new(
            Some("orders_customer_fk".to_owned()),
            "orders".to_owned(),
            "customer_id".to_owned(),
            "customers".to_owned(),
            "customer_id".to_owned(),
        )],
    ));
    let pinned = RdbmsCatalog::new(name(), version(), fk_only)
        .load()
        .expect("a sparse dictionary loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert!(
        !produced.declares(DeclarableKind::Definition(DefinitionKind::Descriptions)),
        "no comments means no descriptions produced"
    );
    assert_eq!(
        <RdbmsCatalog<SparseReader> as SemanticCatalog>::capabilities().checked_against(&produced),
        Ok(()),
        "the declaration must not over-claim descriptions the sparse dictionary did not carry"
    );

    // The twin: comments but no foreign key - Relationships absent, lawfully.
    let prose_only = SparseReader(Dictionary::new(
        vec![Table::new(
            "orders".to_owned(),
            vec!["order_id".to_owned()],
            Some("Orders.".to_owned()),
        )],
        Vec::new(),
    ));
    let pinned = RdbmsCatalog::new(name(), version(), prose_only)
        .load()
        .expect("a prose-only dictionary loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert!(
        !produced.declares(DeclarableKind::Definition(DefinitionKind::Relationships)),
        "no foreign key means no relationship produced"
    );
    assert_eq!(
        <RdbmsCatalog<SparseReader> as SemanticCatalog>::capabilities().checked_against(&produced),
        Ok(()),
        "the declaration must not over-claim a relationship the sparse dictionary did not carry"
    );
}

/// A reader that hands its recorded dictionary back unchanged.
struct SparseReader(Dictionary);

impl DictionaryReader for SparseReader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        Ok(self.0.clone())
    }
}

/// A table comment carrying a control character refuses the load rather than altering the prose.
///
/// Not a theoretical guard. `Description::parse` refuses exactly the set `sutura_app::prompt::quote`
/// DROPS, and a bare `\r` is the reachable member: a comment typed in a Windows editor - or read
/// from a CRLF dump - carries one, the agent-facing renderer removes it, and the definition digest
/// is taken over the AUTHORED text. So the prose an agent reads is not the prose that was pinned,
/// with nothing downstream able to tell. This adapter must surface that as a refusal naming the
/// table; normalising it would be the adapter altering authored text.
///
/// The `\r` is mid-prose because `Description::parse` trims first - a description that merely *ends*
/// in one is not refused at all, and a test written that way would assert nothing.
#[test]
fn a_table_comment_carrying_a_control_character_refuses_the_load() {
    let crlf = SparseReader(Dictionary::new(
        vec![Table::new(
            "orders".to_owned(),
            vec!["order_id".to_owned()],
            Some("Orders placed by customers.\r\nOne row per order.".to_owned()),
        )],
        Vec::new(),
    ));
    let refused = RdbmsCatalog::new(name(), version(), crlf)
        .load()
        .expect_err("a control character the renderer would drop is not a loadable description");
    match refused {
        RdbmsError::Description { table, cause } => {
            assert_eq!(table, "orders", "the refusal names the table a reader has to go back to");
            assert_eq!(cause, InvalidDescription::ControlCharacter { code: 0x0D });
        }
        other => panic!("a description the renderer would alter must refuse as `Description`: {other:?}"),
    }
}

/// A reader that cannot read its dictionary.
///
/// The port's first FALLIBLE implementor, and the reason it exists: every other one in the tree
/// hands back a recorded [`Dictionary`], so `RdbmsError::Read` was constructed nowhere and the `?`
/// on `read_dictionary` could be deleted with the whole suite still green. A fake at the port
/// boundary rather than a stubbed socket - the port's contract includes its failure.
struct FailingReader;

impl DictionaryReader for FailingReader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        Err(RdbmsError::Read("the dictionary connection was refused".into()))
    }
}

/// A reader's failure is surfaced by the load, not swallowed into an empty bundle.
///
/// The failure mode this closes is the quiet one, and it is measured rather than argued: with the
/// `?` on `read_dictionary` neutralised, this load SUCCEEDS and pins a bundle of `models: {}` - a
/// deployment told its schema is empty rather than unreadable. The variant is asserted rather than
/// error-ness because the variant is what a caller branches on, which is the point of a typed
/// error; the swallowed read is already caught by the load returning `Ok` at all.
#[test]
fn a_reader_failure_is_surfaced_by_the_load() {
    let refused = RdbmsCatalog::new(name(), version(), FailingReader)
        .load()
        .expect_err("a reader that cannot read has no dictionary to assemble");
    assert!(
        matches!(refused, RdbmsError::Read(_)),
        "the reader's own failure must reach the caller unchanged: {refused:?}"
    );
}

/// Content for a knowledge kind the adapter did not declare fails the load.
///
/// `Knowledge::assemble`'s `UndeclaredContent` guard refuses a bundle whose input carries notes for
/// a capability the declared `KnowledgeCapabilities` do not cover - the "content for a kind it did
/// not declare" shape. The adapter declares no knowledge, so a glossary entry under a declaration
/// that provides none is refused, and the refusal names the capability. Built and refused through
/// the adapter's own types, so the guard is reachable the day a composed bundle feeds one in.
#[test]
fn content_for_a_kind_it_did_not_declare_fails_the_load() {
    let model = Model::new(
        ModelName::parse("orders").expect("a model name is a name"),
        name(),
        TableName::parse("orders").expect("a table name is a name"),
        ["order_date", "customer_id", "amount_cents"]
            .into_iter()
            .map(|c| ColumnName::parse(c).expect("a column is a name"))
            .collect(),
        Description::parse("Orders placed.").expect("a description is a description"),
    );
    let metric = Metric::new(
        MetricName::parse("revenue").expect("a metric name is a name"),
        ModelName::parse("orders").expect("a model name is a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Sum,
            ColumnName::parse("amount_cents").expect("a column is a name"),
        ))),
        Vec::new(),
        ColumnName::parse("order_date").expect("a column is a name"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::parse("").expect("empty is a description"),
    )
    .expect("no dimensions to duplicate");
    let definitions = Definitions::assemble(vec![model], Vec::new(), vec![metric]).expect("a model and a metric hold together");

    let entry = GlossaryEntry::new(
        Phrase::parse("revenue").expect("a phrase is a phrase"),
        BTreeSet::new(),
        Referent::Metric {
            metric: MetricName::parse("revenue").expect("a metric name is a name"),
        },
        NoteBody::parse("Net revenue in minor units.").expect("a note body is a body"),
    );

    let refused = Knowledge::assemble(
        &definitions,
        KnowledgeInput::new(KnowledgeCapabilities::none(), vec![entry], Vec::new(), Vec::new(), Vec::new()),
    )
    .expect_err("a glossary under a declaration that provides none is undeclared content");
    assert!(
        matches!(
            refused,
            InconsistentKnowledge::UndeclaredContent {
                capability: Capability::Glossary,
                ..
            }
        ),
        "{refused:?}"
    );
}

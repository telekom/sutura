use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::{DeclarableKind, DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{InconsistentDefinitions, InvalidDescription};
use sutura_domain::definitions::{DefinitionDigest, NotDigestible};
use sutura_domain::knowledge::KnowledgeCapabilities;
use sutura_domain::model::{Grain, JoinType, MetricName, ModelName, SourceName};
use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog};
use sutura_domain::query::{Query, RefusalReason};

use crate::fixture::FixtureReader;
use crate::{
    Dictionary, DictionaryReader, RdbmsCatalog, RdbmsError, Relationship, SingleColumnTargetUniqueness, Table, TableAddress,
};

fn name() -> SourceName {
    SourceName::parse("local").expect("a test name is a name")
}

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("test").expect("a test version is a version")
}

fn over() -> RdbmsCatalog<FixtureReader> {
    crate::fixture::over_fixture_source(name(), version())
}

fn address(schema: &str, table: &str) -> TableAddress {
    TableAddress::in_schema(schema.to_owned(), table.to_owned())
}

fn public(table: &str) -> TableAddress {
    address("public", table)
}

fn table(model: &str, columns: Vec<String>, description: Option<String>) -> Table {
    Table::new(model.to_owned(), public(model), columns, description)
}

fn foreign_key(
    name: Option<&str>,
    origin_table: &str,
    origin_column: &str,
    target_table: &str,
    target_column: &str,
) -> Relationship {
    Relationship::new(
        name.map(str::to_owned),
        public(origin_table),
        origin_column.to_owned(),
        public(target_table),
        target_column.to_owned(),
    )
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

/// The adapter's capability declaration is exact.
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
    assert_eq!(
        declaration,
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure])
                .and_may_provide([DefinitionKind::Descriptions, DefinitionKind::Relationships]),
            KnowledgeCapabilities::none(),
        )
    );
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
/// table comments carries `Relationships` and `Structure` but no `Descriptions`; a schema
/// with comments and no foreign key carries prose and no relationship. Both are FAITHFUL bundles,
/// and because descriptions and relationships are declared-and-conditional, `checked_against`'s
/// `Unprovided` direction exempts the absent half rather than failing a sparse dictionary.
#[test]
fn a_sparse_dictionary_does_not_overclaim_its_declaration() {
    // A dictionary with the FK and no comments at all: Descriptions absent, lawfully.
    let fk_only = SparseReader(Dictionary::new(
        vec![
            table("orders", vec!["order_id".to_owned(), "customer_id".to_owned()], None),
            table("customers", vec!["customer_id".to_owned()], None),
        ],
        vec![
            foreign_key(
                Some("orders_customer_fk"),
                "orders",
                "customer_id",
                "customers",
                "customer_id",
            )
            .with_target_uniqueness(SingleColumnTargetUniqueness::UniqueConstraint),
        ],
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
        vec![table("orders", vec!["order_id".to_owned()], Some("Orders.".to_owned()))],
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

/// An unconditional `Structure` declaration requires at least one model in every loaded bundle.
///
/// `Definitions::assemble` deliberately accepts no models, and pinning that value also succeeds, so
/// this adapter must close the gap itself. Otherwise its required capability is false for one legal
/// reader result even though the global declaring-adapter fixture remains green.
#[test]
fn an_empty_dictionary_refuses_instead_of_overclaiming_structure() {
    let empty = SparseReader(Dictionary::new(Vec::new(), Vec::new()));
    let refused = RdbmsCatalog::new(name(), version(), empty)
        .load()
        .expect_err("a dictionary with no visible table cannot provide Structure");
    assert!(matches!(refused, RdbmsError::NoVisibleTables), "{refused:?}");
}

/// An unnamed relationship gets a bounded, deterministic name even when its endpoints are long.
///
/// The readable prefix is truncated before a stable endpoint fingerprint is appended. That keeps
/// the domain's 63-character limit without silently truncating away the part that distinguishes two
/// foreign keys.
#[test]
fn an_unnamed_relationship_with_long_endpoints_gets_a_bounded_deterministic_name() {
    let origin_table = "orders_for_enterprise_accounts";
    let origin_column = "enterprise_customer_identifier";
    let target_table = "enterprise_customer_accounts";
    let target_column = "canonical_customer_identifier";
    let reader = SparseReader(Dictionary::new(
        vec![
            table(origin_table, vec![origin_column.to_owned()], None),
            table(target_table, vec![target_column.to_owned()], None),
        ],
        vec![
            foreign_key(None, origin_table, origin_column, target_table, target_column)
                .with_target_uniqueness(SingleColumnTargetUniqueness::PrimaryKey),
        ],
    ));

    let catalog = RdbmsCatalog::new(name(), version(), reader);
    let first = catalog
        .load()
        .expect("long endpoints still produce a valid relationship name");
    let second = catalog.load().expect("the same dictionary loads again");
    let first_name = first.definitions().relationships().keys().next().expect("one relationship");
    let second_name = second.definitions().relationships().keys().next().expect("one relationship");
    assert!(first_name.as_str().len() <= 63);
    assert_eq!(first_name, second_name);
}

/// A relationship is accepted only when the reader supplies single-column unique-target evidence.
///
/// Belonging to a composite primary or unique constraint does not make `target_column` individually
/// unique, so [`SingleColumnTargetUniqueness`] deliberately has no variant a reader can use for
/// membership alone. The relationship below represents that case by carrying no qualifying
/// evidence and must refuse rather than asserting `ManyToOne`.
#[test]
fn a_relationship_requires_single_column_target_uniqueness_evidence() {
    let pinned = over().load().expect("the evidenced relationship loads");
    let relationship = pinned
        .definitions()
        .relationships()
        .values()
        .next()
        .expect("the fixture carries one relationship");
    assert_eq!(relationship.join_type(), JoinType::ManyToOne);

    let relationship_without_evidence = SparseReader(Dictionary::new(
        vec![
            table("orders", vec!["customer_id".to_owned()], None),
            table("customers", vec!["customer_id".to_owned()], None),
        ],
        vec![foreign_key(
            Some("orders_customer_fk"),
            "orders",
            "customer_id",
            "customers",
            "customer_id",
        )],
    ));
    assert!(matches!(
        RdbmsCatalog::new(name(), version(), relationship_without_evidence).load(),
        Err(RdbmsError::TargetUniquenessUnknown { table, column })
            if table == "public.customers" && column == "customer_id"
    ));
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
        vec![table(
            "orders",
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
            assert_eq!(
                table, "public.orders",
                "the refusal names the table a reader has to go back to"
            );
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
        Err(RdbmsError::Read(Box::new(std::io::Error::other("recorded reader failure"))))
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

/// Schema qualification is part of the physical table identity, while each table still has a
/// distinct semantic model identity.
///
/// Without the two identities, a dictionary reader either drops the schema and makes both rows one
/// `orders` model or passes the qualified spelling as a model name and the identifier parser refuses
/// the dot. Either answer loses which physical table a generated `FROM` must select.
#[test]
fn schema_qualified_tables_keep_distinct_model_and_physical_identities() {
    let two_schemas = SparseReader(Dictionary::new(
        vec![
            Table::new(
                "sales_orders".to_owned(),
                address("sales", "orders"),
                vec!["order_id".to_owned()],
                None,
            ),
            Table::new(
                "support_orders".to_owned(),
                address("support", "orders"),
                vec!["order_id".to_owned()],
                None,
            ),
        ],
        Vec::new(),
    ));

    let pinned = RdbmsCatalog::new(name(), version(), two_schemas)
        .load()
        .expect("two schema-qualified tables with one bare name are distinct models");
    let models = pinned.definitions().models();
    assert_eq!(models.len(), 2);
    let model_names: std::collections::BTreeSet<&str> = models.keys().map(ModelName::as_str).collect();
    assert_eq!(model_names, ["sales_orders", "support_orders"].into_iter().collect());
    let physical_tables: std::collections::BTreeSet<String> = models.values().map(|model| model.table().to_string()).collect();
    assert_eq!(
        physical_tables,
        [String::from("sales.orders"), String::from("support.orders")]
            .into_iter()
            .collect()
    );
}

/// Physical identifiers are references to database objects, so parsing may not change them.
///
/// The shared domain parser trims before checking the identifier grammar. That is appropriate for
/// authored semantic names, but not for a dictionary spelling: a generated statement always quotes
/// these names, and the database resolves the whitespace as part of the quoted identifier.
#[test]
fn a_physical_table_identifier_that_would_be_trimmed_is_refused() {
    let table_with_space = SparseReader(Dictionary::new(
        vec![Table::new(
            "orders".to_owned(),
            public("orders "),
            vec!["order_id".to_owned()],
            None,
        )],
        Vec::new(),
    ));
    assert!(matches!(
        RdbmsCatalog::new(name(), version(), table_with_space).load(),
        Err(RdbmsError::TableName { table, .. }) if table == "public.orders "
    ));
}

#[test]
fn a_physical_column_identifier_that_would_be_trimmed_is_refused() {
    let column_with_space = SparseReader(Dictionary::new(
        vec![table("orders", vec!["order_id ".to_owned()], None)],
        Vec::new(),
    ));
    assert!(matches!(
        RdbmsCatalog::new(name(), version(), column_with_space).load(),
        Err(RdbmsError::ColumnName { table, column, .. })
            if table == "public.orders" && column == "order_id "
    ));
}

/// `DuckDB` keeps edge whitespace inside double-quoted identifiers.
///
/// This pins the target behaviour behind the adapter refusal above: the exact quoted names select
/// the object, while their trimmed spellings do not name that table or column.
#[test]
fn duckdb_preserves_edge_whitespace_in_quoted_identifiers() {
    let connection = duckdb::Connection::open_in_memory().expect("an in-memory DuckDB opens");
    connection
        .execute_batch(
            r#"CREATE TABLE "orders " ("order_id " INTEGER);
               INSERT INTO "orders " VALUES (7);"#,
        )
        .expect("DuckDB accepts edge whitespace in quoted identifiers");
    let value: i32 = connection
        .query_row(r#"SELECT "order_id " FROM "orders ""#, [], |row| row.get(0))
        .expect("the exact quoted spellings resolve");
    assert_eq!(value, 7);
    assert!(
        connection.prepare(r#"SELECT "order_id " FROM "orders""#).is_err(),
        "the trimmed table spelling must not resolve"
    );
    assert!(
        connection.prepare(r#"SELECT "order_id" FROM "orders ""#).is_err(),
        "the trimmed column spelling must not resolve"
    );
}

/// Error wrappers keep their structured causes in the standard error chain.
#[test]
fn an_inconsistent_definition_failure_exposes_its_source() {
    let error = RdbmsError::Inconsistent {
        cause: InconsistentDefinitions::DuplicateModel {
            model: ModelName::parse("orders").expect("a test model name is a name"),
        },
    };
    assert!(
        std::error::Error::source(&error).is_some(),
        "the wrapper must retain its typed cause: {error:?}"
    );
}

#[test]
fn a_digest_failure_exposes_its_source() {
    let error = RdbmsError::Digest {
        cause: NotDigestible::NotADigest {
            cause: DefinitionDigest::parse("not-a-digest").expect_err("the fixture is not a digest"),
        },
    };
    assert!(
        std::error::Error::source(&error).is_some(),
        "the wrapper must retain its typed cause: {error:?}"
    );
}

/// A constraint name is local to its table and cannot be used as the bundle-wide relationship key.
///
/// Two schemas may each define `orders_customer_fk`. The qualified endpoints distinguish those
/// constraints, so both relationships must survive assembly under different domain names.
#[test]
fn repeated_constraint_names_are_namespaced_by_their_endpoints() {
    let repeated_name = SparseReader(Dictionary::new(
        vec![
            Table::new(
                "sales_orders".to_owned(),
                address("sales", "orders"),
                vec!["customer_id".to_owned()],
                None,
            ),
            Table::new(
                "sales_customers".to_owned(),
                address("sales", "customers"),
                vec!["customer_id".to_owned()],
                None,
            ),
            Table::new(
                "support_orders".to_owned(),
                address("support", "orders"),
                vec!["customer_id".to_owned()],
                None,
            ),
            Table::new(
                "support_customers".to_owned(),
                address("support", "customers"),
                vec!["customer_id".to_owned()],
                None,
            ),
        ],
        vec![
            Relationship::new(
                Some("orders_customer_fk".to_owned()),
                address("sales", "orders"),
                "customer_id".to_owned(),
                address("sales", "customers"),
                "customer_id".to_owned(),
            )
            .with_target_uniqueness(SingleColumnTargetUniqueness::PrimaryKey),
            Relationship::new(
                Some("orders_customer_fk".to_owned()),
                address("support", "orders"),
                "customer_id".to_owned(),
                address("support", "customers"),
                "customer_id".to_owned(),
            )
            .with_target_uniqueness(SingleColumnTargetUniqueness::PrimaryKey),
        ],
    ));

    let pinned = RdbmsCatalog::new(name(), version(), repeated_name)
        .load()
        .expect("qualified endpoints namespace repeated constraint names");
    let relationships = pinned.definitions().relationships();
    assert_eq!(relationships.len(), 2);
    let names: std::collections::BTreeSet<_> = relationships
        .values()
        .map(sutura_domain::catalog::Relationship::name)
        .collect();
    assert_eq!(names.len(), 2);
}

/// A physical address identifies one model; a second model cannot silently replace the first.
#[test]
fn duplicate_physical_table_addresses_are_refused() {
    let duplicate = SparseReader(Dictionary::new(
        vec![
            Table::new("orders".to_owned(), public("orders"), vec!["order_id".to_owned()], None),
            Table::new("orders_alias".to_owned(), public("orders"), vec!["order_id".to_owned()], None),
        ],
        Vec::new(),
    ));

    assert!(matches!(
        RdbmsCatalog::new(name(), version(), duplicate).load(),
        Err(RdbmsError::DuplicateTable { table }) if table == "public.orders"
    ));
}

/// A relationship endpoint is resolved by physical identity, never guessed from a bare table name.
#[test]
fn a_relationship_to_an_unknown_physical_table_is_refused() {
    let unknown = SparseReader(Dictionary::new(
        vec![table("orders", vec!["customer_id".to_owned()], None)],
        vec![
            foreign_key(
                Some("orders_customer_fk"),
                "orders",
                "customer_id",
                "customers",
                "customer_id",
            )
            .with_target_uniqueness(SingleColumnTargetUniqueness::PrimaryKey),
        ],
    ));

    assert!(matches!(
        RdbmsCatalog::new(name(), version(), unknown).load(),
        Err(RdbmsError::UnknownRelationshipTable { table }) if table == "public.customers"
    ));
}

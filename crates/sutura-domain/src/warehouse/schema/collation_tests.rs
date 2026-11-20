//! The collation fact: what pins, what it hashes as, and what two models over one column may say.

use std::collections::BTreeMap;

use super::{PinnedSchemas, TypeDialect};
use crate::catalog::{Collation, Column, Definitions, Description, Model};
use crate::model::{ColumnName, ModelName, QualifiedTable, SourceName, TableName};
use crate::pinned::NotValidated;

/// The digest the base tree computes for [`baseline`], before any collation fact existed.
const BASE_DIGEST: &str = "0097b110d8842cef7c23c4e4eddea59b3f9632dd25ed21b4b15a37657cbcbd9d";

fn column(name: &str, declared: &str) -> Column {
    let name = ColumnName::parse(name).expect("a test column is a column");
    Column::from_metadata(name, Some(declared), None, None).expect("a test column has no description to refuse")
}

fn collated(name: &str, declared: &str, collation: Collation) -> Column {
    column(name, declared).with_collation(collation)
}

fn model(name: &str, table: &str, columns: Vec<Column>) -> Model {
    Model::new(
        ModelName::parse(name).expect("a test model is a model"),
        SourceName::parse("lake").expect("a test source is a source"),
        TableName::parse(table).expect("a test table is a table"),
        columns,
        Description::default(),
    )
}

fn definitions(models: Vec<Model>) -> Definitions {
    Definitions::assemble(models, vec![], vec![]).expect("a test bundle holds together")
}

fn lake() -> SourceName {
    SourceName::parse("lake").expect("a test source is a source")
}

fn pin(dialect: TypeDialect, models: Vec<Model>) -> Result<PinnedSchemas, NotValidated> {
    PinnedSchemas::pin(&definitions(models), &BTreeMap::from([(lake(), dialect)]))
}

fn pin_notes(dialect: TypeDialect, columns: Vec<Column>) -> (PinnedSchemas, ModelName) {
    let pinned = pin(dialect, vec![model("notes", "notes", columns)]).expect("a typed bundle pins");
    (pinned, ModelName::parse("notes").expect("a test model is a model"))
}

fn pinned_collation(pinned: &PinnedSchemas, model: &ModelName, column: &str) -> Collation {
    let schema = pinned.of(model).expect("the model is pinned");
    Collation::of_field(schema.field_with_name(column).expect("the column is pinned"))
}

/// A fixed schema with no collation fact: a number, a text column and a type that falls back.
fn baseline(body: Collation) -> PinnedSchemas {
    let columns = vec![
        column("id", "bigint"),
        collated("body", "text", body),
        column("odd", "GEOGRAPHY"),
    ];
    pin_notes(TypeDialect::Postgres, columns).0
}

#[test]
fn a_schema_with_no_collation_fact_hashes_as_the_base_tree_did_and_each_fact_moves_it() {
    assert_eq!(baseline(Collation::Unknown).digest().as_str(), BASE_DIGEST);
    let binary = baseline(Collation::Binary);
    let folded = baseline(Collation::named("und:ci"));
    let other = baseline(Collation::named("und:ai"));
    let digests = [binary.digest(), folded.digest(), other.digest()];
    for (index, left) in digests.iter().enumerate() {
        assert_ne!(left.as_str(), BASE_DIGEST, "a recorded collation re-pins");
        for right in &digests[index + 1..] {
            assert_ne!(left, right, "each collation re-pins");
        }
    }
}

#[test]
fn a_collation_pins_only_on_a_column_the_dialect_reads_as_text() {
    let columns = vec![
        collated("exact", "text", Collation::Binary),
        collated("folded", "varchar(20)", Collation::named("und:ci")),
        column("silent", "text"),
        collated("count", "bigint", Collation::Binary),
        collated("odd", "GEOGRAPHY", Collation::Binary),
    ];
    let (pinned, notes) = pin_notes(TypeDialect::Postgres, columns);
    assert_eq!(pinned_collation(&pinned, &notes, "exact"), Collation::Binary);
    assert_eq!(pinned_collation(&pinned, &notes, "folded"), Collation::named("und:ci"));
    for column in ["silent", "count", "odd"] {
        let collation = pinned_collation(&pinned, &notes, column);
        assert_eq!(collation, Collation::Unknown, "{column}");
        assert!(!collation.is_binary(), "{column}");
    }
}

#[test]
fn an_engine_with_no_column_collation_pins_binary_over_what_a_catalog_declares_but_not_on_a_fallback() {
    for dialect in [TypeDialect::DataFusion, TypeDialect::ClickHouse] {
        let columns = vec![
            collated("folded", "text", Collation::named("und:ci")),
            column("silent", "text"),
            collated("odd", "GEOGRAPHY", Collation::Binary),
        ];
        let (pinned, notes) = pin_notes(dialect, columns);
        assert_eq!(pinned_collation(&pinned, &notes, "folded"), Collation::Binary, "{dialect:?}");
        assert_eq!(pinned_collation(&pinned, &notes, "silent"), Collation::Binary, "{dialect:?}");
        assert_eq!(pinned_collation(&pinned, &notes, "odd"), Collation::Unknown, "{dialect:?}");
    }
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let (pinned, notes) = pin_notes(dialect, vec![column("silent", "text")]);
        assert_eq!(pinned_collation(&pinned, &notes, "silent"), Collation::Unknown, "{dialect:?}");
    }
}

#[test]
fn two_models_over_one_physical_column_may_not_pin_different_known_collations() {
    let alias = |collation| {
        vec![
            model("notes_a", "notes", vec![collated("body", "text", Collation::Binary)]),
            model("notes_b", "notes", vec![collated("body", "text", collation)]),
        ]
    };
    let refused = pin(TypeDialect::Postgres, alias(Collation::named("und:ci"))).expect_err("binary against und:ci");
    assert_eq!(
        refused,
        NotValidated::CollationDisagreement {
            model: ModelName::parse("notes_b").expect("a test model is a model"),
            column: ColumnName::parse("body").expect("a test column is a column"),
        }
    );
    let two_names = vec![
        model("notes_a", "notes", vec![collated("body", "text", Collation::named("und:ai"))]),
        model("notes_b", "notes", vec![collated("body", "text", Collation::named("und:ci"))]),
    ];
    assert!(matches!(
        pin(TypeDialect::Postgres, two_names),
        Err(NotValidated::CollationDisagreement { .. })
    ));
    pin(TypeDialect::Postgres, alias(Collation::Binary)).expect("agreeing aliases pin");
    pin(TypeDialect::Postgres, alias(Collation::Unknown)).expect("an alias that says nothing contradicts nothing");
    pin(TypeDialect::DataFusion, alias(Collation::named("und:ci")))
        .expect("the engine fixes one collation, so the authored names do not disagree");
}

#[test]
fn differently_spelled_aliases_of_one_physical_column_may_not_pin_different_known_collations() {
    let spelled = |name: &str, table: &str, column: &str, collation| {
        Model::new(
            ModelName::parse(name).expect("a test model is a model"),
            lake(),
            QualifiedTable::parse(table).expect("a test table is a table"),
            vec![collated(column, "text", collation)],
            Description::default(),
        )
    };
    for (table, column) in [("public.notes", "body"), ("NOTES", "BODY"), ("Public.Notes", "Body")] {
        let refused = pin(
            TypeDialect::Postgres,
            vec![
                spelled("notes_a", "notes", "body", Collation::Binary),
                spelled("notes_b", table, column, Collation::named("und:ci")),
            ],
        )
        .expect_err(table);
        assert!(
            matches!(refused, NotValidated::CollationDisagreement { .. }),
            "{table}.{column}"
        );
    }
    // Two qualified spellings of different datasets are two tables; a different column is no alias.
    pin(
        TypeDialect::Postgres,
        vec![
            spelled("notes_a", "one.notes", "body", Collation::Binary),
            spelled("notes_b", "two.notes", "body", Collation::named("und:ci")),
            spelled("notes_c", "one.notes", "title", Collation::named("und:ci")),
        ],
    )
    .expect("different datasets and different columns do not contradict");
}

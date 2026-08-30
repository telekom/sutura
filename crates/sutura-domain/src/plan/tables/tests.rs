//! What the checked table set accepts, and the four shapes it refuses.

use crate::model::{
    ColumnName, DatasetName, IdentifierCase, JoinType, ProjectName, QualifiedTable, RelationshipName, TableName, TableQualifier,
};
use crate::plan::{PlanColumn, PlanJoin};

use super::{AmbiguousTables, StatementTables};

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a test table is a table")
}

fn dataset(name: &str) -> DatasetName {
    DatasetName::parse(name).expect("a test dataset is a dataset")
}

fn project(name: &str) -> ProjectName {
    ProjectName::parse(name).expect("a test project is a project")
}

fn in_project(project_name: &str, dataset_name: &str, table_name: &str) -> QualifiedTable {
    QualifiedTable::new(
        Some(TableQualifier::in_project(project(project_name), dataset(dataset_name))),
        table(table_name),
    )
}

fn column(table_name: &str, column_name: &str) -> PlanColumn {
    PlanColumn::new(
        table(table_name),
        ColumnName::parse(column_name).expect("a test column is a column"),
    )
}

fn join_to(relationship: &str, joined: QualifiedTable) -> PlanJoin {
    PlanJoin::new(
        RelationshipName::parse(relationship).expect("a test relationship is one"),
        joined,
        JoinType::ManyToOne,
        column("orders", "customer_id"),
        column("customers", "id"),
    )
}

/// The shape the whole feature exists for: two projects, two datasets, two DIFFERENT table names.
#[test]
fn two_paths_ending_in_different_names_are_one_statement() {
    let tables = StatementTables::parse(
        in_project("analytics-prod", "sales", "orders"),
        vec![join_to("orders_customer", in_project("reference-data", "crm", "customers"))],
    )
    .expect("two names, two identifiers");
    assert_eq!(tables.table(), &in_project("analytics-prod", "sales", "orders"));
    assert_eq!(tables.joins().len(), 1);
}

/// The reproduced defect: the paths differ, the identifier a column is qualified by does not.
#[test]
fn two_paths_ending_in_one_name_are_refused_and_the_refusal_names_both() {
    let fact = in_project("analytics-prod", "sales", "orders");
    let collides = in_project("reference-data", "crm", "orders");
    let refused = StatementTables::parse(fact.clone(), vec![join_to("orders_customer", collides.clone())])
        .expect_err("one identifier, two tables");
    assert_eq!(
        refused,
        AmbiguousTables::OneIdentifierTwoTables {
            alias: table("orders"),
            // The dotted text, which is what the variant carries: see its own note for the size lint
            // that decided that, and `Display` on `QualifiedTable` for why the text round-trips.
            first: fact.to_string(),
            second: collides.to_string(),
        }
    );
}

/// `first` is the earlier occurrence, which is what makes the message readable as a statement.
///
/// Asserted rather than assumed, because the walk could as easily have reported the pair the other way
/// round and the message says "reads {first} and {second}".
#[test]
fn the_refusal_reports_the_earlier_occurrence_first() {
    let fact = in_project("analytics-prod", "sales", "orders");
    let second = in_project("second-project", "crm", "customers");
    let third = in_project("third-project", "crm", "customers");
    let refused = StatementTables::parse(
        fact,
        vec![
            join_to("orders_customer", second.clone()),
            join_to("orders_account", third.clone()),
        ],
    )
    .expect_err("two joins, one identifier");
    match refused {
        AmbiguousTables::OneIdentifierTwoTables {
            first, second: later, ..
        } => {
            assert_eq!(first, second.to_string(), "the earlier join is `first`");
            assert_eq!(later, third.to_string(), "the later join is `second`");
        }
    }
}

/// A pair spelled to look like two names, which an equality check let through.
///
/// The comparison is `IdentifierCase::COARSEST` rather than `==` because `GoogleSQL` resolves an alias
/// case-insensitively and a real `DuckDB` folds a quoted identifier when it resolves it.
#[test]
fn two_paths_differing_only_in_case_are_one_identifier() {
    let refused = StatementTables::parse(
        in_project("analytics-prod", "sales", "Orders"),
        vec![join_to("orders_customer", in_project("reference-data", "crm", "orders"))],
    )
    .expect_err("Orders and orders are one identifier where an alias folds");
    assert_eq!(refused.alias(), &table("orders"), "the refusal names the later spelling");
    // Stated where the comparison is read, so the test says WHY rather than only what: an equality
    // check on these two strings passes, which is the hole this case closes.
    assert_ne!("Orders", "orders");
    assert!(IdentifierCase::COARSEST.names_one_thing("Orders", "orders"));
}

/// The same table joined twice is two occurrences under one identifier, so it is refused too.
///
/// Reachable without any qualification at all: two relationships to one target model produce two joins
/// on one table, which every target reads as a duplicate alias.
#[test]
fn one_table_joined_twice_is_refused() {
    let joined = in_project("reference-data", "crm", "customers");
    let refused = StatementTables::parse(
        in_project("analytics-prod", "sales", "orders"),
        vec![
            join_to("orders_customer", joined.clone()),
            join_to("orders_billing_customer", joined),
        ],
    )
    .expect_err("one table in one statement twice");
    assert_eq!(refused.alias(), &table("customers"));
}

/// One table has no pair to compare, so the no-join spelling cannot fail and does not pretend to.
#[test]
fn one_table_and_no_joins_needs_no_check() {
    let only = StatementTables::only(table("orders"));
    assert_eq!(only.table(), &QualifiedTable::from(table("orders")));
    assert!(only.joins().is_empty());
}

/// A bare name and a qualified path ending in the same name collide as well.
///
/// Worth its own case because the two halves reach the comparison through different constructors:
/// `impl Into<QualifiedTable>` for the bare one, a parsed qualifier for the other.
#[test]
fn a_bare_name_collides_with_a_qualified_path_ending_in_it() {
    let refused = StatementTables::parse(
        table("orders"),
        vec![join_to("orders_customer", in_project("reference-data", "crm", "orders"))],
    )
    .expect_err("a bare name is still an identifier");
    assert_eq!(refused.alias(), &table("orders"));
}

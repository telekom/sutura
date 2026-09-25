use super::{
    PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanJoin, PlanJoinKey, PlanKey, PlanMeasure, PlanPredicate, PlanTerm,
    PredicateOrigin, QueryPlan, ResultLabel, StatementTables,
};
use crate::calendar::{Date, TimeRange};
use crate::model::{Aggregate, ColumnName, Grain, JoinType, MetricName, QualifiedTable, RelationshipName, SourceName, TableName};
use crate::warehouse::ParamValue;

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a test table is a table")
}

fn column(table_name: &str, column_name: &str) -> PlanColumn {
    PlanColumn::new(
        table(table_name),
        ColumnName::parse(column_name).expect("a test column is a column"),
    )
}

fn bindings() -> PlanBindings {
    PlanBindings::parse(
        vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: column("fct", "d"),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: column("fct", "d"),
                    param: 1,
                },
            ),
        ],
        vec![ParamValue::Date(day("2026-06-01")), ParamValue::Date(day("2026-07-01"))],
    )
    .expect("a test plan binds its two range bounds in placeholder order")
}

/// One fact joined to one dimension, so a caller of `resolve_tables` has both a `FROM` path
/// and a joined one to tell apart.
fn plan_with_join(fact: QualifiedTable, joined: QualifiedTable) -> QueryPlan {
    let join = PlanJoin::new(
        RelationshipName::parse("dim").expect("a test relationship is one"),
        joined,
        JoinType::ManyToOne,
        vec![PlanJoinKey::Equal {
            origin: column("fct", "dim_id"),
            target: column("dim", "id"),
        }],
    );
    QueryPlan::new(
        SourceName::parse("warehouse").expect("a test source is a source"),
        MetricName::parse("m").expect("a test metric is a metric"),
        StatementTables::parse(fact, vec![join]).expect("a test plan names two distinguishable tables"),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("fct", "d")),
        Vec::<PlanKey>::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("fct", "amount"),
            },
        },
        ResultLabel::measure(&MetricName::parse("m").expect("a test metric is a metric")),
        bindings(),
        TimeRange::new(day("2026-06-01"), day("2026-07-01")).expect("a test range is a range"),
    )
}

#[test]
fn resolve_tables_rewrites_the_from_table_and_every_join_and_nothing_else() {
    let plan = plan_with_join(QualifiedTable::from(table("fct")), QualifiedTable::from(table("dim")));

    let resolved = plan
        .clone()
        .resolve_tables(|path| {
            Ok::<_, core::convert::Infallible>(
                QualifiedTable::parse(format!("resolved_ds.{}", path.name())).expect("a test path parses"),
            )
        })
        .expect("an infallible resolve cannot fail");

    assert_eq!(resolved.table().to_string(), "resolved_ds.fct");
    assert_eq!(resolved.joins()[0].table().to_string(), "resolved_ds.dim");
    // Nothing else about the plan moved.
    assert_eq!(resolved.metric(), plan.metric());
    assert_eq!(resolved.source(), plan.source());
    assert_eq!(resolved.bucket(), plan.bucket());
}

#[test]
fn resolve_tables_stops_at_the_first_failure_rather_than_resolving_the_join_too() {
    let plan = plan_with_join(QualifiedTable::from(table("fct")), QualifiedTable::from(table("dim")));
    let mut calls = 0_u32;

    let error = plan
        .resolve_tables(|_| {
            calls += 1;
            Err::<QualifiedTable, &str>("no")
        })
        .expect_err("the closure always refuses");

    assert!(matches!(&error, super::ResolveTablesError::Resolve(_)), "{error}");
    assert_eq!(calls, 1, "the walk stops at the FROM table and never reaches the join");
}

/// A resolve that introduces a collision two tables one statement cannot tell apart is refused,
/// not silently constructed. `StatementTables::parse` holds this at plan construction; `resolve_tables`
/// re-validates through it so a `resolve` closure that rewrites a table NAME into one already in
/// the statement is caught rather than shipped to the renderer.
#[test]
fn resolve_tables_refuses_a_resolve_that_makes_two_tables_share_an_identifier() {
    // Two distinguishable tables: `fct` and `dim`.
    let plan = plan_with_join(QualifiedTable::from(table("fct")), QualifiedTable::from(table("dim")));

    // Resolve rewrites the join's name to `fct` - colliding with the FROM table.
    let error = plan
        .resolve_tables(|path| {
            Ok::<_, core::convert::Infallible>(if path.name().as_str() == "dim" {
                QualifiedTable::from(table("fct"))
            } else {
                path.clone()
            })
        })
        .expect_err("a resolve that collides two tables is refused");

    assert!(
        matches!(&error, super::ResolveTablesError::Ambiguous(ambiguous) if ambiguous.alias().as_str() == "fct"),
        "{error}",
    );
}

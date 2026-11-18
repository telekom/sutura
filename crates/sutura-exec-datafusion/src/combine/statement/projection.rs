//! Admit the final source text after every dialect and text-policy rewrite.

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use datafusion::arrow::datatypes::Schema;
use datafusion::common::{plan_datafusion_err, plan_err};
use datafusion::error::{DataFusionError, Result};
use datafusion::sql::sqlparser::ast::{Expr, Select, SelectItem, Statement, Visit as _, Visitor};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{StatementLegs, StatementStopped};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::RawRows;

pub(super) fn duplicate_names(schema: &Schema) -> bool {
    let mut names = BTreeSet::new();
    schema.fields().iter().any(|field| !names.insert(field.name()))
}

pub(super) fn output(schema: &Schema, rows: RawRows) -> Result<RawRows> {
    if !duplicate_names(schema) {
        return Ok(rows);
    }
    let (columns, rows) = rows.into_parts();
    if columns != (0..schema.fields().len()).map(super::label).collect::<Vec<_>>() {
        return plan_err!("a source answered other columns than its labelled projection");
    }
    Ok(RawRows::of(
        schema.fields().iter().map(|field| field.name().clone()).collect(),
        rows,
    ))
}

type Opened<S> = Result<(), StatementStopped<S>>;

pub(super) fn open<'a, L>(
    legs: &mut L,
    sources: &BTreeSet<SourceName>,
    statements: impl IntoIterator<Item = &'a RawStatement>,
) -> Opened<L::Stop>
where
    L: StatementLegs,
{
    for statement in statements {
        checked(statement.as_str()).map_err(|cause| super::unplanned("the final source projection was not admitted", &cause))?;
    }
    legs.open(sources).map_err(StatementStopped::Leg)
}

fn checked(sql: &str) -> Result<()> {
    let statements = Parser::parse_sql(&GenericDialect, sql)?;
    if !matches!(statements.as_slice(), [Statement::Query(_)]) {
        return plan_err!("source projection guard requires one query");
    }
    if let ControlFlow::Break(cause) = statements.visit(&mut Explicit) {
        return Err(cause);
    }
    Ok(())
}

struct Explicit;

impl Visitor for Explicit {
    type Break = DataFusionError;

    fn pre_visit_expr(&mut self, expression: &Expr) -> ControlFlow<Self::Break> {
        if matches!(expression, Expr::Wildcard(_) | Expr::QualifiedWildcard(_, _)) {
            return ControlFlow::Break(plan_datafusion_err!("source projection guard refused a wildcard expression"));
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        if select
            .projection
            .iter()
            .any(|item| matches!(item, SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)))
        {
            return ControlFlow::Break(plan_datafusion_err!("source projection guard refused a wildcard projection"));
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::checked;

    #[test]
    fn the_final_source_guard_rejects_nested_projection_wildcards_only() {
        for sql in [
            "SELECT * FROM items",
            "SELECT i.*, i.id FROM items i",
            "SELECT id FROM (SELECT * FROM items) i",
            "WITH i AS (SELECT items.* FROM items) SELECT id FROM i",
            "SELECT id FROM items UNION ALL SELECT * FROM other_items",
            "SELECT id FROM items WHERE EXISTS (SELECT * FROM other_items)",
            "SELECT (SELECT * FROM other_items) FROM items",
        ] {
            let cause = checked(sql).expect_err("no wildcard source projection is admitted");
            assert!(cause.to_string().contains("source projection guard"), "{sql}: {cause}");
        }
        for sql in [
            "SELECT id FROM items; SELECT id FROM other_items",
            "SELECT FROM",
            "SELECT (i.*) FROM items i",
        ] {
            assert!(checked(sql).is_err(), "{sql}");
        }
        for sql in [
            "SELECT count(*) FROM items",
            "SELECT id * 2, '*' AS marker FROM items",
            "SELECT i.\"*\" FROM items i",
            "SELECT count(*) FROM (SELECT id FROM items) i",
            "SELECT id /* * projection comment */ FROM items -- *",
            "SELECT count(i.*) FROM items i",
        ] {
            assert!(checked(sql).is_ok(), "{sql}");
        }
    }

    #[test]
    fn every_final_statement_is_checked_before_any_source_opens() {
        use std::collections::BTreeSet;
        use sutura_domain::model::SourceName;
        use sutura_domain::plan::{StatementLegs, StatementStopped};
        use sutura_domain::raw::RawStatement;
        use sutura_domain::warehouse::RawRows;

        #[derive(Default)]
        struct Legs {
            opened: usize,
            ran: usize,
        }
        impl StatementLegs for Legs {
            type Stop = ();
            fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), ()> {
                self.opened += 1;
                Ok(())
            }
            fn run(&mut self, _: &SourceName, _: &RawStatement) -> Result<RawRows, ()> {
                self.ran += 1;
                Ok(RawRows::of(Vec::new(), Vec::new()))
            }
        }
        let sources = BTreeSet::from([SourceName::parse("source").unwrap()]);
        for sqls in [
            vec!["SELECT * FROM items"],
            vec!["SELECT id FROM items", "SELECT i.* FROM items i"],
        ] {
            let statements: Vec<_> = sqls.into_iter().map(|sql| RawStatement::parse(sql).unwrap()).collect();
            let mut legs = Legs::default();
            let result = super::open(&mut legs, &sources, &statements);
            assert!(matches!(result, Err(StatementStopped::NotPlanned)), "{result:?}");
            assert_eq!(legs.opened, 0);
            assert_eq!(legs.ran, 0);
        }
        let mut legs = Legs::default();
        super::open(
            &mut legs,
            &sources,
            &[RawStatement::parse("SELECT count(*) FROM items").unwrap()],
        )
        .unwrap();
        assert_eq!(legs.opened, 1);
        assert_eq!(legs.ran, 0);
    }
}

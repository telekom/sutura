//! Admit the final source text after every dialect and text-policy rewrite.

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use datafusion::arrow::datatypes::Schema;
use datafusion::sql::sqlparser::ast::{Expr, Select, SelectItem, Statement, Visit as _, Visitor};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::{Parser, ParserError};
use sutura_domain::catalog::PrefilterIneligible;
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::{PolicyRead, StatementLegs, StatementStopped};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::RawRows;
use sutura_domain::warehouse::schema::TypeDialect;

/// Why the final source text, or the rows a source answered to it, were not admitted.
#[derive(Debug, thiserror::Error)]
pub(super) enum Inadmissible {
    #[error("the source text did not parse: {0}")]
    Unparsed(#[from] ParserError),
    #[error("the source text is not one query")]
    NotOneQuery,
    #[error("a select projects a wildcard")]
    WildcardProjection,
    #[error("an expression is a wildcard")]
    WildcardExpression,
    #[error("a source answered other columns than its labelled projection")]
    WrongLabels,
    #[error("a statement that was not admitted reached a source")]
    Unadmitted,
    #[error("a source answered {answered} columns for a schema of {expected}")]
    WidthMismatch { answered: usize, expected: usize },
}

pub(super) fn duplicate_names(schema: &Schema) -> bool {
    let mut names = BTreeSet::new();
    schema.fields().iter().any(|field| !names.insert(field.name()))
}

pub(super) fn output(schema: &Schema, rows: RawRows) -> Result<RawRows, Inadmissible> {
    let expected = schema.fields().len();
    let answered = rows.columns().len();
    if answered != expected {
        return Err(Inadmissible::WidthMismatch { answered, expected });
    }
    let labelled = rows
        .columns()
        .iter()
        .enumerate()
        .all(|(index, column)| *column == super::label(index));
    if !labelled {
        return if duplicate_names(schema) {
            Err(Inadmissible::WrongLabels)
        } else {
            Ok(rows)
        };
    }
    let (_labels, rows) = rows.into_parts();
    Ok(RawRows::of(
        schema.fields().iter().map(|field| field.name().clone()).collect(),
        rows,
    ))
}

type Stopped<T, S> = Result<T, StatementStopped<S>>;

/// The only way to reach the legs: a source opens after every final statement is admitted, and runs
/// only a statement that was. Limit: admission reads the text, not what the source does with it.
pub(super) struct Guarded<'l, L> {
    legs: &'l mut L,
    admitted: BTreeSet<String>,
}

impl<'l, L> Guarded<'l, L>
where
    L: StatementLegs,
{
    pub(super) const fn new(legs: &'l mut L) -> Self {
        Self {
            legs,
            admitted: BTreeSet::new(),
        }
    }

    pub(super) fn open<'a>(
        &mut self,
        sources: &BTreeSet<SourceName>,
        statements: impl IntoIterator<Item = (&'a RawStatement, TypeDialect)>,
    ) -> Stopped<(), L::Stop> {
        for (statement, dialect) in statements {
            checked(statement.as_str(), dialect)
                .map_err(|cause| super::unplanned("the final source projection was not admitted", &cause))?;
            self.admitted.insert(statement.as_str().to_owned());
        }
        self.legs.open(sources).map_err(StatementStopped::Leg)
    }

    pub(super) fn run_planned(
        &mut self,
        source: &SourceName,
        statement: &RawStatement,
        tables: &BTreeSet<QualifiedTable>,
        policy: PolicyRead,
        ineligible: Option<PrefilterIneligible>,
    ) -> Stopped<RawRows, L::Stop> {
        if !self.admitted.contains(statement.as_str()) {
            return Err(super::unplanned(
                "a source statement ran unadmitted",
                &Inadmissible::Unadmitted,
            ));
        }
        self.legs
            .run_planned(source, statement, tables, policy, ineligible)
            .map_err(StatementStopped::Leg)
    }
}

/// Read with the source's backslash escapes: a `BigQuery` or `ClickHouse` literal escapes its
/// backslash and quote. Every other source reads a literal as `GenericDialect` does, whose grammar
/// also takes what the planner writes there, such as an aggregate `FILTER` for Oracle.
fn checked(sql: &str, dialect: TypeDialect) -> Result<(), Inadmissible> {
    let reader = match dialect {
        TypeDialect::BigQuery | TypeDialect::ClickHouse => super::literal::reader(dialect),
        TypeDialect::Postgres
        | TypeDialect::Cube
        | TypeDialect::DuckDb
        | TypeDialect::Oracle
        | TypeDialect::DataFusion
        | TypeDialect::Foundry => &GenericDialect {},
    };
    let statements = Parser::parse_sql(reader, sql)?;
    if !matches!(statements.as_slice(), [Statement::Query(_)]) {
        return Err(Inadmissible::NotOneQuery);
    }
    match statements.visit(&mut Explicit) {
        ControlFlow::Break(cause) => Err(cause),
        ControlFlow::Continue(()) => Ok(()),
    }
}

struct Explicit;

impl Visitor for Explicit {
    type Break = Inadmissible;

    fn pre_visit_expr(&mut self, expression: &Expr) -> ControlFlow<Self::Break> {
        if matches!(expression, Expr::Wildcard(_) | Expr::QualifiedWildcard(_, _)) {
            return ControlFlow::Break(Inadmissible::WildcardExpression);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        if select
            .projection
            .iter()
            .any(|item| matches!(item, SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)))
        {
            return ControlFlow::Break(Inadmissible::WildcardProjection);
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use sutura_domain::model::SourceName;
    use sutura_domain::plan::{PolicyRead, StatementLegs, StatementStopped};
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::RawRows;
    use sutura_domain::warehouse::schema::TypeDialect;

    use std::ops::ControlFlow;

    use datafusion::sql::sqlparser::ast::Expr;

    use super::{Explicit, Guarded, Inadmissible, checked, output};

    fn schema(names: &[&str]) -> Schema {
        Schema::new(
            names
                .iter()
                .map(|name| Field::new(*name, DataType::Int64, false))
                .collect::<Vec<_>>(),
        )
    }

    fn answered(columns: &[&str]) -> RawRows {
        RawRows::of(columns.iter().map(|name| (*name).to_owned()).collect(), Vec::new())
    }

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
            let cause = checked(sql, TypeDialect::DataFusion).expect_err("no wildcard source projection is admitted");
            assert!(matches!(cause, Inadmissible::WildcardProjection), "{sql}: {cause}");
        }
        assert!(matches!(
            checked("SELECT id FROM items; SELECT id FROM other_items", TypeDialect::DataFusion),
            Err(Inadmissible::NotOneQuery)
        ));
        assert!(matches!(
            checked("SELECT FROM", TypeDialect::DataFusion),
            Err(Inadmissible::Unparsed(_))
        ));
        assert!(checked("SELECT (i.*) FROM items i", TypeDialect::DataFusion).is_err());
        for sql in [
            "SELECT count(*) FROM items",
            "SELECT id * 2, '*' AS marker FROM items",
            "SELECT i.\"*\" FROM items i",
            "SELECT count(*) FROM (SELECT id FROM items) i",
            "SELECT id /* * projection comment */ FROM items -- *",
            "SELECT count(i.*) FROM items i",
        ] {
            assert!(checked(sql, TypeDialect::DataFusion).is_ok(), "{sql}");
        }
    }

    #[test]
    fn a_wildcard_expression_is_refused_as_an_expression() {
        use datafusion::sql::sqlparser::ast::Visitor as _;
        use datafusion::sql::sqlparser::ast::helpers::attached_token::AttachedToken;
        let wildcard = Expr::Wildcard(AttachedToken::empty());
        assert!(matches!(
            Explicit.pre_visit_expr(&wildcard),
            ControlFlow::Break(Inadmissible::WildcardExpression)
        ));
    }

    #[test]
    fn an_answer_of_the_wrong_labels_is_refused_by_variant() {
        let duplicated = schema(&["id", "id"]);
        for columns in [["id", "id"], ["c1", "c0"], ["c0", "other"]] {
            let cause = output(&duplicated, answered(&columns)).expect_err("only c0, c1 label a duplicated schema");
            assert!(matches!(cause, Inadmissible::WrongLabels), "{columns:?}: {cause}");
        }
        let rows = output(&duplicated, answered(&["c0", "c1"])).expect("the labels restore the names");
        assert_eq!(rows.columns(), ["id", "id"]);
    }

    #[test]
    fn an_answer_of_another_width_is_refused_by_variant() {
        for (names, columns) in [
            (vec!["id", "name"], vec!["id"]),
            (vec!["id"], vec!["id", "name"]),
            (vec!["id", "id"], vec!["c0"]),
        ] {
            let cause = output(&schema(&names), answered(&columns)).expect_err("a width off the schema is refused");
            assert!(
                matches!(cause, Inadmissible::WidthMismatch { answered: got, expected: want } if got == columns.len() && want == names.len()),
                "{names:?} {columns:?}: {cause}"
            );
        }
        let rows = output(&schema(&["id", "name"]), answered(&["id", "name"])).expect("a unique schema of its own width");
        assert_eq!(rows.columns(), ["id", "name"]);
    }

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

    fn source() -> SourceName {
        SourceName::parse("source").unwrap()
    }

    #[test]
    fn every_final_statement_is_checked_before_any_source_opens() {
        let sources = BTreeSet::from([source()]);
        for sqls in [
            vec!["SELECT * FROM items"],
            vec!["SELECT id FROM items", "SELECT i.* FROM items i"],
        ] {
            let statements: Vec<_> = sqls.into_iter().map(|sql| RawStatement::parse(sql).unwrap()).collect();
            let mut legs = Legs::default();
            let result = Guarded::new(&mut legs).open(
                &sources,
                statements.iter().map(|statement| (statement, TypeDialect::DataFusion)),
            );
            assert!(matches!(result, Err(StatementStopped::NotPlanned)), "{result:?}");
            assert_eq!(legs.opened, 0);
            assert_eq!(legs.ran, 0);
        }
        let mut legs = Legs::default();
        let admitted = RawStatement::parse("SELECT count(*) FROM items").unwrap();
        let mut guarded = Guarded::new(&mut legs);
        guarded.open(&sources, [(&admitted, TypeDialect::DataFusion)]).unwrap();
        guarded
            .run_planned(&source(), &admitted, &BTreeSet::new(), PolicyRead::Unread, None)
            .unwrap();
        assert_eq!(legs.opened, 1);
        assert_eq!(legs.ran, 1);
    }

    #[test]
    fn a_statement_that_was_not_admitted_never_reaches_a_source() {
        let sources = BTreeSet::from([source()]);
        let admitted = RawStatement::parse("SELECT id FROM items").unwrap();
        let other = RawStatement::parse("SELECT id FROM other_items").unwrap();
        let mut legs = Legs::default();
        let mut guarded = Guarded::new(&mut legs);
        let before_open = guarded.run_planned(&source(), &admitted, &BTreeSet::new(), PolicyRead::Unread, None);
        assert!(matches!(before_open, Err(StatementStopped::NotPlanned)), "{before_open:?}");
        guarded.open(&sources, [(&admitted, TypeDialect::DataFusion)]).unwrap();
        let other_text = guarded.run_planned(&source(), &other, &BTreeSet::new(), PolicyRead::Unread, None);
        assert!(matches!(other_text, Err(StatementStopped::NotPlanned)), "{other_text:?}");
        assert_eq!(legs.ran, 0);
    }
}

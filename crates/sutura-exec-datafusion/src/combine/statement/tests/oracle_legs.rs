//! An Oracle leg the shared fake engine can execute, apart from `tests.rs` so its `open`/`run`
//! are not read as edits to that file's own fixtures.

use super::*;

/// `statement` with every `FETCH ... ROWS ONLY` (any nesting depth, including inside a derived
/// table) rewritten to `LIMIT`/`OFFSET`: the fake engine behind [`StatementLegs`] is `DataFusion`'s
/// own SQL frontend, which does not implement `FETCH` - only a live Oracle venue executes Oracle's
/// own `FETCH FIRST` spelling. `None` if a clause the rewrite can't carry over losslessly
/// (`WITH TIES`/`PERCENT`, or a limit already set alongside the fetch) is found.
pub(super) fn oracle_fetch_to_limit(statement: &str) -> Option<String> {
    use datafusion::sql::sqlparser::ast::{LimitClause, Query, VisitMut as _, VisitorMut};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;
    use std::ops::ControlFlow;

    struct OracleLimits;

    impl VisitorMut for OracleLimits {
        type Break = ();

        fn post_visit_query(&mut self, query: &mut Query) -> ControlFlow<Self::Break> {
            let Some(fetch) = query.fetch.take() else {
                return ControlFlow::Continue(());
            };
            if fetch.with_ties || fetch.percent {
                return ControlFlow::Break(());
            }
            let Some(quantity) = fetch.quantity else {
                return ControlFlow::Break(());
            };
            let offset = match query.limit_clause.take() {
                None => None,
                Some(LimitClause::LimitOffset {
                    limit: None,
                    offset,
                    limit_by,
                }) if limit_by.is_empty() => offset,
                Some(_) => return ControlFlow::Break(()),
            };
            query.limit_clause = Some(LimitClause::LimitOffset {
                limit: Some(quantity),
                offset,
                limit_by: Vec::new(),
            });
            ControlFlow::Continue(())
        }
    }

    let mut parsed = Parser::parse_sql(&GenericDialect, statement).ok()?;
    let [sql] = parsed.as_mut_slice() else {
        return None;
    };
    if sql.visit(&mut OracleLimits).is_break() {
        return None;
    }
    Some(sql.to_string())
}

/// An Oracle leg, executed by the shared fake engine: generated `FETCH` is normalized back to
/// `LIMIT` via [`oracle_fetch_to_limit`] before running, so an Oracle dialect cell can be executed,
/// not just captured as text. Private, like every other test fixture here: visible to `tests` and
/// every one of its descendant modules, which is every module that uses it.
pub(super) struct OracleLegs {
    pub(super) engines: Engines,
    pub(super) statements: Vec<String>,
}

impl StatementLegs for OracleLegs {
    type Stop = SourceName;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        self.engines.open(sources)
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        if !explicit_source_projection(statement.as_str()) {
            return Err(source.clone());
        }
        if *source == named("geo") {
            self.statements.push(String::from(statement.as_str()));
            eprintln!("source SQL: {}", statement.as_str());
            // Only the fake normalizes FETCH; the live venue executes Oracle spelling.
            let Some(normalized) = oracle_fetch_to_limit(statement.as_str()) else {
                return Err(source.clone());
            };
            let statement = RawStatement::parse(normalized).map_err(|_cause| source.clone())?;
            return self.engines.run(source, &statement);
        }
        self.engines.run(source, statement)
    }
}

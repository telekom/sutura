//! The planner's Oracle leg, executed by a fake source engine; actual Oracle belongs to its venue.

use std::ops::ControlFlow;

use datafusion::sql::sqlparser::ast::{LimitClause, Query, VisitMut as _, VisitorMut};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;

use super::*;

struct OracleLegs {
    engines: Engines,
    statements: Vec<String>,
}

impl StatementLegs for OracleLegs {
    type Stop = SourceName;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        self.engines.open(sources)
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        if *source == named("geo") {
            self.statements.push(String::from(statement.as_str()));
            // ponytail: only the fake normalizes FETCH; the live venue executes Oracle spelling.
            let mut parsed = Parser::parse_sql(&GenericDialect, statement.as_str()).map_err(|_cause| source.clone())?;
            let [sql] = parsed.as_mut_slice() else {
                return Err(source.clone());
            };
            if sql.visit(&mut OracleLimits).is_break() {
                return Err(source.clone());
            }
            let statement = RawStatement::parse(sql.to_string()).map_err(|_cause| source.clone())?;
            return self.engines.run(source, &statement);
        }
        self.engines.run(source, statement)
    }
}

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

#[test]
fn an_oracle_part_preserves_quoted_aliases_and_fetch_limits_alone_and_in_a_federation() {
    let tables = vec![
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_customer").expect("a fixture table"),
            TypeDialect::DuckDb,
            customers().schema(),
        ),
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").expect("a fixture table"),
            TypeDialect::Oracle,
            regions().schema(),
        ),
    ];
    for sql in ["select r.region from dim_region r order by r.region limit 1 offset 1", AREAS] {
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        let statement = RawStatement::parse(sql).expect("a bounded fixture query");
        let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        let Some(Ok(rows)) = answer else {
            panic!("the Oracle part must run: {answer:?}");
        };
        assert_eq!(rows, legs.engines.as_one(sql), "Oracle SQL: {:?}", legs.statements);
        assert_eq!(legs.statements.len(), 1);
        let text = &legs.statements[0];
        assert!(!text.contains("LIMIT"), "{text}");
        assert!(!text.contains("AS \"r\""), "a table alias has no AS: {text}");
        if sql.contains("offset") {
            assert!(text.contains("OFFSET 1 ROWS FETCH FIRST 1 ROWS ONLY"), "{text}");
        } else {
            assert!(
                text.contains("AS \"c0\""),
                "the local combine needs the quoted column label: {text}"
            );
        }
    }
}

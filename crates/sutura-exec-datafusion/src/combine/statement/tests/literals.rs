//! A string literal reaches its source as the value the caller wrote, in every dialect.

use datafusion::sql::sqlparser::dialect::{
    BigQueryDialect, ClickHouseDialect, Dialect, DuckDbDialect, GenericDialect, OracleDialect, PostgreSqlDialect,
};
use datafusion::sql::sqlparser::tokenizer::{Token, Tokenizer};

use super::*;

/// What a source tokenizes `sql`'s string literals to, in `sqlparser`'s model of `dialect`.
pub(in crate::combine::statement) fn read_back(sql: &str, dialect: TypeDialect) -> Vec<String> {
    let reader: &dyn Dialect = match dialect {
        TypeDialect::BigQuery => &BigQueryDialect {},
        TypeDialect::ClickHouse => &ClickHouseDialect {},
        TypeDialect::Postgres | TypeDialect::Cube => &PostgreSqlDialect {},
        TypeDialect::DuckDb => &DuckDbDialect {},
        TypeDialect::Oracle => &OracleDialect {},
        TypeDialect::DataFusion => &GenericDialect {},
    };
    let mut values: Vec<String> = Tokenizer::new(reader, sql)
        .tokenize()
        .unwrap_or_else(|cause| panic!("{dialect:?} tokenizes {sql}: {cause}"))
        .into_iter()
        .filter_map(|token| match token {
            Token::SingleQuotedString(value) => Some(value),
            _ => None,
        })
        .collect();
    values.sort();
    values
}

pub(in crate::combine::statement) const DIALECTS: [TypeDialect; 7] = [
    TypeDialect::BigQuery,
    TypeDialect::ClickHouse,
    TypeDialect::Postgres,
    TypeDialect::Cube,
    TypeDialect::DuckDb,
    TypeDialect::Oracle,
    TypeDialect::DataFusion,
];

/// A part whose leg is never run: what each source would have been sent.
struct Captured(Vec<String>);

impl StatementLegs for Captured {
    type Stop = SourceName;

    fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        Ok(())
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.0.push(String::from(statement.as_str()));
        Err(source.clone())
    }
}

fn regions_at(dialect: TypeDialect) -> [StatementTable; 1] {
    [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("dim_region").expect("a fixture table"),
        dialect,
        regions().schema(),
    )]
}

#[test]
fn a_backslash_literal_reaches_a_backslash_escaping_source_as_written() {
    let statement = RawStatement::parse(r"select region from dim_region where region = 'a\' or region = 'b\''c'")
        .expect("a bounded fixture query");
    for dialect in [TypeDialect::BigQuery, TypeDialect::ClickHouse] {
        let mut legs = Captured(Vec::new());
        let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &regions_at(dialect),
                false,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert!(
            matches!(answer, Some(Err(StatementStopped::Leg(_)))),
            "{dialect:?}: {answer:?}"
        );
        let [sql] = legs.0.as_slice() else {
            panic!("{dialect:?}: one statement reaches the source: {:?}", legs.0);
        };
        for written in [r"'a\\'", r"'b\\\'c'"] {
            assert!(sql.contains(written), "{dialect:?}: {written} is missing: {sql}");
        }
        assert_eq!(read_back(sql, dialect), [r"a\", r"b\'c"], "{dialect:?}: {sql}");
    }
}

#[test]
fn a_literal_holding_a_backslash_before_a_quote_answers_its_value_at_a_quote_doubling_source() {
    let statement = r"select region, 'a\' as tail, 'b\''c' as middle from dim_region where region = 'north'";
    for dialect in [TypeDialect::DuckDb, TypeDialect::DataFusion, TypeDialect::Postgres] {
        let mut legs = Engines::new(None);
        let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
            StatementQuestion::new(
                &RawStatement::parse(statement).expect("a bounded fixture query"),
                &regions_at(dialect),
                false,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        let Some(Ok(rows)) = answer else {
            panic!("{dialect:?}: the statement is answered at its source: {answer:?}");
        };
        assert_eq!(rows, legs.as_one(statement), "{dialect:?}");
        assert_eq!(legs.ran, [named("geo")], "{dialect:?}");
    }
}

//! Planned SQL has two independent instruments: source spelling, and rows from the real registry.
//! The recording leg below only observes rendering; it is never counted as engine execution.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema, TimeUnit};
use sutura_domain::model::{ModelName, QualifiedTable, SourceName};
use sutura_domain::plan::{FederationCombiner as _, StatementLegs, StatementQuestion, StatementTable};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content};
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{RawRows, RowSet, Value};
use sutura_exec_datafusion::DataFusionCombiner;

use crate::adapters::{DataSystemUnderTest, data_root, deadline, load_on, open, presented, runs_here, source};
use crate::shared::settings;

#[path = "statements/corpus.rs"]
mod corpus;

use corpus::{CASES, Expected, FixtureRow};

const WORKING_SET: u64 = 1 << 30;

/// A renderer instrument: a successful whole-source plan hands its SQL to this port once.
#[derive(Default)]
struct Recorded {
    statements: Vec<String>,
}

impl StatementLegs for Recorded {
    type Stop = ();

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        if sources == &BTreeSet::from([source()]) {
            Ok(())
        } else {
            Err(())
        }
    }

    fn run(&mut self, _source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.statements.push(statement.as_str().to_owned());
        Ok(RawRows::of(Vec::new(), Vec::new()))
    }
}

fn table(path: QualifiedTable, dialect: TypeDialect) -> StatementTable {
    let date = match dialect {
        TypeDialect::Oracle => DataType::Timestamp(TimeUnit::Second, None),
        TypeDialect::Cube => DataType::Timestamp(TimeUnit::Microsecond, None),
        TypeDialect::DuckDb
        | TypeDialect::Postgres
        | TypeDialect::DataFusion
        | TypeDialect::ClickHouse
        | TypeDialect::BigQuery => DataType::Date32,
    };
    let key = if dialect == TypeDialect::Cube {
        DataType::Float64
    } else {
        DataType::Int64
    };
    StatementTable::new(
        source(),
        path,
        dialect,
        // Nullable declarations keep IS NOT NULL and COUNT(column) distinct from their simplified forms.
        Arc::new(Schema::new(vec![
            Field::new("usage_date", date, true),
            Field::new("subscription_key", key, true),
            Field::new("data_gb", DataType::Float64, true),
            Field::new("voice_min", DataType::Float64, true),
        ])),
    )
}

/// Caller SQL quotes each physical path component, including a cloud project containing a hyphen.
fn quoted(path: &QualifiedTable) -> String {
    path.to_string()
        .split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn pins_planned_source_statements(name: &str, dialect: TypeDialect) {
    let path = QualifiedTable::parse("fct_usage_daily").expect("a fixture table");
    let tables = [table(path, dialect)];
    let combiner = DataFusionCombiner::new().expect("a statement combiner starts");
    let mut corpus = String::new();
    for case in CASES {
        let statement = RawStatement::parse(case.sql("\"fct_usage_daily\"")).expect("a corpus query");
        let mut recorded = Recorded::default();
        let answer = combiner.answer_statement(
            StatementQuestion::new(&statement, &tables, false, WORKING_SET, deadline()),
            &mut recorded,
        );
        assert!(matches!(answer, Some(Ok(_))), "{} at {dialect:?}: {answer:?}", case.name);
        let [sql] = recorded.statements.as_slice() else {
            panic!("{} must produce one source statement: {:?}", case.name, recorded.statements);
        };
        corpus.push_str(case.name);
        corpus.push('\n');
        corpus.push_str(sql);
        corpus.push_str("\n\n");
    }
    settings(name).bind(|| insta::assert_snapshot!("planned_statement_shapes", corpus));
}

macro_rules! dialect {
    ($name:ident, $dialect:expr) => {
        mod $name {
            pub(super) const DIALECT: sutura_domain::warehouse::schema::TypeDialect = $dialect;

            #[test]
            fn pins_planned_source_statements() {
                super::pins_planned_source_statements(stringify!($name), DIALECT);
            }
        }
    };
}
crate::adapters::registered!(statement_dialects: dialect);

/// A native engine leg. This calls the adapter's statement port, with its real venue and bounds.
struct Executing<'a, W> {
    warehouse: &'a W,
    table: &'a QualifiedTable,
}

impl<W: DataSystemUnderTest> StatementLegs for Executing<'_, W> {
    type Stop = String;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        if sources == &BTreeSet::from([self.warehouse.source().clone()]) {
            Ok(())
        } else {
            Err(String::from("the native leg opened an unexpected source set"))
        }
    }

    fn run(&mut self, _source: &SourceName, _statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        Err(String::from(
            "the native matrix must enter through the planned-statement port",
        ))
    }

    fn run_planned(
        &mut self,
        source: &SourceName,
        statement: &RawStatement,
        tables: &BTreeSet<QualifiedTable>,
    ) -> Result<RawRows, Self::Stop> {
        if source != self.warehouse.source() || tables != &BTreeSet::from([self.table.clone()]) {
            return Err(String::from("the native leg received an unexpected source or scan set"));
        }
        self.warehouse
            .execute_planned_statement(statement, tables, &presented(), deadline())
            .ok_or_else(|| String::from("a registered engine must execute planned SQL"))?
            .map_err(|cause| format!("{cause:?}"))
    }
}

fn fixture_rows() -> Vec<FixtureRow> {
    let text = std::fs::read_to_string(data_root().join("fct_usage_daily.csv")).expect("the reference CSV is readable");
    text.lines()
        .skip(1)
        .map(|line| {
            let fields = line.split(',').collect::<Vec<_>>();
            let [date, key, _data, voice] = fields.as_slice() else {
                panic!("a reference row has four unquoted fields: {line}");
            };
            FixtureRow {
                date: (*date).to_owned(),
                key: key.parse().expect("an integer key"),
                voice: voice.parse().expect("numeric voice minutes"),
            }
        })
        .collect()
}

fn expected(expected: Expected, fixture: &[FixtureRow]) -> Vec<Vec<Value>> {
    match expected {
        Expected::Keys(predicate) => fixture
            .iter()
            .filter(|row| predicate.accepts(row))
            .map(|row| vec![Value::Integer(row.key)])
            .collect(),
        Expected::Count(predicate) => vec![vec![Value::Integer(
            i64::try_from(fixture.iter().filter(|row| predicate.accepts(row)).count()).expect("a small corpus"),
        )]],
        Expected::Limited { wide } => {
            let matching = fixture.iter().filter(|row| row.key == 1002).collect::<Vec<_>>();
            assert!(matching.len() >= 2, "the no-order limit must actually cut matching rows");
            assert!(
                matching.iter().all(|row| row.voice == 0.0),
                "limited fixture rows have zero voice minutes"
            );
            let row = if wide {
                vec![Value::Integer(1002), Value::Integer(0)]
            } else {
                vec![Value::Integer(1002)]
            };
            vec![row; 2]
        }
        Expected::Grouped => {
            let mut counts = BTreeMap::<i64, i64>::new();
            for row in fixture {
                *counts.entry(row.key).or_default() += 1;
            }
            counts
                .into_iter()
                .map(|(key, count)| vec![Value::Integer(key), Value::Integer(count)])
                .collect()
        }
    }
}

/// Snapshots keep each typed row and its multiplicity, compacted only for repeated keys.
fn summarized(rows: &RowSet) -> String {
    let mut repeated = BTreeMap::<String, usize>::new();
    for row in rows.rows() {
        *repeated.entry(format!("{row:?}")).or_default() += 1;
    }
    let mut output = String::new();
    for (row, count) in repeated {
        writeln!(output, "  {count} x {row}").expect("a String accepts text");
    }
    output
}

fn every_planned_shape_answers_at_the_native_engine<W>(expected_dialect: TypeDialect)
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    if !runs_here::<W>() {
        return;
    }
    let pinned = load_on::<W>();
    let warehouse: W = open(&pinned);
    let dialect = warehouse
        .type_dialect()
        .expect("a registered engine must declare its SQL dialect");
    assert_eq!(
        dialect, expected_dialect,
        "the native engine and its renderer must share a dialect"
    );
    let model = pinned
        .definitions()
        .model(&ModelName::parse("daily_usage").expect("a fixture model"))
        .expect("the corpus declares daily usage");
    let path = quoted(model.table());
    let tables = [table(model.table().clone(), dialect)];
    let fixture = fixture_rows();
    let combiner = DataFusionCombiner::new().expect("a statement combiner starts");
    let mut output = String::new();
    for case in CASES {
        let statement = RawStatement::parse(case.sql(&path)).expect("a corpus query");
        let answer = combiner.answer_statement(
            StatementQuestion::new(&statement, &tables, false, WORKING_SET, deadline()),
            &mut Executing {
                warehouse: &warehouse,
                table: model.table(),
            },
        );
        let Some(Ok(rows)) = answer else {
            panic!("{} at {}: {answer:?}", case.name, W::NAME);
        };
        let wanted = RowSet::new(case.expected.columns(), expected(case.expected, &fixture)).expect("rectangular fixture rows");
        let (columns, values) = rows.into_parts();
        let rows = RowSet::new(columns, values).expect("rectangular source rows");
        agree_on_content(&rows, &wanted, RealTolerance::DIFFERENTIAL)
            .unwrap_or_else(|cause| panic!("{} at {}: {cause}", case.name, W::NAME));
        writeln!(output, "{}: {} rows", case.name, rows.rows().len()).expect("a String accepts text");
        writeln!(output, "  columns: {:?}", rows.columns()).expect("a String accepts text");
        output.push_str(&summarized(&rows));
    }
    settings(W::NAME).bind(|| insta::assert_snapshot!("planned_statement_rows", output));
}

macro_rules! data_system {
    ($name:ident, $warehouse:ty) => {
        mod $name {
            #[test]
            fn every_planned_shape_answers_at_the_native_engine() {
                super::every_planned_shape_answers_at_the_native_engine::<$warehouse>(super::super::$name::DIALECT);
            }
        }
    };
}

// The dialect and system axes use the same names, so keep their registry expansions apart.
mod engines {
    use super::every_planned_shape_answers_at_the_native_engine;
    crate::adapters::registered!(data_systems: data_system);
}

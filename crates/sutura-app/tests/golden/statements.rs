//! Planned SQL has two independent instruments: source spelling, and rows from the real registry.
//! The recording leg below only observes rendering; it is never counted as engine execution.
//! Cube joined cases execute both inputs at Cube and combine locally; other joined cases run at their source.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema, TimeUnit};
use sutura_domain::model::{ModelName, QualifiedTable, SourceName};
use sutura_domain::plan::{FederationCombiner as _, StatementLegs, StatementQuestion, StatementTable};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{RawRows, RowSet, Value};
use sutura_exec_datafusion::DataFusionCombiner;

use crate::adapters::{DataSystemUnderTest, data_root, deadline, load_on, open, presented, runs_here, source};
use crate::shared::settings;

#[path = "statements/corpus.rs"]
mod corpus;
#[path = "statements/text.rs"]
mod text;

use corpus::{CASES, Expected, FixtureRow, JoinRows, SubscriptionRow};

const WORKING_SET: u64 = 1 << 30;

/// A renderer instrument: records source parts and returns empty, position-labelled rows.
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
        let parsed = polyglot_sql::parse_one(statement.as_str(), polyglot_sql::DialectType::Generic).map_err(|_cause| ())?;
        let polyglot_sql::expressions::Expression::Select(select) = parsed else {
            return Err(());
        };
        let columns = (0..select.expressions.len()).map(|index| format!("c{index}")).collect();
        Ok(RawRows::of(columns, Vec::new()))
    }
}

fn tables(usage: QualifiedTable, subscriptions: QualifiedTable, dialect: TypeDialect) -> [StatementTable; 2] {
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
    let usage = StatementTable::new(
        source(),
        usage,
        dialect,
        // Nullable declarations keep IS NOT NULL and COUNT(column) distinct from their simplified forms.
        Arc::new(Schema::new(vec![
            Field::new("usage_date", date.clone(), true),
            Field::new("subscription_key", key.clone(), true),
            Field::new("data_gb", DataType::Float64, true),
            Field::new("voice_min", DataType::Float64, true),
        ])),
    );
    let subscriptions = StatementTable::new(
        source(),
        subscriptions,
        dialect,
        Arc::new(Schema::new(vec![
            Field::new("month", date, true),
            Field::new("subscription_key", key, true),
        ])),
    );
    [usage, subscriptions]
}

/// Caller SQL quotes each physical path component, including a cloud project containing a hyphen.
fn quoted(path: &QualifiedTable) -> String {
    path.to_string()
        .split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn pins_source_corpus(name: &str, dialect: TypeDialect) {
    let path = QualifiedTable::parse("fct_usage_daily").expect("a fixture table");
    let tables = tables(
        path,
        QualifiedTable::parse("fct_subscription_monthly").expect("a fixture table"),
        dialect,
    );
    pins_planned_statements_over(name, dialect, &tables);
}

fn pins_planned_statements_over(name: &str, dialect: TypeDialect, tables: &[StatementTable]) {
    let _logging = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .finish(),
    );
    let combiner = DataFusionCombiner::new().expect("a statement combiner starts");
    let mut corpus = String::new();
    for case in CASES {
        let statement =
            RawStatement::parse(case.sql("\"fct_usage_daily\"", "\"fct_subscription_monthly\"")).expect("a corpus query");
        let mut recorded = Recorded::default();
        let answer = combiner.answer_statement(
            StatementQuestion::new(&statement, tables, false, WORKING_SET, deadline()),
            &mut recorded,
        );
        assert!(matches!(answer, Some(Ok(_))), "{} at {dialect:?}: {answer:?}", case.name);
        assert!(
            !recorded.statements.is_empty(),
            "{} must produce source statements",
            case.name
        );
        corpus.push_str(case.name);
        corpus.push('\n');
        corpus.push_str(&recorded.statements.join("\n"));
        corpus.push_str("\n\n");
    }
    settings(name).bind(|| insta::assert_snapshot!("planned_statement_shapes", corpus));
}

fn pins_pushdown_with_an_unread_mapped_number(name: &str, dialect: TypeDialect) {
    let path = QualifiedTable::parse("fct_usage_daily").expect("a fixture table");
    let mut tables = tables(
        path.clone(),
        QualifiedTable::parse("fct_subscription_monthly").expect("a fixture table"),
        dialect,
    );
    let admitted = &tables[0];
    let mut fields = admitted.schema().fields().iter().cloned().collect::<Vec<_>>();
    fields.push(Arc::new(Field::new("unread_number", DataType::Utf8, true).with_metadata(
        std::collections::HashMap::from([(String::from("sutura:unbounded_number"), String::from("utf8"))]),
    )));
    tables[0] = StatementTable::new(source(), path, dialect, Arc::new(Schema::new(fields)));
    pins_planned_statements_over(name, dialect, &tables);
}

macro_rules! dialect {
    ($name:ident, $dialect:expr) => {
        mod $name {
            pub(super) const DIALECT: sutura_domain::warehouse::schema::TypeDialect = $dialect;

            #[test]
            fn pins_bounded_redacted_text_sql() {
                super::text::pins(stringify!($name), DIALECT);
            }

            #[test]
            fn pins_planned_source_statements() {
                super::pins_source_corpus(stringify!($name), DIALECT);
            }

            #[test]
            fn an_unread_mapped_number_preserves_every_planned_source_golden() {
                super::pins_pushdown_with_an_unread_mapped_number(stringify!($name), DIALECT);
            }
        }
    };
}
crate::adapters::registered!(statement_dialects: dialect);

/// A native engine leg. This calls the adapter's statement port, with its real venue and bounds.
struct Executing<'a, W> {
    warehouse: &'a W,
    expected_scans: Vec<BTreeSet<QualifiedTable>>,
    observed_scans: Vec<BTreeSet<QualifiedTable>>,
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
        if source != self.warehouse.source() || !self.expected_scans.contains(tables) {
            return Err(String::from("the native leg received an unexpected source or scan set"));
        }
        self.observed_scans.push(tables.clone());
        self.warehouse
            .execute_planned_statement(statement, tables, &presented(), deadline())
            .ok_or_else(|| String::from("a registered engine must execute planned SQL"))?
            .map_err(|cause| format!("{cause:?}"))
    }
}

fn subscription_rows() -> Vec<SubscriptionRow> {
    let text = std::fs::read_to_string(data_root().join("fct_subscription_monthly.csv")).expect("the reference CSV is readable");
    text.lines()
        .skip(1)
        .map(|line| {
            let fields = line.split(',').collect::<Vec<_>>();
            let [month, key, _customer, _product, _status, _mrr, _churned, _term] = fields.as_slice() else {
                panic!("a subscription row has eight unquoted fields: {line}");
            };
            SubscriptionRow {
                month: (*month).to_owned(),
                key: key.parse().expect("an integer key"),
            }
        })
        .collect()
}

fn joined_rows(shape: JoinRows, usage: &[FixtureRow], subscriptions: &[SubscriptionRow]) -> Vec<Vec<Value>> {
    let mut rows = Vec::new();
    let input_limit = if matches!(shape, JoinRows::LimitedInput) {
        assert!(
            subscriptions.iter().filter(|row| row.key == 1003).count() > 2,
            "the input limit cuts repeated keys, whose projected values are identical"
        );
        2
    } else {
        usize::MAX
    };
    for left in usage.iter().filter(|row| (1001..=1002).contains(&row.key)) {
        let right_key = left.key + i64::from(matches!(shape, JoinRows::NestedWindow | JoinRows::LimitedInput));
        for right in subscriptions.iter().filter(|row| row.key == right_key).take(input_limit) {
            if matches!(
                shape,
                JoinRows::LeftFilter | JoinRows::OrderedWindow | JoinRows::NestedWindow | JoinRows::LimitedInput
            ) && left.voice != 0.0
                || matches!(shape, JoinRows::RightFilter) && right.month.as_str() < "2026-04-01"
            {
                continue;
            }
            let day: i64 = left
                .date
                .get(8..10)
                .expect("a fixture day field")
                .parse()
                .expect("a fixture day");
            let month: i64 = if matches!(shape, JoinRows::LimitedInput) {
                0
            } else {
                right
                    .month
                    .get(5..7)
                    .expect("a fixture month field")
                    .parse()
                    .expect("a fixture month")
            };
            rows.push((left.key, right.key, day, month));
        }
    }
    assert!(
        !rows.is_empty() && rows.len() < 200,
        "the unordered joined fixture fits below LIMIT"
    );
    if matches!(shape, JoinRows::OrderedWindow | JoinRows::NestedWindow) {
        rows.sort_by_key(|&(_left, _right, day, month)| (std::cmp::Reverse(day), month));
        if matches!(shape, JoinRows::NestedWindow) {
            rows = rows.into_iter().skip(1).take(8).collect();
            rows.sort_by_key(|&(_left, _right, day, month)| (std::cmp::Reverse(month), day));
            rows = rows.into_iter().skip(2).take(3).collect();
        } else {
            rows = rows.into_iter().skip(2).take(5).collect();
        }
    }
    rows.into_iter()
        .map(|(left, right, day, month)| {
            vec![
                Value::Integer(left),
                Value::Integer(right),
                Value::Integer(day),
                Value::Integer(month),
            ]
        })
        .collect()
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

fn expected(expected: Expected, fixture: &[FixtureRow], subscriptions: &[SubscriptionRow]) -> Vec<Vec<Value>> {
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
        Expected::Joined(shape) => joined_rows(shape, fixture, subscriptions),
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

fn runs_native_statement_corpus<W>(expected_dialect: TypeDialect)
where
    W: DataSystemUnderTest + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    if !runs_here::<W>() {
        return;
    }
    text::served::<W>(expected_dialect);
    let pinned = text::with_disjoint_policy(&load_on::<W>(), expected_dialect, false);
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
    text::executes(&warehouse, &pinned, dialect);
    let path = quoted(model.table());
    let subscriptions = pinned
        .definitions()
        .model(&ModelName::parse("subscriptions").expect("a fixture model"))
        .expect("the corpus declares subscriptions");
    let tables = tables(model.table().clone(), subscriptions.table().clone(), dialect);
    let admitted = tables.iter().map(|table| table.table().clone()).collect::<BTreeSet<_>>();
    let fixture = fixture_rows();
    let subscription_fixture = subscription_rows();
    let combiner = DataFusionCombiner::new().expect("a statement combiner starts");
    let mut output = String::new();
    for case in CASES {
        let statement = RawStatement::parse(case.sql(&path, &quoted(subscriptions.table()))).expect("a corpus query");
        let expected_scans = match case.expected {
            Expected::Joined(_) if dialect == TypeDialect::Cube => {
                admitted.iter().cloned().map(|table| BTreeSet::from([table])).collect()
            }
            Expected::Joined(_) => vec![admitted.clone()],
            _ => vec![BTreeSet::from([model.table().clone()])],
        };
        let mut executing = Executing {
            warehouse: &warehouse,
            expected_scans,
            observed_scans: Vec::new(),
        };
        let answer = combiner.answer_statement(
            StatementQuestion::new(&statement, &tables, false, WORKING_SET, deadline()),
            &mut executing,
        );
        let Some(Ok(rows)) = answer else {
            panic!("{} at {}: {answer:?}", case.name, W::NAME);
        };
        if matches!(case.expected, Expected::Joined(_)) {
            executing.observed_scans.sort_unstable();
            executing.expected_scans.sort_unstable();
            assert_eq!(
                executing.observed_scans,
                executing.expected_scans,
                "{} at {}: each expected native leg must run exactly once",
                case.name,
                W::NAME
            );
        }
        let wanted = RowSet::new(
            case.expected.columns(),
            expected(case.expected, &fixture, &subscription_fixture),
        )
        .expect("rectangular fixture rows");
        let (columns, values) = rows.into_parts();
        let rows = RowSet::new(columns, values).expect("rectangular source rows");
        agree_on_content(&rows, &wanted, RealTolerance::DIFFERENTIAL)
            .unwrap_or_else(|cause| panic!("{} at {}: {cause}", case.name, W::NAME));
        if matches!(
            case.expected,
            Expected::Joined(JoinRows::OrderedWindow | JoinRows::NestedWindow)
        ) {
            agree_on_order(&rows, &wanted, RealTolerance::DIFFERENTIAL)
                .unwrap_or_else(|cause| panic!("{} at {}: {cause}", case.name, W::NAME));
        }
        writeln!(
            output,
            "{}: {} rows; columns: {:?}",
            case.name,
            rows.rows().len(),
            rows.columns()
        )
        .expect("a String accepts text");
        output.push_str(&summarized(&rows));
    }
    settings(W::NAME).bind(|| insta::assert_snapshot!("planned_statement_rows", output));
}

macro_rules! data_system {
    (oracle, $warehouse:ty) => {
        mod oracle {
            #[test]
            fn every_planned_shape_answers_at_the_oracle_engine() {
                super::runs_native_statement_corpus::<$warehouse>(super::super::oracle::DIALECT);
            }
        }
    };
    (cube, $warehouse:ty) => {
        mod cube {
            #[test]
            fn every_planned_shape_answers_at_the_cube_engine() {
                super::runs_native_statement_corpus::<$warehouse>(super::super::cube::DIALECT);
            }
        }
    };
    (bigquery, $warehouse:ty) => {
        mod bigquery {
            #[test]
            fn every_planned_shape_answers_at_the_bigquery_engine() {
                super::runs_native_statement_corpus::<$warehouse>(super::super::bigquery::DIALECT);
            }
        }
    };
    ($name:ident, $warehouse:ty) => {
        mod $name {
            #[test]
            fn every_planned_shape_answers_at_the_native_engine() {
                super::runs_native_statement_corpus::<$warehouse>(super::super::$name::DIALECT);
            }
        }
    };
}

// The dialect and system axes use the same names, so keep their registry expansions apart.
mod engines {
    use super::runs_native_statement_corpus;
    crate::adapters::registered!(data_systems: data_system);
}

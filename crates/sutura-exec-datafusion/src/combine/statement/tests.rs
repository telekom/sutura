//! What a `run_sql` statement plans to: the switch, the cut, the refused statement kinds, the
//! working set. Each source is a `DataFusion` session of its own holding its table, the fake behind
//! [`StatementLegs`], so a part's text is run as a source would run it.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use datafusion::arrow::array::{Int64Array, RecordBatch, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::prelude::SessionContext;
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::{
    FederationCombiner as _, StatementAnswer, StatementLegs, StatementQuestion, StatementStopped, StatementTable,
};
use sutura_domain::raw::{RawStatement, SqlConstruct};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{Accumulating, RawRows, ResultBudget};
use tokio::runtime::Runtime;

use crate::combine::DataFusionCombiner;

mod cube_pushdown;
#[cfg(test)]
pub(super) mod literals;
#[cfg(test)]
mod names;
mod oracle;
mod oracle_prefilter;
mod policy_search;
mod raw_condition;
#[cfg(test)]
mod registration;
mod resolution;
mod text;
mod text_prefilter;

fn explicit_source_projection(sql: &str) -> bool {
    use datafusion::sql::sqlparser::ast::{Select, SelectItem, Visit as _, Visitor};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;
    use std::ops::ControlFlow;

    struct Projections;
    impl Visitor for Projections {
        type Break = ();

        fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<()> {
            if select
                .projection
                .iter()
                .any(|item| matches!(item, SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)))
            {
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        }
    }
    Parser::parse_sql(&GenericDialect, sql).is_ok_and(|statements| statements.visit(&mut Projections).is_continue())
}

fn source_labels(sql: &str) -> Vec<String> {
    use datafusion::sql::sqlparser::ast::{SelectItem, SetExpr, Statement};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;
    let parsed = Parser::parse_sql(&GenericDialect, sql).unwrap();
    let [Statement::Query(query)] = parsed.as_slice() else {
        panic!("one source query: {sql}")
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        panic!("one output SELECT: {sql}")
    };
    select
        .projection
        .iter()
        .map(|item| {
            let SelectItem::ExprWithAlias { alias, .. } = item else {
                panic!("an explicit unique source alias: {sql}")
            };
            alias.value.clone()
        })
        .collect()
}

#[test]
fn duplicate_source_outputs_restore_exact_names_positions_and_empty_metadata() {
    struct Recording<'a> {
        engines: &'a mut Engines,
        statements: &'a mut Vec<String>,
    }

    impl StatementLegs for Recording<'_> {
        type Stop = SourceName;

        fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
            self.engines.open(sources)
        }

        fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
            self.statements.push(statement.as_str().to_owned());
            self.engines.run(source, statement)
        }
    }

    for predicate in ["c.customer_id < 4", "c.customer_id = 1000"] {
        for suffix in ["", " order by c.customer_id limit 200"] {
            let sql = format!(
                "select c.customer_id, d.customer_id, c.region as \"c0\", d.region as \"MiXeD\" \
                 from dim_customer c join dim_customer d on c.customer_id + 1 = d.customer_id where {predicate}{suffix}"
            );
            let mut legs = Engines::new(None);
            let mut statements = Vec::new();
            let statement = RawStatement::parse(&sql).expect("a test statement is a statement");
            let tables = tables();
            let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
                StatementQuestion::new(
                    &statement,
                    &tables,
                    false,
                    UNBOUNDED,
                    opened(Duration::ZERO, Duration::from_secs(60)),
                ),
                &mut Recording {
                    engines: &mut legs,
                    statements: &mut statements,
                },
            );
            let Some(Ok(rows)) = answer else {
                panic!(
                    "duplicate outputs must run: {sql}: {answer:?}; opened={:?}; ran={:?}",
                    legs.opened, legs.ran
                )
            };
            assert_eq!(rows.columns(), ["customer_id", "customer_id", "c0", "MiXeD"]);
            assert_eq!(
                source_labels(statements.first().expect("a source statement")),
                ["c0", "c1", "c2", "c3"]
            );
            assert_eq!(rows, legs.as_one(&sql), "positions and multiplicities: {sql}");
            assert_eq!(legs.opened, [BTreeSet::from([named("crm")])]);
            assert_eq!(legs.ran, [named("crm")]);
        }
    }
    let mut legs = Engines::new(None);
    let answer = asked(AREAS, true, UNBOUNDED, &mut legs);
    let Some(Ok(rows)) = answer else {
        panic!(
            "all final federation parts must be admitted before open: {answer:?}; opened={:?}; ran={:?}",
            legs.opened, legs.ran
        )
    };
    assert_eq!(rows, legs.as_one(AREAS));
    assert_eq!(legs.opened, [BTreeSet::from([named("crm"), named("geo")])]);
    assert_eq!(legs.ran.len(), 2);
}

/// Records the text of every leg before delegating to the engines.
struct Texts<'a> {
    engines: &'a mut Engines,
    texts: Vec<String>,
}

impl StatementLegs for Texts<'_> {
    type Stop = SourceName;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        self.engines.open(sources)
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.texts.push(statement.as_str().to_owned());
        self.engines.run(source, statement)
    }
}

#[test]
fn a_window_over_a_derived_input_sends_every_input_column_by_name() {
    // The optimizer drops the projection that mirrors the window, and the limit keeps its input
    // a derived table, the one shape on which the window arm itself lists the input's columns.
    let sql = "select customer_id, region, row_number() over (order by customer_id) \
               from (select customer_id, region from dim_customer order by customer_id limit 3)";
    let mut engines = Engines::new(None);
    let statement = RawStatement::parse(sql).expect("a test statement is a statement");
    let tables = tables();
    let mut legs = Texts {
        engines: &mut engines,
        texts: Vec::new(),
    };
    let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    let texts = legs.texts;
    let Some(Ok(rows)) = answer else {
        panic!("the window must run with explicit source columns: {answer:?}; sent={texts:?}")
    };
    let [text] = texts.as_slice() else {
        panic!("one source statement: {texts:?}")
    };
    assert!(
        text.contains("derived_window_input"),
        "the window must sit over a derived input: {text}"
    );
    assert!(!text.contains('*'), "no wildcard reaches the source: {text}");
    assert_eq!(rows.columns().len(), 3);
    assert_eq!(rows.rows(), engines.as_one(sql).rows());
}

#[test]
fn cube_unsigned_results_are_refused_before_open_and_signed_results_still_run() {
    for kind in [
        DataType::UInt16,
        DataType::UInt32,
        DataType::UInt64,
        DataType::Int16,
        DataType::Int32,
        DataType::Int64,
    ] {
        let unsigned = matches!(kind, DataType::UInt16 | DataType::UInt32 | DataType::UInt64);
        let tables = [StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_customer").expect("a fixture table"),
            TypeDialect::Cube,
            Arc::new(Schema::new(vec![Field::new("customer_id", kind, false)])),
        )];
        let mut legs = Engines::new(None);
        let statement =
            RawStatement::parse("with ids as (select customer_id from dim_customer) select customer_id as projected from ids")
                .expect("a bounded nested fixture query");
        let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                false,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        if unsigned {
            assert!(matches!(answer, Some(Err(StatementStopped::NotPlanned))), "{answer:?}");
            assert!(legs.opened.is_empty(), "unsafe wire output cannot mint credentials");
            assert!(legs.ran.is_empty(), "unsafe wire output cannot reach a source");
        } else {
            let Some(Ok(rows)) = answer else {
                panic!("the signed output must run: {answer:?}");
            };
            assert_eq!(rows.rows().len(), 4);
            assert_eq!(legs.opened, vec![BTreeSet::from([named("crm")])]);
        }
    }
}

const AREAS: &str = "select r.sales_area, count(*) as customers from dim_customer c join dim_region r on c.region = r.region \
                     group by r.sales_area order by r.sales_area";

const UNBOUNDED: u64 = 1 << 30;

fn named(source: &str) -> SourceName {
    SourceName::parse(source).expect("a test source is a source")
}

fn customers() -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("customer_id", DataType::Int64, false),
        Field::new("region", DataType::Utf8, false),
    ]);
    RecordBatch::try_new(
        Arc::new(schema),
        vec![
            Arc::new(Int64Array::from(vec![1, 2, 3, 4])),
            Arc::new(StringArray::from(vec!["north", "north", "south", "east"])),
        ],
    )
    .expect("the customers are rectangular")
}

fn regions() -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("region", DataType::Utf8, false),
        Field::new("sales_area", DataType::Utf8, false),
    ]);
    RecordBatch::try_new(
        Arc::new(schema),
        vec![
            Arc::new(StringArray::from(vec!["north", "south", "east"])),
            Arc::new(StringArray::from(vec!["emea", "emea", "apac"])),
        ],
    )
    .expect("the regions are rectangular")
}

/// Customers at `crm`, a `DuckDB` source, and regions at `geo`, a `Postgres` one.
fn tables() -> Vec<StatementTable> {
    let table = |path: &str| QualifiedTable::parse(path).expect("a test table is a table");
    vec![
        StatementTable::new(named("crm"), table("dim_customer"), TypeDialect::DuckDb, customers().schema()),
        StatementTable::new(named("geo"), table("dim_region"), TypeDialect::Postgres, regions().schema()),
    ]
}

fn holding(tables: &[(&str, RecordBatch)]) -> SessionContext {
    let context = SessionContext::new();
    for (name, batch) in tables {
        drop(context.register_batch(*name, batch.clone()).expect("a test table registers"));
    }
    context
}

/// `statement` with each `ClickHouse` UTF-8 function spelled as the `DataFusion` function it stands
/// for, so the engine standing in for a source can run text written for `ClickHouse`.
fn as_datafusion(statement: &str) -> String {
    super::CHARACTER_FUNCTIONS
        .iter()
        .fold(statement.to_owned(), |text, (plain, utf8)| {
            text.replace(&format!("{utf8}("), &format!("{plain}("))
        })
}

fn rows_of(runtime: &Runtime, context: &SessionContext, statement: &str) -> RawRows {
    runtime.block_on(async {
        let frame = context
            .sql(&as_datafusion(statement))
            .await
            .expect("the engine plans the text it is handed");
        let mut read = Accumulating::announcing(
            Arc::clone(frame.schema().inner()),
            usize::MAX,
            ResultBudget::of_bytes(NonZeroUsize::MAX),
        );
        for batch in frame.collect().await.expect("the engine runs the text it is handed") {
            read.push(batch).expect("a batch of the announced schema");
        }
        let (columns, rows) = read.finish().to_rows().expect("the cells read").into_parts();
        RawRows::of(columns, rows)
    })
}

/// Each listed source as a session of its own; a stop at `stopping`.
struct Engines {
    runtime: Runtime,
    at: BTreeMap<SourceName, SessionContext>,
    stopping: Option<SourceName>,
    opened: Vec<BTreeSet<SourceName>>,
    ran: Vec<SourceName>,
}

impl Engines {
    fn new(stopping: Option<&str>) -> Self {
        Self {
            runtime: tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("a test runtime builds"),
            at: BTreeMap::from([
                (named("crm"), holding(&[("dim_customer", customers())])),
                (named("geo"), holding(&[("dim_region", regions())])),
            ]),
            stopping: stopping.map(named),
            opened: Vec::new(),
            ran: Vec::new(),
        }
    }

    /// `statement`'s rows from one session holding every table: what the statement answers when
    /// nothing is federated.
    fn as_one(&self, statement: &str) -> RawRows {
        rows_of(
            &self.runtime,
            &holding(&[("dim_customer", customers()), ("dim_region", regions())]),
            statement,
        )
    }
}

impl StatementLegs for Engines {
    type Stop = SourceName;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        self.opened.push(sources.clone());
        Ok(())
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        if !explicit_source_projection(statement.as_str()) {
            return Err(source.clone());
        }
        self.ran.push(source.clone());
        if self.stopping.as_ref() == Some(source) {
            return Err(source.clone());
        }
        let context = self.at.get(source).expect("a leg runs at a listed source");
        Ok(rows_of(&self.runtime, context, statement.as_str()))
    }
}

/// A deadline opened `ago` with `budget` to spend.
fn opened(ago: Duration, budget: Duration) -> Deadline {
    Deadline::opened_at(
        Instant::now()
            .checked_sub(ago)
            .expect("the monotonic clock reaches back that far"),
        Budget::parse(budget).expect("a test budget is not zero"),
    )
}

fn asked(statement: &str, federation_allowed: bool, working_set_bytes: u64, legs: &mut Engines) -> StatementAnswer<SourceName> {
    asked_by(
        statement,
        federation_allowed,
        working_set_bytes,
        opened(Duration::ZERO, Duration::from_secs(60)),
        legs,
    )
}

fn asked_by(
    statement: &str,
    federation_allowed: bool,
    working_set_bytes: u64,
    deadline: Deadline,
    legs: &mut Engines,
) -> StatementAnswer<SourceName> {
    let statement = RawStatement::parse(statement).expect("a test statement is a statement");
    let tables = tables();
    DataFusionCombiner::new().expect("a combiner builds").answer_statement(
        StatementQuestion::new(&statement, &tables, federation_allowed, working_set_bytes, deadline),
        legs,
    )
}

#[test]
fn without_federation_a_statement_over_two_sources_is_refused_before_any_leg() {
    let mut legs = Engines::new(None);
    let answer = asked(AREAS, false, UNBOUNDED, &mut legs);
    assert!(
        matches!(answer, Some(Err(StatementStopped::SpansMoreThanOneSource))),
        "{answer:?}"
    );
    assert!(legs.opened.is_empty(), "nothing was opened: {:?}", legs.opened);
    assert!(legs.ran.is_empty(), "no leg ran: {:?}", legs.ran);
}

/// The ADR's own cell, over the engine: a join of a `DuckDB` table and a `Postgres` one answers as
/// one session holding both tables does, under one open of both sources.
#[test]
fn with_federation_a_join_across_two_sources_answers_as_one_engine_holding_both() {
    let mut legs = Engines::new(None);
    let answer = asked(AREAS, true, UNBOUNDED, &mut legs);
    let Some(Ok(rows)) = answer else {
        panic!("a join across two answering sources is answered: {answer:?}");
    };
    assert_eq!(rows, legs.as_one(AREAS));
    assert_eq!(legs.opened, [BTreeSet::from([named("crm"), named("geo")])]);
    legs.ran.sort();
    assert_eq!(legs.ran, [named("crm"), named("geo")]);
}

/// A plan one source answers whole is that source's statement, with or without federation.
#[test]
fn without_federation_a_statement_over_one_source_runs_whole_at_it() {
    let statement = "select region, sales_area from dim_region where sales_area = 'emea' order by region";
    let mut legs = Engines::new(None);
    let answer = asked(statement, false, UNBOUNDED, &mut legs);
    let Some(Ok(rows)) = answer else {
        panic!("a statement over one answering source is answered: {answer:?}");
    };
    assert_eq!(rows, legs.as_one(statement));
    assert_eq!(legs.opened, [BTreeSet::from([named("geo")])]);
    assert_eq!(legs.ran, [named("geo")]);
}

#[test]
fn an_ordered_source_limit_keeps_its_offset_and_fetch_after_pushdown() {
    let statement = "select region from dim_region order by region limit 1 offset 1";
    let mut legs = Engines::new(None);
    let answer = asked(statement, false, UNBOUNDED, &mut legs);
    let Some(Ok(rows)) = answer else {
        panic!("an ordered source limit is answered: {answer:?}");
    };
    assert_eq!(rows, legs.as_one(statement));
}

/// Only a query plans: no definition, write, session statement, explanation or description, and no
/// statement over a table the question does not carry or over none.
#[test]
fn unsupported_statements_name_only_a_closed_construct_and_open_nothing() {
    for (statement, construct) in [
        ("drop table dim_region", SqlConstruct::NonQuery),
        ("create table dim_area (area varchar)", SqlConstruct::NonQuery),
        ("insert into dim_region values ('west', 'amer')", SqlConstruct::NonQuery),
        ("set datafusion.execution.batch_size = 1", SqlConstruct::NonQuery),
        ("explain select region from dim_region", SqlConstruct::NonQuery),
        ("describe dim_region", SqlConstruct::NonQuery),
        ("select 1", SqlConstruct::SelectWithoutTable),
        (
            "select region from dim_region fetch first 1 rows only",
            SqlConstruct::FetchFirst,
        ),
        ("select from", SqlConstruct::Query),
    ] {
        let mut legs = Engines::new(None);
        let answer = asked(statement, true, UNBOUNDED, &mut legs);
        assert!(
            matches!(answer, Some(Err(StatementStopped::UnsupportedSyntax(actual))) if actual == construct),
            "{statement}: {answer:?}"
        );
        assert!(legs.opened.is_empty(), "{statement} opened {:?}", legs.opened);
    }
}

#[test]
fn unknown_tables_and_columns_are_distinct_and_open_nothing() {
    for (statement, table) in [
        ("select region from dim_nowhere", true),
        (
            "with dim_region as (select region from dim_nowhere) select * from dim_region",
            true,
        ),
        ("select absent_column from dim_region", false),
        ("select r.absent_column from dim_region r", false),
    ] {
        let mut legs = Engines::new(None);
        let answer = asked(statement, true, UNBOUNDED, &mut legs);
        assert!(
            matches!(
                (&answer, table),
                (Some(Err(StatementStopped::UnknownTable)), true) | (Some(Err(StatementStopped::UnknownColumn)), false)
            ),
            "{statement}: {answer:?}"
        );
        assert!(legs.opened.is_empty(), "{statement} opened {:?}", legs.opened);
        assert_eq!(legs.ran.len(), 0);
    }
}

#[test]
fn a_leg_that_stops_ends_the_answer_with_its_stop() {
    let mut legs = Engines::new(Some("geo"));
    let answer = asked(AREAS, true, UNBOUNDED, &mut legs);
    assert!(
        matches!(answer, Some(Err(StatementStopped::Leg(ref source))) if *source == named("geo")),
        "{answer:?}"
    );
}

/// The local part runs under the working set: one byte holds no join.
#[test]
fn a_local_part_over_the_working_set_is_an_answer_too_large() {
    let mut legs = Engines::new(None);
    let answer = asked(AREAS, true, 1, &mut legs);
    assert!(matches!(answer, Some(Err(StatementStopped::AnswerTooLarge))), "{answer:?}");
}

/// The session holds no table function, so a statement generates no rows of its own, with or
/// without federation.
#[test]
fn a_table_function_does_not_exist_in_the_session() {
    for statement in [
        "select count(*) from dim_region, generate_series(1, 3)",
        "select count(*) from dim_region, range(1, 3)",
    ] {
        for federation_allowed in [false, true] {
            let mut legs = Engines::new(None);
            let answer = asked(statement, federation_allowed, UNBOUNDED, &mut legs);
            assert!(
                matches!(
                    answer,
                    Some(Err(StatementStopped::UnsupportedSyntax(SqlConstruct::TableFunction)))
                ),
                "{statement}: {answer:?}"
            );
            assert!(legs.opened.is_empty(), "{statement} opened {:?}", legs.opened);
        }
    }
}

/// A budget spent before the call refuses it before it plans, and nothing is opened.
#[test]
fn a_spent_budget_stops_the_statement_before_it_plans() {
    let mut legs = Engines::new(None);
    let answer = asked_by(
        AREAS,
        true,
        UNBOUNDED,
        opened(Duration::from_secs(10), Duration::from_secs(1)),
        &mut legs,
    );
    assert!(matches!(answer, Some(Err(StatementStopped::DeadlineExceeded))), "{answer:?}");
    assert!(legs.opened.is_empty(), "nothing was opened: {:?}", legs.opened);
}

/// A local part with more rows than the budget can sum stops at the deadline instead of running to
/// its end: twelve pairs of a four-row and a three-row table cross joined, every one of them read.
#[test]
fn a_local_part_past_the_deadline_stops_at_it() {
    let pairs = 0..12;
    let statement = format!(
        "select sum({}), max({}) from {}",
        pairs
            .clone()
            .map(|pair| format!("c{pair}.customer_id"))
            .collect::<Vec<_>>()
            .join(" + "),
        pairs
            .clone()
            .map(|pair| format!("r{pair}.region"))
            .collect::<Vec<_>>()
            .join(" || "),
        pairs
            .map(|pair| format!("dim_customer c{pair}, dim_region r{pair}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let mut legs = Engines::new(None);
    let started = Instant::now();
    let answer = asked_by(
        &statement,
        true,
        UNBOUNDED,
        opened(Duration::ZERO, Duration::from_secs(2)),
        &mut legs,
    );
    assert!(matches!(answer, Some(Err(StatementStopped::DeadlineExceeded))), "{answer:?}");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "stopped long after the deadline: {:?}",
        started.elapsed()
    );
    assert!(!legs.ran.is_empty(), "the legs ran before the local part did");
}

/// A correlated `EXISTS` in a filter is cut like a join: refused without federation, a part of its
/// own with it.
#[test]
fn a_correlated_exists_over_a_second_source_is_a_part_of_its_own() {
    let statement = "select region from dim_region r where exists \
                     (select 1 from dim_customer c where c.region = r.region and c.customer_id > 2) order by region";
    let mut legs = Engines::new(None);
    let refused = asked(statement, false, UNBOUNDED, &mut legs);
    assert!(
        matches!(refused, Some(Err(StatementStopped::SpansMoreThanOneSource))),
        "{refused:?}"
    );
    let answer = asked(statement, true, UNBOUNDED, &mut legs);
    let Some(Ok(rows)) = answer else {
        panic!("an exists over two answering sources is answered: {answer:?}");
    };
    assert_eq!(rows, legs.as_one(statement));
    legs.ran.sort();
    assert_eq!(legs.ran, [named("crm"), named("geo")]);
}

/// An `EXISTS` the planner leaves in a projection hides its table from the cut, so the part holding
/// it reads two sources and is refused rather than sent whole to one of them.
#[test]
fn an_exists_in_a_projection_over_a_second_source_is_not_planned() {
    let statement = "select region, exists (select 1 from dim_customer c where c.region = r.region) as served \
                     from dim_region r";
    for federation_allowed in [false, true] {
        let mut legs = Engines::new(None);
        let answer = asked(statement, federation_allowed, UNBOUNDED, &mut legs);
        assert!(matches!(answer, Some(Err(StatementStopped::NotPlanned))), "{answer:?}");
        assert!(legs.opened.is_empty(), "nothing was opened: {:?}", legs.opened);
    }
}

#[cfg(test)]
mod bounds;
#[cfg(test)]
mod logged;
#[cfg(test)]
mod scan_only;

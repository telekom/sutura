//! What a `run_sql` statement plans to: the switch, the cut, the refused statement kinds, the
//! working set. Each source is a `DataFusion` session of its own holding its table, the fake behind
//! [`StatementLegs`], so a part's text is run as a source would run it.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::sync::Arc;

use datafusion::arrow::array::{Int64Array, RecordBatch, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::prelude::SessionContext;
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::{
    FederationCombiner as _, StatementAnswer, StatementLegs, StatementQuestion, StatementStopped, StatementTable,
};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{Accumulating, RawRows, ResultBudget};
use tokio::runtime::Runtime;

use crate::combine::DataFusionCombiner;

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

fn rows_of(runtime: &Runtime, context: &SessionContext, statement: &str) -> RawRows {
    runtime.block_on(async {
        let frame = context.sql(statement).await.expect("the engine plans the text it is handed");
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
        self.ran.push(source.clone());
        if self.stopping.as_ref() == Some(source) {
            return Err(source.clone());
        }
        let context = self.at.get(source).expect("a leg runs at a listed source");
        Ok(rows_of(&self.runtime, context, statement.as_str()))
    }
}

fn asked(statement: &str, federation_allowed: bool, working_set_bytes: u64, legs: &mut Engines) -> StatementAnswer<SourceName> {
    let statement = RawStatement::parse(statement).expect("a test statement is a statement");
    let tables = tables();
    DataFusionCombiner::new().expect("a combiner builds").answer_statement(
        StatementQuestion::new(&statement, &tables, federation_allowed, working_set_bytes),
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

/// Only a query plans: no definition, write, session statement, explanation or description, and no
/// statement over a table the question does not carry or over none.
#[test]
fn a_statement_that_is_not_a_query_over_a_listed_table_is_not_planned_and_opens_nothing() {
    for statement in [
        "drop table dim_region",
        "create table dim_area (area varchar)",
        "insert into dim_region values ('west', 'amer')",
        "set datafusion.execution.batch_size = 1",
        "explain select region from dim_region",
        "describe dim_region",
        "select region from dim_nowhere",
        "select 1",
    ] {
        let mut legs = Engines::new(None);
        let answer = asked(statement, true, UNBOUNDED, &mut legs);
        assert!(
            matches!(answer, Some(Err(StatementStopped::NotPlanned))),
            "{statement}: {answer:?}"
        );
        assert!(legs.opened.is_empty(), "{statement} opened {:?}", legs.opened);
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

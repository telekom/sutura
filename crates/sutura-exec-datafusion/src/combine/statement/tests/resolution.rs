//! Identifier resolution is observed through the source statements and their returned rows.

use datafusion::arrow::array::ArrayRef;
use datafusion::common::TableReference;
use datafusion::datasource::memory::MemorySchemaProvider;

use super::*;

type NumericColumn<'a> = (&'a str, Vec<i64>);

fn numeric_oracle(columns: &[NumericColumn<'_>]) -> (Vec<StatementTable>, Engines) {
    let batch = RecordBatch::try_new(
        Arc::new(Schema::new(
            columns
                .iter()
                .map(|(name, _)| Field::new(*name, DataType::Int64, false))
                .collect::<Vec<_>>(),
        )),
        columns
            .iter()
            .map(|(_, values)| Arc::new(Int64Array::from(values.clone())) as ArrayRef)
            .collect(),
    )
    .expect("the numeric fixture is rectangular");
    let context = SessionContext::new();
    drop(
        context
            .register_batch(TableReference::bare("T"), batch.clone())
            .expect("the exact Oracle table registers"),
    );
    let mut legs = Engines::new(None);
    drop(legs.at.insert(named("geo"), context));
    (
        vec![StatementTable::new(
            named("geo"),
            QualifiedTable::parse("T").expect("an exact Oracle path"),
            TypeDialect::Oracle,
            batch.schema(),
        )],
        legs,
    )
}

fn over_oracle(sql: &str, tables: &[StatementTable], legs: &mut Engines) -> StatementAnswer<SourceName> {
    let statement = RawStatement::parse(sql).expect("a bounded query");
    DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            tables,
            true,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        legs,
    )
}

#[test]
fn a_failed_oracle_fold_never_binds_a_quoted_lowercase_column_in_any_scope() {
    for sql in [
        "select id from t",
        "select a.id from t a",
        "with q as (select \"id\" from t) select id from q",
        "select q.id from (select \"id\" from t) q",
        "select * from t l join t r using(id)",
    ] {
        let (tables, mut legs) = numeric_oracle(&[("id", vec![7])]);
        let answer = over_oracle(sql, &tables, &mut legs);
        assert!(
            matches!(answer, Some(Err(StatementStopped::UnknownColumn))),
            "{sql}: {answer:?}"
        );
        assert!(legs.opened.is_empty(), "{sql}");
        assert!(legs.ran.is_empty(), "{sql}");
    }
    for sql in [
        "select \"id\" from t",
        "select a.\"id\" from t a",
        "with q as (select \"id\" from t) select \"id\" from q",
        "select \"id\" from t l join t r using(\"id\")",
    ] {
        let (tables, mut legs) = numeric_oracle(&[("id", vec![7])]);
        let answer = over_oracle(sql, &tables, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{sql}: {answer:?}");
        };
        assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(7)]], "{sql}");
    }
}

#[test]
fn coexisting_oracle_cases_and_using_keys_bind_their_own_values() {
    for (sql, expected) in [
        ("select id from t order by id", [1, 2]),
        ("select \"id\" from t order by \"id\"", [7, 8]),
        ("select id from t l join t r using(id) order by id", [1, 2]),
        ("select \"id\" from t l join t r using(\"id\") order by \"id\"", [7, 8]),
    ] {
        let (tables, mut legs) = numeric_oracle(&[("ID", vec![1, 2]), ("id", vec![7, 8])]);
        let answer = over_oracle(sql, &tables, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{sql}: {answer:?}");
        };
        let expected = expected.map(|value| vec![sutura_domain::warehouse::Value::Integer(value)]);
        assert_eq!(rows.rows(), expected, "{sql}");
    }
}

#[test]
fn oracle_inputs_take_precedence_over_output_aliases_in_where_and_join_on() {
    for sql in [
        "select a as b from t where b > 10",
        "select l.a as b from t l join u r on b = r.a",
    ] {
        let (mut tables, mut legs) = numeric_oracle(&[("A", vec![1, 2]), ("B", vec![20, 5])]);
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("A", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![20, 30]))],
        )
        .expect("the other input is rectangular");
        drop(
            legs.at
                .get(&named("geo"))
                .expect("the Oracle fake")
                .register_batch(TableReference::bare("U"), batch.clone())
                .expect("the join input registers"),
        );
        tables.push(StatementTable::new(
            named("geo"),
            QualifiedTable::parse("U").expect("a fixture table"),
            TypeDialect::Oracle,
            batch.schema(),
        ));
        let answer = over_oracle(sql, &tables, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{sql}: {answer:?}");
        };
        assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(1)]], "{sql}");
    }
}

#[test]
fn oracle_output_aliases_take_precedence_in_having_and_qualify() {
    for sql in [
        "select a as b from t group by a, b having b > 10",
        "select sum(a) as b from t group by b having b > 10",
        "select a as b from t qualify row_number() over (order by a) > 0 and b > 10",
    ] {
        let (tables, mut legs) = numeric_oracle(&[("A", vec![1, 20]), ("B", vec![20, 1])]);
        let answer = over_oracle(sql, &tables, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{sql}: {answer:?}");
        };
        assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(20)]], "{sql}");
    }
}

fn oracle_inputs() -> (Vec<StatementTable>, Engines) {
    let original = regions();
    let batch = RecordBatch::try_new(
        Arc::new(Schema::new(vec![
            Field::new("REGION", DataType::Utf8, false),
            Field::new("SalesArea", DataType::Utf8, false),
        ])),
        original.columns().to_vec(),
    )
    .expect("the Oracle metadata preserves the two columns");
    let context = SessionContext::new();
    drop(
        context
            .register_batch(TableReference::bare("DIM_REGION"), batch.clone())
            .expect("the exact table registers"),
    );
    let mut legs = Engines::new(None);
    drop(legs.at.insert(named("geo"), context));
    let tables = vec![
        tables().remove(0),
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("DIM_REGION").expect("an exact Oracle path"),
            TypeDialect::Oracle,
            batch.schema(),
        ),
    ];
    (tables, legs)
}

#[test]
fn oracle_names_fold_in_their_scope_and_quoted_columns_and_aliases_stay_exact() {
    for (sql, expected) in [
        (
            "select region from dim_region order by region",
            "select \"REGION\" from \"DIM_REGION\" order by \"REGION\"",
        ),
        (
            "select r.region as \"Label\" from dim_region r order by \"Label\"",
            "select \"REGION\" as \"Label\" from \"DIM_REGION\" order by \"Label\"",
        ),
        (
            "select r.\"SalesArea\" from dim_region r order by r.\"SalesArea\"",
            "select \"SalesArea\" from \"DIM_REGION\" order by \"SalesArea\"",
        ),
        (
            "with r as (select region as code from dim_region) select code from r order by code",
            "select \"REGION\" as code from \"DIM_REGION\" order by code",
        ),
        (
            "select d.region from (select region from dim_region) d order by d.region",
            "select \"REGION\" from \"DIM_REGION\" order by \"REGION\"",
        ),
    ] {
        let (tables, mut legs) = oracle_inputs();
        let statement = RawStatement::parse(sql).expect("a bounded query");
        let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
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
            panic!("{sql}: {answer:?}");
        };
        assert_eq!(
            rows,
            rows_of(&legs.runtime, legs.at.get(&named("geo")).expect("the Oracle fake"), expected),
            "{sql}"
        );
        assert_eq!(legs.ran, [named("geo")]);
    }
}

#[test]
fn oracle_and_lowercase_sources_resolve_independently_in_one_join() {
    let (tables, mut legs) = oracle_inputs();
    let statement = RawStatement::parse(
        "select r.\"SalesArea\", count(*) as customers from dim_customer c join dim_region r on c.region = r.region \
         group by r.\"SalesArea\" order by r.\"SalesArea\"",
    )
    .expect("a bounded mixed-source query");
    let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
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
        panic!("both source cases must resolve: {answer:?}");
    };
    assert_eq!(
        rows.rows(),
        [
            vec![
                sutura_domain::warehouse::Value::Text(String::from("apac")),
                sutura_domain::warehouse::Value::Integer(1)
            ],
            vec![
                sutura_domain::warehouse::Value::Text(String::from("emea")),
                sutura_domain::warehouse::Value::Integer(3)
            ],
        ]
    );
    assert_eq!(legs.opened, [BTreeSet::from([named("crm"), named("geo")])]);
}

#[test]
fn oracle_qualifiers_fold_without_changing_the_physical_path() {
    let (mut tables, mut legs) = oracle_inputs();
    let context = SessionContext::new();
    drop(
        context
            .catalog("datafusion")
            .expect("the fake's default catalog")
            .register_schema("REPORTING", Arc::new(MemorySchemaProvider::new()))
            .expect("the exact schema registers"),
    );
    let batch =
        RecordBatch::try_new(Arc::clone(tables[1].schema()), regions().columns().to_vec()).expect("the same physical columns");
    drop(
        context
            .register_batch(TableReference::partial("REPORTING", "DIM_REGION"), batch)
            .expect("the exact table registers"),
    );
    drop(legs.at.insert(named("geo"), context));
    tables[1] = StatementTable::new(
        named("geo"),
        QualifiedTable::parse("REPORTING.DIM_REGION").expect("a qualified physical path"),
        TypeDialect::Oracle,
        Arc::clone(tables[1].schema()),
    );
    let statement = RawStatement::parse("select region from reporting.dim_region order by region").expect("a bounded query");
    let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
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
        panic!("the exact physical path resolves: {answer:?}");
    };
    assert_eq!(
        rows,
        rows_of(
            &legs.runtime,
            legs.at.get(&named("geo")).expect("the qualified fake"),
            "select \"REGION\" from \"REPORTING\".\"DIM_REGION\" order by \"REGION\""
        )
    );
    assert_eq!(legs.ran, [named("geo")]);
}

#[test]
fn quoted_oracle_names_do_not_fold_and_colliding_source_paths_are_absent() {
    for sql in [
        "select region from \"dim_region\"",
        "select salesarea from dim_region",
        "select \"region\" from dim_region",
    ] {
        let (tables, mut legs) = oracle_inputs();
        let statement = RawStatement::parse(sql).expect("a bounded query");
        let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        if sql.contains("\"dim_region\"") {
            assert!(matches!(answer, Some(Err(StatementStopped::UnknownTable))), "{answer:?}");
        } else {
            assert!(matches!(answer, Some(Err(StatementStopped::UnknownColumn))), "{answer:?}");
        }
        assert_eq!(legs.opened.len(), 0);
    }
    let (mut tables, mut legs) = oracle_inputs();
    tables.push(StatementTable::new(
        named("other"),
        tables[1].table().clone(),
        TypeDialect::Oracle,
        Arc::clone(tables[1].schema()),
    ));
    let statement = RawStatement::parse("select region from dim_region").expect("a bounded query");
    let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            true,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(matches!(answer, Some(Err(StatementStopped::UnknownTable))), "{answer:?}");
    assert_eq!(legs.opened.len(), 0);
}

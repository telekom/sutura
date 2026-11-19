use super::*;

#[test]
fn visible_models_of_one_physical_table_share_one_registration() {
    let schema = |columns: &[usize]| customers().project(columns).unwrap().schema();
    let table = |columns| {
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_customer").unwrap(),
            TypeDialect::DuckDb,
            schema(columns),
        )
    };
    let mut visible = tables();
    visible[0] = table(&[0]);
    visible.push(table(&[1]));
    let combiner = DataFusionCombiner::new().unwrap();
    for text in [
        "select customer_id, region from dim_customer order by customer_id",
        "select sales_area from dim_region order by sales_area",
    ] {
        let statement = RawStatement::parse(text).unwrap();
        let mut legs = Engines::new(None);
        let answer = combiner.answer_statement(
            StatementQuestion::new(
                &statement,
                &visible,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert_eq!(answer.unwrap().unwrap(), legs.as_one(text));
    }
    for (dialect, schema) in [
        (TypeDialect::Postgres, schema(&[0])),
        (
            TypeDialect::DuckDb,
            Arc::new(Schema::new(vec![Field::new("customer_id", DataType::Utf8, false)])),
        ),
    ] {
        let mut inconsistent = visible.clone();
        inconsistent.push(StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_customer").unwrap(),
            dialect,
            schema,
        ));
        let statement = RawStatement::parse("select customer_id from dim_customer").unwrap();
        let mut legs = Engines::new(None);
        let answer = combiner.answer_statement(
            StatementQuestion::new(
                &statement,
                &inconsistent,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert!(matches!(answer, Some(Err(StatementStopped::NotPlanned))), "{answer:?}");
        assert!(legs.opened.is_empty(), "refused before opening a leg: {:?}", legs.opened);
    }
}

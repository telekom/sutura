//! #2106's review: `registered_tables` used to key registration by physical path alone, so a
//! `schema.table` held by two listed sources silently dropped BOTH entries rather than one - a
//! three-part reference naming either source's own table had nothing to resolve to. Registration
//! now keys by `(source, table)`, so a collision stays reachable by naming its source, and an
//! unprefixed reference to the same physical path remains refused exactly as it was before any
//! source held it twice.
//!
//! This module also pins that a two-part reference always resolves a physical path first - a
//! schema name that happens to equal some OTHER source's id is never reinterpreted as that
//! source's prefix once a physical table already matches it.

use datafusion::arrow::array::{ArrayRef, Int64Array};
use datafusion::common::TableReference;
use datafusion::datasource::memory::MemorySchemaProvider;

use super::*;

/// `crm` and `geo` each hold their own `reporting.shipments`, under the same physical path - the
/// shape `registered_tables` used to drop entirely rather than keep either.
fn with_reporting_shipments_at_both_crm_and_geo() -> (Vec<StatementTable>, Engines) {
    let crm_shipments = RecordBatch::try_new(
        Arc::new(Schema::new(vec![Field::new("shipment_id", DataType::Int64, false)])),
        vec![Arc::new(Int64Array::from(vec![42])) as ArrayRef],
    )
    .expect("the crm shipments fixture is rectangular");
    let geo_shipments = RecordBatch::try_new(
        Arc::new(Schema::new(vec![Field::new("shipment_id", DataType::Int64, false)])),
        vec![Arc::new(Int64Array::from(vec![7])) as ArrayRef],
    )
    .expect("the geo shipments fixture is rectangular");
    let legs = Engines::new(None);
    for (source, batch) in [(named("crm"), &crm_shipments), (named("geo"), &geo_shipments)] {
        let context = legs.at.get(&source).expect("a fake session");
        drop(
            context
                .catalog("datafusion")
                .expect("the fake's default catalog")
                .register_schema("reporting", Arc::new(MemorySchemaProvider::new()))
                .expect("the reporting schema registers"),
        );
        drop(
            context
                .register_batch(TableReference::partial("reporting", "shipments"), batch.clone())
                .expect("the shipments table registers"),
        );
    }
    let mut tables = tables();
    for (source, dialect, batch) in [
        (named("crm"), TypeDialect::DuckDb, &crm_shipments),
        (named("geo"), TypeDialect::Postgres, &geo_shipments),
    ] {
        tables.push(StatementTable::new(
            source,
            QualifiedTable::parse("reporting.shipments").expect("a dataset-qualified table"),
            dialect,
            batch.schema(),
        ));
    }
    (tables, legs)
}

/// A collision held by two sources answers through either three-part name, at its own source's own
/// data - and the unprefixed two-part reference to the same physical path stays refused, the same
/// outcome it had before either source held it twice.
#[test]
fn a_two_part_collision_refuses_while_each_three_part_name_still_answers_at_its_own_source() {
    let (tables, mut legs) = with_reporting_shipments_at_both_crm_and_geo();

    let at_crm = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &RawStatement::parse("select shipment_id from crm.reporting.shipments").expect("a bounded query"),
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    let Some(Ok(rows)) = at_crm else {
        panic!("{at_crm:?}");
    };
    assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(42)]]);
    assert_eq!(legs.ran, [named("crm")]);

    let at_geo = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &RawStatement::parse("select shipment_id from geo.reporting.shipments").expect("a bounded query"),
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    let Some(Ok(rows)) = at_geo else {
        panic!("{at_geo:?}");
    };
    assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(7)]]);
    assert_eq!(legs.ran, [named("crm"), named("geo")]);

    let unprefixed = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &RawStatement::parse("select shipment_id from reporting.shipments").expect("a bounded query"),
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(
        matches!(unprefixed, Some(Err(StatementStopped::UnknownTable))),
        "{unprefixed:?}"
    );
    assert_eq!(legs.ran, [named("crm"), named("geo")], "the refused call opened nothing new");
}

/// `crm` is both a source id (holding the bare table `orders`) and, at a different source
/// (`finance`), a dataset name holding its own `orders` - the shape a caller could misread as the
/// source prefix `crm.orders` rather than the schema-qualified physical path it already is. The
/// two-part form keeps resolving the physical path first, exactly as before; the three-part form
/// reaches the same table by naming its source explicitly.
fn with_crm_as_both_a_source_and_a_schema() -> (Vec<StatementTable>, Engines) {
    let crm_orders = RecordBatch::try_new(
        Arc::new(Schema::new(vec![Field::new("order_id", DataType::Int64, false)])),
        vec![Arc::new(Int64Array::from(vec![1])) as ArrayRef],
    )
    .expect("the crm orders fixture is rectangular");
    let finance_orders = RecordBatch::try_new(
        Arc::new(Schema::new(vec![Field::new("order_id", DataType::Int64, false)])),
        vec![Arc::new(Int64Array::from(vec![9])) as ArrayRef],
    )
    .expect("the finance orders fixture is rectangular");
    let mut legs = Engines::new(None);
    let crm_context = legs.at.get(&named("crm")).expect("crm has a fake session");
    drop(
        crm_context
            .register_batch(TableReference::bare("orders"), crm_orders.clone())
            .expect("crm's bare orders registers"),
    );
    let finance_context = holding(&[]);
    drop(
        finance_context
            .catalog("datafusion")
            .expect("the fake's default catalog")
            .register_schema("crm", Arc::new(MemorySchemaProvider::new()))
            .expect("finance's crm schema registers"),
    );
    drop(
        finance_context
            .register_batch(TableReference::partial("crm", "orders"), finance_orders.clone())
            .expect("finance's schema-qualified orders registers"),
    );
    drop(legs.at.insert(named("finance"), finance_context));
    let tables = vec![
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("orders").expect("a fixture table"),
            TypeDialect::DuckDb,
            crm_orders.schema(),
        ),
        StatementTable::new(
            named("finance"),
            QualifiedTable::parse("crm.orders").expect("a fixture table"),
            TypeDialect::DuckDb,
            finance_orders.schema(),
        ),
    ];
    (tables, legs)
}

#[test]
fn a_two_part_overlap_resolves_the_physical_schema_and_a_three_part_form_reaches_the_same_table() {
    let (tables, mut legs) = with_crm_as_both_a_source_and_a_schema();

    let two_part = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &RawStatement::parse("select order_id from crm.orders").expect("a bounded query"),
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    let Some(Ok(rows)) = two_part else {
        panic!("{two_part:?}");
    };
    assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(9)]]);
    assert_eq!(legs.ran, [named("finance")]);

    let three_part = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &RawStatement::parse("select order_id from finance.crm.orders").expect("a bounded query"),
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    let Some(Ok(rows)) = three_part else {
        panic!("{three_part:?}");
    };
    assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(9)]]);
    assert_eq!(legs.ran, [named("finance"), named("finance")]);
}

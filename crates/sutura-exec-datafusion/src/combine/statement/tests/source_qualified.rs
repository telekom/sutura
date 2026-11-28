//! #2101: `run_sql` names a table by its own path (`table` / `schema.table`) or by that path
//! prefixed with the sutura source it lives at (`source.table` / `source.schema.table`) - one
//! syntax, not a flag behind a second one. The prefix only picks among `tables`, which is already
//! the caller's listed and permitted set, so it is a second way to SELECT, never a second way to
//! widen access: an unknown source, one not listed for `run_sql`, or one the caller may not use are
//! indistinguishable by the time the resolver sees `tables` at all, and a known source that simply
//! does not hold the named schema/table refuses the same way.
//!
//! No dedicated test pins the bare, unprefixed form unchanged - every pre-existing resolver test in
//! this module already exercises it on every run, and a standalone pin of behaviour this suite
//! already runs continuously would pass unchanged on base, which is worse than no test at all.

use datafusion::arrow::array::{ArrayRef, Int64Array};
use datafusion::common::TableReference;
use datafusion::datasource::memory::MemorySchemaProvider;

use super::*;

/// `crm` additionally holds a dataset-qualified `reporting.shipments`, beside the bare tables
/// `tables()` already sets up at `crm` (`dim_customer`) and `geo` (`dim_region`) - the shape a
/// `source.schema.table` reference needs, since neither existing fixture table carries a schema.
fn with_a_dataset_qualified_table_at_crm() -> (Vec<StatementTable>, Engines) {
    let shipments = RecordBatch::try_new(
        Arc::new(Schema::new(vec![Field::new("shipment_id", DataType::Int64, false)])),
        vec![Arc::new(Int64Array::from(vec![42])) as ArrayRef],
    )
    .expect("the shipments fixture is rectangular");
    let legs = Engines::new(None);
    let context = legs.at.get(&named("crm")).expect("crm has a fake session");
    drop(
        context
            .catalog("datafusion")
            .expect("the fake's default catalog")
            .register_schema("reporting", Arc::new(MemorySchemaProvider::new()))
            .expect("the reporting schema registers"),
    );
    drop(
        context
            .register_batch(TableReference::partial("reporting", "shipments"), shipments.clone())
            .expect("the shipments table registers"),
    );
    let mut tables = tables();
    tables.push(StatementTable::new(
        named("crm"),
        QualifiedTable::parse("reporting.shipments").expect("a dataset-qualified table"),
        TypeDialect::DuckDb,
        shipments.schema(),
    ));
    (tables, legs)
}

#[test]
fn a_three_part_reference_names_its_source_and_answers_there() {
    let (tables, mut legs) = with_a_dataset_qualified_table_at_crm();
    let statement = RawStatement::parse("select shipment_id from crm.reporting.shipments").expect("a bounded query");
    let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    let Some(Ok(rows)) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(42)]]);
    assert_eq!(legs.opened, [BTreeSet::from([named("crm")])]);
    assert_eq!(legs.ran, [named("crm")]);
}

/// The shorter `source.table` form, over the pre-existing bare-table fixtures: a caller may qualify
/// either the table itself or `source.table`, interchangeably, for the same physical table.
#[test]
fn a_two_part_source_dot_table_reference_answers_the_same_as_the_bare_table() {
    for (source, statement, bare) in [
        (
            "crm",
            "select customer_id from crm.dim_customer order by customer_id",
            "select customer_id from dim_customer order by customer_id",
        ),
        (
            "geo",
            "select region from geo.dim_region order by region",
            "select region from dim_region order by region",
        ),
    ] {
        let mut legs = Engines::new(None);
        let answer = asked(statement, false, UNBOUNDED, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{statement}: {answer:?}");
        };
        assert_eq!(rows, legs.as_one(bare), "{statement}");
        assert_eq!(legs.opened, [BTreeSet::from([named(source)])], "{statement}");
        assert_eq!(legs.ran, [named(source)], "{statement}");
    }
}

#[test]
fn an_unknown_or_unlisted_source_prefix_refuses_like_an_unknown_table_and_opens_nothing() {
    let (tables, mut legs) = with_a_dataset_qualified_table_at_crm();
    // `nowhere` stands in for all three cases the resolver cannot tell apart: a source that does not
    // exist, one that exists but is not in `tools.run_sql.sources`, and one the caller may not use -
    // `tables` is already filtered to the caller's listed and permitted set before the resolver ever
    // sees it, so a prefix naming anything else simply matches no candidate.
    for sql in [
        "select shipment_id from nowhere.reporting.shipments",
        "select customer_id from nowhere.dim_customer",
    ] {
        let statement = RawStatement::parse(sql).expect("a bounded query");
        let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                false,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert!(
            matches!(answer, Some(Err(StatementStopped::UnknownTable))),
            "{sql}: {answer:?}"
        );
    }
    assert!(legs.opened.is_empty(), "nothing was opened: {:?}", legs.opened);
    assert!(legs.ran.is_empty(), "no leg ran: {:?}", legs.ran);
}

/// A known source that simply does not hold the named schema or table refuses the same way an
/// unknown source does - the prefix selects among `tables`, it does not widen what any source holds.
#[test]
fn a_known_source_without_the_named_schema_or_table_is_also_unknown_table() {
    let (tables, mut legs) = with_a_dataset_qualified_table_at_crm();
    for sql in [
        // `geo` exists and is listed, but holds neither `reporting.shipments` nor `dim_customer`.
        "select shipment_id from geo.reporting.shipments",
        "select customer_id from geo.dim_customer",
        // `crm` exists and is listed, but holds no `other.shipments`.
        "select shipment_id from crm.other.shipments",
    ] {
        let statement = RawStatement::parse(sql).expect("a bounded query");
        let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                false,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert!(
            matches!(answer, Some(Err(StatementStopped::UnknownTable))),
            "{sql}: {answer:?}"
        );
    }
    assert!(legs.opened.is_empty(), "nothing was opened: {:?}", legs.opened);
    assert!(legs.ran.is_empty(), "no leg ran: {:?}", legs.ran);
}

/// A source-prefixed reference that otherwise fails to plan must not fall back to running the
/// caller's raw text against the single source it names - that text still carries the
/// `source.schema.table` prefix, which is a sutura concept the source's own planner has no notion
/// of. `lpad` is excluded from the session's admitted scalar functions, so the statement never
/// builds a plan, and the resolver has already set `prefixed` by the time that failure surfaces:
/// this proves the real combiner answers `SourcePrefixRefused`, not a plain `NotPlanned` that a
/// caller might read as safe to retry verbatim, and that no leg ever opens or runs for it.
#[test]
fn a_prefixed_reference_that_fails_to_plan_refuses_the_source_prefix_not_a_plain_not_planned() {
    let (tables, mut legs) = with_a_dataset_qualified_table_at_crm();
    let statement = RawStatement::parse("select lpad(shipment_id, 3) from crm.reporting.shipments").expect("a bounded query");
    let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(
        matches!(answer, Some(Err(StatementStopped::SourcePrefixRefused))),
        "{answer:?}"
    );
    assert!(legs.opened.is_empty(), "a refused prefix opens nothing: {:?}", legs.opened);
    assert!(legs.ran.is_empty(), "a refused prefix runs nothing: {:?}", legs.ran);
}

//! [`super::recertify_leg`]'s own cells: a leg this worker's catalog certifies differently, an
//! uncertified leg, an unknown source, and a certified leg
//! (`docs/adr/20261102082308-workers-read-sources.md`, Decision 5).

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::model::{Aggregate, ColumnName, Grain, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::remote::{LegWire, to_wire};
use sutura_domain::plan::{
    InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanTerm, ResultLabel, StatementTables, range_filters,
};
use sutura_domain::query::RefusalReason;
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::ParamValue;

use super::recertify_leg;
use crate::Warehouses;
use crate::tests::{bundle, certified, june, metric, source};
use crate::tests_support::{DryRunOutcome, LegPreflightWarehouse};

fn table() -> TableName {
    TableName::parse("orders").expect("a test table is a table")
}

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn planned() -> MetricDigest {
    bundle().metric_digest(&metric()).expect("the bundle pins its metric")
}

fn bucket_column() -> PlanColumn {
    PlanColumn::new(table(), column("order_date"))
}

/// A leg [`bundle`]'s own pin certifies.
fn certified_wire() -> (LegWire, Vec<ParamValue>) {
    wire_under(planned())
}

/// [`certified_wire`] sent under `metric` - a sender's pin that defines the same metric differently
/// when `metric` is not [`planned`].
fn wire_under(metric: MetricDigest) -> (LegWire, Vec<ParamValue>) {
    bucketed(metric, bucket_column())
}

/// A leg under `metric` bucketed, and bounded by its range, on `bucket`.
fn bucketed(metric: MetricDigest, bucket: PlanColumn) -> (LegWire, Vec<ParamValue>) {
    let mut params = Vec::new();
    let [from, until] = range_filters(june(), &bucket, &mut params);
    let leg = LegPlan::Fact {
        source: source(),
        metric,
        tables: StatementTables::only(table()),
        bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Month, bucket),
        keys: Vec::new(),
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: PlanColumn::new(table(), column("amount_cents")),
            },
            ResultLabel::internal(InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::parse(vec![from, until], params.clone()).expect("coherent"),
        range: june(),
    };
    let wire = to_wire(&leg).expect("a leg built from this pin's own metric sends");
    (wire, params)
}

/// Identical to [`certified_wire`] except the bucket reads a column the metric never declared as
/// its time column - refused whatever the digest says.
fn uncertified_wire() -> (LegWire, Vec<ParamValue>) {
    bucketed(planned(), PlanColumn::new(table(), column("amount_cents")))
}

/// A source this worker's registry never opened.
fn undeclared_source() -> SourceName {
    SourceName::parse("undeclared").expect("a test source is a source")
}

fn opened() -> Warehouses<LegPreflightWarehouse> {
    Warehouses::of(LegPreflightWarehouse::new(
        source(),
        SourcePosture::ImpersonationAtSource,
        certified(),
        DryRunOutcome::Accepted,
    ))
}

#[test]
fn a_certified_leg_passes_and_returns_its_plan() {
    let (wire, params) = certified_wire();
    let plan = recertify_leg(&bundle(), &opened(), wire, params).expect("a certified leg over an opened source re-certifies");
    assert_eq!(plan.source(), &source());
}

#[test]
fn a_leg_the_catalog_certifies_differently_is_refused_naming_the_source() {
    let theirs = MetricDigest::new(metric(), DefinitionDigest::parse("a".repeat(64)).expect("a digest"));
    let (wire, params) = wire_under(theirs);
    let refused = recertify_leg(&bundle(), &opened(), wire, params)
        .expect_err("a leg whose metric this catalog defines differently cannot re-certify");
    let RefusalReason::RemoteLegNotCertified { source: named } = refused else {
        panic!("re-certification must refuse with the closed code, got {refused:?}");
    };
    assert_eq!(named, source());
}

#[test]
fn an_uncertified_leg_is_refused() {
    let (wire, params) = uncertified_wire();
    let refused = recertify_leg(&bundle(), &opened(), wire, params)
        .expect_err("a leg over a column the metric never declared cannot re-certify");
    assert_eq!(refused, crate::remote_leg::leg_not_certified(source()));
}

#[test]
fn an_unknown_source_is_refused_before_the_leg_is_even_parsed() {
    let (wire, params) = certified_wire();
    let registry = Warehouses::of(LegPreflightWarehouse::new(
        undeclared_source(),
        SourcePosture::ImpersonationAtSource,
        certified(),
        DryRunOutcome::Accepted,
    ));
    let refused = recertify_leg(&bundle(), &registry, wire, params)
        .expect_err("a source this worker never opened cannot re-certify a leg against it");
    let RefusalReason::RemoteLegNotCertified { source: named } = refused else {
        panic!("re-certification must refuse with the closed code, got {refused:?}");
    };
    assert_eq!(
        named,
        source(),
        "the refusal names the leg's own source, not what this worker opened"
    );
}

#[test]
fn a_partition_whose_leg_the_catalog_certifies_differently_is_never_read() {
    let theirs = MetricDigest::new(metric(), DefinitionDigest::parse("a".repeat(64)).expect("a digest"));
    let (leg, params) = wire_under(theirs);
    let wire = serde_json::to_vec(&sutura_domain::plan::remote::PartitionLeg::new(leg, params)).expect("a partition leg encodes");
    let read = core::cell::Cell::new(false);
    let refused = super::read_recertified(&bundle(), &opened(), &source(), &wire, |_warehouse, _leg| read.set(true));
    assert_eq!(refused, Err(RefusalReason::RemoteLegNotCertified { source: source() }));
    assert!(
        !read.get(),
        "a leg this catalog certifies differently never reaches the warehouse"
    );
}

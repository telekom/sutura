//! Tests for [`super`]: a leg written for the wire and read back against another node's pin.
//!
//! The sender and the receiver are two pins of two bundles that differ only where a cell says so.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{LegNotCertified, LegPart, LegWire, NotSendable, WireLabel, to_wire};
use crate::calendar::{Date, TimeRange};
use crate::capabilities::MetadataCapabilities;
use crate::catalog::{
    Audience, Definitions, Description, Dimension, DimensionValue, JoinKey, JoinKeys, Metric, Model, Relationship, ViaChain,
};
use crate::knowledge::Knowledge;
use crate::measure::{AggregatedColumn, Measure, RequiredFilter, Term, ZeroDenominator};
use crate::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, QualifiedTable, RelationshipName, SourceName,
    TableName,
};
use crate::pinned::{Contribution, ContributionManifest, DefinitionVersion, NotAgreed, PinnedDefinitions};
use crate::plan::{
    InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanJoin, PlanKey, PlanPredicate,
    PlanTerm, PredicateOrigin, ResultLabel, StatementTables,
};
use crate::warehouse::ParamValue;

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn table(raw: &str) -> TableName {
    TableName::parse(raw).expect("a test table is a table")
}

fn model_name(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model is a model")
}

fn relationship_name(raw: &str) -> RelationshipName {
    RelationshipName::parse(raw).expect("a test relationship is a relationship")
}

fn source(raw: &str) -> SourceName {
    SourceName::parse(raw).expect("a test source is a source")
}

fn model(name: &str, source_name: &str, columns: &[&str]) -> Model {
    Model::new(
        model_name(name),
        source(source_name),
        table(name),
        columns.iter().map(|c| column(c)),
        Description::default(),
    )
}

fn relationship(name: &str, origin: &str, target: &str, origin_key: &str, target_key: &str) -> Relationship {
    Relationship::new(
        relationship_name(name),
        model_name(origin),
        model_name(target),
        JoinType::ManyToOne,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: column(origin_key),
            target: column(target_key),
        }])
        .expect("a test relationship declares one key"),
    )
}

/// A relationship on one key per column in `columns`, each column of the same name on both sides,
/// in the order given.
fn compound(name: &str, origin: &str, target: &str, columns: &[&str]) -> Relationship {
    Relationship::new(
        relationship_name(name),
        model_name(origin),
        model_name(target),
        JoinType::ManyToOne,
        JoinKeys::of(
            columns
                .iter()
                .map(|key| JoinKey::Equal {
                    origin: column(key),
                    target: column(key),
                })
                .collect(),
        )
        .expect("a test relationship declares a key"),
    )
}

fn metric_name(raw: &str) -> MetricName {
    MetricName::parse(raw).expect("a test metric is a metric")
}

fn sum(of: &str) -> Term {
    Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column(of)))
}

fn simple(of: &str) -> Measure {
    Measure::Simple(sum(of))
}

/// What a receiver's bundle varies in: the two knobs the digest cells turn.
#[derive(Clone, Copy)]
struct Bundle {
    /// The wording of `revenue`'s description, which moves `revenue`'s digest.
    revenue_description: &'static str,
    /// A metric beside `revenue`, which moves the bundle's digest and not `revenue`'s.
    with_another_metric: bool,
}

const ORDINARY: Bundle = Bundle {
    revenue_description: "Paid orders",
    with_another_metric: false,
};

/// `sales` owns `orders`, `channels`, `refunds` and `calendar`; `crm` owns `customers`, `regions`
/// and `accounts` and `ledgers`. `revenue` crosses into `customers` and `accounts`, `net_revenue`
/// into `accounts` and, on a two-part key, into `ledgers`, which `refunds_ledger` also reaches on
/// the same two keys declared in the other order.
fn pinned(bundle: Bundle) -> PinnedDefinitions {
    let models = vec![
        model(
            "orders",
            "sales",
            &[
                "id",
                "amount_cents",
                "order_date",
                "status",
                "customer_id",
                "channel_id",
                "day_key",
                "ship_day_key",
                "account_id",
                "ref_a",
                "ref_b",
            ],
        ),
        model("channels", "sales", &["id", "channel_name"]),
        model(
            "refunds",
            "sales",
            &[
                "refund_cents",
                "amount_cents",
                "order_id",
                "account_id",
                "day_key",
                "ref_a",
                "ref_b",
            ],
        ),
        model("calendar", "sales", &["day_key", "order_date"]),
        model("customers", "crm", &["id", "region_id"]),
        model("regions", "crm", &["id", "region_name"]),
        model("accounts", "crm", &["id", "segment", "tier"]),
        model("ledgers", "crm", &["ref_a", "ref_b", "book"]),
    ];
    let relationships = vec![
        relationship("orders_channel", "orders", "channels", "channel_id", "id"),
        relationship("orders_customer", "orders", "customers", "customer_id", "id"),
        relationship("customers_region", "customers", "regions", "region_id", "id"),
        relationship("orders_calendar", "orders", "calendar", "day_key", "day_key"),
        relationship("refunds_calendar", "refunds", "calendar", "day_key", "day_key"),
        relationship("calendar_refunds", "calendar", "refunds", "day_key", "day_key"),
        relationship("refunds_order", "refunds", "orders", "order_id", "id"),
        relationship("orders_account", "orders", "accounts", "account_id", "id"),
        relationship("refunds_account", "refunds", "accounts", "account_id", "id"),
        compound("orders_ledger", "orders", "ledgers", &["ref_a", "ref_b"]),
        compound("refunds_ledger", "refunds", "ledgers", &["ref_b", "ref_a"]),
        relationship("orders_ship_calendar", "orders", "calendar", "ship_day_key", "day_key"),
    ];
    let channel = Dimension::new(
        DimensionName::parse("channel_name").expect("a test dimension is a dimension"),
        column("channel_name"),
        Some(ViaChain::of(vec![relationship_name("orders_channel")]).expect("a chain has a hop")),
        None,
        Description::default(),
    );
    let region = Dimension::new(
        DimensionName::parse("region_name").expect("a test dimension is a dimension"),
        column("region_name"),
        Some(
            ViaChain::of(vec![
                relationship_name("orders_customer"),
                relationship_name("customers_region"),
            ])
            .expect("a chain has a hop"),
        ),
        Some(BTreeSet::from(
            ["EMEA", "APAC"].map(|value| DimensionValue::parse(value).expect("a test value is a value")),
        )),
        Description::default(),
    );
    let on_accounts = |name: &str| {
        Dimension::new(
            DimensionName::parse(name).expect("a test dimension is a dimension"),
            column(name),
            Some(ViaChain::of(vec![relationship_name("orders_account")]).expect("a chain has a hop")),
            None,
            Description::default(),
        )
    };
    let book = Dimension::new(
        DimensionName::parse("book").expect("a test dimension is a dimension"),
        column("book"),
        Some(ViaChain::of(vec![relationship_name("orders_ledger")]).expect("a chain has a hop")),
        None,
        Description::default(),
    );
    let ship_day = Dimension::new(
        DimensionName::parse("ship_day").expect("a test dimension is a dimension"),
        column("order_date"),
        Some(ViaChain::of(vec![relationship_name("orders_ship_calendar")]).expect("a chain has a hop")),
        None,
        Description::default(),
    );
    let revenue = Metric::new(
        metric_name("revenue"),
        model_name("orders"),
        simple("amount_cents"),
        vec![RequiredFilter::Equals {
            column: column("status"),
            value: DimensionValue::parse("paid").expect("a test value is a value"),
        }],
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        vec![channel, region, on_accounts("tier")],
        None,
        Description::parse(bundle.revenue_description).expect("a test description is a description"),
        Audience::Open,
    )
    .expect("the dimensions do not collide");
    let net = Metric::new(
        metric_name("net_revenue"),
        model_name("orders"),
        Measure::Ratio {
            numerator: sum("amount_cents"),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::Sum,
                column("refund_cents"),
                model_name("refunds"),
            )),
            zero_denominator: ZeroDenominator::Null,
        },
        vec![RequiredFilter::Equals {
            column: column("status"),
            value: DimensionValue::parse("paid").expect("a test value is a value"),
        }],
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        vec![ship_day, on_accounts("segment"), book],
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("the dimensions do not collide")
    .with_shared_calendar(model_name("calendar"));
    let mut metrics = vec![revenue, net];
    if bundle.with_another_metric {
        metrics.push(
            Metric::new(
                metric_name("order_value"),
                model_name("orders"),
                simple("amount_cents"),
                Vec::new(),
                column("order_date"),
                BTreeSet::from([Grain::Month]),
                Vec::new(),
                None,
                Description::default(),
                Audience::Open,
            )
            .expect("no dimensions to collide"),
        );
    }
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        Definitions::assemble(models, relationships, metrics).expect("the test bundle is consistent"),
        Knowledge::none(),
        ContributionManifest::single(source("sales"), Contribution::of(MetadataCapabilities::nothing())),
    )
    .expect("the test definitions hash")
}

fn col(of_table: &str, of_column: &str) -> PlanColumn {
    PlanColumn::new(table(of_table), column(of_column))
}

fn day(raw: &str) -> Date {
    Date::parse(raw).expect("a test date is a date")
}

fn range() -> TimeRange {
    TimeRange::new(day("2026-01-01"), day("2026-02-01")).expect("January is a range")
}

fn in_range(of: PlanColumn) -> [PlanFilter; 2] {
    [
        PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::AtOrAfter {
                column: of.clone(),
                param: 0,
            },
        ),
        PlanFilter::new(PredicateOrigin::Definition, PlanPredicate::Before { column: of, param: 1 }),
    ]
}

fn dated() -> Vec<ParamValue> {
    vec![ParamValue::Date(day("2026-01-01")), ParamValue::Date(day("2026-02-01"))]
}

fn orders_path() -> QualifiedTable {
    QualifiedTable::from(table("orders"))
}

/// What the sender plans for `revenue` grouped by channel: the metric's own table and one join.
fn revenue_leg(sender: &PinnedDefinitions) -> (LegPlan, Vec<ParamValue>) {
    let relationship = sender
        .definitions()
        .relationship(&relationship_name("orders_channel"))
        .expect("declared");
    let channels = sender.definitions().model(&model_name("channels")).expect("declared");
    let [from, until] = in_range(col("orders", "order_date"));
    let paid = PlanFilter::new(
        PredicateOrigin::Definition,
        PlanPredicate::Equals {
            column: col("orders", "status"),
            param: 2,
        },
    );
    let mut params = dated();
    params.push(ParamValue::Text(String::from("paid")));
    let leg = LegPlan::Fact {
        source: source("sales"),
        metric: metric_name("revenue"),
        tables: StatementTables::parse(orders_path(), vec![PlanJoin::hop(relationship, &table("orders"), channels)])
            .expect("two tables"),
        bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Month, col("orders", "order_date")),
        keys: vec![
            PlanKey::new(
                ResultLabel::dimension(&DimensionName::parse("channel_name").expect("a dimension")),
                col("channels", "channel_name"),
            ),
            PlanKey::new(ResultLabel::internal(InternalLabel::link(0)), col("orders", "customer_id")),
        ],
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: col("orders", "amount_cents"),
            },
            ResultLabel::internal(InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::parse(vec![from, until, paid], params.clone()).expect("coherent"),
        range: range(),
    };
    (leg, params)
}

/// The remote dimension's leg: the customers a region filter keeps, and the region each is in.
fn region_leg(sender: &PinnedDefinitions) -> (LegPlan, Vec<ParamValue>) {
    let relationship = sender
        .definitions()
        .relationship(&relationship_name("customers_region"))
        .expect("declared");
    let regions = sender.definitions().model(&model_name("regions")).expect("declared");
    let params = vec![ParamValue::Text(String::from("EMEA"))];
    let leg = LegPlan::Lookup {
        source: source("crm"),
        metric: crate::model::MetricName::parse("revenue").expect("a test metric is a metric"),
        table: StatementTables::parse(
            table("customers"),
            vec![PlanJoin::hop(relationship, &table("customers"), regions)],
        )
        .expect("two tables"),
        keys: vec![
            PlanKey::new(ResultLabel::internal(InternalLabel::link(0)), col("customers", "id")),
            PlanKey::new(
                ResultLabel::dimension(&DimensionName::parse("region_name").expect("a dimension")),
                col("regions", "region_name"),
            ),
        ],
        bindings: PlanBindings::parse(
            vec![PlanFilter::new(
                PredicateOrigin::Requested,
                PlanPredicate::Equals {
                    column: col("regions", "region_name"),
                    param: 0,
                },
            )],
            params.clone(),
        )
        .expect("coherent"),
    };
    (leg, params)
}

/// The refund leg of `net_revenue`: the second fact model, bucketed through the shared calendar.
fn refund_leg(sender: &PinnedDefinitions) -> (LegPlan, Vec<ParamValue>) {
    let relationship = sender
        .definitions()
        .relationship(&relationship_name("refunds_calendar"))
        .expect("declared");
    let calendar = sender.definitions().model(&model_name("calendar")).expect("declared");
    let leg = LegPlan::Fact {
        source: source("sales"),
        metric: metric_name("net_revenue"),
        tables: StatementTables::parse(
            table("refunds"),
            vec![PlanJoin::hop(relationship, &table("refunds"), calendar)],
        )
        .expect("two tables"),
        bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Month, col("calendar", "order_date")),
        keys: vec![PlanKey::new(
            ResultLabel::internal(InternalLabel::link(0)),
            col("refunds", "account_id"),
        )],
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: col("refunds", "refund_cents"),
            },
            ResultLabel::internal(InternalLabel::Leaf(1)),
        )],
        bindings: PlanBindings::parse(in_range(col("calendar", "order_date")).into(), dated()).expect("coherent"),
        range: range(),
    };
    (leg, dated())
}

/// Over a real serializer, so a cell reads what a transport would carry.
fn sent(leg: &LegPlan, sender: &PinnedDefinitions) -> Value {
    serde_json::to_value(to_wire(leg, sender).expect("the sender's own leg is sendable")).expect("a leg serializes")
}

fn received(json: Value) -> Result<LegWire, serde_json::Error> {
    serde_json::from_value(json)
}

fn well_formed(json: Value) -> LegWire {
    received(json).expect("a well-formed document")
}

/// The receiver's verdict on a document, with the values that travelled beside it.
fn parsed(json: Value, receiver: &PinnedDefinitions, params: Vec<ParamValue>) -> Result<LegPlan, LegNotCertified> {
    well_formed(json).parse(receiver, params)
}

fn refusal(json: Value, params: Vec<ParamValue>) -> LegNotCertified {
    parsed(json, &pinned(ORDINARY), params).expect_err("the receiver does not certify this leg")
}

fn fact() -> (Value, Vec<ParamValue>) {
    let (leg, params) = revenue_leg(&pinned(ORDINARY));
    (sent(&leg, &pinned(ORDINARY)), params)
}

#[test]
fn a_fact_leg_arrives_as_the_leg_the_sender_planned() {
    let sender = pinned(ORDINARY);
    let (leg, params) = revenue_leg(&sender);
    assert_eq!(
        parsed(sent(&leg, &sender), &pinned(ORDINARY), params).expect("certified"),
        leg
    );
}

#[test]
fn a_lookup_leg_arrives_as_the_leg_the_sender_planned() {
    let sender = pinned(ORDINARY);
    let (leg, params) = region_leg(&sender);
    assert_eq!(
        parsed(sent(&leg, &sender), &pinned(ORDINARY), params).expect("certified"),
        leg
    );
}

#[test]
fn the_second_fact_of_a_cross_model_ratio_arrives_without_the_first_models_required_filters() {
    let sender = pinned(ORDINARY);
    let (leg, params) = refund_leg(&sender);
    assert_eq!(
        parsed(sent(&leg, &sender), &pinned(ORDINARY), params).expect("certified"),
        leg
    );
}

#[test]
fn a_leg_whose_metric_digest_differs_is_refused() {
    let sender = pinned(Bundle {
        revenue_description: "Orders, any status",
        ..ORDINARY
    });
    let (leg, params) = revenue_leg(&sender);
    let refused = parsed(sent(&leg, &sender), &pinned(ORDINARY), params).expect_err("revenue means another thing here");
    assert!(
        matches!(refused, LegNotCertified::Metric(NotAgreed::Differs { ref metric }) if *metric == metric_name("revenue")),
        "{refused:?}"
    );
}

#[test]
fn a_leg_naming_a_metric_the_receiver_does_not_define_is_refused() {
    let (mut json, params) = fact();
    json["metric"]["metric"] = json!("churn");
    let refused = refusal(json, params);
    assert!(
        matches!(refused, LegNotCertified::Metric(NotAgreed::MetricUnknown { .. })),
        "{refused:?}"
    );
}

#[test]
fn a_fact_leg_with_no_digest_for_its_metric_does_not_parse() {
    let (mut json, _) = fact();
    json["metric"]
        .as_object_mut()
        .expect("a metric digest is an object")
        .remove("digest");
    let error = received(json).expect_err("a metric without its digest is no leg");
    assert!(error.to_string().contains("digest"), "{error}");
}

#[test]
fn a_digest_that_is_not_a_hash_does_not_parse() {
    let (mut json, _) = fact();
    json["metric"]["digest"] = json!("not a hash");
    received(json).expect_err("a digest is 64 hex digits");
}

#[test]
fn a_field_the_document_does_not_declare_does_not_parse() {
    let (json, _) = fact();
    for (at, undeclared) in [("", "sql"), ("bucket", "label"), ("metric", "bundle_digest")] {
        let mut tampered = json.clone();
        let object = if at.is_empty() { &mut tampered } else { &mut tampered[at] };
        object[undeclared] = json!("select 1");
        let error = received(tampered).expect_err("an undeclared field is an error");
        assert!(error.to_string().contains(undeclared), "{at}: {error}");
    }
}

#[test]
fn a_leg_whose_bundle_digest_differs_and_whose_metric_digests_agree_is_accepted() {
    let sender = pinned(ORDINARY);
    let receiver = pinned(Bundle {
        with_another_metric: true,
        ..ORDINARY
    });
    assert_ne!(sender.digest(), receiver.digest(), "the two bundles are two versions");
    let (leg, params) = revenue_leg(&sender);
    assert_eq!(
        parsed(sent(&leg, &sender), &receiver, params).expect("the metric is the same"),
        leg
    );
}

#[test]
fn a_table_the_receiver_does_not_certify_for_the_metric_and_source_is_refused() {
    let (json, params) = fact();
    for (source, table) in [
        ("sales", "salaries"),
        ("sales", "customers"),
        ("sales", "channels"),
        ("crm", "orders"),
    ] {
        let mut tampered = json.clone();
        tampered["source"] = json!(source);
        tampered["table"] = json!(table);
        let refused = refusal(tampered, params.clone());
        assert!(
            matches!(refused, LegNotCertified::TableNotCertified { .. }),
            "{source}.{table}: {refused:?}"
        );
    }
}

#[test]
fn a_join_the_receiver_does_not_declare_from_the_statement_inside_the_source_is_refused() {
    let (json, params) = fact();
    for relationship in ["orders_secrets", "customers_region", "orders_customer", "refunds_calendar"] {
        let mut tampered = json.clone();
        tampered["joins"] = json!([relationship]);
        let refused = refusal(tampered, params.clone());
        assert!(
            matches!(refused, LegNotCertified::RelationshipNotCertified { .. }),
            "{relationship}: {refused:?}"
        );
    }
}

#[test]
fn a_column_the_statements_tables_do_not_declare_is_refused() {
    let (json, params) = fact();
    for (at, column, of_table) in [
        ("keys", "salary", "orders"),
        ("keys", "region_name", "regions"),
        ("filters", "salary", "orders"),
    ] {
        let mut tampered = json.clone();
        let entry = if at == "keys" {
            &mut tampered["keys"][0]["column"]
        } else {
            &mut tampered["filters"][0]["predicate"]["at_or_after"]["column"]
        };
        *entry = json!({"table": of_table, "column": column});
        let refused = refusal(tampered, params.clone());
        assert!(
            matches!(refused, LegNotCertified::ColumnNotCertified { .. }),
            "{at} {column}: {refused:?}"
        );
    }
}

#[test]
fn a_grain_and_a_bucket_column_the_metric_does_not_declare_are_refused() {
    let (json, params) = fact();
    let mut daily = json.clone();
    daily["bucket"]["grain"] = json!("day");
    assert!(matches!(
        refusal(daily, params.clone()),
        LegNotCertified::GrainNotDeclared { grain: Grain::Day, .. }
    ));
    let mut other = json;
    other["bucket"]["column"] = json!({"table": "orders", "column": "customer_id"});
    assert!(matches!(refusal(other, params), LegNotCertified::BucketNotTimeColumn { .. }));
}

#[test]
fn a_term_over_a_column_the_metric_does_not_measure_is_refused() {
    let (mut json, params) = fact();
    json["terms"][0]["term"]["aggregate"]["column"] = json!({"table": "orders", "column": "customer_id"});
    let refused = refusal(json, params);
    assert!(
        matches!(refused, LegNotCertified::TermNotMetrics { ref column, .. } if *column == self::column("customer_id")),
        "{refused:?}"
    );
}

#[test]
fn a_bucket_on_a_joined_tables_column_of_the_same_name_is_refused() {
    let (mut json, params) = fact();
    json["joins"] = json!(["orders_channel", "orders_calendar"]);
    json["bucket"]["column"] = json!({"table": "calendar", "column": "order_date"});
    let refused = refusal(json, params);
    assert!(
        matches!(refused, LegNotCertified::BucketNotTimeColumn { ref table, .. } if *table == self::table("calendar")),
        "{refused:?}"
    );
}

#[test]
fn a_term_on_a_column_the_measure_reads_from_another_table_is_refused() {
    let sender = pinned(ORDINARY);
    let (leg, params) = refund_leg(&sender);
    let mut json = sent(&leg, &sender);
    json["terms"][0]["term"]["aggregate"]["column"] = json!({"table": "refunds", "column": "amount_cents"});
    let refused = refusal(json, params);
    assert!(
        matches!(refused, LegNotCertified::TermNotMetrics { ref table, .. } if *table == self::table("refunds")),
        "{refused:?}"
    );
}

#[test]
fn a_term_on_a_table_the_leg_is_not_rooted_on_is_refused() {
    let sender = pinned(ORDINARY);
    let (leg, params) = refund_leg(&sender);
    let mut json = sent(&leg, &sender);
    json["joins"] = json!(["refunds_calendar", "refunds_order"]);
    json["terms"][0]["term"]["aggregate"]["column"] = json!({"table": "orders", "column": "amount_cents"});
    let refused = refusal(json, params);
    assert!(
        matches!(refused, LegNotCertified::TermNotMetrics { ref table, .. } if *table == self::table("orders")),
        "{refused:?}"
    );
}

#[test]
fn a_term_of_the_other_model_on_the_metrics_own_leg_is_refused() {
    let sender = pinned(ORDINARY);
    let (leg, mut params) = refund_leg(&sender);
    let mut json = sent(&leg, &sender);
    let (revenue, _) = fact();
    json["table"] = json!("orders");
    json["joins"] = json!(["orders_calendar", "calendar_refunds"]);
    json["keys"][0]["column"] = json!({"table": "orders", "column": "customer_id"});
    json["filters"]
        .as_array_mut()
        .expect("filters are a list")
        .push(revenue["filters"][2].clone());
    params.push(ParamValue::Text(String::from("paid")));
    let refused = refusal(json, params);
    assert!(
        matches!(refused, LegNotCertified::TermNotMetrics { ref table, .. } if *table == self::table("refunds")),
        "{refused:?}"
    );
}

#[test]
fn a_fact_leg_not_bounded_by_its_own_range_is_refused() {
    let (json, params) = fact();
    let mut unbounded = json.clone();
    unbounded["filters"].as_array_mut().expect("filters are a list").drain(..2);
    let refused = refusal(unbounded, params.clone());
    assert!(
        matches!(refused, LegNotCertified::RangeNotBound { .. }),
        "unbounded: {refused:?}"
    );
    let mut elsewhere = params;
    if let Some(start) = elsewhere.first_mut() {
        *start = ParamValue::Date(day("1900-01-01"));
    }
    let refused = refusal(json, elsewhere);
    assert!(
        matches!(refused, LegNotCertified::RangeNotBound { .. }),
        "other dates: {refused:?}"
    );
}

#[test]
fn a_range_longer_than_this_node_plans_is_refused() {
    let (mut json, params) = fact();
    json["range"] = json!({"start": "1900-01-01", "end": "2100-01-01"});
    let refused = refusal(json, params);
    assert!(
        matches!(refused, LegNotCertified::RangeTooLong { limit, .. } if limit == crate::query::MAX_RANGE_DAYS),
        "{refused:?}"
    );
}

#[test]
fn a_join_from_a_table_only_a_later_join_reaches_is_refused() {
    let (mut json, params) = fact();
    json["joins"] = json!(["orders_channel", "calendar_refunds", "orders_calendar"]);
    let refused = refusal(json, params);
    assert!(
        matches!(refused, LegNotCertified::RelationshipNotCertified { ref relationship } if *relationship == relationship_name("calendar_refunds")),
        "{refused:?}"
    );
}

#[test]
fn a_leg_without_the_receivers_required_filter_is_refused_and_so_is_one_bound_to_another_value() {
    let (json, params) = fact();
    let mut absent = json.clone();
    absent["filters"].as_array_mut().expect("filters are a list").pop();
    let refused = refusal(absent, params[..2].to_vec());
    assert!(
        matches!(refused, LegNotCertified::RequiredFilterMissing { ref column, .. } if *column == self::column("status")),
        "{refused:?}"
    );
    let mut refunded = params;
    refunded[2] = ParamValue::Text(String::from("refunded"));
    assert!(matches!(
        refusal(json.clone(), refunded),
        LegNotCertified::RequiredFilterMissing { .. }
    ));
    let mut requested = json;
    requested["filters"][2]["origin"] = json!("requested");
    let (_, params) = fact();
    assert!(matches!(
        refusal(requested, params),
        LegNotCertified::RequiredFilterMissing { .. }
    ));
}

#[test]
fn a_part_longer_than_the_receivers_own_definitions_could_make_is_refused() {
    let (json, params) = fact();
    for (part, list, entry) in [
        (LegPart::Keys, "keys", json["keys"][0].clone()),
        (LegPart::Terms, "terms", json["terms"][0].clone()),
        (LegPart::Filters, "filters", json["filters"][0].clone()),
        (LegPart::Joins, "joins", json!("orders_channel")),
    ] {
        let mut tampered = json.clone();
        tampered[list] = Value::Array(vec![entry; 40]);
        let refused = refusal(tampered, params.clone());
        assert!(
            matches!(refused, LegNotCertified::TooMany { part: found, .. } if found == part),
            "{part:?}: {refused:?}"
        );
    }
}

#[test]
fn a_fact_leg_may_carry_a_key_for_every_dimension_every_lookup_link_part_and_two_distinct_keys() {
    let widest = 2;
    let limit = crate::query::MAX_DIMENSIONS + crate::query::MAX_LOOKUP_LEGS * widest + 2;
    let (json, params) = fact();
    let mut full = json.clone();
    full["keys"] = Value::Array(vec![json["keys"][0].clone(); limit]);
    let accepted = parsed(full, &pinned(ORDINARY), params.clone());
    assert!(accepted.is_ok(), "{accepted:?}");
    let mut over = json.clone();
    over["keys"] = Value::Array(vec![json["keys"][0].clone(); limit + 1]);
    let refused = parsed(over, &pinned(ORDINARY), params);
    assert!(
        matches!(refused, Err(LegNotCertified::TooMany { part: LegPart::Keys, limit: found }) if found == limit),
        "{refused:?}"
    );
}

#[test]
fn a_join_named_twice_is_a_statement_that_names_one_table_twice() {
    let (mut json, params) = fact();
    json["joins"] = json!(["orders_channel", "orders_channel"]);
    assert!(matches!(refusal(json, params), LegNotCertified::Tables(_)));
}

#[test]
fn values_that_the_predicates_do_not_bind_are_refused() {
    let (json, mut params) = fact();
    params.push(ParamValue::Text(String::from("unread")));
    assert!(matches!(refusal(json, params), LegNotCertified::Bindings(_)));
}

#[test]
fn a_label_that_names_neither_an_internal_label_nor_a_dimension_does_not_parse() {
    let (mut json, _) = fact();
    for label in ["0_leaf_07", "0_link_0", "0_link_", "0_link_01", "0_other", "not a name", ""] {
        json["keys"][0]["label"] = json!(label);
        assert!(received(json.clone()).is_err(), "{label:?}");
    }
    for label in ["0_leaf_7", "2_link", "2_link_1"] {
        WireLabel::parse(label).expect("a canonical internal label");
    }
}

#[test]
fn a_leg_over_a_metric_the_senders_own_pin_does_not_define_cannot_be_written() {
    let sender = pinned(ORDINARY);
    let (leg, _) = revenue_leg(&sender);
    let LegPlan::Fact {
        source,
        tables,
        bucket,
        keys,
        terms,
        bindings,
        range,
        ..
    } = leg
    else {
        panic!("a fact leg");
    };
    let churn = LegPlan::Fact {
        source,
        metric: metric_name("churn"),
        tables,
        bucket,
        keys,
        terms,
        bindings,
        range,
    };
    assert!(matches!(to_wire(&churn, &sender), Err(NotSendable::Digest(_))));
}

#[test]
fn a_leg_with_a_key_truncated_to_a_grain_cannot_be_written() {
    let sender = pinned(ORDINARY);
    let (leg, _) = region_leg(&sender);
    let LegPlan::Lookup {
        source,
        metric,
        table,
        keys,
        bindings,
    } = leg
    else {
        panic!("a lookup leg");
    };
    let month = PlanKey::truncated(
        ResultLabel::internal(InternalLabel::link(0)),
        col("customers", "id"),
        Grain::Month,
    );
    let lookup = LegPlan::Lookup {
        source,
        metric,
        table,
        keys: std::iter::once(month).chain(keys.into_iter().skip(1)).collect(),
        bindings,
    };
    let refused = to_wire(&lookup, &sender);
    assert!(matches!(refused, Err(NotSendable::KeyTruncated { .. })), "{refused:?}");
}

#[cfg(test)]
mod calendars;
#[cfg(test)]
mod crossings;
#[cfg(test)]
mod surface;

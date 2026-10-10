use core::cell::RefCell;
use std::collections::VecDeque;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::identity::{Presented, Secret};
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
use sutura_domain::nonempty::NonEmpty;
use sutura_domain::plan::{
    Executable, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PlannedMeasure,
    PredicateOrigin, QueryPlan, ResultLabel, StatementTables,
};
use sutura_domain::query::{Top, TopBy, TopDirection, TopN};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::{ParamValue, Warehouse as _};

use crate::{Answer, CubeError, CubeTransport, CubeWarehouse};

const TOKEN: &str = "declared-cube-token-canary-41d7";
const TABLE: &str = "fct_subscription_monthly";

/// What each request sent: the token, and the query for a load or none for the metadata read.
type Sent = Vec<(String, Option<serde_json::Value>)>;

/// Cube's answers, scripted: the metadata answer, then one answer per load. Records what was sent.
struct Scripted {
    meta: Option<Vec<u8>>,
    load: RefCell<VecDeque<Vec<u8>>>,
    sent: RefCell<Sent>,
}

#[derive(Debug, thiserror::Error)]
#[error("connection refused")]
struct Down;

impl Scripted {
    fn answering(load: Vec<u8>) -> Self {
        Self::answering_each(vec![load])
    }

    /// One answer per load, in order.
    fn answering_each(loads: Vec<Vec<u8>>) -> Self {
        Self {
            meta: Some(bytes(&served())),
            load: RefCell::new(loads.into()),
            sent: RefCell::new(Vec::new()),
        }
    }

    fn loads(&self) -> Vec<serde_json::Value> {
        self.sent.borrow().iter().filter_map(|(_, query)| query.clone()).collect()
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "a cell asserting WHICH token reached Cube needs its text; production never reads it as a string"
)]
fn text_of(token: &Secret) -> String {
    String::from(token.expose_secret())
}

fn bytes(value: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("a test answer encodes")
}

impl CubeTransport for Scripted {
    type Error = Down;

    fn meta(&self, token: &Secret, _deadline: Deadline) -> Answer<Self::Error> {
        self.sent.borrow_mut().push((text_of(token), None));
        self.meta.clone().ok_or(Down)
    }

    /// A load the cell did not script answers as Cube down, which no cell expects.
    fn load(&self, query: &serde_json::Value, token: &Secret, _deadline: Deadline) -> Answer<Self::Error> {
        self.sent.borrow_mut().push((text_of(token), Some(query.clone())));
        self.load.borrow_mut().pop_front().ok_or(Down)
    }

    fn source_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }
}

/// The members the test plans name, as Cube's metadata answer lists them.
fn served() -> serde_json::Value {
    serde_json::json!({ "cubes": [{
        "name": TABLE,
        "measures": [{ "name": "fct_subscription_monthly.active_subscriptions" },
                     { "name": "fct_subscription_monthly.subscription_base" },
                     { "name": "fct_subscription_monthly.annual_subscriptions" },
                     { "name": "fct_subscription_monthly.revenue_per_churned_subscription" }],
        "dimensions": [{ "name": "fct_subscription_monthly.month" },
                       { "name": "fct_subscription_monthly.status" },
                       { "name": "fct_subscription_monthly.contract_term" }],
        "segments": []
    }]})
}

fn declared() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(AcknowledgementReason::parse("one Cube token for every caller").expect("a reason"))
}

fn source() -> SourceName {
    SourceName::parse("metrics").expect("a name")
}

fn warehouse(transport: Scripted) -> CubeWarehouse<Scripted> {
    CubeWarehouse::of(
        source(),
        SourcePosture::SharedServiceUser { declared: declared() },
        Secret::new(String::from(TOKEN)),
        transport,
    )
}

fn shared() -> Presented {
    Presented::SharedServiceUser { declared: declared() }
}

fn deadline() -> Deadline {
    Deadline::opened_at(
        std::time::Instant::now(),
        Budget::parse(core::time::Duration::from_secs(30)).expect("a budget"),
    )
}

fn column(name: &str) -> PlanColumn {
    PlanColumn::new(
        TableName::parse(TABLE).expect("a table"),
        ColumnName::parse(name).expect("a column"),
    )
}

fn metric(name: &str) -> MetricName {
    MetricName::parse(name).expect("a metric")
}

fn june() -> TimeRange {
    TimeRange::new(Date::new(2026, 6, 1).expect("a date"), Date::new(2026, 7, 1).expect("a date")).expect("a range")
}

fn bucket() -> PlanBucket {
    PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("month"))
}

/// The range, then `extra`, bound in that order after `leading` parameters.
fn bindings(leading: Vec<(PlanFilter, ParamValue)>, requested: Option<(&str, &str)>) -> PlanBindings {
    let mut filters: Vec<PlanFilter> = Vec::new();
    let mut params: Vec<ParamValue> = Vec::new();
    for (filter, param) in leading {
        filters.push(filter);
        params.push(param);
    }
    let at = params.len();
    filters.push(PlanFilter::new(
        PredicateOrigin::Definition,
        PlanPredicate::AtOrAfter {
            column: column("month"),
            param: at,
        },
    ));
    filters.push(PlanFilter::new(
        PredicateOrigin::Definition,
        PlanPredicate::Before {
            column: column("month"),
            param: at + 1,
        },
    ));
    params.push(ParamValue::Date(june().start()));
    params.push(ParamValue::Date(june().end()));
    if let Some((on, value)) = requested {
        filters.push(PlanFilter::new(
            PredicateOrigin::Requested,
            PlanPredicate::Equals {
                column: column(on),
                param: at + 2,
            },
        ));
        params.push(ParamValue::Text(String::from(value)));
    }
    PlanBindings::parse(filters, params).expect("the test bindings bind")
}

fn status_is_active(param: usize) -> PlanPredicate {
    PlanPredicate::Equals {
        column: column("status"),
        param,
    }
}

fn distinct_subscriptions() -> PlanMeasure {
    PlanMeasure::Simple {
        term: PlanTerm::Aggregate {
            aggregate: Aggregate::CountDistinct,
            column: column("subscription_key"),
        },
    }
}

/// `active_subscriptions` by month in June: one metric, its required `status = 'active'`, and
/// `requested` if any.
fn active_subscriptions(requested: Option<(&str, &str)>) -> QueryPlan {
    let required = (
        PlanFilter::new(PredicateOrigin::Definition, status_is_active(0)),
        ParamValue::Text(String::from("active")),
    );
    QueryPlan::new(
        source(),
        metric("active_subscriptions"),
        StatementTables::only(TableName::parse(TABLE).expect("a table")),
        bucket(),
        Vec::new(),
        distinct_subscriptions(),
        ResultLabel::measure(&metric("active_subscriptions")),
        bindings(vec![required], requested),
        june(),
    )
}

fn answer(rows: serde_json::Value) -> Vec<u8> {
    let mut answer = serde_json::json!({ "query": {} });
    answer["data"] = rows;
    bytes(&answer)
}

fn june_row(member: &str, value: &str) -> serde_json::Value {
    serde_json::json!([{ "fct_subscription_monthly.month.month": "2026-06-01T00:00:00.000", member: value }])
}

fn filters(query: &serde_json::Value) -> Vec<serde_json::Value> {
    query["filters"].as_array().expect("the query carries filters").clone()
}

#[test]
fn a_required_filter_reaches_the_load_query_as_a_cube_filter() {
    let warehouse = warehouse(Scripted::answering(answer(june_row(
        "fct_subscription_monthly.active_subscriptions",
        "59",
    ))));
    let rows = warehouse
        .execute(Executable::Query(&active_subscriptions(None)), &shared(), deadline())
        .expect("Cube answers");
    assert_eq!(rows.rows(), 1);
    let loads = warehouse.transport.loads();
    let [query] = loads.as_slice() else {
        panic!("one load: {loads:?}")
    };
    assert!(
        filters(query).contains(&serde_json::json!({
            "member": "fct_subscription_monthly.status", "operator": "equals", "values": ["active"]
        })),
        "{query}"
    );
}

#[test]
fn every_filter_becomes_a_cube_filter_with_json_values() {
    let warehouse = warehouse(Scripted::answering(answer(june_row(
        "fct_subscription_monthly.active_subscriptions",
        "12",
    ))));
    let plan = active_subscriptions(Some(("contract_term", "annual' OR 1=1")));
    drop(
        warehouse
            .execute(Executable::Query(&plan), &shared(), deadline())
            .expect("Cube answers"),
    );
    let query = warehouse.transport.loads().remove(0);
    assert_eq!(
        filters(&query),
        [
            serde_json::json!({ "member": "fct_subscription_monthly.status", "operator": "equals", "values": ["active"] }),
            serde_json::json!({ "member": "fct_subscription_monthly.month", "operator": "afterOrOnDate", "values": ["2026-06-01"] }),
            serde_json::json!({ "member": "fct_subscription_monthly.month", "operator": "beforeDate", "values": ["2026-07-01"] }),
            serde_json::json!({ "member": "fct_subscription_monthly.contract_term", "operator": "equals", "values": ["annual' OR 1=1"] }),
        ]
    );
    assert_eq!(
        query["measures"],
        serde_json::json!(["fct_subscription_monthly.active_subscriptions"])
    );
    assert_eq!(
        query["timeDimensions"],
        serde_json::json!([{ "dimension": "fct_subscription_monthly.month", "granularity": "month" }])
    );
}

#[test]
fn a_required_filter_on_a_member_cube_does_not_serve_refuses_before_any_load() {
    let mut transport = Scripted::answering(answer(serde_json::json!([])));
    let mut meta = served();
    meta["cubes"][0]["dimensions"] = serde_json::json!([{ "name": "fct_subscription_monthly.month" }]);
    transport.meta = Some(bytes(&meta));
    let warehouse = warehouse(transport);
    let refused = warehouse
        .execute(Executable::Query(&active_subscriptions(None)), &shared(), deadline())
        .expect_err("Cube serves no status member");
    assert!(
        matches!(refused, CubeError::AbsentMember { ref member } if member == "fct_subscription_monthly.status"),
        "{refused:?}"
    );
    assert_eq!(warehouse.transport.loads(), Vec::<serde_json::Value>::new());
}

#[test]
fn a_metric_cube_does_not_serve_refuses_before_any_load() {
    let mut transport = Scripted::answering(answer(serde_json::json!([])));
    let mut meta = served();
    meta["cubes"][0]["measures"] = serde_json::json!([]);
    transport.meta = Some(bytes(&meta));
    let warehouse = warehouse(transport);
    let refused = warehouse
        .execute(Executable::Query(&active_subscriptions(None)), &shared(), deadline())
        .expect_err("Cube serves no such measure");
    assert!(
        matches!(refused, CubeError::AbsentMember { ref member } if member == "fct_subscription_monthly.active_subscriptions"),
        "{refused:?}"
    );
    assert_eq!(warehouse.transport.loads(), Vec::<serde_json::Value>::new());
}

#[test]
fn a_guard_beside_another_metric_is_asked_in_its_own_query_and_joined_on_the_groups() {
    let guarded = PlannedMeasure::new(
        metric("active_subscriptions"),
        ResultLabel::measure(&metric("active_subscriptions")),
        distinct_subscriptions(),
        vec![status_is_active(0)],
    );
    let other = PlannedMeasure::new(
        metric("subscription_base"),
        ResultLabel::measure(&metric("subscription_base")),
        distinct_subscriptions(),
        Vec::new(),
    );
    let guard = (
        PlanFilter::new(PredicateOrigin::Definition, status_is_active(0)),
        ParamValue::Text(String::from("active")),
    );
    let plan = QueryPlan::with_measures(
        source(),
        NonEmpty::of(guarded, vec![other]),
        StatementTables::only(TableName::parse(TABLE).expect("a table")),
        bucket(),
        Vec::new(),
        bindings(vec![guard], None),
        june(),
    );
    let may = serde_json::json!({ "fct_subscription_monthly.month.month": "2026-05-01T00:00:00.000" });
    let june_group = serde_json::json!({ "fct_subscription_monthly.month.month": "2026-06-01T00:00:00.000" });
    let warehouse = warehouse(Scripted::answering_each(vec![
        answer(serde_json::json!([may, june_group])),
        answer(june_row("fct_subscription_monthly.active_subscriptions", "59")),
        answer(serde_json::json!([
            { "fct_subscription_monthly.month.month": "2026-05-01T00:00:00.000", "fct_subscription_monthly.subscription_base": "61" },
            { "fct_subscription_monthly.month.month": "2026-06-01T00:00:00.000", "fct_subscription_monthly.subscription_base": "62" }
        ])),
    ]));
    let batches = warehouse
        .execute(Executable::Query(&plan), &shared(), deadline())
        .expect("Cube answers each query");
    let loads = warehouse.transport.loads();
    let [groups, active, base] = loads.as_slice() else {
        panic!("a group query and one query per metric: {loads:?}")
    };
    let status = serde_json::json!({ "member": "fct_subscription_monthly.status", "operator": "equals", "values": ["active"] });
    assert_eq!(groups["measures"], serde_json::json!([]));
    assert!(!filters(groups).contains(&status), "{groups}");
    assert!(filters(active).contains(&status), "{active}");
    assert!(!filters(base).contains(&status), "{base}");
    let rows = batches.to_rows().expect("the batches read back");
    let text = |date: &str| sutura_domain::warehouse::Value::Text(String::from(date));
    let integer = sutura_domain::warehouse::Value::Integer;
    assert_eq!(
        rows.rows(),
        [
            vec![text("2026-05-01"), integer(0), integer(61)],
            vec![text("2026-06-01"), integer(59), integer(62)],
        ]
    );
}

#[test]
fn the_declared_token_is_what_every_request_carries() {
    let warehouse = warehouse(Scripted::answering(answer(june_row(
        "fct_subscription_monthly.active_subscriptions",
        "59",
    ))));
    drop(
        warehouse
            .execute(Executable::Query(&active_subscriptions(None)), &shared(), deadline())
            .expect("Cube answers"),
    );
    let sent = warehouse.transport.sent.borrow();
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|(token, _)| token == TOKEN), "{sent:?}");
}

#[test]
fn a_subject_token_is_refused_before_any_request() {
    let warehouse = warehouse(Scripted::answering(answer(serde_json::json!([]))));
    let presented = Presented::SubjectToken {
        material: Secret::new(String::from("a-callers-token")),
        impersonate: None,
    };
    let refused = warehouse
        .execute(Executable::Query(&active_subscriptions(None)), &presented, deadline())
        .expect_err("a shared source sends only its declared token");
    assert!(matches!(refused, CubeError::NoPlaceForASubject { .. }), "{refused:?}");
    assert_eq!(*warehouse.transport.sent.borrow(), Sent::new());
}

#[test]
fn cube_down_is_a_typed_error_and_never_an_answer() {
    let mut transport = Scripted::answering(answer(serde_json::json!([])));
    transport.meta = None;
    let warehouse = warehouse(transport);
    let refused = warehouse
        .execute(Executable::Query(&active_subscriptions(None)), &shared(), deadline())
        .expect_err("Cube is down");
    assert!(matches!(refused, CubeError::Endpoint { .. }), "{refused:?}");
}

#[test]
fn the_answer_reads_as_the_plans_labels_and_types() {
    let warehouse = warehouse(Scripted::answering(answer(june_row(
        "fct_subscription_monthly.active_subscriptions",
        "59",
    ))));
    let batches = warehouse
        .execute(Executable::Query(&active_subscriptions(None)), &shared(), deadline())
        .expect("Cube answers");
    let rows = batches.to_rows().expect("the batches read back");
    assert_eq!(rows.columns(), ["period", "active_subscriptions"]);
    assert_eq!(
        rows.rows(),
        [vec![
            sutura_domain::warehouse::Value::Text(String::from("2026-06-01")),
            sutura_domain::warehouse::Value::Integer(59),
        ]]
    );
}

#[test]
fn no_value_for_a_metric_whose_zero_denominator_fails_is_an_error() {
    let plan = QueryPlan::new(
        source(),
        metric("revenue_per_churned_subscription"),
        StatementTables::only(TableName::parse(TABLE).expect("a table")),
        bucket(),
        vec![PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("contract_term").expect("a dimension")),
            column("contract_term"),
        )],
        PlanMeasure::Ratio {
            numerator: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("mrr_cents"),
            },
            denominator: PlanTerm::CountIf {
                column: column("churned_in_month"),
            },
            zero_denominator: ZeroDenominator::Fail,
        },
        ResultLabel::measure(&metric("revenue_per_churned_subscription")),
        bindings(Vec::new(), None),
        june(),
    );
    let warehouse = warehouse(Scripted::answering(answer(serde_json::json!([{
        "fct_subscription_monthly.contract_term": "annual",
        "fct_subscription_monthly.month.month": "2026-06-01T00:00:00.000",
        "fct_subscription_monthly.revenue_per_churned_subscription": null
    }]))));
    let refused = warehouse
        .execute(Executable::Query(&plan), &shared(), deadline())
        .expect_err("no value is not an answer for a metric that fails on zero");
    assert!(matches!(refused, CubeError::ZeroDenominator { .. }), "{refused:?}");
}

fn planned(name: &str, guard: Vec<PlanPredicate>) -> PlannedMeasure {
    PlannedMeasure::new(
        metric(name),
        ResultLabel::measure(&metric(name)),
        distinct_subscriptions(),
        guard,
    )
}

/// Three metrics by month in June: `active_subscriptions` under `status = 'active'`,
/// `annual_subscriptions` under `contract_term = 'annual'`, and `subscription_base` under none.
fn three_metrics() -> QueryPlan {
    let guard = |on: &str, param: usize, value: &str| {
        let predicate = PlanPredicate::Equals {
            column: column(on),
            param,
        };
        (
            (predicate.clone(), PlanFilter::new(PredicateOrigin::Definition, predicate)),
            ParamValue::Text(String::from(value)),
        )
    };
    let ((active, active_filter), active_value) = guard("status", 0, "active");
    let ((annual, annual_filter), annual_value) = guard("contract_term", 1, "annual");
    QueryPlan::with_measures(
        source(),
        NonEmpty::of(
            planned("active_subscriptions", vec![active]),
            vec![
                planned("annual_subscriptions", vec![annual]),
                planned("subscription_base", Vec::new()),
            ],
        ),
        StatementTables::only(TableName::parse(TABLE).expect("a table")),
        bucket(),
        Vec::new(),
        bindings(vec![(active_filter, active_value), (annual_filter, annual_value)], None),
        june(),
    )
}

#[test]
fn a_split_plan_asks_each_metric_under_its_own_required_filter_and_no_other() {
    let june_group = serde_json::json!([{ "fct_subscription_monthly.month.month": "2026-06-01T00:00:00.000" }]);
    let warehouse = warehouse(Scripted::answering_each(vec![
        answer(june_group),
        answer(june_row("fct_subscription_monthly.active_subscriptions", "59")),
        answer(june_row("fct_subscription_monthly.annual_subscriptions", "12")),
        answer(june_row("fct_subscription_monthly.subscription_base", "62")),
    ]));
    let batches = warehouse
        .execute(Executable::Query(&three_metrics()), &shared(), deadline())
        .expect("Cube answers each query");
    let equals = |on: &str, value: &str| serde_json::json!({ "member": format!("fct_subscription_monthly.{on}"), "operator": "equals", "values": [value] });
    let range = [
        serde_json::json!({ "member": "fct_subscription_monthly.month", "operator": "afterOrOnDate", "values": ["2026-06-01"] }),
        serde_json::json!({ "member": "fct_subscription_monthly.month", "operator": "beforeDate", "values": ["2026-07-01"] }),
    ];
    let with = |own: serde_json::Value| [vec![own], range.to_vec()].concat();
    let loads = warehouse.transport.loads();
    let asked: Vec<_> = loads
        .iter()
        .map(|query| (query["measures"].clone(), filters(query)))
        .collect();
    assert_eq!(
        asked,
        [
            (serde_json::json!([]), range.to_vec()),
            (
                serde_json::json!(["fct_subscription_monthly.active_subscriptions"]),
                with(equals("status", "active"))
            ),
            (
                serde_json::json!(["fct_subscription_monthly.annual_subscriptions"]),
                with(equals("contract_term", "annual"))
            ),
            (
                serde_json::json!(["fct_subscription_monthly.subscription_base"]),
                range.to_vec()
            ),
        ]
    );
    let rows = batches.to_rows().expect("the batches read back");
    let integer = sutura_domain::warehouse::Value::Integer;
    assert_eq!(
        rows.rows(),
        [vec![
            sutura_domain::warehouse::Value::Text(String::from("2026-06-01")),
            integer(59),
            integer(12),
            integer(62),
        ]]
    );
}

#[test]
fn a_top_over_a_split_plan_refuses_before_any_request() {
    let warehouse = warehouse(Scripted::answering(answer(serde_json::json!([]))));
    let plan = three_metrics().with_top(Top::new(TopN::parse(3).expect("a top"), TopBy::Metric, TopDirection::Desc));
    let refused = warehouse
        .execute(Executable::Query(&plan), &shared(), deadline())
        .expect_err("no one Cube query ranks the joined answer");
    assert!(
        matches!(
            refused,
            CubeError::NotExpressible {
                cause: crate::query::NotExpressible::TopAcrossMetricQueries
            }
        ),
        "{refused:?}"
    );
    assert_eq!(*warehouse.transport.sent.borrow(), Sent::new());
}

#![forbid(unsafe_code)]
//! An integer in a Cube answer, read over the HTTP transport against a loopback fake.

#[cfg(test)]
mod tests {
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::identity::{Presented, Secret};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
    use sutura_domain::plan::{
        Executable, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin,
        QueryPlan, ResultLabel, StatementTables,
    };
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_domain::warehouse::{ParamValue, ResultBatches, Warehouse as _};
    use sutura_exec_cube::http::{Endpoint, HttpCube, HttpCubeError, ReadBounds};
    use sutura_exec_cube::{CubeError, CubeWarehouse};
    use sutura_http_client::test_support::{FakeServer, Scripted};

    const TABLE: &str = "fct_subscription_monthly";
    const MEASURE: &str = "fct_subscription_monthly.active_subscriptions";

    fn column(name: &str) -> PlanColumn {
        PlanColumn::new(
            TableName::parse(TABLE).expect("a table"),
            ColumnName::parse(name).expect("a column"),
        )
    }

    /// `active_subscriptions` by month in June 2026.
    fn plan() -> QueryPlan {
        let metric = MetricName::parse("active_subscriptions").expect("a metric");
        let june =
            TimeRange::new(Date::new(2026, 6, 1).expect("a date"), Date::new(2026, 7, 1).expect("a date")).expect("a range");
        let range = [
            PlanPredicate::AtOrAfter {
                column: column("month"),
                param: 0,
            },
            PlanPredicate::Before {
                column: column("month"),
                param: 1,
            },
        ]
        .map(|predicate| PlanFilter::new(PredicateOrigin::Definition, predicate));
        let params = vec![ParamValue::Date(june.start()), ParamValue::Date(june.end())];
        QueryPlan::new(
            SourceName::parse("metrics").expect("a name"),
            metric.clone(),
            StatementTables::only(TableName::parse(TABLE).expect("a table")),
            PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("month")),
            Vec::new(),
            PlanMeasure::Simple {
                term: PlanTerm::Aggregate {
                    aggregate: Aggregate::CountDistinct,
                    column: column("subscription_key"),
                },
            },
            ResultLabel::measure(&metric),
            PlanBindings::parse(range.into(), params).expect("the range binds"),
            june,
        )
    }

    fn declared() -> SharedIdentityDeclared {
        SharedIdentityDeclared::of(AcknowledgementReason::parse("one Cube token for every caller").expect("a reason"))
    }

    fn shared() -> Presented {
        Presented::SharedServiceUser { declared: declared() }
    }

    fn deadline() -> Deadline {
        Deadline::opened_at(
            std::time::Instant::now(),
            Budget::parse(core::time::Duration::from_secs(10)).expect("a budget"),
        )
    }

    fn cube(server: &FakeServer) -> CubeWarehouse<HttpCube> {
        CubeWarehouse::of(
            SourceName::parse("metrics").expect("a name"),
            SourcePosture::SharedServiceUser { declared: declared() },
            Secret::new(String::from("cube-token-canary-5e21")),
            HttpCube::new(
                Endpoint::parse(&server.endpoint()).expect("the fake server's own endpoint is usable"),
                ReadBounds::parse(10, 1 << 20).expect("a positive timeout and cap are usable bounds"),
                None,
            ),
        )
    }

    /// The plan, answered by a Cube whose one June row carries `value` for the metric.
    fn answered(value: &str) -> Result<ResultBatches, CubeError<HttpCubeError>> {
        let meta = serde_json::json!({ "cubes": [{
            "name": TABLE,
            "measures": [{ "name": MEASURE }],
            "dimensions": [{ "name": "fct_subscription_monthly.month" }],
            "segments": []
        }]});
        let load = serde_json::json!({ "data": [{ "fct_subscription_monthly.month.month": "2026-06-01T00:00:00.000", MEASURE: value }] });
        let server = FakeServer::start(vec![Scripted::ok(&meta), Scripted::ok(&load)]);
        let answer = cube(&server).execute(Executable::Query(&plan()), &shared(), deadline());
        server.finish();
        answer
    }

    #[test]
    fn an_integer_beyond_two_to_the_fifty_third_is_refused_rather_than_answered_rounded() {
        let rounded = answered("9223372036854776000").expect_err("a value Cube carried through a double is not an answer");
        assert!(
            rounded
                .to_string()
                .contains(&format!("member {MEASURE} came back as an integer beyond 2^53")),
            "{rounded}"
        );
        answered("9007199254740992").expect("2^53 itself is exact");
    }
}

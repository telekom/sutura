//! The served `kind: duckdb` cell (`telekom/sutura#1292`): the example written into a local database
//! file, opened read-only by the composed binary, answering a certified question and a raw statement
//! over HTTP - the zero-infrastructure source `run_sql` has.
//!
//! **What this proves about the composition root**: a single-kind `duckdb` deployment keeps its
//! concrete adapter, so `run_sql` reads `DuckDbWarehouse::ACCEPTS_RAW_STATEMENTS` and not the
//! conservative default `crate::serve::kind::AnyWarehouse` carries. What the raw path refuses is
//! measured against the driver in `crates/sutura-exec-duckdb/tests/raw.rs`; this cell holds that a
//! served write is refused and leaves the file's table in place.
//!
//! **The identity limit, same as every other served cell in this suite**: one process holds the file
//! under its own operating-system identity, `shared-service-user`.

#[cfg(unix)]
#[cfg(test)]
#[cfg(feature = "duckdb")]
mod tests {
    use crate::harness::{TOKEN, raw_sql_settings, recurring_revenue_june, start_configured, v1};

    const SEGMENTS: &str = "select segment, count(*) as customers from dim_customer group by segment order by segment";

    #[test]
    fn a_duckdb_file_answers_a_certified_question_and_a_raw_statement_from_the_served_binary() {
        let served = start_configured("duckdb-run-sql", &raw_sql_settings("duckdb-run-sql"));

        // The anchors re-ran against the file at boot, so the certified figure is the example's own.
        let reply = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &recurring_revenue_june(),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        assert_eq!(
            reply.json()["rows"],
            serde_json::json!([["2026-06-01", "202121"]]),
            "{}",
            reply.body
        );

        let raw = |statement: &str| {
            served.post(
                &v1(sutura_http::constants::base_paths::RUN_SQL),
                Some(TOKEN),
                &serde_json::json!({ "statement": statement }).to_string(),
            )
        };
        let segments = raw(SEGMENTS);
        assert_eq!(segments.status, 200, "{}", segments.body);
        let body = segments.json();
        assert_eq!(body["outcome"], "raw_rows", "{}", segments.body);
        assert_eq!(
            body["rows"],
            serde_json::json!([["business", "12"], ["consumer", "27"], ["wholesale", "1"]]),
            "{}",
            segments.body
        );

        // A write is refused, and the table it named is still there and unchanged afterwards.
        let dropped = raw("drop table dim_customer");
        assert_eq!(dropped.status, 422, "{}", dropped.body);
        assert_eq!(dropped.json()["outcome"], "raw_refusal", "{}", dropped.body);
        assert_eq!(raw(SEGMENTS).json()["rows"], body["rows"]);
    }
    #[cfg(feature = "agent")]
    #[test]
    fn a_served_duckdb_answer_reports_its_statement_and_only_its_own_bindings_on_both_transports() {
        let case = "duckdb-executed-sql";
        let settings = crate::harness::raw_sql_deployment(
            case,
            crate::harness::AGENT_LOOPBACK,
            &format!("{}  access_token: \"{}\"\n", crate::harness::SINGLE_USER, TOKEN),
        );
        let served = start_configured(case, &settings);
        let initialized = served.mcp(
            Some(TOKEN),
            &serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                    "clientInfo": {"name": "execution-details", "version": "0.0.0"}}
            })
            .to_string(),
        );
        assert_eq!(initialized.status, 200, "{}", initialized.body);
        let revenue = include_str!("../snapshots/recurring-revenue-june__statement.snap")
            .lines()
            .find(|line| line.starts_with("SELECT "))
            .expect("the golden carries SQL");
        let churn = include_str!("../snapshots/churn-rate-by-month__statement.snap")
            .lines()
            .find(|line| line.starts_with("SELECT "))
            .expect("the golden carries SQL");
        for (metric, start, end, expected_sql, fixed) in [
            ("recurring_revenue", "2026-06-01", "2026-07-01", revenue, true),
            ("churn_rate", "2026-05-01", "2026-06-01", churn, false),
        ] {
            let question = serde_json::json!({"metrics": [metric], "grain": "month", "range": {"start": start, "end": end}});
            let http = served.post(
                &v1(sutura_http::constants::base_paths::QUERY),
                Some(TOKEN),
                &question.to_string(),
            );
            assert_eq!(http.status, 200, "{}", http.body);
            let body = http.json();
            let sql = body
                .pointer("/executions/0")
                .filter(|value| value.is_object())
                .expect("a DuckDB answer carries its executed statement");
            assert_eq!(sql["source"], "local");
            assert_eq!(sql["dialect"], "duckdb");
            assert_eq!(sql["sql"], expected_sql);
            let mut parameters = [start, end].into_iter().enumerate().map(|(position, day)| {
                let days = sutura_domain::calendar::Date::parse(day).expect("fixture date").days_since_epoch();
                serde_json::json!({"binding": {"kind": "position", "value": position}, "value": {"type": "date32", "value": days}})
            }).collect::<Vec<_>>();
            if fixed {
                parameters.push(serde_json::json!({"binding": {"kind": "position", "value": 2}, "value": {"type": "utf8", "value": "active"}}));
                assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "202121"]]));
            }
            assert_eq!(sql["parameters"], serde_json::json!(parameters));
            assert_eq!(body["executions"].as_array().expect("submissions").len(), 1);
            assert_eq!(sql["kind"], "sql");
            assert!(
                !http.body.contains(TOKEN),
                "the response must not carry the access credential"
            );
            let mcp = served.mcp(
                Some(TOKEN),
                &serde_json::json!({
                    "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                    "params": {"name": "ask_metric", "arguments": question}
                })
                .to_string(),
            );
            assert_eq!(mcp.status, 200, "{}", mcp.body);
            let result = mcp.json();
            assert_eq!(result["result"]["structuredContent"]["executions"], body["executions"]);
            let text = result["result"]["content"][0]["text"]
                .as_str()
                .expect("MCP has a text result");
            assert!(text.contains(expected_sql), "{text}");
            let days = sutura_domain::calendar::Date::parse(start).expect("fixture date").days_since_epoch();
            assert!(text.contains(&days.to_string()), "{text}");
            assert!(!mcp.body.contains(TOKEN), "MCP must not carry the access credential");
        }
        let refused = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            r#"{"metrics":["absent"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#,
        );
        assert_eq!(refused.status, 404, "{}", refused.body);
        assert!(refused.json().get("executions").is_none());
        let raw_input = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            r#"{"sql":"SELECT 1"}"#,
        );
        assert_eq!(raw_input.status, 400, "{}", raw_input.body);
    }

    #[test]
    fn reported_execution_text_counts_toward_the_served_response_budget() {
        let case = "execution-text-budget";
        let root = crate::harness::derived_beside(&crate::harness::config_path(case));
        let catalog = root.join("catalog");
        std::fs::create_dir_all(&catalog).expect("the fixture catalog creates");
        for (name, document) in [
            (
                "declaration.md",
                "---\nkind: declaration\ndefinitions: [structure, metrics, grains, anchors, descriptions, column_types]\nknowledge: []\n---\n",
            ),
            (
                "model.md",
                "---\nkind: model\nname: events\nsource: local\ntable: events\ncolumns:\n  - {name: month, type: DATE, nullable: false}\n  - {name: amount, type: NUMERIC, nullable: false}\n  - {name: payload, type: VARCHAR, nullable: false}\n---\nThe fixture events.\n",
            ),
            (
                "metric.md",
                "---\nkind: metric\nname: payload_total\nmodel: events\nmeasure:\n  simple: {aggregate: sum, column: amount}\ntime_column: month\ngrains: [month]\ndimensions:\n  - {name: payload, column: payload}\nanchor:\n  range: {start: 2026-06-01, end: 2026-07-01}\n  value: 1\naudience: open\n---\nThe fixture total.\n",
            ),
        ] {
            std::fs::write(catalog.join(name), document).expect("the fixture document writes");
        }
        let limit = sutura_domain::query::ResponseByteLimit::DEFAULT.bytes();
        // The payload, ten date bytes and one measure byte leave exactly one byte for execution.
        let payload = "x".repeat(usize::try_from(limit - 12).expect("the test budget fits usize"));
        std::fs::write(
            root.join("events.csv"),
            format!("month,amount,payload\n2026-06-01,1,{payload}\n"),
        )
        .expect("the fixture data writes");
        let settings = crate::harness::settings_over(
            &catalog,
            &root,
            crate::harness::LOOPBACK,
            &format!("{}  access_token: \"{}\"\n", crate::harness::SINGLE_USER, TOKEN),
            &crate::harness::files_source("local", &root),
        );
        let served = start_configured(case, &settings);
        let mut question = serde_json::json!({"metrics": ["payload_total"], "grain": "month", "range": {"start": "2026-06-01", "end": "2026-07-01"}});
        let small = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &question.to_string(),
        );
        assert_eq!(small.status, 200, "{}", small.body);
        assert_eq!(small.json()["rows"], serde_json::json!([["2026-06-01", "1"]]));
        question["dimensions"] = serde_json::json!(["payload"]);
        let bounded = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &question.to_string(),
        );
        assert_eq!(bounded.status, 413, "rows alone fit; execution text must cross the budget");
        let body = bounded.json();
        assert_eq!(body["reason"]["code"], "result_too_large");
        assert!(body.get("executions").is_none());
    }

    #[test]
    fn files_and_combined_answers_report_every_submitted_plan() {
        let served = start_configured(
            "files-logical-plan",
            &crate::harness::settings(&crate::harness::example_root()),
        );
        let reply = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &recurring_revenue_june(),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["executions"][0]["kind"], "datafusion");
        assert_eq!(body["executions"][0]["source"], "local");
        let plan = body
            .pointer("/executions/0/logical_plan")
            .and_then(serde_json::Value::as_str)
            .expect("files reports its submitted logical plan");
        assert!(plan.contains("TableScan: fct_subscription_monthly"), "{plan}");
        assert!(plan.contains("Aggregate:") && plan.contains("mrr_cents"), "{plan}");
        assert!(!plan.contains(TOKEN));
        assert!(
            !plan.contains(".csv"),
            "the logical display must not print the file provider: {plan}"
        );
        assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "202121"]]));
        let stacked = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            r#"{"metrics":["recurring_revenue","data_per_subscription"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#,
        );
        assert_eq!(stacked.status, 200, "{}", stacked.body);
        let body = stacked.json();
        assert_eq!(body["executions"].as_array().expect("submissions").len(), 1);
        let plan = body["executions"][0]["logical_plan"].as_str().expect("one complete stacked plan is reported");
        assert!(plan.contains("TableScan: fct_subscription_monthly"), "{plan}");
        assert!(plan.contains("TableScan: fct_usage_daily"), "{plan}");
        assert_eq!(
            body["columns"],
            serde_json::json!(["period", "recurring_revenue", "data_per_subscription"])
        );
        assert_eq!(body["rows"][0][1], "202121");
        let combined = start_configured(
            "combined-execution-unavailable",
            &crate::harness::settings_spanning_two_sources("combined-execution-unavailable"),
        );
        let reply = combined.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &crate::harness::recurring_revenue_by_region(),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        let executions = body["executions"].as_array().expect("all source and combine submissions");
        assert!(executions.len() >= 3, "{body}");
        assert_eq!(executions[0]["stage"], "files");
        assert_eq!(executions[1]["stage"], "files");
        assert_ne!(executions[0]["source"], executions[1]["source"]);
        assert!(executions.iter().skip(2).all(|execution| execution["stage"] == "combine"));
        assert!(executions.iter().all(|execution| execution["logical_plan"].as_str().is_some_and(|plan| !plan.is_empty())));
    }
}

// `tools.run_sql.sources` and `federation_allowed` on the served binary, declared here because
// `served.rs` is at its line cap. `#[path]` resolves beside this file.
#[path = "run_sql_federation.rs"]
mod run_sql_federation;

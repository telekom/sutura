//! Startup names each model omitted from planned SQL and the declarations responsible.

use crate::harness::{TOKEN, raw_sql_settings, start_configured, v1};

#[test]
fn startup_warns_with_the_model_column_and_declared_type_omitted_from_planned_sql() {
    let served = start_configured("planned-type-warnings", &raw_sql_settings("planned-type-warnings"));
    let warnings: Vec<serde_json::Value> = served
        .startup
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|line: &serde_json::Value| {
            line["msg"] == "model excluded from planned run_sql because column types are unsupported or undeclared"
        })
        .collect();
    let warning = warnings
        .iter()
        .find(|line| line["model"] == "subscriptions")
        .expect("the omitted model is named at startup");
    assert_eq!(warning["level"], 40, "{warning}");
    assert_eq!(warning["source"], "local");
    let columns = warning["columns"].as_str().expect("the declarations are recorded");
    assert!(columns.contains("mrr_cents") && columns.contains("NUMERIC"), "{warning}");
    assert!(!warning.to_string().contains(TOKEN));
    let reply = served.post(
        &v1(sutura_http::constants::base_paths::RUN_SQL),
        Some(TOKEN),
        &serde_json::json!({"statement": "select * from fct_subscription_monthly"}).to_string(),
    );
    assert_eq!(reply.status, 422, "{}", reply.body);
}

//! Real Oracle raw calls over the existing native and verified login fixtures.

use serde_json::json;
use sutura_http::constants::base_paths::RUN_SQL;

use super::{FixtureSource, Tier};
use crate::harness::{Reply, Served, TOKEN, start_configured, v1};

fn settings(tier: &Tier, case: &str, enabled: bool, federation: bool) -> String {
    format!(
        "{}tools:\n  run_sql:\n    enabled: {enabled}\n    sources: [local]\n    federation_allowed: {federation}\n",
        tier.settings(case, false, true, false, FixtureSource::Oracle),
    )
}

fn ran(served: &Served, statement: &str) -> Reply {
    served.post(&v1(RUN_SQL), Some(TOKEN), &json!({"statement": statement}).to_string())
}

#[test]
#[ignore = "requires just oracle-native-acceptance"]
fn oracle_raw_served_rows_and_overflow_are_ungoverned_and_never_a_prefix() {
    let tier = Tier::here();
    let case = "oracle-raw-served";
    let off = start_configured("oracle-raw-off", &settings(&tier, "oracle-raw-off", false, false));
    let reply = ran(&off, "SELECT ID FROM EVENTS");
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.json()["code"], "tool_not_enabled", "{}", reply.body);
    drop(off);

    let served = std::panic::catch_unwind(|| start_configured(case, &settings(&tier, case, true, false)))
        .expect("the configured Oracle source accepts raw statements");
    #[cfg(feature = "sql")]
    let statement = "SELECT e.\"ID\" FROM \"EVENTS\" e ORDER BY e.\"ID\" LIMIT 2";
    #[cfg(not(feature = "sql"))]
    let statement = "SELECT ID FROM EVENTS ORDER BY ID FETCH FIRST 2 ROWS ONLY";
    let reply = ran(&served, statement);
    assert_eq!(reply.status, 200, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["outcome"], "raw_rows", "{}", reply.body);
    assert_eq!(body["rows"], json!([["1"], ["2"]]), "{}", reply.body);
    assert!(body.get("provenance").is_none(), "{}", reply.body);
    assert!(body.get("executed_statement").is_none(), "{}", reply.body);
    assert!(body.get("executed_sql").is_none(), "{}", reply.body);
    assert!(body.get("logical_plan").is_none(), "{}", reply.body);
    assert!(body.get("executed_as").is_none(), "{}", reply.body);
    let audit = served.awaiting("raw_answered");
    let record = audit
        .iter()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|line| line["msg"] == "[REQUEST - EVENT] raw_answered")
        .expect("the raw call has an audit record");
    assert_eq!(record["rows"], 2, "{record}");
    assert!(
        !record.to_string().contains(&tier.legacy_password),
        "credentials stay out of the audit"
    );
    assert_eq!(record["executed_as"], "local=shared-service-user", "{record}");

    #[cfg(feature = "sql")]
    let overflow = String::from("SELECT a.\"ID\" FROM \"EVENTS\" a CROSS JOIN \"EVENTS\" b");
    #[cfg(not(feature = "sql"))]
    let overflow = format!(
        "SELECT LEVEL N FROM dual CONNECT BY LEVEL <= {}",
        sutura_domain::plan::MAX_ROWS + 1
    );
    let refused = ran(&served, &overflow);
    assert_eq!(refused.status, 413, "{}", refused.body);
    let body = refused.json();
    assert_eq!(body["outcome"], "raw_refusal", "{}", refused.body);
    assert_eq!(body["code"], "too_many_rows", "{}", refused.body);
    assert!(body.get("rows").is_none(), "no successful prefix: {}", refused.body);
    assert!(body.get("provenance").is_none(), "{}", refused.body);
    assert_eq!(
        ran(&served, "SELECT \"ID\" FROM \"EVENTS\" WHERE \"ID\" = 1").json()["rows"],
        json!([["1"]])
    );
}

#[cfg(not(feature = "sql"))]
#[test]
#[ignore = "requires just oracle-native-acceptance"]
fn oracle_raw_for_update_and_adversarial_with_are_refused_without_changing_rows() {
    let tier = Tier::here();
    let case = "oracle-raw-read-only";
    let served = start_configured(case, &settings(&tier, case, true, false));
    // ponytail: native reports a channel refusal here; verified reports the read-only error.
    let (read_only_status, read_only_code) = if tier.channel.starts_with("transport_mode: native-encryption") {
        (422, "statement_failed")
    } else {
        (403, "source_refused")
    };
    for (statement, status, code) in [
        (
            "SELECT ID FROM EVENTS WHERE ID = 1 FOR UPDATE",
            read_only_status,
            read_only_code,
        ),
        (
            "WITH FUNCTION attempt RETURN NUMBER IS BEGIN EXECUTE IMMEDIATE 'DELETE FROM EVENTS'; RETURN 1; END; SELECT attempt FROM dual",
            403,
            "source_refused",
        ),
    ] {
        let reply = ran(&served, statement);
        assert_eq!(reply.status, status, "{}", reply.body);
        assert_eq!(reply.json()["code"], code, "{}", reply.body);
        assert!(reply.json().get("rows").is_none(), "{}", reply.body);
    }
    let healthy = ran(&served, "SELECT COUNT(*) FROM EVENTS");
    assert_eq!(healthy.status, 200, "{}", healthy.body);
    assert_eq!(healthy.json()["rows"], json!([["128"]]), "{}", healthy.body);
}

#[cfg(not(feature = "sql"))]
#[test]
#[ignore = "requires just oracle-native-acceptance"]
fn oracle_raw_deadline_discards_its_session_and_following_call_is_healthy() {
    let tier = Tier::here();
    let case = "oracle-raw-deadline";
    let settings = settings(&tier, case, true, false).replace("server:\n", "server:\n  request_timeout_seconds: 4\n");
    let served = start_configured(case, &settings);
    let started = std::time::Instant::now();
    let reply = ran(
        &served,
        "SELECT /*+ NO_PARALLEL */ /* sutura_raw_deadline_probe */ SUM(ORA_HASH(a.n + 1000 * b.n + 1000000 * c.n)) FROM (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000) a CROSS JOIN (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000) b CROSS JOIN (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000) c",
    );
    let elapsed = started.elapsed();
    println!("oracle_raw_product_query_ms={} status={}", elapsed.as_millis(), reply.status);
    assert!((2500..10000).contains(&elapsed.as_millis()), "{elapsed:?}: {}", reply.body);
    assert!(
        matches!(reply.status, 408 | 422),
        "a deadline or native recovery refusal is required: {}",
        reply.body
    );
    assert!(reply.json().get("rows").is_none(), "{}", reply.body);
    let healthy = ran(&served, "SELECT ID FROM EVENTS WHERE ID = 1");
    assert_eq!(healthy.status, 200, "{}", healthy.body);
    assert_eq!(healthy.json()["rows"], json!([["1"]]), "{}", healthy.body);
}

#[cfg(feature = "sql")]
#[test]
#[ignore = "requires just oracle-native-acceptance"]
fn oracle_raw_planned_quoted_aliases_and_offset_execute_at_the_source() {
    let tier = Tier::here();
    let case = "oracle-raw-planned";
    let served = std::panic::catch_unwind(|| start_configured(case, &settings(&tier, case, true, true)))
        .expect("the configured Oracle source accepts raw statements");
    let reply = ran(
        &served,
        "SELECT e.\"ID\" AS \"chosen\" FROM \"EVENTS\" e ORDER BY e.\"ID\" LIMIT 1 OFFSET 1",
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.json()["columns"], json!(["chosen"]), "{}", reply.body);
    assert_eq!(reply.json()["rows"], json!([["2"]]), "{}", reply.body);
    assert_eq!(
        reply.json()["executed_as"],
        json!([{"source":"local", "posture":"shared-service-user"}]),
        "{}",
        reply.body
    );
    let refused = ran(&served, "SELECT e.\"UNDECLARED_COLUMN\" FROM \"EVENTS\" e");
    assert_eq!(refused.status, 422, "{}", refused.body);
    assert_eq!(refused.json()["code"], "statement_failed", "{}", refused.body);
    assert!(!refused.body.contains("UNDECLARED_COLUMN"), "{}", refused.body);
}

#[cfg(feature = "sql")]
#[test]
#[ignore = "requires just oracle-native-acceptance"]
fn oracle_raw_federation_reads_two_registered_sources_only_when_listed_and_allowed() {
    use crate::harness::{config_path, derived_beside};

    let tier = Tier::here();
    let statement = "SELECT a.\"ID\", b.\"ID\" AS \"other_id\" FROM \"EVENTS\" a JOIN \"EVENTS_MORE\" b ON a.\"ID\" = b.\"ID\" ORDER BY a.\"ID\" LIMIT 2";
    for (case, listed, federation, expected_code) in [
        ("oracle-raw-unlisted", "local", true, Some("statement_failed")),
        (
            "oracle-raw-no-federation",
            "local, other",
            false,
            Some("plan_spans_more_than_one_source"),
        ),
        ("oracle-raw-federated", "local, other", true, None),
    ] {
        let original = tier.settings(case, false, true, false, FixtureSource::Oracle);
        let (head, source) = original
            .split_once("\nsources:\n  local:\n")
            .expect("the fixture declares one source");
        let catalog = derived_beside(&config_path(case)).join("catalog");
        std::fs::write(catalog.join("other.md"), "---\nkind: model\nname: events_more\nsource: other\ntable: EVENTS_MORE\ncolumns:\n  - {name: ID, type: NUMBER(10), nullable: false}\n---\nThe second raw fixture table.\n")
            .expect("the second model writes");
        let settings = format!(
            "{head}\nsources:\n  local:\n{source}  other:\n{source}tools:\n  run_sql:\n    enabled: true\n    sources: [{listed}]\n    federation_allowed: {federation}\n"
        );
        let served = std::panic::catch_unwind(|| start_configured(case, &settings))
            .expect("the configured Oracle source accepts raw statements");
        let reply = ran(&served, statement);
        if let Some(code) = expected_code {
            assert_eq!(reply.json()["outcome"], "raw_refusal", "{}", reply.body);
            assert_eq!(reply.json()["code"], code, "{}", reply.body);
            assert!(reply.json().get("rows").is_none(), "{}", reply.body);
        } else {
            assert_eq!(reply.status, 200, "{}", reply.body);
            assert_eq!(reply.json()["rows"], json!([["1", "1"], ["2", "2"]]), "{}", reply.body);
            assert_eq!(
                reply.json()["executed_as"],
                json!([
                    {"source":"local", "posture":"shared-service-user"},
                    {"source":"other", "posture":"shared-service-user"},
                ]),
                "{}",
                reply.body
            );
        }
    }
}

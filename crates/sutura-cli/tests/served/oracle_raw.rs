//! Real Oracle raw calls over the existing native and verified login fixtures.

use serde_json::json;
use sutura_http::constants::base_paths::RUN_SQL;

use super::{FixtureSource, Tier};
#[cfg(feature = "sql")]
use crate::harness::{LOOPBACK, SINGLE_USER};
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
    assert_eq!(refused.json()["code"], "unknown_column", "{}", refused.body);
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
        ("oracle-raw-unlisted", "local", true, Some("unknown_table")),
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

#[cfg(feature = "sql")]
fn lob_catalogs(tier: &Tier, root: &std::path::Path, connection: &str, rules: &str) -> (String, String) {
    let native = format!(
        "  - name: dictionary\n    kind: rdbms\n    version: text-1\n    source_alias: local\n    dictionary_source: native_dictionary\n    include: [{{schema: {}, table: TEXT_EVENTS}}]\n{rules}    connection:\n      dialect: oracle\n      {}\n",
        tier.legacy_user,
        connection.replace('\n', "\n  ")
    );
    let catalog = root.join("models");
    std::fs::create_dir_all(&catalog).unwrap();
    std::fs::write(
        catalog.join("declaration.md"),
        "---\nkind: declaration\ndefinitions: [structure, descriptions, column_types]\nknowledge: []\n---\n",
    )
    .unwrap();
    std::fs::write(catalog.join("model.md"), "---\nkind: model\nname: text_events\nsource: local\ntable: TEXT_EVENTS\ncolumns:\n  - {name: ID, type: 'NUMBER(10)'}\n  - {name: NATIVE_NUMBER, type: NUMBER}\n  - name: SCRIPT\n    type: CLOB\n    max_text_bytes: 4000\n    redact: [{pattern: '(?i)IDENTIFIED[[:space:]]+BY[[:space:]]+[^[:space:]]+', replace: 'IDENTIFIED BY ***'}]\n  - name: SCRIPT_N\n    type: NCLOB\n    max_text_bytes: 4000\n    redact: [{pattern: '(?i)IDENTIFIED[[:space:]]+BY[[:space:]]+[^[:space:]]+', replace: 'IDENTIFIED BY ***'}]\n---\nBounded fixture text.\n").unwrap();
    let markdown = format!(
        "  - name: text\n    kind: markdown\n    version: text-1\n    dir: {}\n    data_dir: {}\n",
        catalog.display(),
        root.display()
    );
    (native, markdown)
}

#[cfg(feature = "sql")]
#[test]
#[ignore = "requires just oracle-native-acceptance"]
fn bounded_lobs_are_redacted_before_projection_predicates_order_and_min() {
    use crate::harness::{config_path, derived_beside, refused_to_start};
    use sutura_config::Environment;

    let tier = Tier::here();
    let root = derived_beside(&config_path("oracle-text-policy"));
    std::fs::create_dir_all(&root).unwrap();
    let password = root.join("password");
    std::fs::write(&password, &tier.legacy_password).unwrap();
    let connection = tier.connection(&tier.legacy_user, &password, true);
    let rules = "    column_rules:\n      - column: SCRIPT*\n        max_text_bytes: 4000\n        redact:\n          - {pattern: '(?i)IDENTIFIED[[:space:]]+BY[[:space:]]+[^[:space:]]+', replace: 'IDENTIFIED BY ***'}\n";
    let (native, markdown) = lob_catalogs(&tier, &root, &connection, rules);
    let configured = |catalog: &str| {
        format!(
            "server:\n{LOOPBACK}security:\n{SINGLE_USER}  access_token: {TOKEN}\ntelemetry:\n  format: bunyan\ncatalogs:\n{catalog}sources:\n  local:\n    kind: oracle\n    {connection}\n    unbounded_number: utf8\n    posture: shared-service-user\ntools:\n  run_sql:\n    enabled: true\n    sources: [local]\n"
        )
    };
    let refused = refused_to_start(
        Environment::Development,
        crate::harness::written("oracle-text-no-bound", &configured(&native.replace(rules, ""))),
        &[],
    )
    .join("\n");
    assert!(refused.contains("no exact supported mapping"), "{refused}");

    for (name, catalog) in [("native", native), ("markdown", markdown)] {
        let served = start_configured(&format!("oracle-text-{name}"), &configured(&catalog));
        let path = if name == "native" {
            format!("\"{}\".\"TEXT_EVENTS\"", tier.legacy_user)
        } else {
            String::from("\"TEXT_EVENTS\"")
        };
        let unplanned_probe = ran(
            &served,
            &format!("select ID from {path} where DBMS_LOB.INSTR(SCRIPT, 'secret') > 0"),
        );
        assert_eq!(unplanned_probe.status, 422, "{}", unplanned_probe.body);
        assert_eq!(unplanned_probe.json()["code"], "statement_failed", "{}", unplanned_probe.body);
        let query = |sql: String| {
            let reply = ran(&served, &sql);
            assert_eq!(reply.status, 200, "{sql}: {}\n{:?}", reply.body, served.log());
            reply.json()["rows"].clone()
        };
        assert_eq!(
            query(format!("select SCRIPT, SCRIPT_N from {path} where ID = 1")),
            json!([["IDENTIFIED BY ***", "IDENTIFIED BY ***"]])
        );
        for search in [
            "SCRIPT like '%late-note%'",
            "instr(SCRIPT, 'late-note') > 0",
            "SCRIPT_N like '%late-note%'",
        ] {
            assert_eq!(query(format!("select ID from {path} where {search}")), json!([["6"]]));
        }
        assert_eq!(
            query(format!(
                "select ID from {path} where ID in (1, 2, 6) order by ID desc limit 2"
            )),
            json!([["6"], ["2"]])
        );
        let window = query(format!(
            "select substr(SCRIPT, 3000, 200), substr(SCRIPT_N, 3000, 200) from {path} where ID = 6"
        ));
        for value in window[0].as_array().unwrap() {
            let text = value.as_str().unwrap();
            assert!(text.starts_with("late-note IDENTIFIED BY *** "), "{text}");
            assert!(!text.contains("secret") && text.len() == 200, "{text}");
        }
        let ascii = query(format!("select SCRIPT, SCRIPT_N from {path} where ID = 6"));
        for (index, value) in ascii[0].as_array().unwrap().iter().enumerate() {
            let text = value.as_str().unwrap();
            let bytes = if index == 0 { 4000 } else { 2000 };
            assert_eq!(text.len(), bytes, "ASCII should use the scalar byte window");
            assert!(text.ends_with("...[truncated]") && !text.contains("secret"), "{text}");
        }
        for native in [
            format!(
                "select NATIVE_NUMBER, SCRIPT, SCRIPT_N, substr(SCRIPT, 3000, 200) as windowed from {path} where NATIVE_NUMBER > 7 and instr(SCRIPT, 'late-note') > 0 order by NATIVE_NUMBER desc limit 1"
            ),
            format!(
                "with candidate as (select * from {path} where NATIVE_NUMBER > 7) select NATIVE_NUMBER, SCRIPT, SCRIPT_N, substr(SCRIPT, 3000, 200) as windowed from candidate where instr(SCRIPT, 'late-note') > 0 order by NATIVE_NUMBER desc limit 1"
            ),
        ] {
            let row = query(native);
            assert_eq!(row[0][0], "10", "the predicate must retain native NUMBER semantics");
            for (index, bytes) in [(1, 4000), (2, 2000)] {
                let text = row[0][index].as_str().unwrap();
                assert_eq!(text.len(), bytes, "numeric routing must retain the output bound");
                assert!(text.ends_with("...[truncated]") && !text.contains("secret"), "{text}");
            }
            let window = row[0][3].as_str().unwrap();
            assert!(
                window.starts_with("late-note IDENTIFIED BY *** ") && window.len() == 200,
                "{window}"
            );
        }
        assert_eq!(
            query(format!("select min(NATIVE_NUMBER), max(NATIVE_NUMBER) from {path}")),
            json!([["1", "10"]]),
            "numeric aggregate output must remain exact on a LOB-bearing table"
        );
        let ordered = query(format!(
            "select NATIVE_NUMBER, substr(SCRIPT, 3000, 40), substr(SCRIPT_N, 3000, 40) from {path} where NATIVE_NUMBER >= 6 order by NATIVE_NUMBER desc limit 2"
        ));
        assert_eq!(ordered.as_array().unwrap().len(), 2);
        assert_eq!(ordered[0][0], "10", "the native wrapper must preserve numeric order");
        assert_eq!(ordered[1][0], "7", "the native wrapper must preserve numeric order");
        for (index, row) in ordered.as_array().unwrap().iter().enumerate() {
            for value in row.as_array().unwrap().iter().skip(1) {
                let text = value.as_str().unwrap();
                assert_eq!(text.len(), 40, "the short native window must survive");
                assert!(!text.contains("secret"), "{text}");
                if index == 0 {
                    assert!(text.starts_with("late-note IDENTIFIED BY *** "), "{text}");
                } else {
                    assert!(text.chars().all(|character| character == 'z'), "{text}");
                }
            }
        }
        // A non-ASCII character after the first thousand still selects the safe reservation.
        let mixed = query(format!("select SCRIPT, SCRIPT_N from {path} where ID = 7"));
        for (index, value) in mixed[0].as_array().unwrap().iter().enumerate() {
            let text = value.as_str().unwrap();
            let bytes = if index == 0 { 1010 } else { 2000 };
            assert_eq!(text.len(), bytes, "the full candidate window must be checked: {text}");
        }
        for column in ["SCRIPT", "SCRIPT_N"] {
            for prefix in ["s", "se", "sec", "secr", "secre", "secret"] {
                assert_eq!(
                    query(format!(
                        "select ID from {path} where {column} like '%IDENTIFIED BY {prefix}%'"
                    )),
                    json!([])
                );
            }
            assert_eq!(
                query(format!(
                    "select ID from {path} where instr({column}, 'IDENTIFIED BY secret') > 0"
                )),
                json!([])
            );
        }
        for unsupported in [
            format!("select min(SCRIPT) from {path} where ID < 3"),
            format!("select ID from {path} where SCRIPT = 'IDENTIFIED BY ***' order by SCRIPT, ID"),
        ] {
            let refused = ran(&served, &unsupported);
            assert_eq!(
                refused.status, 422,
                "a full LOB must never be silently clipped: {}",
                refused.body
            );
            assert_eq!(refused.json()["code"], "source_failed", "{}", refused.body);
        }
        let cut = query(format!("select SCRIPT, SCRIPT_N from {path} where ID IN (5, 6)"));
        for value in cut.as_array().unwrap().iter().flat_map(|row| row.as_array().unwrap()) {
            let text = value.as_str().unwrap();
            assert!(text.len() <= 4000 && text.ends_with("...[truncated]"), "{text}");
        }
        assert_eq!(
            query(format!("select SCRIPT, SCRIPT_N from {path} where ID = 4")),
            json!([["null", "null"]])
        );
        assert_eq!(
            query(format!("select ID from {path} where SCRIPT IS NULL AND SCRIPT_N IS NULL")),
            json!([["4"]])
        );
    }
}

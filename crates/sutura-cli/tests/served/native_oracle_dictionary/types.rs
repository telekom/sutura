//! Live type acceptance through native metadata, source configuration and planned SQL.

use super::*;
use crate::harness::start_configured_with_environment;

pub(super) fn number_and_zone_types(fixture: &Fixture) {
    fixture.owner.execute(
        "CREATE TABLE TYPE_EVENTS (ID NUMBER(10,0), FULL_VALUE NUMBER, SEEN_TZ TIMESTAMP(6) WITH TIME ZONE, SEEN_LTZ TIMESTAMP(6) WITH LOCAL TIME ZONE)", &[]).unwrap();
    let cases = [
        (
            "12345678901234567890123456789012345678",
            "2026-03-08 01:59:59.123456 America/New_York EST",
            "2026-03-08T06:59:59.123456",
        ),
        (
            "0.12345678901234567890123456789012345678",
            "2026-03-08 03:00:00.654321 America/New_York EDT",
            "2026-03-08T07:00:00.654321",
        ),
        (
            "7",
            "2026-11-01 01:30:00.000001 America/New_York EDT",
            "2026-11-01T05:30:00.000001",
        ),
        (
            "-12345678901234567890123456789012345678",
            "2026-11-01 01:30:00.000001 America/New_York EST",
            "2026-11-01T06:30:00.000001",
        ),
    ];
    for (id, (number, timestamp, _)) in cases.iter().enumerate() {
        fixture.owner.execute(&format!(
            "INSERT INTO TYPE_EVENTS VALUES ({id}, {number}, TO_TIMESTAMP_TZ('{timestamp}', 'YYYY-MM-DD HH24:MI:SS.FF6 TZR TZD'), TO_TIMESTAMP_TZ('{timestamp}', 'YYYY-MM-DD HH24:MI:SS.FF6 TZR TZD'))"), &[]).unwrap();
    }
    fixture.owner.execute(
        "INSERT INTO TYPE_EVENTS VALUES (4, 1E80, TO_TIMESTAMP_TZ('2026-07-01 05:45:00.000000 +05:45', 'YYYY-MM-DD HH24:MI:SS.FF6 TZH:TZM'), TO_TIMESTAMP_TZ('2026-07-01 05:45:00.000000 +05:45', 'YYYY-MM-DD HH24:MI:SS.FF6 TZH:TZM'))", &[]).unwrap();
    fixture
        .owner
        .execute("INSERT INTO TYPE_EVENTS VALUES (5, NULL, NULL, NULL)", &[])
        .unwrap();
    fixture.owner.commit().unwrap();
    fixture
        .owner
        .execute("CREATE TABLE TYPE_KEYS AS SELECT ID, SEEN_TZ, SEEN_LTZ FROM TYPE_EVENTS", &[])
        .unwrap();
    for table in ["TYPE_EVENTS", "TYPE_KEYS"] {
        fixture
            .owner
            .execute(&format!("GRANT SELECT ON {table} TO {}", fixture.reader), &[])
            .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        fixture.owner.rollback().unwrap();
        fixture.owner.execute("SET TRANSACTION READ ONLY", &[]).unwrap();
        match fixture.owner.query_row("SELECT ID FROM TYPE_EVENTS WHERE ID = 0", &[]) {
            Ok(_) => {
                fixture.owner.rollback().unwrap();
                break;
            }
            Err(cause) => {
                assert!(
                    cause.to_string().contains("ORA-01466") && Instant::now() < deadline,
                    "{cause}"
                );
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    let absent = fixture.settings("").replace("table: EVENTS", "table: TYPE_EVENTS");
    let log = refused_to_start(Environment::Development, written("oracle-number-policy-absent", &absent), &[]).join("\n");
    assert!(
        log.contains("FULL_VALUE") && log.contains("no exact supported mapping for NUMBER"),
        "{log}"
    );
    let settings = absent.replace("    posture:", "    unbounded_number: utf8\n    posture:");
    let mut served = start_configured_with_environment(
        "oracle-exact-types",
        &settings,
        None,
        &[
            ("ORA_SDTZ", "+09:00"),
            ("TZ", "Pacific/Honolulu"),
            ("RUST_LOG", "info,sutura_exec_datafusion::combine::statement=debug"),
        ],
        &[],
    );
    let reply = run(
        &served,
        &format!(
            "select \"FULL_VALUE\", \"SEEN_TZ\", \"SEEN_LTZ\" from \"{}\".\"TYPE_EVENTS\" order by \"ID\"",
            fixture.schema
        ),
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    let mut expected: Vec<serde_json::Value> = cases
        .iter()
        .map(|(number, _, utc)| serde_json::json!([number, format!("{utc}Z"), format!("{utc}Z")]))
        .collect();
    let huge = format!("1{}", "0".repeat(80));
    expected.push(serde_json::json!([huge, "2026-07-01T00:00:00Z", "2026-07-01T00:00:00Z"]));
    expected.push(serde_json::json!(["null", "null", "null"]));
    assert_eq!(reply.json()["rows"], serde_json::json!(expected));
    let lexical = serde_json::json!([[cases[3].0], [cases[1].0], [huge], [cases[0].0], [cases[2].0], ["null"]]);
    text_operators(
        &mut served,
        &format!("\"{}\".\"TYPE_EVENTS\" t", fixture.schema),
        &huge,
        &lexical,
        1,
    );
    drop(served);

    let lookup = format!(
        "  - name: keys\n    kind: rdbms\n    version: dict-1\n    source_alias: lookup\n    dictionary_source: native_dictionary\n    include: [{{schema: {}, table: TYPE_KEYS}}]\n    connection:\n      dialect: oracle\n      {}\n",
        fixture.schema,
        fixture.connection.replace('\n', "\n  ")
    );
    let sources = format!(
        "  lookup:\n    kind: oracle\n    {}\n    posture: shared-service-user\n",
        fixture.connection
    );
    let federated = settings
        .replace("sources:\n", &format!("{lookup}sources:\n"))
        .replace("tools:\n", &format!("{sources}tools:\n"))
        .replace("sources: [warehouse]", "sources: [warehouse, lookup]");
    let mut served = start_configured_with_environment(
        "oracle-exact-types-federated",
        &federated,
        None,
        &[
            ("ORA_SDTZ", "-07:00"),
            ("TZ", "Asia/Tokyo"),
            ("RUST_LOG", "info,sutura_exec_datafusion::combine::statement=debug"),
        ],
        &[],
    );
    let reply = run(
        &served,
        &format!(
            "select t.\"FULL_VALUE\", t.\"SEEN_TZ\", t.\"SEEN_LTZ\", k.\"ID\" as matched from \"{0}\".\"TYPE_EVENTS\" t left join \"{0}\".\"TYPE_KEYS\" k on t.\"ID\" = k.\"ID\" and t.\"SEEN_TZ\" = k.\"SEEN_TZ\" and t.\"SEEN_LTZ\" = k.\"SEEN_LTZ\" order by t.\"ID\"",
            fixture.schema
        ),
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    let mut numbers: Vec<serde_json::Value> = cases
        .iter()
        .enumerate()
        .map(|(id, (number, _, utc))| serde_json::json!([number, format!("{utc}Z"), format!("{utc}Z"), id.to_string()]))
        .collect();
    numbers.push(serde_json::json!([huge, "2026-07-01T00:00:00Z", "2026-07-01T00:00:00Z", "4"]));
    numbers.push(serde_json::json!(["null", "null", "null", "null"]));
    assert_eq!(reply.json()["rows"], serde_json::json!(numbers));
    text_operators(
        &mut served,
        &format!(
            "\"{0}\".\"TYPE_EVENTS\" t join \"{0}\".\"TYPE_KEYS\" k on t.\"ID\" = k.\"ID\"",
            fixture.schema
        ),
        &huge,
        &lexical,
        2,
    );
    drop(served);
    refuses_sub_microsecond_timestamps(fixture, &settings);
}

fn refuses_sub_microsecond_timestamps(fixture: &Fixture, settings: &str) {
    for kind in [
        "TIMESTAMP(9)",
        "TIMESTAMP(9) WITH TIME ZONE",
        "TIMESTAMP(9) WITH LOCAL TIME ZONE",
    ] {
        fixture
            .owner
            .execute(&format!("ALTER TABLE TYPE_EVENTS ADD TOO_PRECISE {kind}"), &[])
            .unwrap();
        let log = refused_to_start(Environment::Development, written("oracle-too-precise", settings), &[]).join("\n");
        assert!(
            log.contains("TOO_PRECISE") && log.contains(&format!("no exact supported mapping for {kind}")),
            "{kind}: {log}"
        );
        fixture
            .owner
            .execute("ALTER TABLE TYPE_EVENTS DROP COLUMN TOO_PRECISE", &[])
            .unwrap();
    }
}

fn text_operators(served: &mut Served, from: &str, huge: &str, lexical: &serde_json::Value, sources: usize) {
    for (statement, expected) in [
        (
            format!("select t.\"FULL_VALUE\" from {from} order by t.\"FULL_VALUE\" nulls last"),
            lexical.clone(),
        ),
        (
            format!(
                "select min(t.\"FULL_VALUE\") as smallest, max(t.\"FULL_VALUE\") as largest from {from} where t.\"ID\" in (0, 2, 4)"
            ),
            serde_json::json!([[huge, "7"]]),
        ),
        (
            format!("select t.\"FULL_VALUE\" from {from} where t.\"FULL_VALUE\" > '2' order by t.\"FULL_VALUE\""),
            serde_json::json!([["7"]]),
        ),
        (
            format!("select t.\"FULL_VALUE\" from {from} where t.\"FULL_VALUE\" like '100%'"),
            serde_json::json!([[huge]]),
        ),
    ] {
        let reply = run(served, &statement);
        if reply.status != 200 {
            let status = served.terminate();
            eprintln!("planned Oracle fixture exited: {status}");
            for line in served.log() {
                let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                if event["msg"].as_str().is_some_and(|message| message.contains("run_sql:")) {
                    eprintln!(
                        "planned Oracle fixture: {}",
                        serde_json::json!({
                            "message": event["msg"],
                            "cause": event["cause"],
                            "source": event["source"],
                            "statement": event["statement"],
                        })
                    );
                }
            }
        }
        assert_eq!(reply.status, 200, "{statement}: {}", reply.body);
        let body = reply.json();
        assert_eq!(body["rows"], expected, "{statement}: {}", reply.body);
        assert_eq!(
            body["executed_as"].as_array().expect("planned sources").len(),
            sources,
            "{statement}: {}",
            reply.body
        );
    }
}

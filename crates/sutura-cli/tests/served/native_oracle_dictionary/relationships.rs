use super::*;
use crate::harness::try_start_configured;

#[test]
#[ignore = "requires just oracle-acceptance"]
fn served_oracle_relationships_require_enforced_complete_keys_and_bounded_declared_uniqueness() {
    let fixture = Fixture::here();
    for statement in [
        "CREATE TABLE APPS (ID NUMBER(10,0) PRIMARY KEY)",
        "INSERT INTO APPS VALUES (7)",
        "ALTER TABLE EVENTS ADD CONSTRAINT EVENTS_APPS FOREIGN KEY (ID) REFERENCES APPS(ID)",
        "CREATE TABLE UNIQUE_APPS (ID NUMBER(10,0) UNIQUE)",
        "CREATE TABLE UNIQUE_EVENTS (ID NUMBER(10,0), CONSTRAINT UNIQUE_LINK FOREIGN KEY(ID) REFERENCES UNIQUE_APPS(ID))",
        "CREATE TABLE PAIR_APPS (TENANT NUMBER(10,0), ID NUMBER(10,0), PRIMARY KEY(TENANT,ID))",
        "CREATE TABLE PAIR_EVENTS (TENANT NUMBER(10,0), ID NUMBER(10,0), CONSTRAINT PAIR_LINK FOREIGN KEY(TENANT,ID) REFERENCES PAIR_APPS(TENANT,ID))",
        "CREATE TABLE UNVALIDATED (ID NUMBER(10,0), CONSTRAINT UNVALIDATED_LINK FOREIGN KEY(ID) REFERENCES APPS(ID) ENABLE NOVALIDATE)",
        "CREATE TABLE DISABLED (ID NUMBER(10,0), CONSTRAINT DISABLED_LINK FOREIGN KEY(ID) REFERENCES APPS(ID) DISABLE VALIDATE)",
        "CREATE TABLE DEFERRED_LINKS (ID NUMBER(10,0), CONSTRAINT DEFERRED_LINK FOREIGN KEY(ID) REFERENCES APPS(ID) DEFERRABLE)",
        "CREATE TABLE LOOSE_APPS (ID NUMBER(10,0))",
        "INSERT INTO LOOSE_APPS VALUES (1)",
        "INSERT INTO LOOSE_APPS VALUES (2)",
        "CREATE TABLE LOOSE_EVENTS (ID NUMBER(10,0))",
    ] {
        fixture.owner.execute(statement, &[]).unwrap();
    }
    fixture.owner.commit().unwrap();
    for table in [
        "APPS",
        "UNIQUE_APPS",
        "UNIQUE_EVENTS",
        "PAIR_APPS",
        "PAIR_EVENTS",
        "UNVALIDATED",
        "DISABLED",
        "DEFERRED_LINKS",
        "LOOSE_APPS",
        "LOOSE_EVENTS",
    ] {
        fixture
            .owner
            .execute(&format!("GRANT SELECT ON {table} TO {}", fixture.reader), &[])
            .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        fixture.owner.rollback().unwrap();
        fixture.owner.execute("SET TRANSACTION READ ONLY", &[]).unwrap();
        match fixture.owner.query_row("SELECT COUNT(*) FROM PAIR_EVENTS", &[]) {
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
    let extra = format!(
        "    relationships:\n      - name: declared_link\n        from: {{table: {s}.LOOSE_EVENTS, columns: [ID]}}\n        to: {{table: {s}.LOOSE_APPS, columns: [ID]}}\n        cardinality: many_to_one\n        check_target_uniqueness: {{max_rows: 2, max_bytes: 10000}}\n",
        s = fixture.schema
    );
    let settings = format!(
        "{}prompt:\n  list_physical_schema: true\n",
        fixture
            .settings(&extra)
            .replace("table: EVENTS", "table: '*'")
            .replace(LOOPBACK, crate::harness::AGENT_LOOPBACK)
    );
    let served = try_start_configured("oracle-relationships", &settings).expect("the bounded Oracle relationships load");
    let call =
        serde_json::json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":"describe_catalog","arguments":{}}});
    let listing = served.mcp(Some(TOKEN), &call.to_string()).json();
    let relationships: Vec<_> = listing["result"]["structuredContent"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|model| model["relationships"].as_array().unwrap())
        .collect();
    assert_eq!(relationships.len(), 4, "{listing}");
    let composite = relationships
        .iter()
        .find(|relation| relation["from"] == "PAIR_EVENTS")
        .unwrap();
    assert_eq!(composite["cardinality"], "many_to_one");
    assert_eq!(
        composite["keys"],
        serde_json::json!([{"from":"TENANT", "to":"TENANT"}, {"from":"ID", "to":"ID"}])
    );
    assert!(relationships.iter().any(|relation| relation["from"] == "UNIQUE_EVENTS"
        && relation["to"] == "UNIQUE_APPS"
        && relation["cardinality"] == "many_to_one"));
    assert!(relationships.iter().any(|relation| relation["from"] == "LOOSE_EVENTS"));
    assert!(relationships.iter().all(|relation| relation["from"] != "UNVALIDATED"
        && relation["from"] != "DISABLED"
        && relation["from"] != "DEFERRED_LINKS"));
    drop(served);
    fixture.owner.execute("INSERT INTO LOOSE_APPS VALUES (3)", &[]).unwrap();
    fixture.owner.commit().unwrap();
    let log = refused_to_start(
        Environment::Development,
        written("oracle-relationship-overflow", &settings),
        &[],
    )
    .join("\n");
    assert!(log.contains("target uniqueness check exceeded"), "{log}");
    fixture.owner.execute("DELETE FROM LOOSE_APPS", &[]).unwrap();
    for _ in 0..2 {
        fixture.owner.execute("INSERT INTO LOOSE_APPS VALUES (1)", &[]).unwrap();
    }
    fixture.owner.commit().unwrap();
    let log = refused_to_start(
        Environment::Development,
        written("oracle-relationship-duplicate", &settings),
        &[],
    )
    .join("\n");
    assert!(log.contains("duplicate nonnull full keys"), "{log}");
    let invalid = settings.replace("columns: [ID]}", "columns: [SECRET]}");
    let log = refused_to_start(Environment::Development, written("oracle-relationship-hidden", &invalid), &[]).join("\n");
    assert!(log.contains("absent or excluded") && log.contains("SECRET"), "{log}");
}

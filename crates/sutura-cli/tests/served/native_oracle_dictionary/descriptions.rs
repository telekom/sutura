use super::*;

#[test]
#[ignore = "requires just oracle-acceptance"]
fn a_served_oracle_dictionary_layers_a_separate_read_only_description_table() {
    let mut fixture = Fixture::here();
    let docs_reader = format!("ND_DOCS_{}_{}", std::process::id(), fixture.tag);
    let password = std::env::var("SUTURA_DEV_PASSWORD").unwrap();
    fixture.extra_readers.push(docs_reader.clone());
    fixture
        .admin
        .execute(&format!("CREATE USER {docs_reader} IDENTIFIED BY \"{password}\""), &[])
        .unwrap();
    fixture
        .admin
        .execute(&format!("GRANT CREATE SESSION TO {docs_reader}"), &[])
        .unwrap();
    fixture.owner.execute("CREATE TABLE COLUMN_DOCS (OBJECT_OWNER VARCHAR2(128), OBJECT_NAME VARCHAR2(128), FIELD_NAME VARCHAR2(128), DESCRIPTION VARCHAR2(256), LIVE VARCHAR2(1))", &[]).unwrap();
    for (column, prose) in [
        ("ID", "Documented event identifier."),
        ("SECRET", "Excluded column."),
        ("GHOST", "Unknown column."),
    ] {
        fixture
            .owner
            .execute(
                "INSERT INTO COLUMN_DOCS VALUES (:1, 'EVENTS', :2, :3, 'Y')",
                &[&fixture.schema, &column, &prose],
            )
            .unwrap();
    }
    fixture.owner.commit().unwrap();
    fixture
        .owner
        .execute(&format!("GRANT SELECT ON COLUMN_DOCS TO {docs_reader}"), &[])
        .unwrap();
    // Fixture DDL is complete before the reader opens its read-only snapshot.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        fixture.owner.rollback().unwrap();
        fixture.owner.execute("SET TRANSACTION READ ONLY", &[]).unwrap();
        match fixture.owner.query_row("SELECT COUNT(*) FROM COLUMN_DOCS", &[]) {
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
        "    descriptions_from:\n      - connection:\n          dialect: oracle\n          {}\n        table: {}.COLUMN_DOCS\n        key: {{schema: OBJECT_OWNER, table: OBJECT_NAME, column: FIELD_NAME}}\n        description: DESCRIPTION\n        live_row_predicate: {{column: LIVE, operator: equals, value: Y}}\n",
        fixture
            .connection
            .replace(&format!("user: {}\n", fixture.reader), &format!("user: {docs_reader}\n"))
            .replace('\n', "\n      "),
        fixture.schema
    );
    let settings = format!(
        "{}prompt:\n  list_physical_schema: true\n",
        fixture.settings(&extra).replace(LOOPBACK, crate::harness::AGENT_LOOPBACK)
    );
    let denied = settings.replace(&format!("user: {docs_reader}\n"), &format!("user: {}\n", fixture.reader));
    let log = refused_to_start(
        Environment::Development,
        written("oracle-description-base-denied", &denied),
        &[],
    )
    .join("\n");
    assert!(log.contains("ORA-00942"), "{log}");
    let served = crate::harness::try_start_configured("oracle-description-overlay", &settings)
        .expect("the separately granted documentation reader starts the deployment");
    let call =
        serde_json::json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":"describe_catalog","arguments":{}}});
    let listing = served.mcp(Some(TOKEN), &call.to_string()).json();
    let columns = listing["result"]["structuredContent"]["models"][0]["columns"]
        .as_array()
        .unwrap();
    let id = columns.iter().find(|column| column["name"] == "ID").unwrap();
    assert_eq!(id["description"], "Documented event identifier.");
    assert!(
        columns
            .iter()
            .all(|column| column["name"] != "SECRET" && column["name"] != "GHOST")
    );
    let selected = run(&served, &format!("select * from \"{}\".\"EVENTS\"", fixture.schema));
    assert_eq!(selected.status, 200, "{}", selected.body);
    assert_eq!(selected.json()["columns"], serde_json::json!(["AMOUNT", "ID"]));
    assert_eq!(
        run(&served, &format!("select \"SECRET\" from \"{}\".\"EVENTS\"", fixture.schema)).status,
        422
    );
    let log = served.awaiting("description overlay load completed");
    assert!(
        log.iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|event| event["msg"] == "description overlay load completed" && event["ignored_columns"] == 2),
        "{log:?}"
    );
}

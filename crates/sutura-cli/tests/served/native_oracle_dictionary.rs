#![expect(clippy::disallowed_methods, reason = "a hosted fixture drives only its own Oracle accounts and tables")]
//! Native dictionary metadata and planned SQL over a private Oracle fixture. This is catalog
//! selection under a shared login; raw pass-through SQL still follows that login's database grants.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sutura_config::Environment;
use crate::harness::{LOOPBACK, SINGLE_USER, Served, TOKEN, refused_to_start, start_configured, v1, written};

struct Fixture {
    admin: oracledb::Connection,
    owner: oracledb::Connection,
    schema: String,
    reader: String,
    connection: String,
    root: PathBuf,
}

impl Fixture {
    fn here() -> Self {
        let tier = sutura_dev::provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle");
        let endpoint = tier.endpoint().expect("the named acceptance task requires Oracle");
        let password = std::env::var("SUTURA_DEV_PASSWORD").unwrap();
        let authority = std::env::var("SUTURA_ORACLE_TIER_CA").unwrap();
        let anchors = std::fs::read_to_string(&authority).unwrap();
        let dial = oracledb::Config::default()
            .set_connect_string(&format!("tcps://{}:{}/FREEPDB1", endpoint.host(), endpoint.port())).unwrap()
            .set_trust_anchors_pem(&anchors).unwrap();
        let admin = oracledb::connect(dial.clone().set_credentials("SYSTEM", &password)).unwrap();
        let schema = format!("ND_OWNER_{}", std::process::id());
        let reader = format!("ND_READER_{}", std::process::id());
        assert!(!password.contains('"'), "the generated fixture credential must be quotable");
        for user in [&schema, &reader] {
            if let Err(cause) = admin.execute(&format!("DROP USER {user} CASCADE"), &[]) {
                assert!(cause.to_string().contains("ORA-01918"), "{cause}");
            }
            admin.execute(&format!("CREATE USER {user} IDENTIFIED BY \"{password}\""), &[]).unwrap();
            admin.execute(&format!("GRANT CREATE SESSION TO {user}"), &[]).unwrap();
        }
        admin.execute(&format!("GRANT CREATE TABLE, UNLIMITED TABLESPACE TO {schema}"), &[]).unwrap();
        let owner = oracledb::connect(dial.set_credentials(&schema, &password)).unwrap();
        for statement in [
            "CREATE TABLE EVENTS (ID NUMBER(10,0), AMOUNT NUMBER(12,2), SECRET VARCHAR2(64), OPAQUE CLOB)",
            "CREATE TABLE HIDDEN (ID NUMBER(10,0))",
            "CREATE TABLE \"MixedCase\" (\"VisibleId\" NUMBER(10,0), \"SecretValue\" VARCHAR2(32))",
            "COMMENT ON TABLE EVENTS IS 'Metadata; ''quotes'' stay descriptive.'",
            "COMMENT ON COLUMN EVENTS.AMOUNT IS 'An exact decimal.'",
            "INSERT INTO EVENTS VALUES (7, 2.50, 'private value', 'private text')",
            "INSERT INTO \"MixedCase\" VALUES (8, 'another private value')",
        ] { owner.execute(statement, &[]).unwrap(); }
        owner.commit().unwrap();
        for table in ["EVENTS", "\"MixedCase\""] {
            owner.execute(&format!("GRANT SELECT ON {table} TO {reader}"), &[]).unwrap();
        }
        // Wait for a read-only snapshot after fixture DDL, without treating ORA-01466 as a verdict.
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            owner.rollback().unwrap();
            owner.execute("SET TRANSACTION READ ONLY", &[]).unwrap();
            match owner.query_row("SELECT ID FROM EVENTS", &[]) {
                Ok(_) => { owner.rollback().unwrap(); break; }
                Err(cause) => {
                    assert!(cause.to_string().contains("ORA-01466") && Instant::now() < deadline, "{cause}");
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
        let root = std::env::temp_dir().join(format!("sutura-native-dictionary-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("password"), password).unwrap();
        let connection = format!("host: 127.0.0.1\n    port: {}\n    service_name: FREEPDB1\n    user: {reader}\n    password_file: {}\n    transport_mode: verified\n    transport_anchors: {authority}", endpoint.port(), root.join("password").display());
        Self { admin, owner, schema, reader, connection, root }
    }

    fn settings(&self, extra: &str) -> String {
        format!("server:\n{LOOPBACK}security:\n{SINGLE_USER}  access_token: {TOKEN}\ntelemetry:\n  format: bunyan\ncatalogs:\n  - name: dictionary\n    kind: rdbms\n    version: dict-1\n    source_alias: warehouse\n    dictionary_source: native_dictionary\n    refresh_seconds: 1\n    include: [{{schema: {}, table: EVENTS}}]\n    exclude_columns: ['SECRET*']\n    exclude_types: [CLOB]\n{extra}    connection:\n      dialect: oracle\n      {}\nsources:\n  warehouse:\n    kind: oracle\n    {}\n    posture: shared-service-user\ntools:\n  run_sql:\n    enabled: true\n    sources: [warehouse]\n    federation_allowed: true\n", self.schema, self.connection.replace('\n', "\n  "), self.connection)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.owner.close());
        for user in [&self.reader, &self.schema] {
            drop(self.admin.execute(&format!("DROP USER {user} CASCADE"), &[]));
        }
        drop(std::fs::remove_dir_all(&self.root));
    }
}

fn run(served: &Served, statement: &str) -> crate::harness::Reply {
    served.post(&v1(sutura_http::constants::base_paths::RUN_SQL), Some(TOKEN), &serde_json::json!({"statement": statement}).to_string())
}

fn digest(served: &Served) -> serde_json::Value {
    let reply = served.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(!reply.body.contains("SECRET"), "excluded metadata is absent from describe_catalog");
    reply.json()["provenance"]["definition_digest"].clone()
}

#[test]
#[ignore = "requires just oracle-acceptance"]
fn a_served_native_oracle_dictionary_keeps_exact_metadata_and_limits_planned_columns() {
    use sutura_catalog_rdbms::{ColumnMetadata, Dictionary, DictionaryReader, RdbmsCatalog, RdbmsError, Table, TableAddress};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
    struct Recorded<'a>(&'a str);
    impl DictionaryReader for Recorded<'_> {
        fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
            Ok(Dictionary::new(vec![Table::new(String::from("EVENTS"), TableAddress::in_schema(self.0.to_owned(), String::from("EVENTS")),
                vec![String::from("ID"), String::from("AMOUNT")], Some(String::from("Metadata; 'quotes' stay descriptive.")))
                .with_column_metadata([
                    (String::from("ID"), ColumnMetadata::new(Some(String::from("NUMBER(10,0)")), None)),
                    (String::from("AMOUNT"), ColumnMetadata::new(Some(String::from("NUMBER(12,2)")), Some(String::from("An exact decimal.")))),
                ])], vec![]))
        }
    }
    let fixture = Fixture::here();
    let expected = RdbmsCatalog::new(SourceName::parse("dictionary").unwrap(), DefinitionVersion::parse("dict-1").unwrap(), Recorded(&fixture.schema))
        .with_source_alias(SourceName::parse("warehouse").unwrap()).load().unwrap();
    let settings = fixture.settings("");
    let served = start_configured("native-oracle", &settings);
    assert_eq!(digest(&served), expected.digest().as_str(), "native comments and numeric precision survive the served load");
    let selected = run(&served, &format!("select * from \"{}\".\"EVENTS\"", fixture.schema));
    assert_eq!(selected.status, 200, "{}", selected.body);
    assert_eq!(selected.json()["columns"], serde_json::json!(["AMOUNT", "ID"]));
    assert_eq!(selected.json()["rows"], serde_json::json!([["2.50", "7"]]));
    for statement in [
        format!("select e.\"SECRET\" from \"{}\".\"EVENTS\" e", fixture.schema),
        format!("select \"SECRET\" from \"{}\".\"EVENTS\"", fixture.schema),
        format!("select \"ID\" from \"{}\".\"EVENTS\" where \"SECRET\" is not null", fixture.schema),
        format!("select a.\"ID\" from \"{0}\".\"EVENTS\" a join \"{0}\".\"EVENTS\" b on a.\"SECRET\" = b.\"SECRET\"", fixture.schema),
    ] {
        let reply = run(&served, &statement);
        assert_eq!(reply.status, 422, "{statement}: {}", reply.body);
        assert_eq!(reply.json()["code"], "statement_failed", "{}", reply.body);
        assert!(!reply.body.contains("private value") && !reply.body.contains("private text"));
    }
    let before = digest(&served);
    fixture.owner.execute("ALTER TABLE EVENTS ADD (ALLOWED NUMBER(10,0) DEFAULT 9, SECRET_NEW VARCHAR2(32))", &[]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let after = loop {
        let next = digest(&served);
        if next != before { break next; }
        assert!(Instant::now() < deadline, "the new allowed column was never published");
        std::thread::sleep(Duration::from_millis(200));
    };
    let reply = run(&served, &format!("select \"ALLOWED\" from \"{}\".\"EVENTS\"", fixture.schema));
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.json()["rows"], serde_json::json!([["9"]]));
    assert_eq!(run(&served, &format!("select \"SECRET_NEW\" from \"{}\".\"EVENTS\"", fixture.schema)).status, 422);
    fixture.owner.execute("ALTER TABLE EVENTS ADD UNSUPPORTED NUMBER", &[]).unwrap();
    served.awaiting("keeping the bundle already served");
    assert_eq!(digest(&served), after, "a failed refresh must retain the entire last bundle");
    drop(served);
    fixture.owner.execute("ALTER TABLE EVENTS DROP COLUMN UNSUPPORTED", &[]).unwrap();
    for kind in ["BINARY_FLOAT", "RAW(16)", "BLOB"] {
        fixture.owner.execute(&format!("ALTER TABLE EVENTS ADD UNSUPPORTED {kind}"), &[]).unwrap();
        let log = refused_to_start(Environment::Development, written("native-oracle-unsupported", &settings), &[]).join("\n");
        assert!(log.contains(&format!("no exact supported mapping for {kind}")), "{kind}: {log}");
        let excluded = settings.replace("exclude_types: [CLOB]", &format!("exclude_types: [CLOB, '{kind}']"));
        drop(start_configured("native-oracle-excluded-type", &excluded));
        fixture.owner.execute("ALTER TABLE EVENTS DROP COLUMN UNSUPPORTED", &[]).unwrap();
    }
    for (case, text, expected) in [
        ("rows", fixture.settings("    max_dictionary_rows: 1\n"), "maximum of 1 rows"),
        ("bytes", fixture.settings("    max_dictionary_bytes: 1\n"), "maximum of 1 bytes"),
        ("visibility", settings.replace("table: EVENTS", "table: HIDDEN"), "matches no visible table"),
        ("schema-data", settings.replace(&format!("schema: {}", fixture.schema), "schema: \"x' OR '1'='1\""), "matches no visible table"),
        ("case", settings.replace("table: EVENTS", "table: events"), "matches no visible table"),
    ] {
        let log = refused_to_start(Environment::Development, written(&format!("native-oracle-{case}"), &text), &[]).join("\n");
        assert!(log.contains(expected), "{case}: {log}");
    }
    let quoted = settings.replace("table: EVENTS", "table: MixedCase").replace("['SECRET*']", "['Secret*']");
    let served = start_configured("native-oracle-quoted", &quoted);
    let reply = run(&served, &format!("select \"VisibleId\" from \"{}\".\"MixedCase\"", fixture.schema));
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.json()["rows"], serde_json::json!([["8"]]));
    assert_eq!(run(&served, &format!("select \"SecretValue\" from \"{}\".\"MixedCase\"", fixture.schema)).status, 422);
}

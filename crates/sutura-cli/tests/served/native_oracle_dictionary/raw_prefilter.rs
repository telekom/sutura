//! Regression for `github.com/telekom/sutura#2038`: an opt-in raw prefilter on a native-dictionary
//! CLOB column with a text policy must still apply, in every spelling #2038 reproduced.

use super::*;
use crate::harness::start_configured_with_environment;

pub(super) fn applies_on_a_native_dictionary_clob(fixture: &Fixture) {
    fixture.owner.execute("ALTER TABLE EVENTS ADD (SCRIPT CLOB)", &[]).unwrap();
    fixture
        .owner
        .execute(
            "UPDATE EVENTS SET SCRIPT = 'alpha IDENTIFIED BY topsecret beta' WHERE ID = 7",
            &[],
        )
        .unwrap();
    fixture.owner.commit().unwrap();
    fixture
        .owner
        .execute(&format!("GRANT SELECT ON EVENTS TO {}", fixture.reader), &[])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        fixture.owner.rollback().unwrap();
        fixture.owner.execute("SET TRANSACTION READ ONLY", &[]).unwrap();
        match fixture.owner.query_row("SELECT SCRIPT FROM EVENTS WHERE ID = 7", &[]) {
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
    let rule = concat!(
        "    column_rules:\n",
        "      - column: SCRIPT\n",
        "        max_text_bytes: 4000\n",
        "        redact:\n",
        "          - pattern: \"(?i)IDENTIFIED BY \\\\S+\"\n",
        "            replace: \"IDENTIFIED BY ***\"\n",
        "        raw_prefilter:\n",
        "          acknowledged_because: this fixture accepts secret-dependent search timing\n",
    );
    let settings = fixture
        .settings(rule)
        .replace("exclude_columns: ['SECRET*']", "exclude_columns: ['SECRET*', 'OPAQUE']")
        .replace("exclude_types: [CLOB]", "exclude_types: []");
    let served = start_configured_with_environment(
        "native-oracle-raw-prefilter",
        &settings,
        None,
        &[("RUST_LOG", "info,sutura_exec_datafusion::combine::statement=debug")],
        &[],
    );
    let quoted = format!("\"{}\".\"EVENTS\"", fixture.schema);
    let lowercase = format!("{}.events", fixture.schema);
    for sql in [
        format!("select \"ID\" from {quoted} where \"SCRIPT\" like '%alpha%'"),
        format!("select \"ID\" from {quoted} where upper(\"SCRIPT\") like '%ALPHA%'"),
        format!("select id from {lowercase} where script like '%alpha%'"),
    ] {
        let reply = run(&served, &sql);
        assert_eq!(reply.status, 200, "{sql}: {}\n{:?}", reply.body, served.log());
        assert_eq!(reply.json()["rows"], serde_json::json!([["7"]]), "{sql}: {}", reply.body);
        assert!(
            !reply.body.contains("topsecret"),
            "the redaction must still mask the stored secret: {sql}: {}",
            reply.body
        );
    }
    drop(served);
}

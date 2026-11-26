//! Real Oracle logins through both production YAML paths. The fixture task asserts the stored
//! verifier before this process starts. Native mode is tested on the patched server; an older
//! server's 10G verifier is tested independently over verified TCPS.

use std::path::PathBuf;

use sutura_config::Environment;

use crate::harness::{
    LOOPBACK, SINGLE_USER, Served, TOKEN, config_path, derived_beside, refused_to_start, start_configured,
    start_configured_with_environment, v1, written,
};

/// The subscriber reads `RUST_LOG` before the settings' filter - see `no_echo::TRACE` - so this is
/// what makes `run_sql`'s own `debug` line (`crates/sutura-app/src/raw.rs`'s `run_at`) land in the
/// process's piped stderr for [`Served::awaiting`] to find, without turning on `trace` noise from
/// everything else.
const DEBUG: [(&str, &str); 1] = [("RUST_LOG", "debug")];

struct Tier {
    port: u16,
    service: String,
    user: String,
    password: String,
    legacy_user: String,
    legacy_password: String,
    verifier: String,
    channel: String,
}

#[derive(Clone, Copy)]
enum FixtureSource {
    Oracle,
    Files,
}

impl Tier {
    fn here() -> Self {
        let required = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("the Oracle acceptance task sets {name}"));
        let channel = if required("SUTURA_ORACLE_ACCEPTANCE_MODE") == "native" {
            String::from(
                "transport_mode: native-encryption\n    native_encryption_acknowledged_because: a throwaway native-encryption fixture",
            )
        } else {
            format!(
                "transport_mode: verified\n    transport_anchors: {}",
                required("SUTURA_ORACLE_ACCEPTANCE_CA")
            )
        };
        Self {
            port: required("SUTURA_ORACLE_ACCEPTANCE_PORT")
                .parse()
                .expect("the published port is a u16"),
            service: required("SUTURA_ORACLE_ACCEPTANCE_SERVICE"),
            user: required("SUTURA_ORACLE_ACCEPTANCE_USER"),
            password: required("SUTURA_ORACLE_ACCEPTANCE_PASSWORD"),
            legacy_user: required("SUTURA_ORACLE_ACCEPTANCE_LEGACY_USER"),
            legacy_password: required("SUTURA_ORACLE_ACCEPTANCE_LEGACY_PASSWORD"),
            verifier: required("SUTURA_ORACLE_ACCEPTANCE_VERIFIER"),
            channel,
        }
    }

    fn connection(&self, user: &str, password_file: &std::path::Path, legacy: bool) -> String {
        let opt_in = if legacy {
            "\n    legacy_password_verifier: true\n    legacy_password_verifier_acknowledged_because: a fixture retains only its legacy verifier"
        } else {
            ""
        };
        format!(
            "host: 127.0.0.1\n    port: {}\n    service_name: {}\n    user: {user}\n    password_file: {}\n    {}{opt_in}",
            self.port,
            self.service,
            password_file.display(),
            self.channel,
        )
    }

    fn settings(&self, case: &str, dictionary: bool, legacy: bool, wrong_password: bool, source_kind: FixtureSource) -> String {
        let root = derived_beside(&config_path(case));
        std::fs::create_dir_all(&root).expect("the fixture directory creates");
        let legacy_password = root.join("legacy-password");
        let password = root.join("password");
        std::fs::write(&password, &self.password).expect("the modern fixture password writes");
        std::fs::write(
            &legacy_password,
            if wrong_password {
                "not-the-fixture-password"
            } else {
                &self.legacy_password
            },
        )
        .expect("the legacy fixture password writes");
        let tested = self.connection(&self.legacy_user, &legacy_password, legacy);
        let (catalog, source) = if dictionary {
            (
                format!(
                    "  - name: dictionary\n    kind: rdbms\n    version: native-fixture\n    environment: test\n    \
                     source_alias: local\n    dictionary_schema: {}\n    connection:\n      dialect: oracle\n      {}\n",
                    self.legacy_user,
                    tested.replace('\n', "\n  "),
                ),
                self.connection(&self.user, &password, false),
            )
        } else {
            let catalog = local_catalog(&root);
            (
                format!(
                    "  - name: model\n    kind: markdown\n    version: native-fixture\n    dir: {}\n    data_dir: {}\n",
                    catalog.display(),
                    root.display()
                ),
                tested,
            )
        };
        let source = match source_kind {
            FixtureSource::Oracle => format!("kind: oracle\n    {source}"),
            FixtureSource::Files => format!("kind: files\n    data_dir: {}", root.display()),
        };
        format!(
            "server:\n{LOOPBACK}security:\n{SINGLE_USER}  access_token: {TOKEN}\n\
             telemetry:\n  format: bunyan\n\
             catalogs:\n{catalog}\
             sources:\n  local:\n    {source}\n    posture: shared-service-user\n",
        )
    }
}

fn local_catalog(root: &std::path::Path) -> PathBuf {
    let catalog = root.join("catalog");
    std::fs::create_dir_all(&catalog).expect("the local catalog creates");
    std::fs::write(
        catalog.join("declaration.md"),
        "---\nkind: declaration\ndefinitions: [structure, metrics, grains, anchors, descriptions, column_types]\nknowledge: []\n---\n",
    )
    .expect("the declaration writes");
    std::fs::write(
        catalog.join("model.md"),
        "---\nkind: model\nname: events\nsource: local\ntable: EVENTS\ncolumns:\n  - {name: ID, type: NUMBER(10), nullable: false}\n  - {name: PAYLOAD, type: VARCHAR, nullable: false}\n  - {name: MONTH, type: DATE, nullable: false}\n---\nThe login fixture.\n",
    )
    .expect("the model writes");
    std::fs::write(
        catalog.join("metric.md"),
        "---\nkind: metric\nname: payload_total\nmodel: events\nmeasure:\n  simple: {aggregate: sum, column: ID}\ntime_column: MONTH\ngrains: [month]\ndimensions:\n  - {name: payload, column: PAYLOAD}\nanchor:\n  range: {start: 2026-06-01, end: 2026-07-01}\n  value: 8256\naudience: open\n---\nThe fixture total by payload.\n",
    ).expect("the metric writes");
    catalog
}

fn protected_queries(deployment: &Served) {
    let mut expected: Vec<_> = (1..=128)
        .map(|id| serde_json::json!([format!("{id:x>2000}"), "2026-06-01", id.to_string()]))
        .collect();
    expected.sort_by(|left, right| left[0].as_str().cmp(&right[0].as_str()));
    for _ in 0..2 {
        let reply = deployment.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            r#"{"metrics":["payload_total"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["payload"]}"#,
        );
        assert_eq!(reply.status, 200, "the protected query answers: {}", reply.body);
        assert_eq!(
            reply.json()["rows"],
            serde_json::json!(expected),
            "the complete multi-record payload arrives"
        );
    }
}

#[test]
#[ignore = "requires just oracle-native-acceptance"]
fn oracle_legacy_logins_reach_both_production_paths_only_with_the_opt_in() {
    let tier = Tier::here();
    for dictionary in [false, true] {
        let path = if dictionary { "dictionary" } else { "source" };
        let case = format!("oracle-{}-{path}", tier.verifier);
        let off = format!("{case}-off");
        let refused = refused_to_start(
            Environment::Development,
            written(&off, &tier.settings(&off, dictionary, false, false, FixtureSource::Oracle)),
            &[],
        )
        .join("\n");
        assert!(
            refused.contains(&format!("({}) is refused", tier.verifier)),
            "the {path} must name the legacy verifier it refused: {refused}"
        );

        let deployment = std::panic::catch_unwind(|| {
            start_configured(&case, &tier.settings(&case, dictionary, true, false, FixtureSource::Oracle))
        })
        .expect("the legacy opt-in must let the correct credentials serve");
        let reply = deployment.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
        assert_eq!(reply.status, 200, "the {path} logs in and loads the fixture: {}", reply.body);
        assert_eq!(reply.json()["provenance"]["definition_version"], "native-fixture");
        if !dictionary {
            protected_queries(&deployment);
        }
        drop(deployment);

        let wrong = format!("{case}-wrong");
        let refused = refused_to_start(
            Environment::Development,
            written(&wrong, &tier.settings(&wrong, dictionary, true, true, FixtureSource::Oracle)),
            &[],
        )
        .join("\n");
        // Every other production path reads the server's real ORA-01017 over the encrypted
        // channel. This login attempt always uses `tier.legacy_user` (the legacy-verifier
        // account), so on native encryption specifically the server sends its login-failure
        // error unprotected - before this exchange's protection is in force for it - so the
        // driver cannot decrypt it, and `transport.rs`'s `extract_packet` reports it as the
        // shape an unauthenticated login's own refusal takes, never as decrypted, trusted data.
        // **Limit:** this is observed only for the legacy account on native encryption in CI;
        // every other path (the modern account, or TCPS) still surfaces the real ORA-01017.
        if tier.channel.starts_with("transport_mode: native-encryption") {
            assert!(
                refused.contains("a DATA packet during login did not authenticate"),
                "the {path} must name the unreadable login refusal, not a generic transport message: {refused}"
            );
        } else {
            assert!(
                refused.contains("ORA-01017"),
                "the {path} must refuse the wrong password: {refused}"
            );
        }
    }
}

/// On the native-encryption path, a statement the server itself fails must reach the operator's
/// log as that server error, not as the generic native-encryption refusal the unprotected
/// break-marker packet used to produce (#2084). Casting the fixture's own non-numeric `PAYLOAD`
/// text to `bigint` in the *projection* fails only after Oracle's own leg already returned those
/// rows successfully - fitting them to the declared output type is this process's own work, so
/// that failure is `StatementStopped::NotPlanned` (a leg's rows that did not fit its table), not a
/// leg failure, and the caller-visible code stays the generic `statement_failed` with no leg log
/// line at all; the same cast in a *predicate* is instead pushed down as a filter and evaluated by
/// Oracle while it runs the query, so its failure is the leg's own and is what this test needs.
/// `bigint` is a standard SQL type the local resolver accepts and pushes down as-is; a
/// vendor-specific type such as `number` parses as `DataType::Custom` and is refused locally before
/// any Oracle leg runs, which would make the ORA code impossible to observe regardless of the fix
/// under test. The statement also has to name a table the catalog already knows - `EVENTS`, from
/// the fixture catalog `local_catalog` builds - because `resolve.rs`'s `factor()` refuses any other
/// table name locally as `Refused::UnknownTable`; asserting the refusal code is `source_failed`
/// (the single-source `run_sql` mapping in `crates/sutura-app/src/raw.rs` for a leg that itself
/// returned `RawRefusalReason::StatementFailed`), not `unknown_table` or `statement_failed`, pins
/// that this is genuinely the server's own refusal. The reply itself can never carry the driver's
/// text: the raw refusal vocabulary carries no driver text by design
/// (`crates/sutura-domain/src/raw.rs`) - whoever controls the statement controls part of what a
/// database says about it, so the closed code is all a caller gets. `run_sql`'s own debug line
/// (`crates/sutura-app/src/raw.rs`'s `run_at`) carries the driver's words verbatim instead, so this
/// starts the deployment at `RUST_LOG=debug` and reads its own piped log for the ORA code, proving
/// the fix admits the break marker and reads the encrypted error record that follows it rather than
/// refusing before ever reaching the server's answer.
#[test]
#[ignore = "requires just oracle-native-acceptance"]
fn a_server_failing_statement_over_native_encryption_logs_the_ora_code() {
    let mut tier = Tier::here();
    tier.legacy_user = tier.user.clone();
    tier.legacy_password = tier.password.clone();
    let case = "oracle-native-statement-failure";
    let settings = format!(
        "{}tools:\n  run_sql:\n    enabled: true\n    sources: [\"local\"]\n",
        tier.settings(case, false, false, false, FixtureSource::Oracle)
    );
    let deployment = std::panic::catch_unwind(|| start_configured_with_environment(case, &settings, None, &DEBUG, &[]))
        .expect("native mode must let the correct credentials serve");
    let reply = deployment.post(
        &v1(sutura_http::constants::base_paths::RUN_SQL),
        Some(TOKEN),
        &serde_json::json!({ "statement": "select ID from EVENTS where cast(PAYLOAD as bigint) = 1" }).to_string(),
    );
    assert_eq!(reply.status, 422, "a source-failed statement refusal is 422: {}", reply.body);
    assert_eq!(reply.json()["outcome"], "raw_refusal", "{}", reply.body);
    assert_eq!(
        reply.json()["code"],
        "source_failed",
        "EVENTS is a catalog table and the cast is pushed down as a filter Oracle itself evaluates, \
         so this must be the server's own refusal, not local resolution or local output-fitting: {}",
        reply.body
    );
    deployment.awaiting("ORA-01722");
}

#[test]
#[ignore = "requires just oracle-native-acceptance"]
#[expect(
    clippy::disallowed_methods,
    reason = "the default driver is also checked against the real required-NNE listener"
)]
fn oracle_native_logins_require_both_strong_services_on_both_production_paths() {
    let mut tier = Tier::here();
    let expected = std::env::var("SUTURA_ORACLE_NATIVE_EXPECT").expect("the native acceptance task declares its server policy");
    assert!(matches!(expected.as_str(), "success" | "encryption" | "integrity"));
    if expected == "success" {
        let address = format!("127.0.0.1:{}/{}", tier.port, tier.service);
        let plain = oracledb::Config::default()
            .set_connect_string(&address)
            .expect("the published TCP endpoint parses")
            .set_credentials(&tier.user, &tier.password);
        let Err(refused) = oracledb::connect(plain) else {
            panic!("a driver without native mode must not log in to the required-NNE listener");
        };
        assert!(
            refused.to_string().contains("native network encryption and data integrity"),
            "the missing native mode is refused: {refused}"
        );
    }
    tier.legacy_user = tier.user.clone();
    tier.legacy_password = tier.password.clone();
    for dictionary in [false, true] {
        let path = if dictionary { "dictionary" } else { "source" };
        let case = format!("oracle-native-{expected}-{path}");
        let source_kind = if expected == "success" || !dictionary {
            FixtureSource::Oracle
        } else {
            FixtureSource::Files
        };
        let settings = tier.settings(&case, dictionary, false, false, source_kind);
        if expected == "success" {
            let deployment = std::panic::catch_unwind(|| start_configured(&case, &settings))
                .expect("native mode must let the correct credentials serve");
            let reply = deployment.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
            assert_eq!(reply.status, 200, "the native {path} loads the fixture: {}", reply.body);
            if !dictionary {
                protected_queries(&deployment);
            }
        } else {
            let refused = refused_to_start(Environment::Development, written(&case, &settings), &[]).join("\n");
            assert!(
                refused.contains("native encryption refused") || refused.contains("ORA-12650") || refused.contains("ORA-12269"),
                "the weak-only {expected} peer must be refused at negotiation on the {path}: {refused}"
            );
        }
    }
}

#[path = "oracle_raw.rs"]
mod raw;

//! Real Oracle logins through both production YAML paths. The fixture task asserts the stored
//! verifier before this process starts. Native mode is tested on the patched server; an older
//! server's 10G verifier is tested independently over verified TCPS.

use std::path::PathBuf;

use sutura_config::Environment;

use crate::harness::{
    LOOPBACK, SINGLE_USER, Served, TOKEN, config_path, derived_beside, refused_to_start, start_configured, v1, written,
};

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

    fn settings(&self, case: &str, dictionary: bool, legacy: bool, wrong_password: bool, oracle_source: bool) -> String {
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
        let source = if oracle_source {
            format!("kind: oracle\n    {source}")
        } else {
            format!("kind: files\n    data_dir: {}", root.display())
        };
        format!(
            "server:\n{LOOPBACK}security:\n{SINGLE_USER}  access_token: {TOKEN}\n\
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
        "---\nkind: model\nname: events\nsource: local\ntable: EVENTS\ncolumns:\n  - {name: ID, type: NUMERIC, nullable: false}\n  - {name: PAYLOAD, type: VARCHAR, nullable: false}\n  - {name: MONTH, type: DATE, nullable: false}\n---\nThe login fixture.\n",
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
        .map(|id| serde_json::json!(["2026-06-01", format!("{id:x>2000}"), id.to_string()]))
        .collect();
    expected.sort_by(|left, right| left[1].as_str().cmp(&right[1].as_str()));
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
            written(&off, &tier.settings(&off, dictionary, false, false, true)),
            &[],
        )
        .join("\n");
        assert!(
            refused.contains(&format!("({}) is refused", tier.verifier)),
            "the {path} must name the legacy verifier it refused: {refused}"
        );

        let wrong = format!("{case}-wrong");
        let refused = refused_to_start(
            Environment::Development,
            written(&wrong, &tier.settings(&wrong, dictionary, true, true, true)),
            &[],
        )
        .join("\n");
        assert!(
            refused.contains("ORA-01017"),
            "the {path} must refuse the wrong password: {refused}"
        );

        let deployment = start_configured(&case, &tier.settings(&case, dictionary, true, false, true));
        let reply = deployment.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
        assert_eq!(reply.status, 200, "the {path} logs in and loads the fixture: {}", reply.body);
        assert_eq!(reply.json()["provenance"]["definition_version"], "native-fixture");
        if !dictionary {
            protected_queries(&deployment);
        }
    }
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
            refused.to_string().contains("native"),
            "the missing native mode is refused: {refused}"
        );
    }
    tier.legacy_user = tier.user.clone();
    tier.legacy_password = tier.password.clone();
    for dictionary in [false, true] {
        let path = if dictionary { "dictionary" } else { "source" };
        let case = format!("oracle-native-{expected}-{path}");
        let settings = tier.settings(&case, dictionary, false, false, expected == "success" || !dictionary);
        if expected == "success" {
            let deployment = start_configured(&case, &settings);
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

//! `examples/metrics-layer` served for real: a `DataHub` catalog, Cube as the only source and
//! Keycloak as the identity provider. The example's own settings run against the compose `cube`
//! profile - `cube-keycloak` and `cube-metrics` - with the loopback `DataHub` fake serving the
//! recorded corpus, and a TLS front this file runs before `cube-metrics`, because an impersonating
//! Cube source is `https` only and Cube serves plain HTTP.
//!
//! Cube verifies each token against the realm's keys, and its access policy reads the `regions`
//! claim, so a caller's rows come from that caller's own token. With `impersonation-at-source`
//! sutura exchanges each caller's token, and alice and bob read their own regions; with
//! `shared-service-user` every caller reads what the deployment's own client reads.
//!
//! `#[ignore]`d, behind `just cube-acceptance`: the tier is a docker service, and `just test` and
//! the nix sandbox do not start one.

#[cfg(test)]
mod tests {
    use std::io::{ErrorKind, Read as _};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use sutura_catalog_datahub::test_support::{DEPLOYMENT_PROPERTY, FakeServer, happy_path_answers};
    use sutura_http_client::tls_test_support::{issue, server_config};

    use crate::harness::{config_path, derived_beside, start_configured, v1};

    /// The client secrets `compose.services.yaml` hands `cube-keycloak`'s realm.
    const EXCHANGE_SECRET: &str = "sutura-dev-exchange-secret";
    const DEPLOYMENT_SECRET: &str = "sutura-dev-deployment-secret";

    /// The recorded corpus's anchored question: `revenue` over June 2026.
    const QUESTION: &str = r#"{"metrics":["revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;

    /// What the example's settings declare, and what this run puts in its place.
    const PORT: &str = "port: 8080";
    const KEY_SET_FILE: &str = "\"/run/sutura/jwks.json\"";
    const ANCHORS: &str = "\"/run/sutura/cube-ca.pem\"";
    const DATAHUB_ENDPOINT: &str = "\"http://127.0.0.1:8081\"";
    const DATAHUB_TOKEN_FILE: &str = "\"/run/sutura/datahub-token\"";
    const METRIC_PROPERTY: &str = "metric_property: \"sutura\"";
    const CUBE_ENDPOINT: &str = "\"https://cube.example.com\"";
    const CUBE_TOKEN_FILE: &str = "\"/run/sutura/cube-token\"";
    const TOKEN_ENDPOINT: &str = "\"http://127.0.0.1:8180/realms/metrics-layer/protocol/openid-connect/token\"";
    const SECRET_FILE: &str = "\"/run/sutura/exchange-secret\"";

    fn tier(service: &str) -> String {
        sutura_dev::provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), service)
            .endpoint()
            .map(|endpoint| format!("{endpoint}"))
            .expect("`just cube-acceptance` provisions the cube tier first")
    }

    fn realm(keycloak: &str, path: &str) -> String {
        format!("http://{keycloak}/realms/metrics-layer/protocol/openid-connect/{path}")
    }

    /// The realm is plain `http` on loopback, so the agent carries no TLS setting of its own.
    fn agent() -> ureq::Agent {
        sutura_http_client::agent(|config| config.timeout_global(Some(Duration::from_secs(30))))
    }

    fn token(keycloak: &str, form: &[(&str, &str)]) -> String {
        let url = realm(keycloak, "token");
        let mut answer = agent()
            .post(&url)
            .send_form(form.iter().copied())
            .unwrap_or_else(|cause| panic!("{url} refused {form:?}: {cause}"));
        let text = answer.body_mut().read_to_string().expect("a token response is text");
        let body: serde_json::Value = serde_json::from_str(&text).expect("a token response is JSON");
        String::from(
            body["access_token"]
                .as_str()
                .unwrap_or_else(|| panic!("no access_token: {body}")),
        )
    }

    fn caller(keycloak: &str, name: &str) -> String {
        token(
            keycloak,
            &[
                ("grant_type", "password"),
                ("client_id", "sutura"),
                ("username", name),
                ("password", name),
            ],
        )
    }

    /// Accepts on loopback with a leaf this run issued and forwards each connection to `upstream`.
    fn tls_front(upstream: String, ca: &Path) -> String {
        let issued = issue();
        std::fs::write(ca, issued.certificate().pem()).expect("the CA bundle is writable");
        let config = server_config(&issued);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port binds");
        let endpoint = format!("https://{}", listener.local_addr().expect("a bound listener has an address"));
        std::thread::spawn(move || {
            for client in listener.incoming().flatten() {
                let config = Arc::clone(&config);
                let upstream = upstream.clone();
                std::thread::spawn(move || forward(client, config, &upstream));
            }
        });
        endpoint
    }

    fn forward(client: TcpStream, config: Arc<rustls::ServerConfig>, upstream: &str) {
        let tick = Some(Duration::from_millis(5));
        let (Ok(mut cube), Ok(connection)) = (TcpStream::connect(upstream), rustls::ServerConnection::new(config)) else {
            return;
        };
        if client.set_read_timeout(tick).is_err() || cube.set_read_timeout(tick).is_err() {
            return;
        }
        let mut tls = rustls::StreamOwned::new(connection, client);
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let open = pump(tls.read(&mut buffer), &buffer, &mut cube) && pump(cube.read(&mut buffer), &buffer, &mut tls);
            if !open {
                return;
            }
        }
    }

    /// Writes what one read returned to `into`; `false` once either side is closed.
    fn pump(read: std::io::Result<usize>, buffer: &[u8], into: &mut impl std::io::Write) -> bool {
        match read {
            Ok(0) => false,
            Ok(length) => into.write_all(&buffer[..length]).is_ok(),
            Err(cause) => matches!(cause.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
        }
    }

    /// The files the example reads, written under a directory this case removes when it ends.
    struct Files(PathBuf);

    impl Drop for Files {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// The example's own settings for `posture`, pointed at this run's tier, fake and files.
    fn example(posture: &str, case: &str, catalog: &FakeServer) -> (String, Files) {
        let keycloak = tier("cube-keycloak");
        let files = Files(derived_beside(&config_path(case)));
        drop(std::fs::remove_dir_all(&files.0));
        std::fs::create_dir_all(&files.0).expect("the files directory is creatable");
        let file = |name: &str, contents: &str| {
            let path = files.0.join(name);
            std::fs::write(&path, contents).expect("an example file is writable");
            format!("\"{}\"", path.display())
        };
        let key_set = agent()
            .get(&realm(&keycloak, "certs"))
            .call()
            .expect("the realm serves its keys")
            .body_mut()
            .read_to_string()
            .expect("the key set is text");
        let deployment = token(
            &keycloak,
            &[
                ("grant_type", "client_credentials"),
                ("client_id", "sutura-deployment"),
                ("client_secret", DEPLOYMENT_SECRET),
            ],
        );
        let ca = files.0.join("cube-ca.pem");
        let front = tls_front(tier("cube-metrics"), &ca);
        let replaced = [
            (PORT, String::from("port: 0")),
            (KEY_SET_FILE, file("jwks.json", &key_set)),
            (ANCHORS, format!("\"{}\"", ca.display())),
            (DATAHUB_ENDPOINT, format!("\"{}\"", catalog.endpoint())),
            (DATAHUB_TOKEN_FILE, file("datahub-token", "datahub-token-under-test\n")),
            (METRIC_PROPERTY, format!("metric_property: \"{DEPLOYMENT_PROPERTY}\"")),
            (CUBE_ENDPOINT, format!("\"{front}\"")),
            (CUBE_TOKEN_FILE, file("cube-token", &deployment)),
            (TOKEN_ENDPOINT, format!("\"{}\"", realm(&keycloak, "token"))),
            (SECRET_FILE, file("exchange-secret", EXCHANGE_SECRET)),
        ];
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../examples/metrics-layer/conf/{posture}.yaml"));
        let declared = std::fs::read_to_string(&path).expect("the example's settings are readable");
        let undeclared: &[&str] = if posture == "impersonation-at-source" {
            &[]
        } else {
            &[TOKEN_ENDPOINT, SECRET_FILE]
        };
        let settings = replaced.iter().fold(declared.clone(), |settings, (from, to)| {
            assert!(
                declared.contains(from) || undeclared.contains(from),
                "{} no longer declares {from}",
                path.display()
            );
            settings.replace(from, to)
        });
        (format!("{settings}telemetry:\n  format: \"bunyan\"\n"), files)
    }

    /// A `DataHub` fake for one deployment's start, which loads the catalog twice.
    fn catalog() -> FakeServer {
        let mut answers = happy_path_answers();
        answers.extend(happy_path_answers());
        FakeServer::start(answers)
    }

    /// What `caller` reads for [`QUESTION`]: its rows, after the answer's own source and posture.
    fn asked(deployment: &crate::harness::Served, token: &str, posture: &str) -> serde_json::Value {
        let reply = deployment.post(&v1(sutura_http::constants::base_paths::QUERY), Some(token), QUESTION);
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "answer", "{}", reply.body);
        assert_eq!(
            body["executed_as"],
            serde_json::json!([{ "source": "warehouse", "posture": posture }]),
            "{}",
            reply.body
        );
        body["rows"].clone()
    }

    #[test]
    #[ignore = "needs `just dev-up-cube` - run `just cube-acceptance`"]
    fn each_caller_reads_only_their_own_regions_through_the_metrics_layer_example() {
        let fake = catalog();
        let (settings, _files) = example("impersonation-at-source", "metrics-layer-impersonation", &fake);
        let deployment = start_configured("metrics-layer-impersonation", &settings);
        let keycloak = tier("cube-keycloak");

        let alice = asked(&deployment, &caller(&keycloak, "alice"), "impersonation-at-source");
        let bob = asked(&deployment, &caller(&keycloak, "bob"), "impersonation-at-source");

        assert_eq!(
            alice,
            serde_json::json!([["2026-06-01", "262345"]]),
            "alice reads the north region"
        );
        assert_eq!(
            bob,
            serde_json::json!([["2026-06-01", "150000"]]),
            "bob reads the south region"
        );
        assert_ne!(alice, bob);
    }

    #[test]
    #[ignore = "needs `just dev-up-cube` - run `just cube-acceptance`"]
    fn every_caller_reads_the_deployments_answer_with_a_shared_service_user() {
        let fake = catalog();
        let (settings, _files) = example("shared-service-user", "metrics-layer-shared", &fake);
        let deployment = start_configured("metrics-layer-shared", &settings);
        let keycloak = tier("cube-keycloak");

        for name in ["alice", "bob"] {
            let rows = asked(&deployment, &caller(&keycloak, name), "shared-service-user");
            assert_eq!(
                rows,
                serde_json::json!([["2026-06-01", "412345"]]),
                "{name} reads the shared answer"
            );
        }
    }
}

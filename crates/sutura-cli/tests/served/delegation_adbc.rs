//! The delegation exchange in a SPAWNED `sutura serve` that opened the ADBC `BigQuery` driver - the
//! composed-binary sibling of `src/serve/tests/delegation_served.rs`, which is in-process and puts a
//! recording transport where this file has the driver (`telekom/sutura#1271`).
//!
//! **What is observed is sutura's half.** The identity provider is sutura's own configuration
//! (`workload_identity.delegation.token_endpoint`), so it is a loopback [`FakeServer`] here. The
//! child runs with no ambient cloud credential and no egress: every `GOOGLE_*`, `CLOUDSDK_*` and
//! `GCE_*` variable is dropped, and its outbound HTTP(S) goes to a loopback proxy that records each
//! request line and refuses it. So the cells read what the identity provider was offered, which
//! Google host the driver asked for, and what the served surface answered - and a refused exchange
//! reaches no source because the proxy saw no request at all.
//!
//! **What this does not show.** Leg 2 against a real pool: nothing reaches Google. Which token the
//! driver carried to the token service, or which account its second hop names - the proxy sees a
//! host, not a body - so a source handed the caller's own token instead of the exchanged one passes
//! here; `delegation_served.rs` and `adbc/subject.rs`'s own cells hold that. Name lookups the
//! driver makes are not observed.
//!
//! `#[ignore]`d because the venue needs the driver at `SUTURA_BIGQUERY_ADBC_DRIVER`, which no nix
//! check carries; the `e2e-datahub-adbc` CI job (locally `just e2e-datahub-adbc`) selects both.

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;

    use sutura_dev::issuer::{MockIssuer, PublishedKeySet, Token};
    use sutura_http_client::test_support::{FakeServer, Scripted};

    use crate::harness::{
        DELEGATED, LOCAL_SOURCE, LOOPBACK, Reply, Served, accepted_by, delegating_bigquery_entry, derived_catalog, example_root,
        files_source, question, settings_over, start_configured_with_environment, v1, without_the_product_family_dimension,
    };

    const BQ_SOURCE: &str = "warehouse";

    /// The one model moved onto [`BQ_SOURCE`]; its metric is single-source, so a refusal is about
    /// identity and not about a plan crossing data systems.
    const MOVED_MODEL: &str = "daily_usage.md";

    /// A caller leg 1 verifies and the source does not declare.
    const UNDECLARED: &str = "analyst-b@example.com";

    /// The inherited variables a cloud client reads an ambient credential or project from.
    const AMBIENT_CLOUD: [&str; 3] = ["GOOGLE_", "CLOUDSDK_", "GCE_"];

    /// The token service the workload-identity credential is exchanged at.
    const TOKEN_SERVICE: &str = "CONNECT sts.googleapis.com:443 ";

    /// Where a service-account key or a user credential is turned into a token - the deployment's own identity.
    const DEPLOYMENT_TOKEN_HOST: &str = "oauth2.googleapis.com";

    fn driver() -> String {
        std::env::var("SUTURA_BIGQUERY_ADBC_DRIVER")
            .ok()
            .filter(|path| !path.trim().is_empty())
            .expect("SUTURA_BIGQUERY_ADBC_DRIVER names the ADBC BigQuery driver; this cell cannot boot without it")
    }

    /// A loopback forward proxy that records the request line of every connection and refuses it.
    ///
    /// Go never proxies a loopback address, so the identity provider and the driver's subject-token
    /// source are still dialled directly.
    struct RefusingProxy {
        url: String,
        lines: Receiver<String>,
    }

    impl RefusingProxy {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
            let url = format!("http://{}", listener.local_addr().expect("a bound listener has an address"));
            let (sent, lines) = channel();
            // Detached: it blocks in `accept` for as long as the test process lives.
            drop(std::thread::spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    if sent.send(request_line(&mut stream)).is_err() {
                        return;
                    }
                    drop(stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
                }
            }));
            Self { url, lines }
        }

        /// The child's proxy variables, both spellings, every scheme.
        fn environment(&self) -> Vec<(&'static str, &str)> {
            [
                "HTTPS_PROXY",
                "https_proxy",
                "HTTP_PROXY",
                "http_proxy",
                "ALL_PROXY",
                "all_proxy",
            ]
            .into_iter()
            .map(|name| (name, self.url.as_str()))
            .collect()
        }

        /// Every request line recorded so far. Each is recorded before its refusal is written, so a
        /// line that led to an answer is here once that answer has arrived.
        fn seen(&self) -> Vec<String> {
            self.lines.try_iter().collect()
        }
    }

    fn request_line(stream: &mut TcpStream) -> String {
        drop(stream.set_read_timeout(Some(Duration::from_secs(10))));
        let mut head = Vec::new();
        let mut chunk = [0_u8; 1024];
        while !head.windows(4).any(|window| window == b"\r\n\r\n") {
            match stream.read(&mut chunk) {
                Ok(read) if read > 0 => head.extend(chunk.iter().take(read)),
                _ => break,
            }
        }
        String::from_utf8_lossy(&head).lines().next().unwrap_or_default().to_owned()
    }

    /// The client secret file the delegation block reads, removed with its directory.
    struct SecretFile(PathBuf);

    impl SecretFile {
        fn create(case: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("sutura-{case}-client-secret-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("a per-test secret directory is creatable");
            let path = dir.join("secret");
            std::fs::write(&path, "idp-client-secret\n").expect("the client secret file is writable");
            Self(path)
        }
    }

    impl Drop for SecretFile {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                drop(std::fs::remove_dir_all(dir));
            }
        }
    }

    /// What the identity provider issues for `subject`: signed by an issuer leg 1 does not trust,
    /// for the audience the delegation block asks for.
    fn exchanged(subject: &str) -> String {
        MockIssuer::generating("https://idp.example.com", "pool-client-id", "the-idp-key")
            .expect("a mock issuer generates a key pair")
            .mint(&Token::for_subject(subject))
            .expect("the identity provider signs a token")
    }

    fn issued(token: &str) -> serde_json::Value {
        serde_json::json!({
            "access_token": token,
            "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
            "token_type": "Bearer",
            "expires_in": 300,
        })
    }

    /// A started `direct` deployment over the driver, its leg-1 issuer, the identity provider it
    /// dials, and the proxy every other dial goes to.
    struct Deployed {
        served: Served,
        issuer: MockIssuer,
        idp: FakeServer,
        proxy: RefusingProxy,
        _published: PublishedKeySet,
        _secret: SecretFile,
    }

    fn deployed(case: &str, answers: Vec<Scripted>) -> Deployed {
        let issuer = MockIssuer::generating("https://issuer.example.com", "https://sutura.example.com", "the-current-key")
            .expect("a mock issuer generates a key pair");
        let published = PublishedKeySet::of(&issuer, case).expect("the key set publishes");
        let idp = FakeServer::start(answers);
        let proxy = RefusingProxy::start();
        let secret = SecretFile::create(case);
        let example = example_root();
        let data = example.join("data");
        let catalog = derived_catalog(case, &example.join("catalog"), MOVED_MODEL, BQ_SOURCE);
        without_the_product_family_dimension(&catalog);
        let security = format!(
            "  identity: \"multi-user\"\n  \
             inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
             authorization_server: \"{}\"\n    key_set_file: \"{}\"\n    algorithms: [\"ES256\"]\n",
            issuer.audience(),
            issuer.issuer(),
            published.path().display(),
        );
        let sources = format!(
            "{}    acknowledged_because: \"the example models off the moved one read fixture files as one identity\"\n{}",
            files_source(LOCAL_SOURCE, &data),
            delegating_bigquery_entry(BQ_SOURCE, &format!("{}/token", idp.endpoint()), &secret.0)
        );
        let settings = settings_over(&catalog, &data, LOOPBACK, &security, &sources);
        let served = start_configured_with_environment(case, &settings, Some(&driver()), &proxy.environment(), &AMBIENT_CLOUD);
        Deployed {
            served,
            issuer,
            idp,
            proxy,
            _published: published,
            _secret: secret,
        }
    }

    fn ask(deployed: &Deployed, subject: &str) -> (String, Reply) {
        let token = deployed.issuer.mint(&accepted_by(subject)).expect("the issuer signs a token");
        let body = question("voice-minutes-by-month", "voice_minutes", "2026-06-01", "2026-07-01");
        let reply = deployed
            .served
            .post(&v1(sutura_http::constants::base_paths::QUERY), Some(&token), &body);
        (token, reply)
    }

    /// The request bodies the identity provider answered, in order.
    fn offered(idp: FakeServer) -> Vec<String> {
        idp.finish().iter().map(|request| request.body().to_owned()).collect()
    }

    #[test]
    #[ignore = "needs the ADBC BigQuery driver at SUTURA_BIGQUERY_ADBC_DRIVER; run by `just e2e-datahub-adbc`"]
    fn a_spawned_deployment_exchanges_the_declared_callers_own_token_and_no_one_elses() {
        let (subject, _) = DELEGATED;
        let mut deployed = deployed("delegation-adbc-exchanges", vec![Scripted::ok(&issued(&exchanged(subject)))]);

        let (_, refused) = ask(&deployed, UNDECLARED);
        assert_eq!(refused.status, 403, "{}", refused.body);
        assert_eq!(refused.json()["reason"]["code"], "credential_unavailable", "{}", refused.body);

        let (token, answer) = ask(&deployed, subject);
        // Past the exchange the driver asks Google's token service, and the proxy refuses it: the
        // data system's failure, not the broker's.
        assert_eq!(answer.status, 503, "{}", answer.body);
        assert_eq!(
            answer.json()["code"],
            "unavailable",
            "the exchange must have succeeded: {}",
            answer.body
        );
        let dialled = deployed.proxy.seen();
        assert!(
            dialled.iter().any(|line| line.starts_with(TOKEN_SERVICE)),
            "the driver must have taken the workload-identity path to the token service: {dialled:?}"
        );
        assert!(
            !dialled.iter().any(|line| line.contains(DEPLOYMENT_TOKEN_HOST)),
            "the driver must not have authenticated as the deployment: {dialled:?}"
        );
        deployed.served.terminate();

        // One offer is also the proof that the undeclared caller's token reached no identity provider.
        let offered = offered(deployed.idp);
        let [offer] = offered.as_slice() else {
            panic!("one declared question is one exchange, the undeclared one none: {offered:?}");
        };
        assert!(
            offer.contains(&format!("subject_token={token}")),
            "the exchange must offer the declared caller's own verified token: {offer}"
        );
    }

    #[test]
    #[ignore = "needs the ADBC BigQuery driver at SUTURA_BIGQUERY_ADBC_DRIVER; run by `just e2e-datahub-adbc`"]
    fn a_spawned_deployments_refused_exchange_answers_identity_unavailable_and_reaches_no_source() {
        let (subject, _) = DELEGATED;
        let mut deployed = deployed(
            "delegation-adbc-refused",
            vec![Scripted::status(400, r#"{"error":"invalid_grant"}"#)],
        );

        let (token, answer) = ask(&deployed, subject);
        assert_eq!(answer.status, 503, "{}", answer.body);
        assert!(answer.body.contains(r#""code":"identity_unavailable""#), "{}", answer.body);
        assert!(
            !answer.body.contains(&token),
            "the refusal must not carry the caller's token: {}",
            answer.body
        );
        deployed.served.terminate();
        let dialled = deployed.proxy.seen();
        assert_eq!(
            dialled,
            Vec::<String>::new(),
            "a refused exchange must dial nothing past the identity provider, under any identity"
        );

        let offered = offered(deployed.idp);
        let [offer] = offered.as_slice() else {
            panic!("a refused exchange is exactly one dial: {offered:?}");
        };
        assert!(
            offer.contains(&format!("subject_token={token}")),
            "the identity provider must have been offered the caller's own verified token: {offer}"
        );
    }
}

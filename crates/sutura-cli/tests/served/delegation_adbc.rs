//! The delegation exchange in a SPAWNED `sutura serve` that opened the ADBC `BigQuery` driver - the
//! composed-binary sibling of `src/serve/tests/delegation_served.rs`, which is in-process and puts a
//! recording transport where this file has the driver (`telekom/sutura#1271`).
//!
//! **What is observed is sutura's half.** The identity provider is sutura's own configuration
//! (`workload_identity.delegation.token_endpoint`), so it is a loopback [`FakeServer`] here, and the
//! cells read what it was offered and what the served surface answered.
//!
//! **What this does not show.** Leg 2 against a real pool: the exchanged token is a mock issuer's,
//! so Google's token service refuses it or is never reached. The driver's token-service and IAM
//! calls, because those hosts are fixed by design (`sutura-exec-bigquery`'s `adbc/subject.rs`). Which
//! credential document the driver was handed - that is held by `adbc/subject.rs`'s own cells and by
//! `delegation_served.rs`, whose transport records it. And "reached no source" is read off the
//! answer: `identity_unavailable` is the broker's failure, raised before any job is built.
//!
//! `#[ignore]`d because the venue needs the driver at `SUTURA_BIGQUERY_ADBC_DRIVER`, which no nix
//! check carries; `e2e-datahub-adbc` (the `just` recipe and the CI job of that name) selects both.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_dev::issuer::{MockIssuer, PublishedKeySet, Token};
    use sutura_http_client::test_support::{FakeServer, Scripted};

    use crate::harness::{
        LOCAL_SOURCE, LOOPBACK, Reply, Served, accepted_by, derived_catalog, example_root, files_source, question, settings_over,
        start_configured_with_driver, v1, without_the_product_family_dimension,
    };

    const BQ_SOURCE: &str = "warehouse";

    /// The one model moved onto [`BQ_SOURCE`]; its metric is single-source, so a refusal is about
    /// identity and not about a plan crossing data systems.
    const MOVED_MODEL: &str = "daily_usage.md";

    /// The audience the exchanged token is asked for and carries.
    const POOL: &str = "pool-client-id";

    /// The one subject the source declares, beside its account.
    const CALLER_A: (&str, &str) = ("analyst-a@example.com", "bq-a@acme-analytics.iam.gserviceaccount.com");

    /// A caller leg 1 verifies and the source does not declare.
    const CALLER_B: &str = "analyst-b@example.com";

    fn driver() -> String {
        std::env::var("SUTURA_BIGQUERY_ADBC_DRIVER")
            .ok()
            .filter(|path| !path.trim().is_empty())
            .expect("SUTURA_BIGQUERY_ADBC_DRIVER names the ADBC BigQuery driver; this cell cannot boot without it")
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

    /// What the identity provider issues for `subject`: signed by an issuer leg 1 does not trust.
    fn exchanged(subject: &str) -> String {
        MockIssuer::generating("https://idp.example.com", POOL, "the-idp-key")
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

    /// An `impersonation-at-source` `bigquery` source declaring [`CALLER_A`] and a delegation at `endpoint`.
    fn delegating_entry(endpoint: &str, secret: &Path) -> String {
        let (subject, account) = CALLER_A;
        format!(
            "  {BQ_SOURCE}:\n    \
               kind: \"bigquery\"\n    \
               billing_project: \"acme-analytics\"\n    \
               dataset: \"warehouse\"\n    \
               credential_file: \"/nonexistent/sutura-test-bigquery.json\"\n    \
               max_bytes_billed: 1073741824\n    \
               posture: \"impersonation-at-source\"\n    \
               workload_identity:\n      \
               audience: \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n      \
               scope: \"https://www.googleapis.com/auth/bigquery.readonly\"\n      \
               impersonate:\n        \
               \"{subject}\": \"{account}\"\n      \
               delegation:\n        \
               token_endpoint: \"{endpoint}/token\"\n        \
               client_id: \"sutura\"\n        \
               client_secret_file: \"{}\"\n        \
               audience: \"{POOL}\"\n",
            secret.display(),
        )
    }

    /// A started `direct` deployment over the driver, its leg-1 issuer, and the identity provider it dials.
    struct Deployed {
        served: Served,
        issuer: MockIssuer,
        idp: FakeServer,
        _published: PublishedKeySet,
        _secret: SecretFile,
    }

    fn deployed(case: &str, answers: Vec<Scripted>) -> Deployed {
        let issuer = MockIssuer::generating("https://issuer.example.com", "https://sutura.example.com", "the-current-key")
            .expect("a mock issuer generates a key pair");
        let published = PublishedKeySet::of(&issuer, case).expect("the key set publishes");
        let idp = FakeServer::start(answers);
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
            delegating_entry(&idp.endpoint(), &secret.0)
        );
        let settings = settings_over(&catalog, &data, LOOPBACK, &security, &sources);
        Deployed {
            served: start_configured_with_driver(case, &settings, Some(&driver())),
            issuer,
            idp,
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
        let (subject, _) = CALLER_A;
        let mut deployed = deployed("delegation-adbc-exchanges", vec![Scripted::ok(&issued(&exchanged(subject)))]);

        let (other, refused) = ask(&deployed, CALLER_B);
        assert_eq!(refused.status, 403, "{}", refused.body);
        assert_eq!(refused.json()["reason"]["code"], "credential_unavailable", "{}", refused.body);

        let (token, answer) = ask(&deployed, subject);
        // Past the exchange the driver dials Google, which refuses a mock issuer's token or is not
        // reachable: the data system's failure, not the broker's. Which credential the driver held
        // is not visible from here.
        assert_eq!(answer.status, 503, "{}", answer.body);
        assert_eq!(
            answer.json()["code"],
            "unavailable",
            "the exchange must have succeeded: {}",
            answer.body
        );
        deployed.served.terminate();

        let offered = offered(deployed.idp);
        let [offer] = offered.as_slice() else {
            panic!("one declared question is one exchange, the undeclared one none: {offered:?}");
        };
        assert!(
            offer.contains(&format!("subject_token={token}")),
            "the exchange must offer the declared caller's own verified token: {offer}"
        );
        assert!(
            !offer.contains(&other),
            "the undeclared caller's token must reach no identity provider: {offer}"
        );
    }

    #[test]
    #[ignore = "needs the ADBC BigQuery driver at SUTURA_BIGQUERY_ADBC_DRIVER; run by `just e2e-datahub-adbc`"]
    fn a_spawned_deployments_refused_exchange_answers_identity_unavailable_and_reaches_no_source() {
        let (subject, _) = CALLER_A;
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

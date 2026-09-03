//! A provisioned **real** identity provider, asked whether it mints two per-subject tokens.
//!
//! # What this test IS, stated before what it is not, because the distinction is the whole point
//!
//! `crates/sutura-http` is the identity home: `crate::inbound` establishes who is asking, and the
//! mock issuer in `sutura_dev` proves every claim of that path against a key pair the test signs
//! with. `docs/adr/0008`'s two-subject property - two subjects, differently granted at the source,
//! reading different rows - is the direction that path exists to serve. What a mock issuer
//! **cannot** answer, by construction, is whether a REAL identity provider will mint a token the
//! validator accepts and that a source could map onto two grants. This is the first thing in this
//! repository that talks to a real one.
//!
//! **It proves the VENUE and not leg 2.** What it asserts is that a real `Keycloak` at the pinned
//! version, provisioned at the discovered loopback endpoint, answers the discovery surface leg 1
//! consumes and mints a non-empty access token for each of two distinct test principals - so `sub`
//! differs between them, which is `docs/adr/0008`'s two-subject ground made reachable. That is
//! deliberately ALL it claims:
//!
//!   * **No subject credential reaches a data system, and nothing is executed AS either subject.**
//!     There is no `CredentialBroker` pointed at this issuer and no source behind it. This is a
//!     venue measurement, not an impersonation proof - `AGENTS.md` keeps the shipped position that
//!     no published source executes AS the asking subject.
//!   * **No token is cryptographically verified here.** The leg-1 verifier is tested against the
//!     mock issuer; here the claim is that a real issuer will hand back a token and sign it, which
//!     the `jwks_uri` fetch backs up. Reading this as evidence leg 1 validates a real signature
//!     would be the overstatement `AGENTS.md` calls the defect itself.
//!
//! # The discovery contract is the only addressing there is
//!
//! `sutura_dev::provisioned::here` is the one decision, shared with every other harness, and the
//! endpoint it hands back is read from `.sutura-dev/endpoints.json` - the file
//! `sutura-keycloak-tier start` writes. There is no host or port constant in this file to fall
//! back to, deliberately, for the reason every other provisioned leg gives: a fallback connects to
//! whatever else holds that port.
//!
//! # Fail-closed where a tier was provisioned, loudly skipped where one was not
//!
//! `sutura_dev::provisioned::here` applies the requirement: a job that set
//! `SUTURA_DEV_REQUIRE_TIER` (as `nix run .#keycloak-acceptance` does) gets a panic on an absent
//! tier, and a developer machine gets a notice on stderr naming what did not run. Nothing here
//! skips silently.
//!
//! # What this is NOT
//!
//! **Not evidence in the default suite.** This is `#[ignore]`d, so `just test` does not run it, and
//! it may not be cited as though it were - what it is evidence of is whatever the last run of
//! `just keycloak-acceptance` (through `nix run .#keycloak-acceptance`) reported.

// `cfg(test)` because clippy only honours `allow-expect-in-tests` and `allow-panic-in-tests` for
// code inside a `#[cfg(test)]` item, and `tests_outside_test_module` wants the `#[test]` function
// inside one - the same reason `crates/sutura-catalog-datahub/tests/provisioned.rs` is shaped this
// way. An integration test target is only built for tests, so the attribute changes nothing about
// what compiles.
#[cfg(test)]
mod tests {
    use std::time::Duration;

    /// How long the issuer gets to answer. Generous: `sutura-keycloak-tier start` has already gated
    /// on its own health check, so a request slower than this is a wedged JVM rather than a cold
    /// one, and a short timeout would turn that into a flake instead of a failure.
    const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

    /// The issuer sits behind a confidential client with a fixed secret and two password-grant
    /// users. These are loopback fixtures, not secrets - see `nix/keycloak-tier.nix` for the
    /// reasoning that they cannot be one.
    const CLIENT_ID: &str = "test-client";
    const CLIENT_SECRET: &str = "sutura-client-secret";
    const ALICE: (&str, &str) = ("alice", "alice-pass");
    const BOB: (&str, &str) = ("bob", "bob-pass");

    /// A probe client, built the way `sutura_catalog_datahub`'s provisioned leg builds one, minus
    /// any redirect: this is loopback plaintext by construction because the tier publishes an
    /// ephemeral HTTP port.
    fn probe_agent() -> ureq::Agent {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(ANSWER_TIMEOUT))
                // A redirect is how a probe silently starts measuring a different server.
                .max_redirects(0)
                .build(),
        )
    }

    /// Read a JSON value out of a `ureq` response, an HTTP message we do not trust further than the
    /// assertion about to parse it.
    fn json_from(agent: &ureq::Agent, url: &str) -> serde_json::Value {
        let response = agent
            .get(url)
            .call()
            .unwrap_or_else(|cause| panic!("the provisioned issuer did not answer `{url}`: {cause}"));
        assert!(
            (200..300).contains(&response.status().as_u16()),
            "the provisioned issuer answered `{url}` with {}, not a 2xx",
            response.status()
        );
        response
            .into_body()
            .read_to_string()
            .expect("a discovery document is small readable JSON")
            .parse::<serde_json::Value>()
            .expect("the issuer returned JSON")
    }

    /// A token minted by a real issuer for one subject, parsed far enough to read `sub`.
    ///
    /// Only the identity token is decoded here - it is the one that carries `sub` without a second
    /// round trip - and it is decoded from the wire, not verified: verification belongs to the
    /// leg-1 path and to `docs/adr/0014`'s surface, and this leg's claim stops at "a real issuer
    /// signed it and named a subject".
    fn id_token_sub(token: &serde_json::Value) -> String {
        let id_token = token
            .get("id_token")
            .and_then(serde_json::Value::as_str)
            .expect("a successful token response carries an identity token");
        let claims = id_token.split('.').nth(1).expect("a JWT has three dot sections");
        let claims = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, claims)
            .expect("the identity token's payload is base64url");
        let claims: serde_json::Value = serde_json::from_slice(&claims).expect("the identity token's payload is JSON");
        claims
            .get("sub")
            .and_then(serde_json::Value::as_str)
            .expect("a signed identity token names a subject")
            .to_owned()
    }

    #[test]
    #[ignore = "needs `just keycloak-acceptance` - a provisioned Keycloak at the discovered \
                endpoint; `just test` does not boot a JVM"]
    fn a_provisioned_keycloak_serves_leg_1s_surface_and_mints_two_per_subject_tokens() {
        let inside = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let provisioned = sutura_dev::provisioned::here(inside, "keycloak");
        let Some(endpoint) = provisioned.endpoint() else {
            // The notice is already on stderr, and in the required direction `here` panicked rather
            // than reaching this line.
            return;
        };
        let agent = probe_agent();

        // 1. The discovery document leg 1 consumes, and the `jwks_uri` a verifier reads off it.
        let discovery = format!("http://{endpoint}/realms/master/.well-known/openid-configuration");
        let openid = json_from(&agent, &discovery);
        let jwks_uri = openid
            .get("jwks_uri")
            .and_then(serde_json::Value::as_str)
            .expect("the openid-configuration names a jwks_uri");

        // 2. The issuer really signs: its published key set carries at least one RSA key.
        let keyset = json_from(&agent, jwks_uri);
        let rsa_keys = keyset
            .get("keys")
            .and_then(serde_json::Value::as_array)
            .expect("a JWKS is a key array")
            .iter()
            .filter(|key| key.get("kty").and_then(serde_json::Value::as_str) == Some("RSA"))
            .count();
        assert!(rsa_keys >= 1, "the provisioned issuer published no RSA signing key");

        // 3. Both principals mint a token through the confidential client's password grant, and the
        //    two are DIFFERENT subjects - `docs/adr/0008`'s two-subject ground made reachable.
        let token_url = format!("http://{endpoint}/realms/master/protocol/openid-connect/token");
        let mint = |(username, password): (&str, &str)| {
            let response = agent
                .post(&token_url)
                .send_form([
                    ("grant_type", "password"),
                    ("client_id", CLIENT_ID),
                    ("client_secret", CLIENT_SECRET),
                    ("username", username),
                    ("password", password),
                ])
                .unwrap_or_else(|cause| panic!("the provisioned issuer refused a password grant for `{username}`: {cause}"));
            assert!(
                (200..300).contains(&response.status().as_u16()),
                "the provisioned issuer answered the password grant for `{username}` with {}, not a 2xx",
                response.status()
            );
            let token: serde_json::Value = response
                .into_body()
                .read_to_string()
                .expect("a token response is small readable JSON")
                .parse()
                .expect("the issuer returned JSON");
            assert!(
                token
                    .get("access_token")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|it| !it.is_empty()),
                "the provisioned issuer minted no access_token for `{username}`"
            );
            id_token_sub(&token)
        };

        let alice_sub = mint(ALICE);
        let bob_sub = mint(BOB);
        assert_ne!(
            alice_sub, bob_sub,
            "a real issuer collapsed two principals into one subject - \
             docs/adr/0008's two-subject ground needs two distinct subjects"
        );
    }
}

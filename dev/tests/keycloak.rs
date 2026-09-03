//! The nix-native Keycloak tier, asked the one question only a real provider can answer.
//!
//! `nix/keycloak-tier.nix` brings up nixpkgs' `keycloak` over loopback and provisions a realm, two
//! clients, an audience mapper and two subjects with no human at a keyboard. This file is what makes
//! that tier worth having: a tier nobody asks a question of is a server started on every run to
//! prove nothing, which is the failure mode #117 was filed about.
//!
//! # The question
//!
//! `docs/where-identity-is-proven.md` has one row that only a real provider can answer - **whether
//! a real identity provider will mint an ID token whose `aud` is a THIRD PARTY's client id** - and
//! it says why the obvious venue cannot: in `sutura_dev::issuer` the audience is a parameter, so a
//! mock answers *yes* by construction, which that page calls worse than no test.
//!
//! It matters because `docs/adr/0008` records, verified against the vendor's documentation, that a
//! workforce-identity token exchange requires the caller's ID token to carry the PROVIDER's
//! configured client id in `aud` - not the `audience` value sent to the exchange endpoint. So the
//! shape of the token a gateway would have to obtain is decided by the identity provider, and this
//! is the only venue in the repository that can be asked.
//!
//! # What a green run here does NOT establish
//!
//! Written here rather than only in the nix module, because this file is what a reader will cite.
//!
//! * **Not that any particular deployment will do it.** Keycloak is a real OIDC provider and it is
//!   not the enterprise provider somebody will federate with. What is shown is that the mapper
//!   exists, is configurable with no human, and produces the token - never that an organisation's
//!   policy permits it.
//! * **Nothing about leg 2.** `AGENTS.md` keeps the position that no source a deployment serves
//!   executes as the asking subject. Two real subjects holding two real tokens do not move that:
//!   what is missing is an adapter that can carry a per-subject credential.
//! * **Not `docs/adr/0008`'s two-subject test**, which asserts that two subjects READ TWO DIFFERENT
//!   ROW SETS. This supplies two subjects whose tokens a validator would accept. That is the
//!   prerequisite, and the compose block for the same service says what the rest needs: a data
//!   system validating a token it trusts, which is not in this venue at all.
//! * **Nothing about signature verification, key rotation or a forged token.** Those have a venue
//!   already - the mock issuer, which can script a rotation and a forgery and this cannot - and
//!   `docs/where-identity-is-proven.md` marks them *redundant* here on purpose. Asking them twice
//!   would spend the tier's 19s per run to re-establish what a fake already holds.
//!
//! **Bring the tier up with `just keycloak-tier-up` before expecting these to assert anything**, or
//! run `just test`, which brings up every nix tier through `nix/with-tier.sh`.

// `cfg(test)` because clippy only honours `allow-expect-in-tests` for code inside a `#[cfg(test)]`
// item, and an integration test target is compiled with `--test` so it is true here. Without it
// every `expect` below is a lint error. Same reason as `dev/tests/provisioned.rs`.
#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{TcpStream, ToSocketAddrs as _};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use base64::Engine as _;
    use sutura_dev::discovery::Endpoint;
    use sutura_dev::provisioned::{self, Provisioned};

    /// How long the provider gets to answer a request on an endpoint already reported up.
    ///
    /// `start` published the endpoint only after the realm's own discovery document came back, so a
    /// request that cannot be answered in this window is not a slow server - it is the wrong one.
    /// A JVM's startup is paid by the tier script and is not in this budget.
    const TIMEOUT: Duration = Duration::from_secs(15);

    /// The directory this test crate lives in - the one thing a test knows about where it is.
    fn inside() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    /// What the tier provisioned, as the tier itself recorded it.
    ///
    /// A second file beside `endpoints.json`, and the split is deliberate: an ADDRESS has exactly one
    /// door (`sutura_dev::provisioned`) because a memorised port would connect to a neighbouring
    /// worktree's fixture, and `Endpoint` has private fields so a constant does not compile. A realm
    /// name cannot do that. What reading it removes is the other drift - a test spelling a client id
    /// the tier script no longer creates.
    struct Fixture {
        realm: serde_json::Value,
    }

    impl Fixture {
        /// Read it, or `None` where the tier has not written one.
        fn read(root: &Path) -> Option<Self> {
            let text = std::fs::read_to_string(Self::path(root)).ok()?;
            let realm = serde_json::from_str(&text).ok()?;
            Some(Self { realm })
        }

        fn path(root: &Path) -> PathBuf {
            root.join(".sutura-dev").join("keycloak.json")
        }

        /// One string field, which the tier writes and this never defaults.
        fn field(&self, name: &str) -> &str {
            self.realm
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("the tier's fixture file has no `{name}`: {}", self.realm))
        }

        /// The subjects the tier created, in the order it created them.
        fn subjects(&self) -> Vec<&str> {
            self.realm
                .get("subjects")
                .and_then(serde_json::Value::as_array)
                .map(|values| values.iter().filter_map(serde_json::Value::as_str).collect())
                .unwrap_or_default()
        }
    }

    /// The provisioned provider and what it holds, or `None` where the tier is not up.
    ///
    /// The skip-or-fail direction is `sutura_dev::requirement`'s and not this file's:
    /// `SUTURA_DEV_REQUIRE_TIER` is set by whatever provisioned the tier - `checks.nextest` and
    /// `nix/with-tier.sh` both do - so an absent tier is a RED run there and a loud skip on a
    /// machine that started nothing.
    fn provider() -> Option<(Endpoint, Fixture)> {
        let root = provisioned::worktree_root(inside())?;
        let endpoint = match provisioned::here(inside(), "keycloak") {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(_) => return None,
        };
        let fixture = Fixture::read(&root).unwrap_or_else(|| {
            panic!(
                "`keycloak` is provisioned at {endpoint} and {} is not readable as the tier's \
                 fixture record - the tier writes both or neither, so this is the docker tier \
                 answering for a service the nix tier is the CI venue for",
                Fixture::path(&root).display()
            )
        });
        Some((endpoint, fixture))
    }

    #[test]
    fn the_provisioned_realm_answers_for_itself_on_the_discovered_endpoint() {
        // The tier came up and a caller reached it, asserted the only way that means anything: the
        // document that comes back names the realm the tier created as its OWN issuer. A liveness
        // probe on the port would pass against any Keycloak on this host - including the compose
        // one, which is a different venue for the same service name - and against anything else
        // that happens to hold the port.
        let Some((endpoint, fixture)) = provider() else {
            return;
        };

        // Not the container port either. The compose tier publishes 8080 ephemerally and this tier
        // derives a port of its own, so an equal value would mean the assertion below was about
        // whichever Keycloak a hardcoded constant found.
        assert_ne!(
            endpoint.port(),
            8080,
            "the endpoint is the container's own port, so nothing here can tell the two venues apart"
        );

        let document = get(
            &endpoint,
            &format!("/realms/{}/.well-known/openid-configuration", fixture.field("realm")),
        )
        .expect("the tier published this endpoint only after this document came back");
        let parsed: serde_json::Value = serde_json::from_str(&document).expect("a discovery document is JSON");
        assert_eq!(
            parsed.get("issuer").and_then(serde_json::Value::as_str),
            Some(fixture.field("issuer")),
            "the server on the discovered port is not the realm this tier provisioned: {parsed}"
        );
    }

    #[test]
    fn a_real_provider_mints_an_id_token_whose_audience_is_a_third_partys_client_id() {
        // THE ASSERTION THIS VENUE EXISTS FOR, and the one `docs/where-identity-is-proven.md` marks
        // as answerable only here. A subject authenticates to the GATEWAY client, and the ID token
        // that comes back is audienced at a SECOND client - one that has no flow enabled, that
        // nothing ever authenticates to, and that did not take part in this exchange. That is the
        // token shape `docs/adr/0008` says a workforce-identity exchange requires, and the mock
        // issuer answers it `yes` by construction, which is why the question is asked here.
        let Some((endpoint, fixture)) = provider() else {
            return;
        };
        let gateway = fixture.field("gateway_client");
        let third_party = fixture.field("third_party_client");
        assert_ne!(
            gateway, third_party,
            "the fixture has to name two different clients or `aud` proves nothing"
        );

        let subject = *fixture.subjects().first().expect("the tier provisions two subjects");
        let token = grant(&endpoint, &fixture, subject);
        let identity = claims(&token, "id_token");

        assert!(
            audience(&identity).iter().any(|entry| entry == third_party),
            "the ID token is not audienced at `{third_party}`: {identity}"
        );
        // `azp` is who ASKED. Both halves matter: an `aud` naming the third party while the
        // authorized party is also the third party would just be a token issued to it.
        assert_eq!(
            identity.get("azp").and_then(serde_json::Value::as_str),
            Some(gateway),
            "the authorized party is not the gateway, so this token was not obtained the way the \
             claim describes: {identity}"
        );
        assert_eq!(
            identity.get("iss").and_then(serde_json::Value::as_str),
            Some(fixture.field("issuer")),
            "the token did not come from the realm this tier provisioned: {identity}"
        );

        // The half that shows the audience is the MAPPER's doing rather than a property of the
        // client: the same exchange's ACCESS token carries no such audience, because the mapper
        // declares `id.token.claim` and not `access.token.claim`. Without this, a reader cannot tell
        // a configured claim from something Keycloak does to every token it issues.
        let access = claims(&token, "access_token");
        assert!(
            !audience(&access).iter().any(|entry| *entry == *third_party),
            "the access token is audienced at `{third_party}` too, so the assertion above is not \
             about the audience mapper: {access}"
        );
    }

    #[test]
    fn two_subjects_get_two_identities_from_the_provider() {
        // The prerequisite `docs/adr/0008`'s two-subject test needs, and NOT that test: what is
        // shown is that the tier provisions two subjects a validator can tell apart, each with a
        // real signature over a real claim set. Reading two different ROW SETS needs an adapter that
        // can carry a per-subject credential, and the module header says so.
        //
        // `sub` and not the username: a username is what was posted, so comparing those two would
        // compare this test's own inputs. `sub` is the provider's own opaque identifier for the
        // account, which is what a downstream check would key on.
        let Some((endpoint, fixture)) = provider() else {
            return;
        };
        let subjects = fixture.subjects();
        assert!(
            subjects.len() >= 2,
            "the tier provisions two subjects; it declared {subjects:?}"
        );

        let mut identifiers: Vec<String> = Vec::new();
        for subject in &subjects {
            let claims = claims(&grant(&endpoint, &fixture, subject), "id_token");
            let identifier = claims
                .get("sub")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("an ID token with no `sub`: {claims}"))
                .to_owned();
            assert_eq!(
                claims.get("preferred_username").and_then(serde_json::Value::as_str),
                Some(*subject),
                "the provider answered for a different account than the one that authenticated: {claims}"
            );
            identifiers.push(identifier);
        }
        identifiers.sort();
        let distinct = identifiers.len();
        identifiers.dedup();
        assert_eq!(
            identifiers.len(),
            distinct,
            "two subjects share one `sub`, so nothing downstream could tell them apart"
        );
    }

    /// The `aud` claim, in both the forms RFC 7519 permits.
    ///
    /// A single string and an array are both legal, and Keycloak emits whichever it needs - one
    /// audience is a bare string. A helper that read only the array would pass the interesting case
    /// and fail the boring one.
    fn audience(claims: &serde_json::Value) -> Vec<String> {
        match claims.get("aud") {
            Some(serde_json::Value::String(one)) => vec![one.clone()],
            Some(serde_json::Value::Array(many)) => {
                many.iter().filter_map(serde_json::Value::as_str).map(str::to_owned).collect()
            }
            _ => Vec::new(),
        }
    }

    /// One token's claim set, decoded from the JWS compact form.
    ///
    /// **It does not verify the signature, and that is deliberate rather than a shortcut.** This
    /// venue is asked about the CONTENTS of a token a real provider issued; whether a signature
    /// verifies, whether a forged one is refused and whether a rotation is noticed all have a venue
    /// already - the mock issuer, over the code that actually does the verifying - and
    /// `docs/where-identity-is-proven.md` marks them redundant here. Decoding without verifying in a
    /// test that is about the claim set is honest; doing it in the verifier would not be.
    fn claims(token: &serde_json::Value, which: &str) -> serde_json::Value {
        let compact = token
            .get(which)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("the token response has no `{which}`: {token}"));
        let payload = compact
            .split('.')
            .nth(1)
            .unwrap_or_else(|| panic!("`{which}` is not a JWS compact serialization"));
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .unwrap_or_else(|e| panic!("`{which}`'s payload is not base64url: {e}"));
        serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("`{which}`'s payload is not JSON: {e}"))
    }

    /// One direct access grant, as the subject.
    ///
    /// The direct grant and not a browser redirect, because the whole point of the tier is that no
    /// human is needed - and `openid` in the scope, because without it there is no ID token to ask
    /// anything of.
    fn grant(endpoint: &Endpoint, fixture: &Fixture, subject: &str) -> serde_json::Value {
        let form = [
            ("client_id", fixture.field("gateway_client")),
            ("grant_type", "password"),
            ("scope", "openid"),
            ("username", subject),
            ("password", fixture.field("subject_password")),
        ]
        .iter()
        .map(|&(key, value)| format!("{key}={}", encoded(value)))
        .collect::<Vec<String>>()
        .join("&");

        let path = format!("/realms/{}/protocol/openid-connect/token", fixture.field("realm"));
        let body = post_form(endpoint, &path, &form)
            .unwrap_or_else(|e| panic!("the token endpoint at {endpoint} did not answer for {subject}: {e}"));
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("the token endpoint answered {body:?}, which is not JSON: {e}"))
    }

    /// Percent-encode one form value.
    ///
    /// Nine lines rather than a dependency: this crate's runtime dependencies are `serde_json` and
    /// `sha2`, and a URL crate in a dev harness for two form fields is supply-chain surface for
    /// something with one caller. The unreserved set is RFC 3986's.
    fn encoded(value: &str) -> String {
        use std::fmt::Write as _;

        let mut out = String::with_capacity(value.len());
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                out.push(char::from(byte));
            } else if write!(out, "%{byte:02X}").is_err() {
                // Writing to a `String` cannot fail; the branch exists because the trait's
                // signature says it can, and a swallowed `Err` would silently produce a form value
                // that is not the credential.
                panic!("a String rejected two hex digits");
            }
        }
        out
    }

    /// Open a connection to a discovered endpoint.
    fn connect(endpoint: &Endpoint) -> std::io::Result<TcpStream> {
        let address = format!("{}:{}", endpoint.host(), endpoint.port());
        let resolved = address
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::other(format!("{address} resolved to nothing")))?;
        let stream = TcpStream::connect_timeout(&resolved, TIMEOUT)?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        Ok(stream)
    }

    /// One HTTP/1.0 request by hand, returning the body and refusing a status that is not `200`.
    ///
    /// Written with a `TcpStream` for the reason `dev/tests/provisioned.rs` gives for the same
    /// helper: an HTTP client in a dev harness is a dependency for something with one caller. 1.0
    /// rather than 1.1 so there is no keep-alive and no chunked framing to reassemble - the
    /// connection closing IS the end of the body.
    fn request(endpoint: &Endpoint, head: &str, body: &str) -> std::io::Result<String> {
        let mut stream = connect(endpoint)?;
        stream.write_all(head.as_bytes())?;
        stream.write_all(body.as_bytes())?;
        stream.flush()?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response)?;
        let response = String::from_utf8_lossy(&response).into_owned();
        let (status, payload) = response
            .split_once("\r\n\r\n")
            .ok_or_else(|| std::io::Error::other(format!("no header/body boundary in {response:?}")))?;
        let first = status.lines().next().unwrap_or_default();
        if !first.contains(" 200") {
            return Err(std::io::Error::other(format!("{first} - {payload}")));
        }
        Ok(payload.to_owned())
    }

    /// One `GET`.
    fn get(endpoint: &Endpoint, path: &str) -> std::io::Result<String> {
        let head = format!(
            "GET {path} HTTP/1.0\r\nHost: {}:{}\r\nAccept: application/json\r\n\r\n",
            endpoint.host(),
            endpoint.port()
        );
        request(endpoint, &head, "")
    }

    /// One `POST` of a form-encoded body.
    fn post_form(endpoint: &Endpoint, path: &str, form: &str) -> std::io::Result<String> {
        let head = format!(
            "POST {path} HTTP/1.0\r\nHost: {}:{}\r\nContent-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\nAccept: application/json\r\n\r\n",
            endpoint.host(),
            endpoint.port(),
            form.len()
        );
        request(endpoint, &head, form)
    }
}

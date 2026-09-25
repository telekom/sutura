//! The served Keycloak fixture: mint real tokens from the nix-native tier's own realm and read back
//! whatever documents and audience it actually publishes, rather than assuming either.
//!
//! **A real client for a real server, not the hand-rolled request `served/harness.rs` writes for its
//! own spawned binary.** That module's `Reply`/`parse`/`Served::send` are a deliberate choice - one
//! plaintext loopback request of a FIXED shape, no dependency - and the reasons stay true for the
//! request it was written for. This module is not that request: it talks to a real Quarkus/Keycloak
//! server this suite did not write, whose responses this fixture has no business assuming the shape
//! of - chunked or not, redirected or not. `ureq` is already a workspace dependency and handles
//! this request without a new HTTP stack.
//!
//! **Reached only by its own `just` task, never by `just test`.** Two reasons, both load-bearing.
//! First, cost: `nix/keycloak-tier.nix`'s own header says a JVM issuer is not something every
//! `cargo nextest run` should pay to boot. Second: `sutura_dev::provisioned::here` reads ONE
//! environment variable (`SUTURA_DEV_REQUIRE_TIER`) for every tier a caller asks about, and
//! `nix/with-tier.sh` already exports it as `1` whenever the Postgres tier comes up - which is every
//! `just test` on a machine with that tier on PATH. A cell that called `here` for `"keycloak"` would
//! inherit that flag from a tier it has nothing to do with: `Requirement::Required` with no Keycloak
//! ever started, which panics rather than skips. So this fixture calls the narrower
//! `sutura_dev::provisioned::in_worktree` directly and panics on its own diagnostic instead - the
//! same fail-loud posture `crates/sutura-exec-bigquery/tests/acceptance.rs` takes for a missing
//! credential, and correct here for the same reason: this fixture is reached only by a caller that
//! asked for it by name, never by accident.
//!
//! **The audience is READ, never asserted.** `security.inbound.resource` has to equal one entry of
//! whatever `aud` this realm's default client scopes put in a token, and nothing here may assume
//! what that is. It is not a THIRD PARTY's client id in the sense issue #105 asks about: this tier
//! provisions one client and a password grant, nothing resembling #105's delegation flow.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;

use super::{config_path, derived_beside, example_root, settings_crediting};

/// The largest body this fixture reads back from the tier, at either endpoint. Two orders of
/// magnitude above a token response or a JWK set holding a handful of keys - bounding an
/// implementor's own read, the way `sutura_http`'s `FileKeySet` bounds its.
const MAX_ANSWER_BYTES: u64 = 64 * 1024;

/// The tier's own realm document - never a literal, because every value in it is generated fresh at
/// `start` (`nix/keycloak-tier.nix`'s `realmFile`) and would otherwise be a stale fixture the moment
/// the tier restarts.
struct Realm {
    /// `<base>/realms/<realm>` - already the full issuer URL the tier's own token endpoint and JWKS
    /// hang off, per its own `provision()`. `https://`: `nix/keycloak-tier.nix`'s own comment on
    /// `resourceAudience` says why a plaintext issuer was refused outright rather than accepted.
    issuer: String,
    client_id: String,
    client_secret: String,
    /// `(username, password)`, in the order `nix/keycloak-tier.nix`'s `subjects` list declares them.
    subjects: Vec<(String, String)>,
    /// The throwaway CA `start` minted for `issuer`'s own `https://` listener - NOT the leaf
    /// `kc.sh` actually serves, which `nix/keycloak-tier.nix`'s own comment names as unusable
    /// directly (`CaUsedAsEndEntity`). Not a secret, but this fixture's agent has to load it
    /// explicitly: no public CA signed the leaf, and a default TLS config trusting only public
    /// roots would refuse it the same way it would refuse a network attacker's.
    tls_certificate_file: PathBuf,
    /// The third-party audience the tier's `id-token-audience` mapper puts into the ID token's `aud`
    /// when a password grant requests `scope=openid`. Read from the realm file rather than restated,
    /// so the value the tier provisions and the value a cell asserts against are one document.
    id_token_audience: String,
}

/// Reads `.sutura-dev/keycloak-realm.json` at the worktree root - the file `stop` removes and every
/// `start` regenerates, `0600`, never committed.
fn read_realm(root: &Path) -> Realm {
    let path = root.join(".sutura-dev/keycloak-realm.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|cause| {
        panic!(
            "{} is the keycloak tier's own realm file and it is not readable: {cause}",
            path.display()
        )
    });
    let value: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|cause| panic!("{} is not valid JSON: {cause}", path.display()));
    let issuer = value
        .get("issuer")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{} carries no `issuer`", path.display()));
    let client = value
        .get("client")
        .unwrap_or_else(|| panic!("{} carries no `client`", path.display()));
    let client_id = client
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{} carries no `client.id`", path.display()));
    let client_secret = client
        .get("secret")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{} carries no `client.secret`", path.display()));
    let tls_certificate_file = value
        .get("tls_certificate_file")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{} carries no `tls_certificate_file`", path.display()));
    let id_token_audience = value
        .get("id_token_audience")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{} carries no `id_token_audience`", path.display()));
    let subjects = value
        .get("subjects")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{} carries no `subjects`", path.display()));
    assert!(
        subjects.len() >= 2,
        "{} carries fewer than two subjects - docs/adr/0008's property needs two: {subjects:?}",
        path.display()
    );
    let read_subject = |entry: &serde_json::Value| {
        let username = entry
            .get("username")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("{} names a subject with no `username`", path.display()));
        let password = entry
            .get("password")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("{} names a subject with no `password`", path.display()));
        (String::from(username), String::from(password))
    };
    Realm {
        issuer: String::from(issuer),
        client_id: String::from(client_id),
        client_secret: String::from(client_secret),
        subjects: subjects.iter().map(read_subject).collect(),
        tls_certificate_file: PathBuf::from(tls_certificate_file),
        id_token_audience: String::from(id_token_audience),
    }
}

/// A short-lived agent for the handful of requests one case makes, trusting ONLY the tier's own
/// CA - not the platform or Mozilla's roots, and not "disable verification entirely". Loopback
/// does not make the `https://` issuer decorative: `nix/keycloak-tier.nix`'s realm file names the
/// exact CA it minted for this exact server's leaf certificate, and loading precisely that one is
/// what keeps this agent refusing anything else presenting itself as this issuer, the same
/// property a real deployment gets from a real CA. Bounded by `START_BUDGET`-scale timeouts, a
/// test harness's own clock rather than a job deadline.
fn agent(tls_certificate_file: &Path) -> ureq::Agent {
    let pem = std::fs::read(tls_certificate_file).unwrap_or_else(|cause| {
        panic!(
            "{} is the keycloak tier's own CA certificate and it is not readable: {cause}",
            tls_certificate_file.display()
        )
    });
    let cert = ureq::tls::Certificate::from_pem(&pem)
        .unwrap_or_else(|cause| panic!("{} is not a PEM certificate: {cause}", tls_certificate_file.display()));
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::new_with_certs(&[cert]))
                    .build(),
            )
            .build(),
    )
}

/// A password grant against the tier's own confidential client - the shared POST for the access
/// token [`mint`] and the ID token [`mint_id_token`] both build on, differing only in `scope` and
/// which response field they read back.
fn grant(
    agent: &ureq::Agent,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    username: &str,
    password: &str,
    scope: Option<&str>,
) -> serde_json::Value {
    let mut form = vec![
        ("grant_type", "password"),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("username", username),
        ("password", password),
    ];
    if let Some(value) = scope {
        form.push(("scope", value));
    }
    let mut answer = agent
        .post(token_url)
        .send_form(form.iter().copied())
        .unwrap_or_else(|cause| panic!("the tier's own token endpoint at {token_url} is unreachable: {cause}"));
    let status = answer.status();
    let body = answer
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER_BYTES)
        .read_to_string()
        .unwrap_or_else(|cause| panic!("the token response is not readable: {cause}"));
    assert!(
        status.is_success(),
        "the tier's own token endpoint refused a subject it provisioned ({status}): {body}"
    );
    serde_json::from_str(&body).unwrap_or_else(|cause| panic!("the token response is not JSON ({cause}): {body}"))
}

/// A password-grant access token for one provisioned subject, against the tier's own confidential
/// client - the token the served cell's bearer gate verifies.
fn mint(agent: &ureq::Agent, token_url: &str, client_id: &str, client_secret: &str, username: &str, password: &str) -> String {
    let value = grant(agent, token_url, client_id, client_secret, username, password, None);
    String::from(
        value
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("the token response carries no `access_token`: {value}")),
    )
}

/// A password-grant ID token for one provisioned subject, obtained by requesting `scope=openid` -
/// the token whose `aud` carries the tier's `id-token-audience` mapper value, which is the whole
/// of issue #105's step-1 probe.
fn mint_id_token(
    agent: &ureq::Agent,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    username: &str,
    password: &str,
) -> String {
    let value = grant(agent, token_url, client_id, client_secret, username, password, Some("openid"));
    String::from(
        value
            .get("id_token")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("the token response carries no `id_token`: {value}")),
    )
}

/// The realm's own published key set, fetched rather than assumed to match `FileKeySet`'s parser.
fn fetch_key_set(agent: &ureq::Agent, jwks_url: &str) -> String {
    let mut answer = agent
        .get(jwks_url)
        .call()
        .unwrap_or_else(|cause| panic!("the tier's own JWKS endpoint at {jwks_url} is unreachable: {cause}"));
    let status = answer.status();
    let body = answer
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER_BYTES)
        .read_to_string()
        .unwrap_or_else(|cause| panic!("the key set response is not readable: {cause}"));
    assert!(status.is_success(), "the tier's own JWKS endpoint answered {status}: {body}");
    body
}

/// Decodes a compact JWT's payload segment to its claim set - the shared step both [`audience_of`]
/// and [`subject_of`] build on, so the two read the same minted document the same way.
fn payload_claims(token: &str) -> serde_json::Value {
    let payload = token
        .split('.')
        .nth(1)
        .unwrap_or_else(|| panic!("{token} is not shaped like a compact JWT"));
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .unwrap_or_else(|cause| panic!("the token's payload segment is not base64url: {cause}"));
    serde_json::from_slice(&bytes).unwrap_or_else(|cause| panic!("the token's payload segment is not JSON: {cause}"))
}

/// The raw `sub` claim of one of this fixture's own minted tokens - decoded from the payload this
/// fixture itself minted, the same source [`audience_of`] reads. The served cell asserts on this
/// full value so its "two provisioned subjects" property does not rest on the first hex character
/// of their UUIDs: the audit record masks each `sub` to its first character plus `***`, so two
/// subjects' masked forms collide 1 in 16 even for two correct, distinct users.
pub(crate) fn subject_of(token: &str) -> String {
    let claims = payload_claims(token);
    String::from(
        claims
            .get("sub")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("the token's payload carries no `sub`: {claims}")),
    )
}

/// The `aud` a minted token actually carries - a string if it is one, or the one ABSOLUTE
/// `https://` entry if it is an array, and a panic if there is neither.
///
/// Shared by the access-token cell (which reads the resource audience) and the ID-token cell
/// (which reads the third-party audience), so both decode the same minted document the same way.
///
/// **Not the array's first entry**, and that is measured rather than a style choice:
/// `nix/keycloak-tier.nix`'s client carries a hardcoded audience mapper naming
/// `resourceAudience`, but Keycloak's built-in "account" client scope adds its own `account`
/// audience to every token regardless, so a real mint comes back `["https://sutura-dev-
/// cli.example.com", "account"]` - two entries, and only one is shaped like the absolute URI
/// `security.inbound.resource` requires. Picking "first" happened to pass here because of that
/// order, which is Keycloak's own mapper-application order and not a contract this fixture may
/// rely on; selecting the `https://`-shaped entry is what actually makes the choice non-accidental.
pub(crate) fn audience_of(token: &str) -> String {
    let claims = payload_claims(token);
    match claims.get("aud") {
        Some(serde_json::Value::String(single)) => single.clone(),
        Some(serde_json::Value::Array(many)) => String::from(
            many.iter()
                .filter_map(serde_json::Value::as_str)
                .find(|candidate| candidate.starts_with("https://"))
                .unwrap_or_else(|| panic!("the token's `aud` array has no absolute `https://` entry: {claims}")),
        ),
        _ => panic!("the token carries no usable `aud`: {claims}"),
    }
}

/// Everything the served leg-1-over-a-real-issuer cell needs, minted once per case.
///
/// `pub(crate)` on every field, not only `resource` and the two tokens: the wave-one E2E lane
/// (a sibling cell outside this module) needs the realm's own issuer and published key set to mint
/// its own deployments against the same tier, the same way `settings_naming` below already builds
/// one `security.inbound` block from them.
pub(crate) struct KeycloakFixture {
    pub(crate) issuer: String,
    /// The audience this realm actually put in [`Self::subject_a_token`] - READ off the token, per
    /// this module's own header.
    pub(crate) resource: String,
    pub(crate) key_set_file: PathBuf,
    pub(crate) subject_a_token: String,
    pub(crate) subject_b_token: String,
    /// The ID token a password grant with `scope=openid` mints for the first subject - the token
    /// whose `aud` carries the tier's `id-token-audience` mapper value, the whole of #105's
    /// step-1 probe. Minted once per case, the same way the two access tokens above are.
    pub(crate) id_token: String,
    /// The third-party audience the tier declares, READ off the realm file rather than restated as
    /// a literal - one document the tier provisions and a cell asserts against, never two.
    pub(crate) id_token_audience: String,
}

impl KeycloakFixture {
    /// Settings for a deployment declaring this fixture's own issuer and key set, with the audience
    /// given - [`Self::resource`] for the accepting case, or a deliberately different one to provoke
    /// the wrong-audience refusal without asking the realm for a second token.
    ///
    /// **`token_type: "any"`, not the `at+jwt` default.** Measured on 2026-09-14: this realm's
    /// password-grant token carries `"typ":"JWT"` in its header, the class Keycloak mints when
    /// nothing configures the RFC 9068 access-token profile - and `RequiredTokenType`'s default
    /// refuses exactly that class, the same defense `docs/adr/0014` names and the mock-issuer
    /// venue already owns (`token_class_where_an_access_token_is_required` in
    /// `docs/where-identity-is-proven.md`'s claims matrix). This fixture's own claim is narrower:
    /// a real signature and a real key set verify, not that this deployment's RFC 9068 class-check
    /// also applies to a provider that does not mint that class by default.
    pub(crate) fn settings_naming(&self, resource: &str) -> String {
        settings_crediting(
            &example_root(),
            &format!(
                "  inbound:\n    mode: \"direct\"\n    resource: \"{resource}\"\n    \
                 authorization_server: \"{}\"\n    key_set_file: \"{}\"\n    algorithms: [\"RS256\"]\n    \
                 token_type: \"any\"\n",
                self.issuer,
                self.key_set_file.display(),
            ),
        )
    }
}

/// Starts from the tier this worktree provisions, or panics naming the task that brings it up -
/// never skips, per this module's own header.
pub(crate) fn settings(case: &str) -> KeycloakFixture {
    let root = sutura_dev::provisioned::worktree_root(Path::new(env!("CARGO_MANIFEST_DIR")))
        .expect("this crate is inside a checkout of the repository the keycloak tier provisions into");
    let _endpoint = sutura_dev::provisioned::in_worktree(&root, "keycloak").unwrap_or_else(|absent| {
        panic!(
            "{absent}\n  This cell is reached only by `just keycloak-served-test`, which brings the tier \
             up first rather than skipping."
        )
    });
    let realm = read_realm(&root);
    let scratch = derived_beside(&config_path(case));
    std::fs::create_dir_all(&scratch).expect("the keycloak case's scratch directory is creatable");

    let token_url = format!("{}/protocol/openid-connect/token", realm.issuer);
    let jwks_url = format!("{}/protocol/openid-connect/certs", realm.issuer);
    let client = agent(&realm.tls_certificate_file);

    let (username_1, password_1) = &realm.subjects[0];
    let (username_2, password_2) = &realm.subjects[1];
    // Trailing-digit names, deliberately: `clippy::similar_names` flags `subject_a_token` beside
    // `subject_b_token` as one typo'd character apart, and a trailing digit is the one difference
    // the lint treats as intentional enumeration rather than a mistake.
    let subject_token_1 = mint(
        &client,
        &token_url,
        &realm.client_id,
        &realm.client_secret,
        username_1,
        password_1,
    );
    let subject_token_2 = mint(
        &client,
        &token_url,
        &realm.client_id,
        &realm.client_secret,
        username_2,
        password_2,
    );
    let resource = audience_of(&subject_token_1);

    // The ID token for #105's step-1 probe: a password grant with `scope=openid` against the same
    // client and subject, so the `id-token-audience` mapper puts the third-party audience into its
    // `aud`. `realm.id_token_audience` is the expected value, read off the realm file the tier
    // itself wrote - not a literal, so the two cannot disagree.
    let id_token = mint_id_token(
        &client,
        &token_url,
        &realm.client_id,
        &realm.client_secret,
        username_1,
        password_1,
    );
    let id_token_audience = realm.id_token_audience;

    let key_set_file = scratch.join("keycloak-jwks.json");
    std::fs::write(&key_set_file, fetch_key_set(&client, &jwks_url)).expect("the fetched key set is writable");

    KeycloakFixture {
        issuer: realm.issuer,
        resource,
        key_set_file,
        subject_a_token: subject_token_1,
        subject_b_token: subject_token_2,
        id_token,
        id_token_audience,
    }
}

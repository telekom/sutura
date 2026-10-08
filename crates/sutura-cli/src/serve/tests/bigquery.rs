//! `bigquery`-kind sources: the settings-tree refusal for a missing key, the composition root's own
//! refusal for a build that did not link the adapter, and - on a build that did - the furthest a
//! fixture with no project can reach: the driver demand, the anchor's verification rule, and the
//! broker this root attaches for an impersonating source. Moved out of `tests.rs` as a pure
//! relocation to keep that file under the 1000-line cap with room to spare.

use super::support::bundle_with_an_anchor;
use super::{bigquery_entry, default_timeout, one_worker, open_engine, opened_bigquery, refusal, registry, wif};
#[cfg(feature = "bigquery")]
use base64::Engine as _;
#[cfg(feature = "bigquery")]
use sutura_domain::identity::{
    Agreed, CredentialBroker as _, Minted, Presented, PrincipalChain, PrincipalName, RequestContext, Secret, SourceSet, Subject,
};
#[cfg(feature = "bigquery")]
use sutura_domain::model::SourceName;
#[cfg(feature = "bigquery")]
use sutura_http_client::test_support::{FakeServer, Scripted};

#[test]
fn a_bigquery_source_missing_a_key_that_kind_is_opened_with_does_not_load() {
    // **A settings-tree refusal rather than a boot one, and it belongs here for the reason the
    // unknown-kind half above belongs here:** this is the binary that would otherwise serve it, and
    // it is where a reader looks for the check. `sutura-config` owns the mechanism and tests each key
    // separately; what this asserts is that the refusal survives `Settings::load` with its key
    // attached, which is the only part a composition root depends on.
    //
    // `max_bytes_billed` and not `billing_project`, because it is the key with no default and the one
    // that bounds what a question may cost - see its own note in `sutura-config`.
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\nsources:\n{}",
        "  warehouse:\n    kind: \"bigquery\"\n    billing_project: \"acme-analytics\"\n    dataset: \
         \"warehouse\"\n    posture: \"shared-service-user\"\n"
    );
    let error = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect_err("a bigquery source with no byte ceiling is not a source this deployment can open");
    let rendered = super::super::flatten(error);
    assert!(
        rendered.contains("max_bytes_billed"),
        "the refusal must name the key: {rendered}"
    );
    assert!(rendered.contains("warehouse"), "the refusal must name the entry: {rendered}");
}

#[test]
#[cfg(not(feature = "bigquery"))]
fn a_bigquery_source_is_refused_by_a_build_that_did_not_link_the_adapter() {
    // **The refusal that used to be about the KIND and is now about the FEATURE.** It parses - the
    // vocabulary of kinds is the repository's and the repository has this adapter - and the
    // composition root refuses it, because only this file can see what was linked.
    //
    // It names the feature as well as the source, because the two available actions are in two
    // different files: change the `kind:`, or build with `--features bigquery`. Under
    // `--all-features` this test is not compiled and its twin below is; that split is the honest
    // consequence of a behaviour that differs by build, and one test cannot assert both.
    //
    // **WHICH VENUE RUNS THIS ONE:** `just gates` and the `The shipped feature set runs its tests`
    // step in `ci.yml`, both through `cargo xtask check-default-feature-tests`. Not `just test` and
    // not the `nextest` nix check - they pass `--all-features`, so this cfg is false there. It was
    // compiled by a gate and executed by nothing at all until that task existed.
    let error = refusal(
        opened_bigquery(&bigquery_entry("warehouse", "shared-service-user", "")),
        "a build with no BigQuery adapter must not start against a bigquery source",
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        error.contains("bigquery` feature") || error.contains("--features bigquery"),
        "the refusal must name the feature that would link it: {error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn a_bigquery_source_refuses_for_a_missing_driver() {
    // **What this proves, and it is the whole seam a test with no driver can reach:** the kind
    // DISPATCHED to the BigQuery adapter, the shared posture was accepted against that adapter's
    // OWN `IMPERSONATION`, both bounds parsed, and the composition then demanded the on-disk ADBC
    // driver - refused here because no `SUTURA_BIGQUERY_ADBC_DRIVER` points at one. The driver
    // authenticates ambiently.
    let error = refusal(
        opened_bigquery(&bigquery_entry("warehouse", "shared-service-user", "")),
        "the ADBC driver is not configured, so this deployment does not start",
    );
    assert!(
        error.contains("SUTURA_BIGQUERY_ADBC_DRIVER"),
        "the refusal must name the driver variable an operator has to set: {error}"
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        !error.contains("--features bigquery"),
        "this build DID link the adapter: {error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn an_impersonating_source_is_opened_and_reaches_the_driver_refusal() {
    // **Issue 87's serve half, and the reversal is the point of the change.** The adapter declares
    // `PerSubjectCredential`, so the serve open fn's capability cross-check ACCEPTS an
    // `impersonation-at-source` entry - it is not refused here by name. Which principal a subject's
    // job runs as is decided at request time, by the broker plus the driver's own impersonation
    // option, and this boot seam cannot reach either without a driver. What a test with no driver
    // CAN reach is the demand for the on-disk driver itself; matching that instead of an
    // `impersonation-at-source` refusal is what shows the capability half still accepts the posture.
    // The ATTACHING half now has its own cells at the foot of this file.
    let error = refusal(
        opened_bigquery(&bigquery_entry(
            "warehouse",
            "impersonation-at-source",
            &format!("{}    verification_identity: \"sutura_anchor_reader\"\n", wif()),
        )),
        "an impersonating source passes the capability check, so its refusal is the missing driver",
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        error.contains("SUTURA_BIGQUERY_ADBC_DRIVER"),
        "the source was OPENED past the posture cross-check and its refusal names the driver: {error}"
    );
    assert!(
        !error.contains("does not attach a broker"),
        "the serve open fn still accepts the posture - impersonation is not refused by name here: {error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn a_bigquery_ceiling_the_adapter_will_not_send_is_a_startup_refusal_naming_the_key() {
    // **The refusal's own cell, and not the predicate's.** `BytesBilledCeiling::parse` has its own
    // cells beside the type; this is the only thing that shows the served root CONSULTS it. Wrap the
    // parse's `?` so the error is discarded while the value is still read - the shape `dead_code`
    // cannot see - and this cell is what reddens.
    //
    // This test lived here before, held the same range parse, and was DELETED when the HTTP wire
    // took `BytesBilledCeiling` with it, on the stated grounds that "there is no boot-time number
    // left to test". There is again: the ADBC driver takes a `bigquery.query.max_bytes_billed`
    // statement option, so `max_bytes_billed` is parsed and sent rather than required and ignored.
    //
    // Zero rather than a value above the cap, because zero is the one an operator reaches by writing
    // a placeholder - and BigQuery reads a ceiling below one as NO ceiling, so it is the value that
    // silently buys nothing rather than the one that refuses everything.
    let entry = bigquery_entry("warehouse", "shared-service-user", "").replace("1073741824", "0");
    let error = refusal(
        opened_bigquery(&entry),
        "a ceiling of zero is how BigQuery spells no ceiling, so it cannot be one",
    );
    assert!(error.contains("max_bytes_billed"), "the refusal must name the key: {error}");
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        !error.contains("SUTURA_BIGQUERY_ADBC_DRIVER"),
        "the ceiling is parsed BEFORE the driver is resolved, so this must not be the driver's refusal: {error}"
    );
}

// `a_request_timeout_that_leaves_no_job_budget_does_not_start` lived here: a ten-second
// `server.request_timeout_seconds` used to refuse a `bigquery` deployment at boot, because
// `QueryDeadline::within_request_timeout` divided that number by the two calls one answer makes and
// found nothing left. `docs/adr/0029` retired that arithmetic, and its second amendment retired the
// replacement too: a request-time job CARRIES the port's own `Deadline` and sends it nowhere, so
// there is no per-call job budget to divide and a ten-second `server.request_timeout_seconds` is a
// usable (if narrow) caller budget over an unbounded job. This comment read *derives
// `timeoutMs`/`jobTimeoutMs` from the port's own `Deadline`* until #929's eighth round; those were
// `jobs.query` request parameters and went with the HTTP transport. The refusal this test held is
// gone with the arithmetic that produced it; deleted rather than adapted, because there is no
// boot-time number left to test.

#[test]
fn an_anchor_on_a_bigquery_source_is_held_to_the_same_verification_rule() {
    // **The anchor check reads a DECLARATION and not a kind, so registering a second adapter must not
    // have moved it - and this is what says so rather than leaving it to be assumed.** It runs before
    // anything is opened, so it fires on a `bigquery` source exactly as it fires on a `files` one: a
    // metric that certifies a number, on a source declared `impersonation-at-source` with nobody named
    // to re-run it as, is a bundle this deployment cannot verify.
    let error = refusal(
        open_engine(
            &bundle_with_an_anchor("warehouse"),
            &registry(&bigquery_entry("warehouse", "impersonation-at-source", wif())),
            one_worker(),
            default_timeout(),
            None,
        ),
        "an anchor on an impersonating source with no verification identity must not start",
    );
    assert!(
        error.contains("verification_identity"),
        "the refusal must name the key that would declare one: {error}"
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");

    // And a SHARED `bigquery` source's anchor is a complete claim, so this is not a check that fires
    // on every anchored bundle: the verification identity IS the shared identity, one service account
    // reaching the dataset for everybody, so the number the anchor certifies is the number every
    // caller gets. It still does not START - the ADBC driver is not there on this machine, and on
    // a build with no adapter the feature is missing - but whatever stops it is not this check.
    let later = refusal(
        open_engine(
            &bundle_with_an_anchor("warehouse"),
            &registry(&bigquery_entry("warehouse", "shared-service-user", "")),
            one_worker(),
            default_timeout(),
            None,
        ),
        "the credential is still not there, so the deployment still does not start",
    );
    assert!(
        !later.contains("verification_identity"),
        "a shared source's anchors run as the shared identity: {later}"
    );
}

/// The `workload_identity` block an impersonating source needs, with whatever the case adds after it.
///
/// `wif()` plus a tail rather than a second literal, so the audience and scope every case shares are
/// written once and only the part under test differs.
#[cfg(feature = "bigquery")]
fn wif_with(extra: &str) -> String {
    format!("{}{extra}    verification_identity: \"sutura_anchor_reader\"\n", wif())
}

/// Two declared subjects, each mapped to its own account - the shape a served impersonating source
/// is ADMITTED on. Whether either subject is answered is not a question this module asks.
#[cfg(feature = "bigquery")]
fn two_declared_subjects() -> &'static str {
    "      impersonate:\n        \"analyst-a@example.com\": \"bq-a@acme-analytics.iam.gserviceaccount.com\"\n        \
     \"analyst-b@example.com\": \"bq-b@acme-analytics.iam.gserviceaccount.com\"\n"
}

#[test]
#[cfg(feature = "bigquery")]
fn a_declared_two_subject_map_admits_the_source_to_the_broker() {
    // **THE NEGATIVE CONTROL for the two boot refusals below**, and it is the cell that says the
    // composition works at all: without it both of those pass over a `build_broker` that refused
    // every registry. A declared two-subject map builds a broker holding this source, so the
    // refusals beneath are about what they name rather than about anything impersonating.
    //
    // **Named for admission, because admission is all it asserts.** It used to be named for the
    // source being *answerable*, and review broke that name: a broker that admits this registry and
    // then refuses to mint for every subject keeps this cell green, because what is read is
    // `count()` and not an answer. What a declared subject is minted is the next cell's question, and
    // no cell anywhere asks a real BigQuery as one.
    let broker = super::super::broker::build_broker(
        &registry(&bigquery_entry(
            "warehouse",
            "impersonation-at-source",
            &wif_with(two_declared_subjects()),
        )),
        None,
    )
    .expect("a declared two-subject map is a source this deployment can serve");
    assert_eq!(broker.count(), 1);
}

/// The half [`a_declared_two_subject_map_admits_the_source_to_the_broker`] does not ask: the broker
/// this root builds mints for a declared subject the account declared beside THAT subject, not
/// another entry's. Minted here and read back off the presented leg, with no request sent anywhere.
#[test]
#[cfg(feature = "bigquery")]
fn a_declared_subject_is_minted_the_account_declared_beside_it() {
    let broker = super::super::broker::build_broker(
        &registry(&bigquery_entry(
            "warehouse",
            "impersonation-at-source",
            &wif_with(two_declared_subjects()),
        )),
        None,
    )
    .expect("a declared two-subject map is a source this deployment can serve");
    let warehouse = SourceName::parse("warehouse").expect("a test source is a source");
    let asked = SourceSet::of(warehouse.clone());
    for (subject_name, assertion, expected_account) in [
        (
            "analyst-a@example.com",
            "assertion.for.analyst-a",
            "bq-a@acme-analytics.iam.gserviceaccount.com",
        ),
        (
            "analyst-b@example.com",
            "assertion.for.analyst-b",
            "bq-b@acme-analytics.iam.gserviceaccount.com",
        ),
    ] {
        let subject = Subject::verified(subject_name).expect("a test subject is a subject");
        let minted = broker
            .mint(
                &RequestContext::with_assertion(PrincipalChain::of(subject.clone()), Secret::new(assertion), 4_102_444_800),
                &asked,
            )
            .expect("a declared subject is mintable");
        let Agreed::Granted { credentials } = minted
            .agreeing_with(&subject, &asked, 4_000_000_000)
            .expect("the grant agrees with the request")
        else {
            panic!("a declared subject is granted");
        };
        match credentials.presented_for(&warehouse).expect("the source was asked for") {
            Presented::SubjectToken { impersonate, .. } => assert_eq!(
                impersonate.as_ref().map(PrincipalName::to_string).as_deref(),
                Some(expected_account),
                "{subject_name} must be minted the account declared beside it, not another entry's",
            ),
            other => panic!("an impersonating source presents a subject token, not {other:?}"),
        }
    }
}

#[test]
#[cfg(feature = "bigquery")]
fn an_impersonating_source_naming_no_subject_does_not_boot() {
    // **The defect class this whole composition was rebuilt to remove.** An impersonating source
    // whose map names nobody can serve no caller: every question would be refused
    // `credential_unavailable` while the startup log said the source opened. So it is a startup
    // failure, and the refusal names the key an operator writes rather than the broker.
    let error = super::super::broker::build_broker(
        &registry(&bigquery_entry("warehouse", "impersonation-at-source", &wif_with(""))),
        None,
    )
    .map(drop)
    .expect_err("a source that can serve no caller must not boot");
    assert!(error.contains("warehouse"), "the refusal must name the entry: {error}");
    assert!(
        error.contains("impersonate"),
        "the refusal must name the key to write: {error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn a_declared_pool_expectation_does_not_boot_because_nothing_in_this_build_reads_it() {
    // `expected_issuer`/`expected_audience` tie leg 1's issuer and audience to what a
    // workload-identity pool accepts, and the only thing that ever read them was the token exchange
    // this build no longer contains. Left in place they would read as a control that is in place -
    // `sutura_config` refuses a verification identity on a shared source for the same reason - so
    // they are refused here, naming both keys.
    let error = super::super::broker::build_broker(
        &registry(&bigquery_entry(
            "warehouse",
            "impersonation-at-source",
            &wif_with(&format!(
                "{}      expected_issuer: \"https://issuer.example.com/realms/sutura\"\n      expected_audience: \
             \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n",
                two_declared_subjects()
            )),
        )),
        None,
    )
    .map(drop)
    .expect_err("a pool expectation no transport in this build checks must not boot");
    assert!(error.contains("expected_issuer"), "{error}");
    assert!(error.contains("expected_audience"), "{error}");
}

// The `direct`-mode half of the serve root's broker: a registry whose `security` block verifies
// callers directly, so a declared delegation exchange is admitted and reached. Same shape as
// `super::registry`, with the inbound block `sutura-config` requires of a `direct` deployment.
#[cfg(feature = "bigquery")]
fn direct_registry(entries: &str) -> sutura_config::SourceRegistry {
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"the test deployment reads its own fixture \
             files\"\n  inbound:\n    mode: \"direct\"\n    resource: \"https://sutura.example.com\"\n    \
             authorization_server: \"https://idp.example.com\"\n    key_set_file: \"/nonexistent/keys.json\"\n    \
             algorithms: [\"RS256\"]\nsources:\n{entries}"
    );
    sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("the test settings load")
    .sources()
    .clone()
}

/// The pool provider's client ID the exchanged token must carry.
#[cfg(feature = "bigquery")]
const POOL: &str = "pool-client-id";

/// One source's declared delegation exchange, indented to sit inside `workload_identity:` after the
/// `impersonate:` map - 6 spaces for `delegation:`, 8 for its keys, exactly as `sutura_config` reads.
#[cfg(feature = "bigquery")]
fn delegation_block(endpoint: &str, secret_file: &std::path::Path) -> String {
    format!(
        "      delegation:\n        token_endpoint: \"{endpoint}\"\n        client_id: \"sutura\"\n        \
         client_secret_file: \"{}\"\n        audience: \"{POOL}\"\n",
        secret_file.display()
    )
}

/// The serve root's broker over `warehouse`, two declared subjects and a delegation at `endpoint`,
/// dialled over `outbound`.
#[cfg(feature = "bigquery")]
fn delegated_broker(
    endpoint: &str,
    secret_file: &std::path::Path,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_exec_bigquery::DeclaredPrincipalBroker, String> {
    super::super::broker::build_broker(
        &direct_registry(&bigquery_entry(
            "warehouse",
            "impersonation-at-source",
            &wif_with(&format!(
                "{}{}",
                two_declared_subjects(),
                delegation_block(endpoint, secret_file)
            )),
        )),
        outbound,
    )
}

/// A per-test client-secret file, so none of these cells shares a path with another (or with a
/// parallel run): created under a directory named for the case and this process, removed on drop.
#[cfg(feature = "bigquery")]
struct SecretFileGuard(std::path::PathBuf);

#[cfg(feature = "bigquery")]
impl SecretFileGuard {
    fn create(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a per-test directory is creatable");
        let path = dir.join("client-secret");
        std::fs::write(&path, "idp-client-secret\n").expect("the client secret file is writable");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(feature = "bigquery")]
impl Drop for SecretFileGuard {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            drop(std::fs::remove_dir_all(dir));
        }
    }
}

/// A compact JWT the fake identity provider "issued": `header.payload.signature`, with a future
/// `exp` and the pool provider as its `aud` - the exchanged token the broker must present.
#[cfg(feature = "bigquery")]
fn exchanged() -> String {
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!(
        "{}.{}.signature",
        encode(br#"{"alg":"RS256","typ":"JWT"}"#),
        encode(
            serde_json::json!({"aud": POOL, "exp": 4_102_444_800_u64 - 1, "sub": "s"})
                .to_string()
                .as_bytes()
        )
    )
}

/// The complete RFC 8693 token response whose `access_token` is [`exchanged`].
#[cfg(feature = "bigquery")]
fn issued() -> serde_json::Value {
    serde_json::json!({
        "access_token": exchanged(),
        "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
        "token_type": "Bearer",
        "expires_in": 300,
    })
}

#[test]
#[cfg(feature = "bigquery")]
fn a_delegated_source_exchanges_the_callers_own_token_and_presents_what_came_back() {
    // THE positive cell for the delegated broker: the subject's OWN inbound assertion is the
    // subject token the identity provider exchanges, and what the source presents is the exchanged
    // token - not the assertion - beside the account declared for that subject.
    let secret = SecretFileGuard::create("a_delegated_source_exchanges_the_callers_own_token_and_presents_what_came_back");
    let fake = FakeServer::start(vec![Scripted::ok(&issued())]);
    let broker = delegated_broker(&format!("{}/token", fake.endpoint()), secret.path(), None)
        .expect("a direct deployment admits a declared delegation");
    let warehouse = SourceName::parse("warehouse").expect("a test source is a source");
    let asked = SourceSet::of(warehouse.clone());
    let subject = Subject::verified("analyst-a@example.com").expect("a test subject is a subject");
    let minted = broker
        .mint(
            &RequestContext::with_assertion(
                PrincipalChain::of(subject.clone()),
                Secret::new("assertion.for.analyst-a"),
                4_102_444_800,
            ),
            &asked,
        )
        .expect("a declared subject at a delegated source is mintable");
    let Agreed::Granted { credentials } = minted
        .agreeing_with(&subject, &asked, 4_000_000_000)
        .expect("the grant agrees with the request")
    else {
        panic!("a declared subject is granted");
    };
    match credentials.presented_for(&warehouse).expect("the source was asked for") {
        Presented::SubjectToken { material, impersonate } => {
            #[expect(
                clippy::disallowed_methods,
                reason = "the cell asserts the presented material is the exchanged token"
            )]
            let material = String::from(material.expose_secret());
            assert_eq!(
                material,
                exchanged(),
                "the source must present the identity provider's token, not the caller's assertion"
            );
            assert_eq!(
                impersonate.as_ref().map(PrincipalName::to_string).as_deref(),
                Some("bq-a@acme-analytics.iam.gserviceaccount.com"),
                "the account declared beside the subject must still be presented",
            );
        }
        other => panic!("a delegated source presents a subject token, not {other:?}"),
    }
    // And the request the broker sent is the caller's own assertion, exchanged for the pool's
    // audience under this source's client credential - with the secret's newline trimmed, so it
    // reaches the wire as `client_secret=idp-client-secret` and not as `%0A`.
    let requests = fake.finish();
    let [request] = requests.as_slice() else {
        panic!("expected exactly one exchange, got {requests:?}");
    };
    let body = request.body();
    assert!(body.contains("subject_token=assertion.for.analyst-a"), "{body}");
    assert!(body.contains("audience=pool-client-id"), "{body}");
    assert!(body.contains("client_id=sutura"), "{body}");
    assert!(body.contains("client_secret=idp-client-secret"), "{body}");
    assert!(
        !body.contains("%0A"),
        "the client secret's trailing newline must be trimmed, not url-encoded: {body}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn a_refused_exchange_fails_the_mint_and_never_presents_the_inbound_token() {
    // A refused exchange is an IDENTITY failure - `503 identity_unavailable` - and never an answer
    // as the caller: the identity provider is a hard runtime dependency. And the refusal must not
    // carry the inbound token it was asked to exchange.
    let secret = SecretFileGuard::create("a_refused_exchange_fails_the_mint_and_never_presents_the_inbound_token");
    let fake = FakeServer::start(vec![Scripted::status(400, r#"{"error":"invalid_grant"}"#)]);
    let broker = delegated_broker(&format!("{}/token", fake.endpoint()), secret.path(), None)
        .expect("a direct deployment admits a declared delegation");
    let asked = SourceSet::of(SourceName::parse("warehouse").expect("a test source is a source"));
    let subject = Subject::verified("analyst-a@example.com").expect("a test subject is a subject");
    let error = broker
        .mint(
            &RequestContext::with_assertion(
                PrincipalChain::of(subject),
                Secret::new("assertion.for.analyst-a"),
                4_102_444_800,
            ),
            &asked,
        )
        .expect_err("a refused exchange must not mint");
    assert!(
        matches!(error, sutura_exec_bigquery::DeclaredPrincipalsUnusable::Delegation { .. }),
        "{error:?}"
    );
    let chain = format!("{error} / {error:?}");
    assert!(
        !chain.contains("assertion.for.analyst-a"),
        "the refusal must never carry the inbound token: {chain}"
    );
    drop(fake.finish());
}

#[test]
#[cfg(feature = "bigquery")]
fn an_undeclared_subject_at_a_delegated_source_is_refused_before_any_exchange() {
    // The authorization decision is decided BEFORE any exchange, so a caller this source does not
    // name is refused without the identity provider ever being dialled - even when the source
    // carries a declared delegation.
    let secret = SecretFileGuard::create("an_undeclared_subject_at_a_delegated_source_is_refused_before_any_exchange");
    let fake = FakeServer::start(vec![]);
    let broker = delegated_broker(&format!("{}/token", fake.endpoint()), secret.path(), None)
        .expect("a direct deployment admits a declared delegation");
    let asked = SourceSet::of(SourceName::parse("warehouse").expect("a test source is a source"));
    let stranger = Subject::verified("stranger@example.com").expect("a test subject is a subject");
    let minted = broker
        .mint(
            &RequestContext::with_assertion(
                PrincipalChain::of(stranger),
                Secret::new("assertion.for.stranger"),
                4_102_444_800,
            ),
            &asked,
        )
        .expect("an undeclared subject is refused, not an error");
    assert!(matches!(minted, Minted::Refused { .. }), "{minted:?}");
    assert!(
        fake.finish().is_empty(),
        "an undeclared subject must stop before any exchange reaches the identity provider",
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn an_unreadable_client_secret_file_does_not_boot() {
    // The client secret is read ONCE, at boot, into a `Secret` - so an unreadable file stops the
    // process naming the key before a single question, rather than failing a request mid-flight.
    let error = delegated_broker(
        "https://idp.example.com/token",
        std::path::Path::new("/nonexistent/sutura-idp-secret"),
        None,
    )
    .map(drop)
    .expect_err("an unreadable client secret file must not boot");
    assert!(error.contains("client_secret_file"), "the refusal must name the key: {error}");
    assert!(error.contains("warehouse"), "the refusal must name the entry: {error}");
}

#[test]
#[cfg(feature = "bigquery")]
fn a_whitespace_only_client_secret_file_does_not_boot() {
    // `echo > file` or a truncated secret is refused by the read every secret file shares, naming
    // the key, rather than trimmed to an empty credential and sent to the identity provider.
    let secret = SecretFileGuard::create("a_whitespace_only_client_secret_file_does_not_boot");
    std::fs::write(secret.path(), " \n\t\n").expect("the client secret file is writable");
    let error = delegated_broker("https://idp.example.com/token", secret.path(), None)
        .map(drop)
        .expect_err("a whitespace-only client secret must not boot");
    assert!(
        error.contains("`sources.warehouse.workload_identity.delegation.client_secret_file` is empty"),
        "{error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn the_delegation_exchange_dials_over_the_declared_outbound_anchors() {
    // The exchange client's agent is built from `security.outbound`: an identity provider whose
    // certificate only the declared bundle names is trusted, and without the declaration the same
    // provider is refused at the handshake - so the mint fails rather than exchanging.
    use sutura_http_client::tls_test_support::{Scratch, TlsFakeServer, declared_bundle, issue};

    let secret = SecretFileGuard::create("the_delegation_exchange_dials_over_the_declared_outbound_anchors");
    let scratch = Scratch::new("delegation-outbound");
    let leaf = issue();
    let outbound = sutura_tls::Declared::new(sutura_tls::Anchors::Bundle(declared_bundle(&scratch, "idp", &leaf)), None);
    let asked = SourceSet::of(SourceName::parse("warehouse").expect("a test source is a source"));
    let subject = Subject::verified("analyst-a@example.com").expect("a test subject is a subject");
    let mint = |outbound: Option<&sutura_tls::Declared>| {
        let idp = TlsFakeServer::start(&leaf, vec![Scripted::ok(&issued())]);
        delegated_broker(&format!("{}/token", idp.endpoint()), secret.path(), outbound)
            .expect("a direct deployment admits a declared delegation")
            .mint(
                &RequestContext::with_assertion(
                    PrincipalChain::of(subject.clone()),
                    Secret::new("assertion.for.analyst-a"),
                    4_102_444_800,
                ),
                &asked,
            )
    };
    let minted = mint(Some(&outbound)).expect("the declared bundle names the identity provider's certificate");
    assert!(
        matches!(
            minted.agreeing_with(&subject, &asked, 4_000_000_000),
            Ok(Agreed::Granted { .. })
        ),
        "the exchange over the declared anchors must grant"
    );
    let refused = mint(None).expect_err("the compiled-in roots do not name a self-signed identity provider");
    assert!(
        matches!(refused, sutura_exec_bigquery::DeclaredPrincipalsUnusable::Delegation { .. }),
        "{refused:?}"
    );
}

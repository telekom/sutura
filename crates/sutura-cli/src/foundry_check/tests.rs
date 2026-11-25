//! `foundry-check` over the configuration `serve` reads, against a loopback fake of Foundry and of
//! the identity provider: the lines it prints, and that no token, host or ontology identifier is
//! among them.
//!
//! **What this does not show:** any Foundry tenant or identity provider answering. The fakes answer
//! what they are scripted to, in order.

use std::path::PathBuf;

use sutura_catalog_foundry::fixture;
use sutura_http_client::test_support::{CapturedRequest, FakeServer, Scripted};

use super::{Report, check_configured};
use sutura_domain::identity::Secret;

const CATALOG_TOKEN: &str = "catalog-token-canary-7b1e";
const SOURCE_TOKEN: &str = "source-token-canary-c04d";
const CALLER_TOKEN: &str = "caller-token-canary-5a3f";
const LINKED_TOKEN: &str = "linked-foundry-token-canary-e2d8";
const CLIENT_SECRET: &str = "client-secret-canary-0f6b";
const ROW_VALUE: &str = "row-value-canary-91f2";
const ONTOLOGY: &str = "ri.ontology.main.ontology.00000000-0000-0000-0000-000000000000";

/// A scratch directory for one cell's secret files, removed when the cell ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(case: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sutura-foundry-check-{case}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path).expect("a scratch directory is creatable");
        Self(path)
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, format!("{text}\n")).expect("a scratch file is writable");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("a recorded page is JSON")
}

fn checked(overlay: &str, caller: Option<&Secret>) -> Report {
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("the test overlay loads");
    let mut report = Report::default();
    check_configured(&settings, caller, &mut report);
    report
}

/// The `catalogs:` entry `ontology` at `endpoint`, its credential lines given.
fn catalog_entry(endpoint: &str, credential: &str, foundry: &str) -> String {
    format!(
        "catalogs:\n  - name: \"ontology\"\n    kind: \"foundry\"\n    version: \"v1\"\n    endpoint: \"{endpoint}\"\n\
         {credential}    foundry:\n      ontology: \"{ONTOLOGY}\"\n{foundry}"
    )
}

/// Everything the output must not carry, for a cell whose fakes listen on `ports`.
fn assert_redacted(out: &str, ports: &[u16]) {
    for leaked in [
        CATALOG_TOKEN,
        SOURCE_TOKEN,
        CALLER_TOKEN,
        LINKED_TOKEN,
        CLIENT_SECRET,
        ROW_VALUE,
        ONTOLOGY,
        "127.0.0.1",
        "localhost",
    ] {
        assert!(!out.contains(leaked), "the output carries {leaked:?}:\n{out}");
    }
    for port in ports {
        assert!(
            !out.contains(&format!(":{port}")),
            "the output carries the port {port}:\n{out}"
        );
    }
}

fn request_lines(requests: &[CapturedRequest]) -> Vec<&str> {
    requests.iter().map(CapturedRequest::request_line).collect()
}

#[test]
fn a_shared_tenant_is_checked_a_step_at_a_time_with_no_token_host_or_identifier_in_the_output() {
    let mut page = json(fixture::OBJECT_TYPES);
    let platform = json(fixture::PLATFORM_OBJECT_TYPES)["data"]
        .as_array()
        .expect("a data array")
        .clone();
    page["data"].as_array_mut().expect("a data array").splice(0..0, platform);
    let none = serde_json::json!({ "data": [] });
    let mut answers = vec![Scripted::ok(&page), Scripted::ok(&none), Scripted::ok(&none)];
    answers.extend(fixture::OUTGOING.iter().map(|(_, page)| Scripted::ok(&json(page))));
    answers.push(Scripted::ok(&serde_json::json!({})));
    answers.push(Scripted::status(
        400,
        &format!(
            r#"{{"errorCode":"INVALID_ARGUMENT","errorName":"SqlQueries:QueryParseError","errorInstanceId":"00000000-0000-0000-0000-000000000001","parameters":{{"near":"{ROW_VALUE}"}}}}"#
        ),
    ));
    let server = FakeServer::start(answers);
    let port = server.addr().port();
    let scratch = Scratch::new("shared");
    let catalog_token = scratch.file("catalog-token", CATALOG_TOKEN);
    let source_token = scratch.file("source-token", SOURCE_TOKEN);
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"one tester checking one tenant\"\n{}\
         sources:\n  ontology:\n    kind: \"foundry\"\n    endpoint: \"{}\"\n    ontology: \"{ONTOLOGY}\"\n    \
         token_file: \"{}\"\n    posture: \"shared-service-user\"\n",
        catalog_entry(
            &server.endpoint(),
            &format!("    token_file: \"{}\"\n", catalog_token.display()),
            ""
        ),
        server.endpoint(),
        source_token.display(),
    );
    let report = checked(&overlay, None);
    let requests = server.finish();
    let out = report.render();

    assert_eq!(
        out,
        "catalog ontology credential: ok - the token file was read\n\
         catalog ontology object types: ok - 6 listed, 2 dotted API name(s) mapped\n\
         catalog ontology mapped com.palantir.system.Evolution: ok - model com_palantir_system_Evolution\n\
         catalog ontology mapped com.palantir.system.EvolutionProposal: ok - model com_palantir_system_EvolutionProposal\n\
         catalog ontology links com.palantir.system.Evolution: ok - 0 link side(s) read, 0 join(s) carried, 0 omitted\n\
         catalog ontology links com.palantir.system.EvolutionProposal: ok - 0 link side(s) read, 0 join(s) carried, 0 omitted\n\
         catalog ontology links Aircraft: ok - 1 link side(s) read, 0 join(s) carried, 1 omitted\n\
         catalog ontology links Airport: ok - 2 link side(s) read, 0 join(s) carried, 2 omitted\n\
         catalog ontology links Employee: ok - 1 link side(s) read, 0 join(s) carried, 1 omitted\n\
         catalog ontology links Flight: ok - 4 link side(s) read, 3 join(s) carried, 1 omitted\n\
         catalog ontology load: ok - 6 model(s), 3 join(s)\n\
         source ontology credential: ok - the token file was read\n\
         source ontology parse-only Aircraft: ok - Foundry parsed the statement\n\
         source ontology parse-only com.palantir.system.Evolution (dotted, one quoted identifier): refused - Foundry refused the SQL request with 400, errorName SqlQueries:QueryParseError\n"
    );
    assert!(!report.passed(), "a refused step fails the run");
    assert_redacted(&out, &[port]);

    let lines = request_lines(&requests);
    assert_eq!(lines.len(), 1 + 6 + 2, "{lines:?}");
    for request in &requests[..7] {
        assert_eq!(request.authorization(), Some(format!("Bearer {CATALOG_TOKEN}").as_str()));
    }
    for (request, reference) in requests[7..].iter().zip([
        "FROM `Aircraft`",
        "FROM `com.palantir.system.Evolution` AS `com_palantir_system_Evolution`",
    ]) {
        assert_eq!(
            request.request_line(),
            "POST /api/v2/sqlQueries/executeOntology?preview=true HTTP/1.1"
        );
        assert_eq!(request.authorization(), Some(format!("Bearer {SOURCE_TOKEN}").as_str()));
        let body: serde_json::Value = serde_json::from_str(request.body()).expect("the request body is JSON");
        assert_eq!(body["dryRun"], true, "{body}");
        assert!(
            body["query"].as_str().is_some_and(|query| query.contains(reference)),
            "{body}"
        );
    }
}

/// A deployment whose `ontology` source runs as each caller through `foundry_login` at `idp`, and
/// whose catalog, when `catalog` is set, is read from `foundry` with a token file.
fn login_overlay(scratch: &Scratch, foundry: &FakeServer, idp: &FakeServer, catalog: bool) -> String {
    let secret = scratch.file("client-secret", CLIENT_SECRET);
    let catalog = if catalog {
        catalog_entry(
            &foundry.endpoint(),
            &format!(
                "    token_file: \"{}\"\n",
                scratch.file("catalog-token", CATALOG_TOKEN).display()
            ),
            "",
        )
    } else {
        String::new()
    };
    format!(
        "security:\n  identity: \"multi-user\"\n  inbound:\n    mode: \"direct\"\n    resource: \"https://sutura.example.com\"\n    \
         authorization_server: \"https://idp.example.com\"\n    key_set_file: \"/nonexistent/keys.json\"\n    \
         algorithms: [\"ES256\"]\n{catalog}sources:\n  ontology:\n    kind: \"foundry\"\n    endpoint: \"{}\"\n    \
         ontology: \"{ONTOLOGY}\"\n    posture: \"impersonation-at-source\"\n    foundry_login:\n      \
         token_endpoint: \"{}/realms/example/broker/foundry/token\"\n      client_id: \"sutura\"\n      \
         client_secret_file: \"{}\"\n",
        foundry.endpoint(),
        idp.endpoint(),
        secret.display(),
    )
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs()
}

#[test]
fn a_foundry_login_source_reports_the_broker_answer_and_parses_under_the_linked_token() {
    let mut answers = vec![Scripted::ok(&json(fixture::OBJECT_TYPES))];
    answers.extend(fixture::OUTGOING.iter().map(|(_, page)| Scripted::ok(&json(page))));
    answers.push(Scripted::ok(&serde_json::json!({})));
    let foundry = FakeServer::start(answers);
    let idp = FakeServer::start(vec![Scripted::ok(&serde_json::json!({
        "access_token": LINKED_TOKEN,
        "token_type": "Bearer",
        "accessTokenExpiration": now() + 3600,
        "scope": "api:sql-queries-execute api:ontologies-read",
        "refresh_token": "never-read-refresh-token",
    }))]);
    let ports = [foundry.addr().port(), idp.addr().port()];
    let scratch = Scratch::new("login");
    let report = checked(
        &login_overlay(&scratch, &foundry, &idp, true),
        Some(&Secret::new(CALLER_TOKEN)),
    );
    let broker = idp.finish();
    let requests = foundry.finish();
    let out = report.render();

    assert!(report.passed(), "{out}");
    let broker_line = out
        .lines()
        .find(|line| line.starts_with("source ontology broker: "))
        .expect("a broker line");
    assert!(
        broker_line
            .starts_with("source ontology broker: ok - a linked Foundry token was returned; accessTokenExpiration present, "),
        "{broker_line}"
    );
    assert!(
        broker_line.ends_with(" s left; scope api:sql-queries-execute api:ontologies-read"),
        "{broker_line}"
    );
    assert!(
        out.ends_with("source ontology parse-only Aircraft: ok - Foundry parsed the statement\n"),
        "{out}"
    );
    assert_redacted(&out, &ports);

    assert_eq!(broker.len(), 1);
    assert!(
        broker[0]
            .body()
            .split('&')
            .any(|pair| pair == format!("token={CALLER_TOKEN}")),
        "the broker was not asked with the caller's own token"
    );
    let parse_only = requests.last().expect("a parse-only request");
    assert_eq!(parse_only.authorization(), Some(format!("Bearer {LINKED_TOKEN}").as_str()));
}

#[test]
fn a_broker_answer_without_an_expiry_is_reported_with_its_scope_and_nothing_reaches_foundry() {
    let foundry = FakeServer::start(Vec::new());
    let idp = FakeServer::start(vec![Scripted::ok(&serde_json::json!({
        "access_token": LINKED_TOKEN,
        "token_type": "Bearer",
        "expires_in": 3599,
        "scope": "api:ontologies-read",
    }))]);
    let ports = [foundry.addr().port(), idp.addr().port()];
    let scratch = Scratch::new("no-expiry");
    let report = checked(
        &login_overlay(&scratch, &foundry, &idp, false),
        Some(&Secret::new(CALLER_TOKEN)),
    );
    assert_eq!(idp.finish().len(), 1);
    assert_eq!(foundry.finish(), [], "a refused broker answer reached Foundry");
    let out = report.render();
    assert_eq!(
        out,
        "source ontology broker: refused - a linked Foundry token was returned, and accessTokenExpiration is absent, so `serve` refuses it; scope api:ontologies-read\n"
    );
    assert!(!report.passed());
    assert_redacted(&out, &ports);
}

#[test]
fn a_foundry_login_source_with_no_caller_token_asks_for_one_and_asks_nobody() {
    let foundry = FakeServer::start(Vec::new());
    let idp = FakeServer::start(Vec::new());
    let scratch = Scratch::new("no-caller");
    let report = checked(&login_overlay(&scratch, &foundry, &idp, false), None);
    assert_eq!(idp.finish(), [], "the identity provider was asked");
    assert_eq!(foundry.finish(), [], "Foundry was asked");
    assert_eq!(
        report.render(),
        "source ontology broker: refused - no caller token: name a file holding one, or set SUTURA_CALLER_TOKEN\n"
    );
}

#[test]
fn a_refused_client_credentials_mint_stops_the_catalog_at_its_first_step() {
    let server = FakeServer::start(vec![Scripted::status(
        401,
        r#"{"error":"invalid_client","error_description":"client secret-canary rejected"}"#,
    )]);
    let port = server.addr().port();
    let scratch = Scratch::new("mint");
    let secret = scratch.file("client-secret", CLIENT_SECRET);
    let overlay = catalog_entry(
        &server.endpoint(),
        "",
        &format!(
            "      client_credentials:\n        client_id: \"sutura-catalog-reader\"\n        \
             client_secret_file: \"{}\"\n        scopes: [\"api:ontologies-read\"]\n",
            secret.display()
        ),
    );
    let report = checked(&overlay, None);
    let requests = server.finish();
    let out = report.render();
    assert_eq!(
        out,
        "catalog ontology credential: refused - the token endpoint refused the client (401, invalid_client)\n"
    );
    assert!(!out.contains("secret-canary"), "{out}");
    assert_redacted(&out, &[port]);
    assert_eq!(request_lines(&requests), ["POST /multipass/api/oauth2/token HTTP/1.1"]);
}

#[test]
fn a_shared_source_minting_its_own_token_is_checked_before_and_under_it() {
    let foundry = FakeServer::start(vec![
        Scripted::ok(&serde_json::json!({
            "access_token": SOURCE_TOKEN,
            "token_type": "bearer",
            "expires_in": 3600,
        })),
        Scripted::status(
            403,
            r#"{"errorCode":"PERMISSION_DENIED","errorName":"Default:PermissionDenied","parameters":{}}"#,
        ),
    ]);
    let port = foundry.addr().port();
    let scratch = Scratch::new("minted-source");
    let secret = scratch.file("client-secret", CLIENT_SECRET);
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"one tester checking one tenant\"\n\
         sources:\n  ontology:\n    kind: \"foundry\"\n    endpoint: \"{}\"\n    ontology: \"{ONTOLOGY}\"\n    \
         posture: \"shared-service-user\"\n    client_credentials:\n      client_id: \"sutura-query\"\n      \
         client_secret_file: \"{}\"\n      scopes: [\"api:sql-queries-execute\", \"api:ontologies-read\"]\n",
        foundry.endpoint(),
        secret.display(),
    );
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("the test overlay loads");
    let alias = sutura_domain::model::SourceName::parse("ontology").expect("a source name");
    let source = crate::foundry::build(
        &alias,
        settings.sources().get(&alias).expect("the source is declared"),
        None,
        crate::sources::working_set(settings.runtime()),
    )
    .expect("the source composes");
    let probes = [super::Probe {
        api_name: String::from("Aircraft"),
        table: sutura_catalog_foundry::model_name("Aircraft").expect("a name").1,
    }];
    let checking = super::Checking {
        subject: "source ontology",
        caller: None,
        probes: &probes,
        minted: true,
    };
    let mut report = Report::default();
    super::check_source(&checking, &source, None, &mut report);
    let requests = foundry.finish();
    let out = report.render();
    assert_eq!(
        out,
        "source ontology credential: ok - a client-credentials token was minted\n\
         source ontology parse-only Aircraft: refused - Foundry refused the SQL request with 403, errorName Default:PermissionDenied\n"
    );
    assert_redacted(&out, &[port]);
    assert_eq!(
        request_lines(&requests),
        [
            "POST /multipass/api/oauth2/token HTTP/1.1",
            "POST /api/v2/sqlQueries/executeOntology?preview=true HTTP/1.1"
        ]
    );
    assert_eq!(
        requests[1].authorization(),
        Some(format!("Bearer {SOURCE_TOKEN}").as_str()),
        "the parse-only request did not carry the minted token"
    );
}

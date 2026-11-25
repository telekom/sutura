//! `foundry-check` over the configuration `serve` reads, against a loopback fake of Foundry and of
//! the identity provider: the lines it prints, and that no token, host, ontology identifier or - by
//! default - object type name is among them.
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
        "Aircraft",
        "Airport",
        "Employee",
        "Flight",
        "com.palantir",
        "com_palantir",
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
    answers.push(Scripted::ok(&page));
    answers.extend(std::iter::repeat_with(|| Scripted::ok(&serde_json::json!({}))).take(3));
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
            "      exclude_object_types: []\n"
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
         catalog ontology object types: ok - 6 listed, 0 excluded by `foundry.exclude_object_types`, 2 dotted API name(s) mapped\n\
         catalog ontology mapped object type #1: ok - maps to model of object type #1\n\
         catalog ontology mapped object type #2: ok - maps to model of object type #2\n\
         catalog ontology links object type #1: ok - 0 link side(s) read, 0 join(s) carried, 0 omitted\n\
         catalog ontology links object type #2: ok - 0 link side(s) read, 0 join(s) carried, 0 omitted\n\
         catalog ontology links object type #3: ok - 1 link side(s) read, 0 join(s) carried, 1 omitted\n\
         catalog ontology links object type #4: ok - 2 link side(s) read, 0 join(s) carried, 2 omitted\n\
         catalog ontology links object type #5: ok - 1 link side(s) read, 0 join(s) carried, 1 omitted\n\
         catalog ontology links object type #6: ok - 4 link side(s) read, 3 join(s) carried, 1 omitted\n\
         catalog ontology load: ok - 6 model(s), 3 join(s)\n\
         source ontology credential: ok - the token file was read\n\
         source ontology parse-only object type #3 by resource identifier: ok - Foundry resolved the reference and parsed the statement\n\
         source ontology parse-only object type #3 by API name: ok - Foundry resolved the reference and parsed the statement\n\
         source ontology parse-only object type #1 (dotted) by resource identifier: ok - Foundry resolved the reference and parsed the statement\n\
         source ontology parse-only object type #1 (dotted) by API name: refused - Foundry refused the SQL request with 400, errorName SqlQueries:QueryParseError\n"
    );
    assert!(!report.passed(), "a refused step fails the run");
    assert_redacted(&out, &[port]);

    let lines = request_lines(&requests);
    assert_eq!(lines.len(), 1 + 6 + 1 + 4, "{lines:?}");
    for request in &requests[..7] {
        assert_eq!(request.authorization(), Some(format!("Bearer {CATALOG_TOKEN}").as_str()));
    }
    // The source's own object-type list, read under the token its questions send, resolves each
    // API name to the RID a statement names.
    assert!(lines[7].starts_with("GET ") && lines[7].contains("/objectTypes"), "{lines:?}");
    assert_eq!(requests[7].authorization(), Some(format!("Bearer {SOURCE_TOKEN}").as_str()));
    for (request, reference) in requests[8..].iter().zip([
        "FROM `ri.ontology.main.object-type.00000000-0000-0000-0000-000000000005`",
        "FROM `Aircraft`",
        "FROM `ri.ontology.main.object-type.00000000-0000-0000-0000-000000000092`",
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
    answers.push(Scripted::ok(&json(fixture::OBJECT_TYPES)));
    answers.extend(std::iter::repeat_with(|| Scripted::ok(&serde_json::json!({}))).take(2));
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
        out.ends_with(
            "source ontology parse-only object type #1 by resource identifier: ok - Foundry resolved the reference and parsed the statement\n\
             source ontology parse-only object type #1 by API name: ok - Foundry resolved the reference and parsed the statement\n"
        ),
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
    // The object-type list the RID is read from, and both parse-only requests, carry the caller's
    // linked token; the catalog's five carry the catalog's.
    assert_eq!(requests.len(), 5 + 3);
    for request in &requests[5..] {
        assert_eq!(request.authorization(), Some(format!("Bearer {LINKED_TOKEN}").as_str()));
    }
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
    report.read_name("Aircraft", None);
    super::check_source(&checking, &source, None, &mut report);
    let requests = foundry.finish();
    let out = report.render();
    assert_eq!(
        out,
        "source ontology credential: ok - a client-credentials token was minted\n\
         source ontology parse-only object type #1 by resource identifier: refused - the object-type list that resolves API names to resource identifiers was not read: Foundry refused the object-type list with 403\n\
         source ontology parse-only object type #1 by API name: refused - Foundry refused the SQL request with 403, errorName Default:PermissionDenied\n"
    );
    assert_redacted(&out, &[port]);
    let lines = request_lines(&requests);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0], "POST /multipass/api/oauth2/token HTTP/1.1");
    assert!(lines[1].starts_with("GET ") && lines[1].contains("/objectTypes"), "{lines:?}");
    assert_eq!(lines[2], "POST /api/v2/sqlQueries/executeOntology?preview=true HTTP/1.1");
    for request in &requests[1..] {
        assert_eq!(
            request.authorization(),
            Some(format!("Bearer {SOURCE_TOKEN}").as_str()),
            "a source request did not carry the minted token"
        );
    }
}

#[test]
fn the_default_exclusion_drops_platform_types_before_their_links_are_read_and_names_none() {
    let mut page = json(fixture::OBJECT_TYPES);
    let platform = json(fixture::PLATFORM_OBJECT_TYPES)["data"]
        .as_array()
        .expect("a data array")
        .clone();
    page["data"].as_array_mut().expect("a data array").splice(0..0, platform);
    let mut answers = vec![Scripted::ok(&page)];
    answers.extend(fixture::OUTGOING.iter().map(|(_, page)| Scripted::ok(&json(page))));
    let server = FakeServer::start(answers);
    let port = server.addr().port();
    let scratch = Scratch::new("excluded");
    let overlay = catalog_entry(
        &server.endpoint(),
        &format!(
            "    token_file: \"{}\"\n",
            scratch.file("catalog-token", CATALOG_TOKEN).display()
        ),
        "",
    );
    let report = checked(&overlay, None);
    let requests = server.finish();
    let out = report.render();
    assert_eq!(
        out,
        "catalog ontology credential: ok - the token file was read\n\
         catalog ontology object types: ok - 6 listed, 2 excluded by `foundry.exclude_object_types`, 0 dotted API name(s) mapped\n\
         catalog ontology links object type #1: ok - 1 link side(s) read, 0 join(s) carried, 1 omitted\n\
         catalog ontology links object type #2: ok - 2 link side(s) read, 0 join(s) carried, 2 omitted\n\
         catalog ontology links object type #3: ok - 1 link side(s) read, 0 join(s) carried, 1 omitted\n\
         catalog ontology links object type #4: ok - 4 link side(s) read, 3 join(s) carried, 1 omitted\n\
         catalog ontology load: ok - 4 model(s), 3 join(s)\n"
    );
    assert!(report.passed(), "{out}");
    assert_redacted(&out, &[port]);
    let lines = request_lines(&requests);
    assert_eq!(lines.len(), 1 + 4, "an excluded type's links were read: {lines:?}");
    assert!(!lines.iter().any(|line| line.contains("com.palantir")), "{lines:?}");
}

#[test]
fn a_refused_load_names_only_what_went_wrong_unless_names_are_asked_for() {
    // `Aircraft` is not listed, so `Flight`'s foreign-key link to it refuses the load.
    let mut page = json(fixture::OBJECT_TYPES);
    page["data"]
        .as_array_mut()
        .expect("a data array")
        .retain(|object_type| object_type["apiName"] != "Aircraft");
    let answers = || {
        let mut answers = vec![Scripted::ok(&page)];
        answers.extend(fixture::OUTGOING[1..].iter().map(|(_, page)| Scripted::ok(&json(page))));
        answers
    };
    for names in [false, true] {
        let server = FakeServer::start(answers());
        let port = server.addr().port();
        let scratch = Scratch::new(if names { "refused-named" } else { "refused" });
        let overlay = catalog_entry(
            &server.endpoint(),
            &format!(
                "    token_file: \"{}\"\n",
                scratch.file("catalog-token", CATALOG_TOKEN).display()
            ),
            "",
        );
        let settings = sutura_config::Settings::load(
            &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(&overlay),
        )
        .expect("the test overlay loads");
        let mut report = if names { Report::naming() } else { Report::default() };
        check_configured(&settings, None, &mut report);
        drop(server.finish());
        let out = report.render();
        assert!(!report.passed());
        let load = out
            .lines()
            .find(|line| line.starts_with("catalog ontology load: refused - "))
            .unwrap_or_else(|| panic!("a refused load line: {out}"));
        if names {
            assert!(load.contains("Aircraft") && load.contains("aircraft"), "{load}");
        } else {
            assert_eq!(
                load,
                "catalog ontology load: refused - a link type points at an object type the read did not list; `--names` prints which"
            );
            assert_redacted(&out, &[port]);
            for leaked in ["aircraft", "tailNumber", "departureAirport", "arrivalAirport"] {
                assert!(!out.contains(leaked), "a link or property name leaked: {leaked}\n{out}");
            }
        }
    }
}

#[test]
fn tenant_names_are_printed_only_when_asked_for() {
    let model = sutura_domain::model::ModelName::parse("com_palantir_system_Evolution").expect("a model name");
    let rendered = |mut report: Report| {
        report.read_name("com.palantir.system.Evolution", Some(&model));
        report.push(
            "catalog c mapped com.palantir.system.Evolution",
            super::Verdict::Ok,
            "maps to com_palantir_system_Evolution",
        );
        report.render()
    };
    assert_eq!(
        rendered(Report::default()),
        "catalog c mapped object type #1: ok - maps to model of object type #1\n"
    );
    assert_eq!(
        rendered(Report::naming()),
        "catalog c mapped com.palantir.system.Evolution: ok - maps to com_palantir_system_Evolution\n"
    );
}

#[test]
fn arguments_take_names_and_one_token_file_and_refuse_anything_more() {
    let args = |raw: &[&str]| raw.iter().map(|arg| String::from(*arg)).collect::<Vec<_>>();
    let none = args(&[]);
    assert_eq!(
        super::arguments(&none),
        Ok(super::Arguments {
            names: false,
            token_file: None
        })
    );
    let both = args(&["--names", "/run/caller-token"]);
    assert_eq!(
        super::arguments(&both),
        Ok(super::Arguments {
            names: true,
            token_file: Some("/run/caller-token")
        })
    );
    for refused in [
        args(&["/run/caller-token", CALLER_TOKEN]),
        args(&["--name"]),
        args(&["--names", "--names"]),
        args(&["/run/caller-token", "--names", "extra"]),
    ] {
        assert!(super::arguments(&refused).is_err(), "{refused:?} was accepted");
    }
}

#[test]
fn a_token_written_where_its_file_belongs_is_refused_and_never_presented() {
    let refusal = super::caller_token(Some(CALLER_TOKEN)).expect_err("a token is no readable file");
    assert!(!refusal.contains(CALLER_TOKEN), "{refusal}");
    let scratch = Scratch::new("caller-file");
    let path = scratch.file("caller-token", CALLER_TOKEN);
    let read = super::caller_token(path.to_str()).expect("a readable file").expect("a token");
    #[expect(clippy::disallowed_methods, reason = "the test compares the token it wrote")]
    let exposed = read.expose_secret() == CALLER_TOKEN;
    assert!(exposed, "the file's token was not the one read");
}

#[test]
fn an_impersonating_source_mints_its_declared_startup_identity_and_nothing_else() {
    let foundry = FakeServer::start(vec![Scripted::ok(&serde_json::json!({
        "access_token": SOURCE_TOKEN,
        "token_type": "bearer",
        "expires_in": 3600,
    }))]);
    let idp = FakeServer::start(Vec::new());
    let ports = [foundry.addr().port(), idp.addr().port()];
    let scratch = Scratch::new("startup-identity");
    let secret = scratch.file("verification-secret", CLIENT_SECRET);
    let overlay = format!(
        "{}    verification_identity: \"sutura_anchor_reader\"\n    verification_client_credentials:\n      \
         client_id: \"sutura-anchors\"\n      client_secret_file: \"{}\"\n      scopes: [\"api:sql-queries-execute\"]\n",
        login_overlay(&scratch, &foundry, &idp, false),
        secret.display(),
    );
    let report = checked(&overlay, None);
    assert_eq!(idp.finish(), [], "the identity provider was asked");
    let requests = foundry.finish();
    let out = report.render();
    assert_eq!(
        out,
        "source ontology startup identity: ok - a client-credentials token was minted\n\
         source ontology broker: refused - no caller token: name a file holding one, or set SUTURA_CALLER_TOKEN\n"
    );
    assert_redacted(&out, &ports);
    assert_eq!(request_lines(&requests), ["POST /multipass/api/oauth2/token HTTP/1.1"]);
}

//! A source password, sent for real and then looked for everywhere the process answers or writes
//! (`telekom/sutura#1293`).
//!
//! Each cell serves a source whose `password_file` holds a password, makes the source log in with
//! it, and looks for any twelve characters of that password in a row - and, where the source speaks
//! HTTP, of its `Basic` form - in the status line, every header and the body of each reply, over
//! `/v1` and `/mcp`, and in every line the process wrote to either stream at `trace`. An
//! absence proves nothing unless the password was sent, so each cell first shows the login was
//! made: an answer for the right password, the system's own refusal for the wrong one.

use crate::harness::{LOCAL_SOURCE, Reply, SINGLE_USER, Served, example_root, settings_over};

/// The subscriber reads `RUST_LOG` before the settings' filter, so this makes every line `trace`.
const TRACE: [(&str, &str); 1] = [("RUST_LOG", "trace")];

/// A password no system under test accepts, distinct enough to find in any text.
const CANARY: &str = "7c41d9e2b85f03a6c19e44d1";

/// The fewest characters of a secret in a row that count as finding it: a line that wraps or chunks
/// what it writes still carries this many of a 24-character secret in one piece.
const RUN: usize = 12;

/// A `trace` event as the bunyan formatter writes its level.
const TRACE_LEVEL: &str = r#""level":10"#;

/// The agent-surface deployment over the example catalog and `source`, with no inbound identity.
fn settings(source: &str) -> String {
    let example = example_root();
    settings_over(
        &example.join("catalog"),
        &example.join("data"),
        crate::harness::AGENT_LOOPBACK,
        SINGLE_USER,
        source,
    )
}

/// A password file holding `password`, in a directory this case owns and removes.
struct PasswordFile(std::path::PathBuf);

impl PasswordFile {
    fn holding(case: &str, password: &str) -> Self {
        let directory = std::env::temp_dir().join(format!("sutura-source-password-{case}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
        let path = directory.join("password");
        std::fs::write(&path, format!("{password}\n")).expect("the password file writes");
        Self(path)
    }
}

impl Drop for PasswordFile {
    fn drop(&mut self) {
        if let Some(directory) = self.0.parent() {
            drop(std::fs::remove_dir_all(directory));
        }
    }
}

/// The question `/v1/query` asks, as the `/mcp` `ask_metric` call after an `initialize`.
fn over_mcp(served: &Served) -> [Reply; 2] {
    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                   "clientInfo": {"name": "sutura-source-password", "version": "0.0.0"}}
    });
    let question: serde_json::Value =
        serde_json::from_str(&crate::harness::recurring_revenue_june()).expect("the question is JSON");
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "ask_metric", "arguments": question}
    });
    [served.mcp(None, &initialize.to_string()), served.mcp(None, &call.to_string())]
}

/// The `/v1` reply, the `/mcp` `initialize` and `ask_metric` replies, and every line written.
type Asked = ([Reply; 3], Vec<String>);

/// The question over `/v1` and over `/mcp`, then every line the process wrote until it stopped.
fn asked(mut served: Served) -> Asked {
    let v1 = served.post(
        &crate::harness::v1(sutura_http::constants::base_paths::QUERY),
        None,
        &crate::harness::recurring_revenue_june(),
    );
    let [initialized, mcp] = over_mcp(&served);
    served.terminate();
    ([v1, initialized, mcp], served.log())
}

/// `user:password` as an HTTP `Basic` header carries it.
fn basic(user: &str, password: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
}

/// The name of each of `secrets` that a reply's head or body, or a line, holds [`RUN`] characters
/// of in a row. Names, so a failure does not print the secret it found.
fn found(secrets: &[(&'static str, &str)], replies: &[Reply], lines: &[String]) -> Vec<&'static str> {
    secrets
        .iter()
        .filter(|&&(_, secret)| {
            (0..=secret.len().saturating_sub(RUN)).any(|at| {
                let run = secret.get(at..at + RUN).unwrap_or(secret);
                replies
                    .iter()
                    .any(|reply| reply.head.contains(run) || reply.body.contains(run))
                    || lines.iter().any(|line| line.contains(run))
            })
        })
        .map(|&(name, _)| name)
        .collect()
}

#[cfg(feature = "postgres")]
#[test]
fn a_postgres_source_password_reaches_no_reply_and_no_log_line() {
    let case = "source-password-postgres";
    let Some(loaded) = crate::harness::load_into_tier(case, LOCAL_SOURCE) else {
        return;
    };
    let password = std::fs::read_to_string(&loaded.password_file).expect("the tier's password file reads");
    let entry = |file: &std::path::Path| {
        crate::harness::source_entry(
            LOCAL_SOURCE,
            loaded.port,
            &loaded.database,
            &loaded.user,
            file,
            &loaded.anchor,
        )
    };
    let wrong = PasswordFile::holding(case, CANARY);

    let (replies, mut lines) = asked(crate::harness::start_configured_with_environment(
        case,
        &settings(&entry(&loaded.password_file)),
        None,
        &TRACE,
        &[],
    ));
    let [v1, _, mcp] = &replies;
    assert!(
        v1.status == 200 && v1.body.contains("202121") && mcp.status == 200 && mcp.body.contains("202121"),
        "the right password must answer over both surfaces, or its absence below proves nothing:\n{}\n{}",
        v1.body,
        mcp.body
    );
    let refused = crate::harness::refused_to_start(
        sutura_config::Environment::Development,
        crate::harness::written(&format!("{case}-canary"), &settings(&entry(&wrong.0))),
        &TRACE,
    );
    assert!(
        refused.iter().any(|line| line.contains("password authentication failed")),
        "the wrong password must be refused by Postgres itself, or its absence below proves nothing:\n{}",
        refused.join("\n")
    );
    lines.extend(refused);
    drop(loaded);

    assert!(
        lines.iter().any(|line| line.contains(TRACE_LEVEL)),
        "no `trace` line was captured, so the capture is not a record of everything logged"
    );
    let leaked = found(
        &[("the right password", password.trim()), ("the wrong password", CANARY)],
        &replies,
        &lines,
    );
    assert!(leaked.is_empty(), "a source password was sent back or logged: {leaked:?}");
}

#[cfg(feature = "oracle")]
#[test]
fn an_oracle_source_password_reaches_no_reply_and_no_log_line() {
    // Every verifier field the driver reads, so it derives its keys and sends the password.
    let blocks = "00".repeat(32);
    let salt = "00".repeat(16);
    let session = [
        ("AUTH_PBKDF2_VGEN_COUNT", "1"),
        ("AUTH_VFR_DATA", "00"),
        ("AUTH_SESSKEY", blocks.as_str()),
        ("AUTH_PBKDF2_SDER_COUNT", "1"),
        ("AUTH_PBKDF2_CSK_SALT", salt.as_str()),
    ];
    let issued = sutura_http_client::tls_test_support::issue();
    let config = sutura_http_client::tls_test_support::server_config(&issued);
    let open = move |mut stream| {
        let mut server = rustls::ServerConnection::new(config).ok()?;
        while server.is_handshaking() {
            let _io = server.complete_io(&mut stream).ok()?;
        }
        Some(rustls::StreamOwned::new(server, stream))
    };
    let (port, sent) = sutura_dev::tns_listener::authenticating_over(open, &session).expect("the fake listener binds");
    let case = "source-password-oracle";
    let wrong = PasswordFile::holding(case, CANARY);
    let anchors = PasswordFile::holding("source-password-oracle-ca", &issued.certificate().pem());
    let entry = format!(
        "  {LOCAL_SOURCE}:\n    kind: \"oracle\"\n    host: \"127.0.0.1\"\n    port: {port}\n    \
         service_name: \"FREEPDB1\"\n    user: \"sutura\"\n    password_file: \"{}\"\n    \
         transport_mode: \"verified\"\n    transport_anchors: \"{}\"\n    posture: \"shared-service-user\"\n",
        wrong.0.display(),
        anchors.0.display()
    );

    let lines = crate::harness::refused_to_start(
        sutura_config::Environment::Development,
        crate::harness::written(case, &settings(&entry)),
        &TRACE,
    );
    let sent = sent
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the listener reports what the driver sent");
    assert!(
        sent.windows(13).any(|name| name == b"AUTH_PASSWORD"),
        "the driver must have answered with its password, or its absence below proves nothing: {sent:?}\n{}",
        lines.join("\n")
    );

    // No `trace` line to ask for: this path logs nothing below `info` before it refuses. The
    // Postgres and ClickHouse cells show the same `TRACE` reaching a spawned process.
    assert!(
        lines.iter().any(|line| line.contains("could not connect to Oracle")),
        "the refusal on standard error was not captured, so the capture is not a record of everything written:\n{}",
        lines.join("\n")
    );
    let leaked = found(&[("the password", CANARY)], &[], &lines);
    assert!(leaked.is_empty(), "a source password was logged: {leaked:?}");
}

#[cfg(feature = "clickhouse")]
#[test]
fn a_clickhouse_source_password_reaches_no_reply_and_no_log_line() {
    use sutura_exec_clickhouse::ClickHouseWarehouse;
    use sutura_exec_clickhouse::transport::Endpoint;

    let provisioned = sutura_dev::provisioned::here(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse");
    let Some(endpoint) = provisioned.endpoint() else {
        return;
    };
    let required =
        |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("the clickhouse tier is present but {name} is not published"));
    let (user, password) = (
        required("SUTURA_CLICKHOUSE_TIER_USER"),
        required("SUTURA_CLICKHOUSE_TIER_PASSWORD"),
    );
    // The served source names no database, so the example goes where it reads: `default`.
    let loader = ClickHouseWarehouse::connect_in_database(
        sutura_domain::model::SourceName::parse("fixture").expect("a fixture source is a source"),
        sutura_domain::source::SourcePosture::SharedServiceUser {
            declared: sutura_domain::source::SharedIdentityDeclared::of(
                sutura_domain::source::AcknowledgementReason::parse("the served example uses one fixture user")
                    .expect("the fixture reason is an acknowledgement"),
            ),
        },
        Endpoint::plaintext(endpoint.host(), endpoint.port()),
        sutura_exec_clickhouse::fixture::credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}")),
        "default",
        sutura_domain::warehouse::ResultBudget::of_bytes(core::num::NonZeroUsize::new(1 << 30).expect("a gibibyte is positive")),
    )
    .unwrap_or_else(|cause| panic!("the clickhouse tier did not open: {cause}"));
    for entry in std::fs::read_dir(example_root().join("data")).expect("the example data directory reads") {
        let path = entry.expect("a data directory entry reads").path();
        if let Some(table) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|_| path.extension().is_some_and(|extension| extension == "csv"))
        {
            let table = sutura_domain::model::TableName::parse(table).expect("an example file is named for its table");
            loader
                .load_csv(&table, &path)
                .unwrap_or_else(|cause| panic!("{} did not load: {cause}", path.display()));
        }
    }
    let case = "source-password-clickhouse";
    let entry = |file: &PasswordFile| {
        format!(
            "  {LOCAL_SOURCE}:\n    kind: \"clickhouse\"\n    host: \"{}\"\n    port: {}\n    user: \"{user}\"\n    \
             password_file: \"{}\"\n    transport_mode: \"plaintext\"\n    posture: \"shared-service-user\"\n",
            endpoint.host(),
            endpoint.port(),
            file.0.display()
        )
    };
    let canary = format!("{case}-canary");
    let right = PasswordFile::holding(case, &password);
    let wrong = PasswordFile::holding(&canary, CANARY);

    let (answered, mut lines) = asked(crate::harness::start_configured_with_environment(
        case,
        &settings(&entry(&right)),
        None,
        &TRACE,
        &[],
    ));
    let [v1, _, mcp] = &answered;
    assert!(
        v1.status == 200 && v1.body.contains("202121") && mcp.status == 200 && mcp.body.contains("202121"),
        "the right password must answer over both surfaces, or its absence below proves nothing:\n{}\n{}",
        v1.body,
        mcp.body
    );
    let refused = crate::harness::refused_to_start(
        sutura_config::Environment::Development,
        crate::harness::written(&canary, &settings(&entry(&wrong))),
        &TRACE,
    );
    assert!(
        refused.iter().any(|line| line.contains("Authentication failed")),
        "the wrong password must be refused by ClickHouse itself, or its absence below proves nothing:\n{}",
        refused.join("\n")
    );
    lines.extend(refused);

    assert!(
        lines.iter().any(|line| line.contains(TRACE_LEVEL)),
        "no `trace` line was captured, so the capture is not a record of everything logged"
    );
    let leaked = found(
        &[
            ("the right password", &password),
            ("the right password as a basic credential", &basic(&user, &password)),
            ("the wrong password", CANARY),
            ("the wrong password as a basic credential", &basic(&user, CANARY)),
        ],
        &answered,
        &lines,
    );
    assert!(leaked.is_empty(), "a source password was sent back or logged: {leaked:?}");
}

//! A source password, sent for real over `sutura mcp` and looked for in everything the process
//! answers and writes (`telekom/sutura#1398`).
//!
//! The stdio surface installs no tracing subscriber, so what it writes is standard error: the
//! startup notice and any refusal. The cell serves a Postgres source whose `password_file` holds the
//! tier's password, asks a question the source answers, then starts a second process on a file
//! holding a password the tier refuses, and looks for any twelve characters of either in a row in
//! every reply and every line of standard error. Each is shown to have been sent: an answer for the
//! right one, the tier's own refusal for the wrong one.

use std::path::PathBuf;
use std::time::Duration;

use sutura_app::Capability;
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_exec_postgres::adbc::{AdbcPostgres, Channel, FixtureAdmin, PostgresDriver};
use sutura_exec_postgres::fixture::FixtureCredential;

use crate::harness::{Agent, SOURCE, VERSION, example_root, recurring_revenue_june, settings_tree, spawn_configured};

/// A password no tier accepts, distinct enough to find in any text.
const WRONG: &str = "e41b7c09d25a8f36b0c71d84";

/// The fewest characters of a secret in a row that count as finding it.
const RUN: usize = 12;

/// Held for as long as the cell asks: the tier's tables are dropped and refilled by every cell that
/// loads them, so two loads must not overlap.
const LOAD_LOCK_KEY: i64 = 0x5375_7475_7261_5351;

/// What the cell reads off the provisioned tier.
struct PasswordTier {
    port: u16,
    user: String,
    password: String,
    database: String,
    anchor: String,
    _lock: FixtureAdmin,
}

/// The tier, with the example's tables loaded into it; `None` where nothing is provisioned.
fn loaded_password_tier() -> Option<PasswordTier> {
    let found = sutura_dev::provisioned::here(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), "postgres");
    let endpoint = found.endpoint()?;
    let required =
        |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("the postgres tier is present but {name} is not published"));
    let source = SourceName::parse("fixture").expect("the fixture source parses");
    let conninfo = FixtureCredential::from_env()
        .unwrap_or_else(|unconfigured| panic!("{unconfigured}"))
        .conninfo(&source, endpoint.host(), endpoint.port(), Channel::Plaintext)
        .expect("the tier's published endpoint builds a connection string");
    let driver = PostgresDriver::from_host().expect("the tier is up, so a driver is named");
    let mut lock = FixtureAdmin::open(&driver, &conninfo).expect("the provisioned tier accepts a lock connection");
    lock.run(&format!("SELECT pg_advisory_lock({LOAD_LOCK_KEY})"))
        .expect("the load lock is acquirable");
    let reason = AcknowledgementReason::parse("the stdio example uses one fixture role").expect("a reason parses");
    let loader = AdbcPostgres::new(
        source,
        SourcePosture::SharedServiceUser {
            declared: SharedIdentityDeclared::of(reason),
        },
        driver,
        conninfo,
    )
    .expect("the default ceiling parses");
    for entry in std::fs::read_dir(example_root().join("data")).expect("the example data directory reads") {
        let path = entry.expect("a directory entry reads").path();
        if path.extension().is_some_and(|extension| extension == "csv") {
            let table = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| TableName::parse(stem).ok())
                .unwrap_or_else(|| panic!("{} is not named for a table", path.display()));
            loader
                .load_csv(&table, &path)
                .unwrap_or_else(|cause| panic!("{} did not load: {cause}", path.display()));
        }
    }
    Some(PasswordTier {
        port: endpoint.port(),
        user: required("SUTURA_POSTGRES_TIER_USER"),
        password: required("SUTURA_POSTGRES_TIER_PASSWORD"),
        database: required("SUTURA_POSTGRES_TIER_DB"),
        anchor: required("SUTURA_POSTGRES_TIER_CA"),
        _lock: lock,
    })
}

/// A deployment of the example catalog over the tier as `kind: postgres`, reading `password_file`.
fn deployment(case: &str, tier: &PasswordTier, password_file: &std::path::Path) -> PathBuf {
    let example = example_root();
    let PasswordTier {
        port,
        user,
        database,
        anchor,
        ..
    } = tier;
    settings_tree(
        case,
        &format!(
            "security:\n  identity: \"single-user\"\n  single_user_because: \"a test reads its own fixture tier as one identity\"\n\
             catalogs:\n  - name: \"model\"\n    kind: \"markdown\"\n    dir: \"{catalog}\"\n    data_dir: \"{data}\"\n    version: \"{VERSION}\"\n\
             sources:\n  {SOURCE}:\n    kind: \"postgres\"\n    host: \"127.0.0.1\"\n    port: {port}\n    database: \"{database}\"\n    \
             user: \"{user}\"\n    password_file: \"{password_file}\"\n    transport_mode: \"verified\"\n    \
             transport_anchors: \"{anchor}\"\n    posture: \"shared-service-user\"\n",
            catalog = example.join("catalog").display(),
            data = example.join("data").display(),
            password_file = password_file.display(),
        ),
    )
}

/// A password file in a directory this case owns, removed with it.
struct PasswordFile(PathBuf);

impl PasswordFile {
    fn holding(case: &str, password: &str) -> Self {
        let directory = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{case}-{}", std::process::id()));
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

/// Every line standard error carried until the process closed it.
fn written_by(agent: &Agent) -> Vec<String> {
    std::iter::from_fn(|| agent.log.recv_timeout(Duration::from_secs(10)).ok()).collect()
}

#[test]
fn a_source_password_reaches_no_reply_and_no_line_of_standard_error_over_stdio() {
    let Some(tier) = loaded_password_tier() else {
        return;
    };
    let case = "mcp-source-password";
    let right = PasswordFile::holding(case, &tier.password);
    let wrong = PasswordFile::holding(&format!("{case}-wrong"), WRONG);

    let mut agent = spawn_configured(&deployment(case, &tier, &right.0));
    let initialized = agent.initialize();
    let answered = agent.call(Capability::AskMetric, &recurring_revenue_june());
    agent.close();
    let mut lines = written_by(&agent);
    assert!(
        answered["structuredContent"]["rows"] == serde_json::json!([["2026-06-01", "202121"]]),
        "the right password must answer over stdio, or its absence below proves nothing: {answered}"
    );

    let mut refused = spawn_configured(&deployment(&format!("{case}-wrong"), &tier, &wrong.0));
    refused.close();
    let refusal = written_by(&refused);
    assert!(
        refusal.iter().any(|line| line.contains("password authentication failed")),
        "the wrong password must be refused by Postgres itself, or its absence below proves nothing:\n{}",
        refusal.join("\n")
    );
    lines.extend(refusal);

    let replies = [initialized.to_string(), answered.to_string()];
    let echoed: Vec<&str> = [("the right password", tier.password.trim()), ("the wrong password", WRONG)]
        .into_iter()
        .filter(|&(_, secret)| {
            (0..=secret.len().saturating_sub(RUN))
                .filter_map(|at| secret.get(at..at + RUN))
                .any(|run| replies.iter().chain(&lines).any(|text| text.contains(run)))
        })
        .map(|(name, _)| name)
        .collect();
    assert!(echoed.is_empty(), "a source password was sent back or written: {echoed:?}");
}

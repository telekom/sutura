//! A value a source binds into its statement, looked for in every line the process writes at
//! `trace`.
//!
//! The `clickhouse` source sends each bound value as a field of the request URL's query. The cell
//! declares one more region, a canary, asks for it, and looks for it in every line the process
//! wrote to either stream. An absence proves nothing unless the value was sent and the request was
//! logged, so the cell first shows the question was answered and the HTTP client's own request
//! line was captured.

use sutura_exec_clickhouse::ClickHouseWarehouse;
use sutura_exec_clickhouse::transport::Endpoint;

use crate::harness::{LOCAL_SOURCE, LOOPBACK, SINGLE_USER, example_root, settings_over};

/// The subscriber reads `RUST_LOG` before the settings' filter, so this makes every line `trace`.
const TRACE: [(&str, &str); 1] = [("RUST_LOG", "trace")];

/// A `trace` event as the bunyan formatter writes its level.
const TRACE_LEVEL: &str = r#""level":10"#;

/// A region no row holds, distinct enough to find in any text.
const CANARY: &str = "3f9a0c71d2e84b56a1c07e93";

/// The regions `recurring_revenue` declares in the example.
const REGIONS: &str = "values: [central, east, north, south, west]";

/// The example data, loaded into the tier's `default` database: a served source that names no
/// database reads there.
fn load_the_example(endpoint: Endpoint) {
    let budget = core::num::NonZeroUsize::new(1 << 30).expect("a gibibyte is positive");
    let reason = sutura_domain::source::AcknowledgementReason::parse("the served example uses one fixture user")
        .expect("the fixture reason is an acknowledgement");
    let loader = ClickHouseWarehouse::connect_in_database(
        sutura_domain::model::SourceName::parse("fixture").expect("a fixture source is a source"),
        sutura_domain::source::SourcePosture::SharedServiceUser {
            declared: sutura_domain::source::SharedIdentityDeclared::of(reason),
        },
        endpoint,
        sutura_exec_clickhouse::fixture::credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}")),
        "default",
        sutura_domain::warehouse::ResultBudget::of_bytes(budget),
    )
    .unwrap_or_else(|cause| panic!("the clickhouse tier did not open: {cause}"));
    let data = std::fs::read_dir(example_root().join("data")).expect("the example data directory reads");
    for path in data.map(|entry| entry.expect("a data directory entry reads").path()) {
        if path.extension().is_some_and(|extension| extension == "csv")
            && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
        {
            let table = sutura_domain::model::TableName::parse(stem).expect("an example file is named for its table");
            loader
                .load_csv(&table, &path)
                .unwrap_or_else(|cause| panic!("{} did not load: {cause}", path.display()));
        }
    }
}

/// The example catalog, copied to `root/catalog` with [`CANARY`] as one more region.
fn catalog_with_the_canary(root: &std::path::Path) -> std::path::PathBuf {
    let catalog = root.join("catalog");
    let copied = std::process::Command::new("cp")
        .arg("-R")
        .arg(example_root().join("catalog"))
        .arg(&catalog)
        .status()
        .expect("cp runs");
    assert!(copied.success(), "the example catalog did not copy");
    let metric = catalog.join("metrics").join("recurring_revenue.md");
    let text = std::fs::read_to_string(&metric).expect("the copied metric reads");
    assert!(text.contains(REGIONS), "{} no longer declares `{REGIONS}`", metric.display());
    let declared = text.replace(REGIONS, &format!("values: [central, east, north, south, west, {CANARY}]"));
    std::fs::write(&metric, declared).expect("the derived metric writes");
    catalog
}

#[test]
fn a_value_a_clickhouse_statement_binds_reaches_no_log_line() {
    let provisioned = sutura_dev::provisioned::here(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse");
    let Some(endpoint) = provisioned.endpoint() else {
        return;
    };
    let tier =
        |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("the clickhouse tier is present but {name} is not published"));
    let (user, password) = (tier("SUTURA_CLICKHOUSE_TIER_USER"), tier("SUTURA_CLICKHOUSE_TIER_PASSWORD"));
    load_the_example(Endpoint::plaintext(endpoint.host(), endpoint.port()));

    let case = "bound-value-clickhouse";
    let root = crate::harness::derived_beside(&crate::harness::config_path(case));
    drop(std::fs::remove_dir_all(&root));
    std::fs::create_dir_all(&root).expect("the derived directory is creatable");
    let catalog = catalog_with_the_canary(&root);
    let password_file = root.join("password");
    std::fs::write(&password_file, format!("{password}\n")).expect("the password file writes");
    let entry = format!(
        "  {LOCAL_SOURCE}:\n    kind: \"clickhouse\"\n    host: \"{}\"\n    port: {}\n    user: \"{user}\"\n    \
         password_file: \"{}\"\n    transport_mode: \"plaintext\"\n    posture: \"shared-service-user\"\n",
        endpoint.host(),
        endpoint.port(),
        password_file.display()
    );
    let settings = settings_over(&catalog, &example_root().join("data"), LOOPBACK, SINGLE_USER, &entry);
    let question = format!(
        r#"{{"metrics":["recurring_revenue"],"grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}},"filters":[{{"op":"eq","dimension":"region","value":"{CANARY}"}}]}}"#
    );

    let mut served = crate::harness::start_configured_with_environment(case, &settings, None, &TRACE, &[]);
    let reply = served.post(
        &crate::harness::v1(sutura_http::constants::base_paths::QUERY),
        None,
        &question,
    );
    served.terminate();
    let lines = served.log();

    assert!(
        reply.status == 200,
        "the question filtered on the canary must be answered, or its absence below proves nothing:\n{}",
        reply.body
    );
    assert!(
        lines.iter().any(|line| line.contains(r#""log.target":"ureq::run""#)),
        "no request line the HTTP client logged was captured, so the capture does not cover the request"
    );
    assert!(
        lines.iter().any(|line| line.contains(TRACE_LEVEL)),
        "no `trace` line was captured, so the capture is not a record of everything logged"
    );
    assert!(
        !lines.iter().any(|line| line.contains(CANARY)),
        "a value the statement bound was logged"
    );
}

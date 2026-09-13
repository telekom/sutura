//! The served PostgreSQL fixture: load the example into the real tier and declare its TLS source.

use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

use sutura_domain::model::{SourceName, TableName};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_exec_postgres::PostgresWarehouse;

use super::{LOCAL_SOURCE, LOOPBACK, SINGLE_USER, TOKEN, config_path, derived_beside, example_root, settings_over};

/// Loads the single-player example into the provisioned Postgres tier and returns settings that
/// make the composed binary reach it over verified TLS.
///
/// `None` is the ordinary no-tier outcome that every adapter fixture uses. Once discovery finds the
/// endpoint, every credential and TLS value is required: a half-provisioned tier is a failing test,
/// not an absent one. The password is copied to this case's temporary directory because a real
/// source declaration names a file rather than carrying secret text in the settings tree.
pub(crate) fn settings(case: &str) -> Option<String> {
    let found = sutura_dev::provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "postgres");
    let endpoint = found.endpoint()?;
    let required =
        |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("the postgres tier is present but {name} is not published"));
    let user = required("SUTURA_POSTGRES_TIER_USER");
    let password = required("SUTURA_POSTGRES_TIER_PASSWORD");
    let database = required("SUTURA_POSTGRES_TIER_DB");
    let anchor = required("SUTURA_POSTGRES_TIER_CA");

    let mut loader_config = tokio_postgres::Config::new();
    loader_config
        .host(endpoint.host())
        .port(endpoint.port())
        .user(&user)
        .password(&password)
        .dbname(&database);
    let source = SourceName::parse(LOCAL_SOURCE).expect("the example source name parses");
    let reason = AcknowledgementReason::parse("the served Postgres example uses one fixture role")
        .expect("the fixture reason is an acknowledgement");
    let posture = SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(reason),
    };
    let loader = PostgresWarehouse::connect(source, posture, &loader_config)
        .expect("the provisioned Postgres tier accepts its published fixture credential");
    let data = example_root().join("data");
    for entry in std::fs::read_dir(&data).expect("the single-player data directory is readable") {
        let path = entry.expect("a fixture directory entry is readable").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("csv") {
            continue;
        }
        let table = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| TableName::parse(stem).ok())
            .unwrap_or_else(|| panic!("{} does not have a table-shaped file name", path.display()));
        loader
            .load_csv(&table, &path)
            .unwrap_or_else(|cause| panic!("{} did not load as {table}: {cause}", path.display()));
    }

    let scratch = derived_beside(&config_path(case));
    std::fs::create_dir_all(&scratch).expect("the Postgres case's scratch directory is creatable");
    let password_file = scratch.join("password");
    let mut secret = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&password_file)
        .expect("the Postgres case's password file is creatable");
    secret
        .write_all(password.as_bytes())
        .expect("the Postgres case's password file is writable");

    let sources = format!(
        "  {LOCAL_SOURCE}:\n    \
           kind: \"postgres\"\n    \
           host: \"127.0.0.1\"\n    \
           port: {}\n    \
           database: \"{database}\"\n    \
           user: \"{user}\"\n    \
           password_file: \"{}\"\n    \
           transport_mode: \"verified\"\n    \
           transport_anchors: \"{anchor}\"\n    \
           posture: \"shared-service-user\"\n",
        endpoint.port(),
        password_file.display(),
    );
    Some(settings_over(
        &example_root().join("catalog"),
        &data,
        LOOPBACK,
        &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
        &sources,
    ))
}

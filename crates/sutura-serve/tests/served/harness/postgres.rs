//! The served PostgreSQL fixture: load the example into the real tier and declare its TLS source.

use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

use sutura_domain::model::{SourceName, TableName};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_exec_postgres::PostgresWarehouse;

use super::{LOCAL_SOURCE, LOOPBACK, SINGLE_USER, TOKEN, config_path, derived_beside, example_root, settings_over};

/// An arbitrary, fixed key for the advisory lock [`with_fixture_load_locked`] holds - any `i64`
/// works, chosen so it does not collide with a small counter a future use of the same mechanism
/// might pick.
const FIXTURE_LOAD_LOCK_KEY: i64 = 0x5375_7475_7261_5351;

/// Runs `load` with an exclusive, SERVER-SIDE lock held around it, so two callers loading the same
/// fixture into the same tier cannot interleave.
///
/// **Why a database lock and not an in-process one.** `settings` is called from more than one test
/// in this binary now - the certified case and, since `docs/adr/0013`'s showcase,
/// [`raw_sql_settings`] - and `nextest` runs each test in its OWN PROCESS. A `static` mutex would
/// only serialise callers that happen to share a process, which nextest's test binaries do not do
/// by default; measured directly - a `tokio::sync::Mutex` guarding this exact section still let two
/// processes' loads interleave. `Schema::create_statement` is `DROP TABLE IF EXISTS ...; CREATE
/// TABLE ...` followed by a separate `COPY`, so two callers loading the SAME tables at the SAME
/// time can race: one caller's `DROP` removes the table the other just `CREATE`d, an instant before
/// that caller's own `COPY` runs against it - measured as `could not load the fixture CSV
/// .../fct_subscription_monthly.csv as table fct_subscription_monthly` and, once the in-process
/// mutex proved insufficient, `.../dim_product.csv`.
///
/// `pg_advisory_lock` is session-scoped on the SERVER, so it serialises across processes rather
/// than only across threads of one - a fixed key here (unrelated to any row or table this catalog
/// uses) is enough, since there is exactly one thing this lock ever guards. Unlocked explicitly
/// with `pg_advisory_unlock` on the way out, on the SAME dedicated connection that acquired it -
/// an advisory lock is tied to the session that took it, so a different connection could not
/// release this one even if it tried.
fn with_fixture_load_locked<T>(config: &tokio_postgres::Config, load: impl FnOnce() -> T) -> T {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime for the advisory lock is buildable");
    let (client, connection) = runtime
        .block_on(config.connect(tokio_postgres::NoTls))
        .expect("the provisioned tier accepts a lock connection");
    // Own task, like `PostgresWarehouse::connect`'s own driver task - polled independently of this
    // function's own `block_on` calls, which is what lets the lock query below go through the same
    // client without deadlocking the executor. Its own error has no caller to route to; the next
    // `block_on` fails on the client's state instead.
    #[expect(
        clippy::let_underscore_must_use,
        clippy::let_underscore_untyped,
        reason = "the connection driver task's own error has no caller to route to, and the next \
                      block_on fails on the connection's state"
    )]
    runtime.spawn(async move {
        let _ = connection.await;
    });
    runtime
        .block_on(client.simple_query(&format!("SELECT pg_advisory_lock({FIXTURE_LOAD_LOCK_KEY})")))
        .expect("the fixture-load advisory lock is acquirable");
    let result = load();
    runtime
        .block_on(client.simple_query(&format!("SELECT pg_advisory_unlock({FIXTURE_LOAD_LOCK_KEY})")))
        .expect("the fixture-load advisory lock is releasable");
    result
}

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
    let data = example_root().join("data");
    with_fixture_load_locked(&loader_config, || {
        let source = SourceName::parse(LOCAL_SOURCE).expect("the example source name parses");
        let reason = AcknowledgementReason::parse("the served Postgres example uses one fixture role")
            .expect("the fixture reason is an acknowledgement");
        let posture = SourcePosture::SharedServiceUser {
            declared: SharedIdentityDeclared::of(reason),
        };
        let loader = PostgresWarehouse::connect(source, posture, &loader_config)
            .expect("the provisioned Postgres tier accepts its published fixture credential");
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
    });

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

/// [`settings`], plus `docs/adr/0013`'s raw tool turned on - `examples/raw-sql`'s showcase.
///
/// **The one line this adds is the whole of the diff, and that is the point of the example**:
/// enabling `run_sql` is a settings change on top of an otherwise ordinary served Postgres
/// deployment, not a different deployment shape.
///
/// **The limit stated here rather than left for a reader to discover.** `examples/raw-sql/README.md`
/// shows the `CREATE ROLE`/`GRANT` an operator runs to give this source a role that can only
/// `SELECT` - `docs/serving.md`'s own guidance for every Postgres source, restated there for this
/// one. This fixture cannot demonstrate that role actually existing: the tier publishes exactly one
/// credential (`SUTURA_POSTGRES_TIER_USER`, the database owner - see `nix/postgres-tier.nix`), which
/// carries no `CREATEROLE` attribute, so nothing running as it may create a second role to connect
/// as instead. Provisioning a dedicated reader role for this tier is future work and not a decision
/// this function makes by omission - the SQL an operator runs is documented and correct; this
/// binary happens to run it under the tier's one broadly-privileged fixture role, which is a fact
/// about the DEV TIER and not about `docs/adr/0013`'s tool or this deployment's own settings.
pub(crate) fn raw_sql_settings(case: &str) -> Option<String> {
    let base = settings(case)?;
    Some(format!("{base}tools:\n  run_sql:\n    enabled: true\n"))
}

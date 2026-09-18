//! The served PostgreSQL fixture: load the example into the real tier and declare its TLS source.

use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use sutura_domain::model::{SourceName, TableName};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_exec_postgres::PostgresWarehouse;

use super::{LOCAL_SOURCE, LOOPBACK, SINGLE_USER, TOKEN, config_path, derived_beside, example_root, settings_over};

/// An arbitrary, fixed key for the advisory lock [`FixtureLoadGuard`] holds - any `i64` works,
/// chosen so it does not collide with a small counter a future use of the same mechanism might
/// pick.
const FIXTURE_LOAD_LOCK_KEY: i64 = 0x5375_7475_7261_5351;

/// Holds the fixture-load advisory lock for as long as it lives - dropping it is what releases it.
///
/// **Why the lock has to outlive the LOAD, not just wrap it.** The load itself is two separate
/// server-side transactions per table (`PostgresWarehouse::load_schema`: a committed
/// `DROP TABLE IF EXISTS ...; CREATE TABLE ...`, then a second, separate `COPY`), so between them
/// the freshly created table is visible and EMPTY. A first version of this lock released as soon as
/// the load loop returned, which serialised the two LOADS against each other but left the window
/// open between "cell A releases the lock and starts querying its own served binary" and "cell B,
/// which was waiting on the lock, immediately begins dropping and refilling the same tables cell
/// A's binary is reading" - the two served cells measured running concurrently, adjacent in
/// `nextest`'s own log. Returning the lock alongside the settings, for the CALLER to hold until its
/// own test is done asking questions, is what closes that window: whichever cell is served and
/// queried finishes before the other's reload can start.
pub(crate) struct FixtureLoadGuard {
    runtime: tokio::runtime::Runtime,
    client: tokio_postgres::Client,
}

impl Drop for FixtureLoadGuard {
    fn drop(&mut self) {
        // Best-effort: this runs during an ordinary drop and, if the test is already panicking,
        // during unwinding too - a second panic here would abort the process rather than fail the
        // one test. Not load-bearing either way: an advisory lock is tied to the SESSION that took
        // it, and this connection's session ends within the same drop regardless of whether the
        // explicit unlock below succeeds.
        drop(
            self.runtime.block_on(
                self.client
                    .simple_query(&format!("SELECT pg_advisory_unlock({FIXTURE_LOAD_LOCK_KEY})")),
            ),
        );
    }
}

/// Acquires the fixture-load advisory lock on a dedicated connection and returns it held.
///
/// **Why a database lock and not an in-process one.** `settings` is called from more than one test
/// in this binary now - the certified case and, since `docs/adr/0013`'s showcase,
/// [`raw_sql_settings`] - and `nextest` runs each test in its OWN PROCESS. A `static` mutex would
/// only serialise callers that happen to share a process, which nextest's test binaries do not do
/// by default; measured directly - a `tokio::sync::Mutex` guarding the load still let two
/// processes' loads interleave. `pg_advisory_lock` is session-scoped on the SERVER, so it
/// serialises across processes rather than only across threads of one - a fixed key here (unrelated
/// to any row or table this catalog uses) is enough, since there is exactly one thing this lock
/// ever guards.
fn lock_fixture_load(config: &tokio_postgres::Config) -> FixtureLoadGuard {
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
    FixtureLoadGuard { runtime, client }
}

/// What loading the single-player example into the provisioned tier produced: everything a
/// `sources:` entry needs to reach the SAME data over `kind: "postgres"`, alongside the
/// [`FixtureLoadGuard`] the caller must hold for as long as its own test still asks the served
/// binary anything.
///
/// Split out of [`settings`] so `super::two_kind` can load the same tier under a source NAME of
/// its own choosing, beside a `files` source, rather than reaching this file's `LOCAL_SOURCE`
/// entry - the whole point of that case is two DIFFERENT kinds in one deployment, not two
/// deployments that each happen to be single-kind.
pub(crate) struct LoadedTier {
    pub(crate) guard: FixtureLoadGuard,
    pub(crate) port: u16,
    pub(crate) user: String,
    pub(crate) password_file: PathBuf,
    pub(crate) database: String,
    pub(crate) anchor: String,
}

/// Loads the single-player example's CSVs into the provisioned Postgres tier under `loader_source`
/// and returns what a caller needs to declare a `kind: "postgres"` source that reaches them.
///
/// `None` is the ordinary no-tier outcome that every adapter fixture uses. Once discovery finds the
/// endpoint, every credential and TLS value is required: a half-provisioned tier is a failing test,
/// not an absent one. The password is copied to this case's own scratch directory because a real
/// source declaration names a file rather than carrying secret text in the settings tree.
pub(crate) fn load_into_tier(case: &str, loader_source: &str) -> Option<LoadedTier> {
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
    // Acquired BEFORE the load and returned to the caller still held - see `FixtureLoadGuard`'s own
    // documentation for why releasing it here, once the load loop returns, is not enough.
    let guard = lock_fixture_load(&loader_config);
    {
        let source = SourceName::parse(loader_source).expect("the fixture's own loader source name parses");
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

    Some(LoadedTier {
        guard,
        port: endpoint.port(),
        user,
        password_file,
        database,
        anchor,
    })
}

/// One `sources:` entry declaring `loaded` as `name`, over verified TLS.
pub(crate) fn source_entry(name: &str, loaded: &LoadedTier) -> String {
    format!(
        "  {name}:\n    \
           kind: \"postgres\"\n    \
           host: \"127.0.0.1\"\n    \
           port: {}\n    \
           database: \"{}\"\n    \
           user: \"{}\"\n    \
           password_file: \"{}\"\n    \
           transport_mode: \"verified\"\n    \
           transport_anchors: \"{}\"\n    \
           posture: \"shared-service-user\"\n",
        loaded.port,
        loaded.database,
        loaded.user,
        loaded.password_file.display(),
        loaded.anchor,
    )
}

/// Loads the single-player example into the provisioned Postgres tier and returns settings that
/// make the composed binary reach it over verified TLS as [`LOCAL_SOURCE`] - alongside the
/// [`FixtureLoadGuard`] the caller must hold for as long as its own test still asks the served
/// binary anything.
///
/// `None` is the ordinary no-tier outcome [`load_into_tier`] returns it for.
pub(crate) fn settings(case: &str) -> Option<(String, FixtureLoadGuard)> {
    let loaded = load_into_tier(case, LOCAL_SOURCE)?;
    let sources = source_entry(LOCAL_SOURCE, &loaded);
    let settings = settings_over(
        &example_root().join("catalog"),
        &example_root().join("data"),
        LOOPBACK,
        &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
        &sources,
    );
    Some((settings, loaded.guard))
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
pub(crate) fn raw_sql_settings(case: &str) -> Option<(String, FixtureLoadGuard)> {
    let (base, guard) = settings(case)?;
    Some((format!("{base}tools:\n  run_sql:\n    enabled: true\n"), guard))
}

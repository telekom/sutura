//! `catalog.kind: rdbms`, with the `rdbms` feature ON: the refusals this build now REACHES
//! rather than the "not linked" one above. No real endpoint here - each cell but one is asserting
//! which refusal string a boot-time misconfiguration earns, not a read. The one that reads dials a
//! fake listener on loopback and asserts what a refresh does when the login fails.
//!
//! The `rdbms` catalog's own connection block is parsed by `sutura-config` (which checks the
//! shape: a host or a unix socket, a port, a database, a user, an absolute `password_file`, and a
//! `transport_mode` whose plaintext arm is refused for a non-loopback host). What that crate
//! cannot check is whether the FILE it names is readable, or whether the TLS MATERIAL it points
//! at parses - those are composition-root refusals, because `sutura-config` does not depend on
//! the adapter that reads the file and loads the material. These cells reach past the settings
//! parser and into `open_one_rdbms_catalog`, the same stand `datahub_served`'s endpoint cells
//! make for that kind.
//!
//! **`open_catalog` returns `Result<OpenedCatalogs, String>`** (`crate::catalog::open_catalog`) -
//! there is no typed refusal here to match, and no variant a caller or a later change could be
//! held to. That limit is main's, not this file's: these cells substring-match the refusal
//! STRING, asserting it names the catalog and the field that failed.
//!
//! Built through `Settings::load` rather than `CatalogSettings::parse` + `with_rdbms`, because
//! `RdbmsSettings::parse` is `pub(crate)` to `sutura-config` on purpose - the raw shapes are
//! private there, so the real settings loader is the only door and the refusals a file alone
//! decides cannot be skipped. This is the same path `serve/tests.rs`'s own `registry` helper
//! takes.

use std::path::PathBuf;

use sutura_config::{Environment, Settings, Sources};

/// One overlay declaring a single `kind: rdbms` catalog whose `source_alias` names the `files`
/// source this same overlay declares, with the `connection` block the caller fills in - the
/// minimum a deployment's `base.yaml` carries for an rdbms entry, built once so the two cells
/// below differ only in the one key each is refusing.
fn overlay(connection: &str) -> String {
    format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a composition-root test reads its own \
             fixture files\"\n\
         sources:\n  warehouse:\n    kind: \"files\"\n    data_dir: \"/srv/warehouse\"\n    \
         posture: \"shared-service-user\"\n\
         catalogs:\n  - name: \"dictionary\"\n    kind: \"rdbms\"\n    version: \"dict-1\"\n    \
         environment: \"prod\"\n    source_alias: \"warehouse\"\n    connection:\n{connection}"
    )
}

/// Loads the overlay and returns the parsed catalogs, or panics naming the settings error - the
/// cells below expect `Settings::load` to ACCEPT the declaration (the misconfiguration is in the
/// composition step, not the settings tree), so a refusal here is a test bug rather than the
/// refusal under test.
fn catalogs(connection: &str) -> sutura_config::Catalogs {
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay(connection)))
        .expect("the settings tree is well-formed; the misconfiguration is in the composition step");
    settings.catalogs().clone()
}

/// A real, readable password file in a per-run scratch directory, so a cell that is about the TLS
/// material rather than the credential reaches the TLS load: `open_one_rdbms_catalog` reads
/// `password_file` BEFORE it builds the TLS client config, so a missing file there masks the
/// anchor refusal this cell is proving. The directory is under the OS temp dir
/// (`std::env::temp_dir()`), cleared on the way in and removed by [`ScratchDir`]'s `Drop` on the
/// way out - no leak across runs. Mode `0o600`, the same as `harness/postgres.rs::load_into_tier`
/// writes its own password file, for the same reason: a credential file readable by anyone else
/// on the host is not one this test should model as normal.
fn readable_password_file(scratch: &ScratchDir) -> PathBuf {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let path = scratch.0.join("password");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .expect("the password file is writable");
    file.write_all(b"a-test-password\n").expect("the password file is writable");
    path
}

/// A directory this file owns and removes when dropped - see `tests/served/rdbms.rs`'s `DataDir`
/// for the same shape. A panic during the owning test's assertions runs this `Drop` during
/// unwinding rather than skipping it; the removal itself is best-effort and does not panic.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn prepared() -> Self {
        let dir = std::env::temp_dir().join(format!("sutura-rdbms-served-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
        Self(dir)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// The `password_file` a `kind: rdbms` catalog's own connection block names is read at boot
/// rather than at parse - `sutura-config` checks it is an ABSOLUTE path, because a relative one
/// resolves against an arbitrary working directory, and nothing else. A path that is absolute
/// and points at nothing reaches `open_one_rdbms_catalog`, which reads it through the same
/// `crate::password_file` read a `sources:` Postgres entry uses, and refuses naming the catalog and the
/// file - the same error identity a declared `postgres` source's missing credential earns.
#[test]
fn a_rdbms_catalog_with_an_unreadable_password_file_is_refused_naming_it() {
    let connection = "      host: \"127.0.0.1\"\n      port: 5432\n      database: \"dictionary\"\n      \
         user: \"reader\"\n      password_file: \"/definitely/not/a/real/secret\"\n      \
         transport_mode: \"plaintext\"\n";
    let err = crate::catalog::open_catalog(&catalogs(connection), None)
        .expect_err("an unreadable password file is a composition refusal");
    assert!(
        err.contains("catalogs.dictionary.connection.password_file"),
        "the refusal names the catalog and the field: {err}"
    );
    assert!(err.contains("could not be read"), "the refusal says what failed: {err}");
    assert!(
        err.contains("/definitely/not/a/real/secret"),
        "the refusal names the file that failed: {err}"
    );
    assert!(
        !err.contains("--features rdbms"),
        "this build DOES link the feature - the refusal must not send an operator chasing one: {err}"
    );
}

/// A `transport_mode: verified` connection whose `transport_anchors` points at no file is
/// accepted by `sutura-config` (which checks the key is present, not that the material loads)
/// and refused at `open_one_rdbms_catalog`, which loads the bundle through
/// `sutura_adbc_postgres::Conninfo::new`. The refusal names the catalog and the material.
///
/// The `password_file` here is a REAL, readable file (see [`readable_password_file`]) because
/// `open_one_rdbms_catalog` reads the credential before it builds the TLS config - a missing
/// password file would mask the anchor refusal this cell is proving with the credential refusal
/// the cell above already owns.
#[test]
fn a_rdbms_catalog_with_a_missing_tls_anchor_bundle_is_refused_naming_it() {
    let scratch = ScratchDir::prepared();
    let password_file = readable_password_file(&scratch);
    let connection = format!(
        "      host: \"127.0.0.1\"\n      port: 5432\n      database: \"dictionary\"\n      \
         user: \"reader\"\n      password_file: \"{password_file}\"\n      \
         transport_mode: \"verified\"\n      transport_anchors: \"/definitely/not/a/real/bundle.pem\"\n",
        password_file = password_file.display(),
    );
    let err = crate::catalog::open_catalog(&catalogs(&connection), None)
        .expect_err("a missing TLS anchor bundle is a composition refusal");
    assert!(
        err.contains("catalogs.dictionary.connection"),
        "the refusal names the catalog and the connection: {err}"
    );
    assert!(
        err.contains("material is not usable"),
        "the refusal says the TLS material failed: {err}"
    );
    assert!(
        err.contains("trust anchors"),
        "the refusal names the trust anchors, the material that failed: {err}"
    );
    assert!(
        !err.contains("--features rdbms"),
        "this build DOES link the feature - the refusal must not send an operator chasing one: {err}"
    );
}

/// `transport_anchors: system` on a catalog connection is refused at boot: libpq's `system` store is
/// not the host store sutura reads. The refusal comes from the shared connector, so this cell holds
/// that the catalog's `verified` arm hands it the declared anchors rather than dialling regardless.
/// `mutual` TLS is not exercised here: it needs a live TLS server.
#[test]
fn a_rdbms_catalog_declaring_the_system_trust_store_is_refused_naming_it() {
    let scratch = ScratchDir::prepared();
    let password_file = readable_password_file(&scratch);
    let connection = format!(
        "      host: \"127.0.0.1\"\n      port: 5432\n      database: \"dictionary\"\n      \
         user: \"reader\"\n      password_file: \"{}\"\n      \
         transport_mode: \"verified\"\n      transport_anchors: \"system\"\n",
        password_file.display(),
    );
    let err =
        crate::catalog::open_catalog(&catalogs(&connection), None).expect_err("the system trust store is a composition refusal");
    assert!(
        err.contains("`catalogs.dictionary.connection` cannot be dialled as declared")
            && err.contains("`transport_anchors: system`"),
        "the refusal names the catalog and the key: {err}"
    );
}

/// `dialect: oracle` opens the Oracle documentation-schema reader - without dialling, as the
/// Postgres one does not either - while `native_dictionary`, which no reader in this build reads, is
/// refused naming the catalog and the key rather than opened as the documentation-schema reader.
#[test]
#[cfg(feature = "oracle")]
fn an_oracle_catalog_opens_its_reader_and_a_native_dictionary_is_refused() {
    let scratch = ScratchDir::prepared();
    let oracle = oracle_connection(&readable_password_file(&scratch).display().to_string());
    let opened = crate::catalog::open_catalog(&catalogs(&oracle), None).expect("the Oracle reader opens at boot");
    assert!(
        matches!(&opened, crate::catalog::OpenedCatalogs::Rdbms(catalogs) if catalogs.len() == 1),
        "one rdbms catalog opened"
    );

    let postgres = "      host: \"127.0.0.1\"\n      port: 5432\n      database: \"dictionary\"\n      \
         user: \"reader\"\n      password_file: \"/run/secrets/dictionary\"\n      transport_mode: \"plaintext\"\n";
    for connection in [postgres, oracle.as_str()] {
        let native = format!("{connection}    dictionary_source: \"native_dictionary\"\n");
        let err = crate::catalog::open_catalog(&catalogs(&native), None).expect_err("no reader opens this declaration");
        assert!(
            err.contains("`catalogs.dictionary` declares `dictionary_source: native_dictionary`")
                && err.contains("not supported by this build"),
            "the refusal names the catalog and the key, under either dialect: {err}"
        );
    }
}

/// An Oracle catalog's `password_file` is read at boot through the same read a `sources:` Oracle
/// entry uses, and a missing one is refused naming the catalog's own key.
#[test]
#[cfg(feature = "oracle")]
fn an_oracle_catalog_with_an_unreadable_password_file_is_refused_naming_it() {
    let err = crate::catalog::open_catalog(&catalogs(&oracle_connection("/definitely/not/a/real/secret")), None)
        .expect_err("an unreadable password file is a composition refusal");
    assert!(
        err.contains("`catalogs.dictionary.connection.password_file` could not be read"),
        "the refusal names the catalog's key: {err}"
    );
}

/// **A refresh whose Oracle login gets a malformed authentication response keeps the pinned
/// bundle and logs the refusal.** The fake accepts the reader's CONNECT and answers the first
/// authentication message with session data that has none of the verifier fields. The poll runs
/// inside `catch_unwind`, so a poll that does not return fails this cell's first assertion.
#[test]
#[cfg(feature = "oracle")]
fn a_refresh_whose_oracle_login_fails_on_authentication_keeps_the_pinned_bundle() {
    use crate::serve::refresh::{Outcome, Refresher};

    let scratch = ScratchDir::prepared();
    let port = sutura_dev::tns_listener::authenticating().expect("the fake listener binds");
    let connection = oracle_connection(&readable_password_file(&scratch).display().to_string())
        .replace("port: 1521", &format!("port: {port}"));
    let opened = crate::catalog::open_catalog(&catalogs(&connection), None).expect("the Oracle reader opens at boot");
    let crate::catalog::OpenedCatalogs::Rdbms(opened) = opened else {
        panic!("an rdbms declaration opens an rdbms catalog")
    };
    let initial = crate::serve::tests::bundle_over(&[("customers", "warehouse", "dim_customer")]);
    let pinned = initial.digest().clone();
    let refresher = Refresher::new(opened, initial);

    let sink = sutura_runtime::testing::Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let subscriber =
        sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
    let polled = tracing::subscriber::with_default(subscriber, || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| refresher.poll_once()))
    });

    assert_eq!(
        polled.ok(),
        Some(Outcome::Rejected),
        "the poll returns and rejects the re-read"
    );
    assert_eq!(refresher.rotating().current().digest(), &pinned, "the pinned bundle stays");
    let log = sink.contents();
    assert!(
        log.contains("keeping the bundle already pinned") && log.contains("AUTH_PBKDF2_VGEN_COUNT"),
        "{log}"
    );
}

#[cfg(feature = "oracle")]
fn oracle_connection(password_file: &str) -> String {
    format!(
        "      dialect: \"oracle\"\n      host: \"127.0.0.1\"\n      port: 1521\n      service_name: \"FREEPDB1\"\n      \
         user: \"reader\"\n      password_file: \"{password_file}\"\n      transport_mode: \"plaintext\"\n"
    )
}

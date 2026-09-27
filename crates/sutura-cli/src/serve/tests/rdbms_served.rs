//! `catalog.kind: rdbms`, with the `rdbms` feature ON: the refusals this build now REACHES
//! rather than the "not linked" one above. No real endpoint here - each cell is asserting which
//! typed refusal a boot-time misconfiguration earns, not a read.
//!
//! The `rdbms` catalog's own connection block is parsed by `sutura-config` (which checks the
//! shape: a host or a unix socket, a port, a database, a user, an absolute `password_file`, and a
//! `transport_mode` whose plaintext arm is refused for a non-loopback host). What that crate
//! cannot check is whether the FILE it names is readable, or whether the TLS MATERIAL it points
//! at parses - those are composition-root refusals, because `sutura-config` does not depend on
//! the adapter that reads the file and loads the material. These cells reach past the settings
//! parser and into `open_one_rdbms_catalog`'s own typed errors, the same stand
//! `datahub_served`'s endpoint cells make for that kind.
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
/// anchor refusal this cell is proving. The directory is a sibling of this binary's own target
/// temp dir; it leaks across runs rather than being cleaned up, the same way the harness's own
/// `config_path` does, and for the same reason - a `Drop` guard would outlive the test's own
/// assertion and a panic during unwinding would abort.
fn readable_password_file() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sutura-rdbms-served-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
    let path = dir.join("password");
    std::fs::write(&path, "a-test-password\n").expect("the password file is writable");
    path
}

/// The `password_file` a `kind: rdbms` catalog's own connection block names is read at boot
/// rather than at parse - `sutura-config` checks it is an ABSOLUTE path, because a relative one
/// resolves against an arbitrary working directory, and nothing else. A path that is absolute
/// and points at nothing reaches `open_one_rdbms_catalog`, which reads it through the same
/// `connection::config` a `sources:` Postgres entry uses, and refuses naming the catalog and the
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
        !err.contains("--features rdbms"),
        "this build DOES link the feature - the refusal must not send an operator chasing one: {err}"
    );
}

/// A `transport_mode: verified` connection whose `transport_anchors` points at no file is
/// accepted by `sutura-config` (which checks the key is present, not that the material loads)
/// and refused at `open_one_rdbms_catalog`, which loads the bundle through
/// `sutura_exec_postgres::tls::client_config`. The refusal names the catalog and the material.
///
/// The `password_file` here is a REAL, readable file (see [`readable_password_file`]) because
/// `open_one_rdbms_catalog` reads the credential before it builds the TLS config - a missing
/// password file would mask the anchor refusal this cell is proving with the credential refusal
/// the cell above already owns.
#[test]
fn a_rdbms_catalog_with_a_missing_tls_anchor_bundle_is_refused_naming_it() {
    let password_file = readable_password_file();
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

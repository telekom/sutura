//! A `catalog.kind: rdbms` entry through `Settings::load` - the only path its refusals are reached
//! by. Each cell asserts the refusal's own words through the error's source chain, so a refusal
//! that is neutralised, or reached by a different one, reddens exactly its cell.

use std::error::Error as _;

use crate::settings::{Environment, Settings, SettingsError, Sources};

/// A complete rdbms entry's own keys. A cell edits one line with [`edit`].
const ENTRY: &[&str] = &[
    "environment: prod",
    "source_alias: warehouse",
    "live_row_predicate: {column: deleted_at, operator: is_null}",
    "max_dictionary_rows: 1000",
    "max_dictionary_bytes: 1048576",
];

/// A complete `connection:` block: a loopback TCP dial, which may be `plaintext`.
const CONNECTION: &[&str] = &[
    "host: 127.0.0.1",
    "port: 5432",
    "database: dictionary",
    "user: reader",
    "password_file: /run/secrets/dictionary",
    "transport_mode: plaintext",
];

/// `base` with each change applied: `key: value` replaces the line for `key` (or appends it), and
/// `-key` removes it.
fn edit<'a>(base: &[&'a str], changes: &[&'a str]) -> Vec<&'a str> {
    let key = |line: &str| line.split(':').next().unwrap_or_default().trim_start_matches('-').to_owned();
    let mut lines: Vec<&str> = base.to_vec();
    for change in changes {
        lines.retain(|line| key(line) != key(change));
        if !change.starts_with('-') {
            lines.push(change);
        }
    }
    lines
}

fn overlay(kind: &str, entry: &[&str], connection: Option<&[&str]>) -> String {
    let indent = |lines: &[&str], by: &str| lines.iter().flat_map(|line| [by, line, "\n"]).collect::<String>();
    let connection = connection
        .map(|lines| format!("    connection:\n{}", indent(lines, "      ")))
        .unwrap_or_default();
    format!(
        "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
         catalogs:\n  - name: dict\n    kind: {kind}\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n{}{connection}",
        indent(entry, "    ")
    )
}

fn load(overlay: &str) -> Result<Settings, crate::settings::SettingsLoadError> {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
}

/// The refusal an overlay meets, as its whole source chain, after asserting it is a CATALOG one.
fn refusal(overlay: &str) -> String {
    let error = load(overlay).expect_err("the entry is refused");
    let reason = error.reason();
    assert!(matches!(reason, SettingsError::Catalog { .. }), "{reason:?}");
    let mut chain = reason.to_string();
    let mut cause = reason.source();
    while let Some(next) = cause {
        chain.push_str(": ");
        chain.push_str(&next.to_string());
        cause = next.source();
    }
    chain
}

fn rdbms_refusal(entry: &[&str], connection: &[&str]) -> String {
    refusal(&overlay("rdbms", &edit(ENTRY, entry), Some(&edit(CONNECTION, connection))))
}

#[track_caller]
fn assert_names(chain: &str, words: &str) {
    assert!(chain.contains(words), "expected `{words}` in: {chain}");
}

#[test]
fn a_complete_rdbms_catalog_loads() {
    load(&overlay("rdbms", ENTRY, Some(CONNECTION))).expect("every rdbms key written, and each usable");
}

#[test]
fn an_rdbms_catalog_without_a_connection_block_is_refused() {
    let chain = refusal(&overlay("rdbms", ENTRY, None));
    assert_names(&chain, "`connection` is required for `kind: rdbms`");
}

#[test]
fn an_rdbms_catalog_without_an_environment_is_refused() {
    assert_names(
        &rdbms_refusal(&["-environment"], &[]),
        "`environment` is required for `kind: rdbms`",
    );
}

#[test]
fn an_empty_environment_is_refused_as_empty_not_as_missing() {
    let chain = rdbms_refusal(&["environment: \"  \""], &[]);
    assert_names(&chain, "`environment` is empty");
    assert!(!chain.contains("is required"), "{chain}");
}

#[test]
fn an_rdbms_catalog_without_a_source_alias_is_refused() {
    assert_names(
        &rdbms_refusal(&["-source_alias"], &[]),
        "`source_alias` is required for `kind: rdbms`",
    );
}

#[test]
fn a_source_alias_that_is_not_a_name_is_refused() {
    assert_names(
        &rdbms_refusal(&["source_alias: \"bad alias\""], &[]),
        "`source_alias` is not a source name",
    );
}

#[test]
fn a_source_alias_naming_no_declared_source_is_refused() {
    assert_names(
        &rdbms_refusal(&["source_alias: elsewhere"], &[]),
        "`elsewhere`, which names no declared source",
    );
}

#[test]
fn an_rdbms_key_on_another_kind_is_refused() {
    for line in ENTRY {
        let key = line.split(':').next().unwrap_or_default();
        let chain = refusal(&overlay("markdown", &[line], None));
        assert_names(&chain, &format!("`catalogs.dict.{key}` is read only when `kind` is `rdbms`"));
    }
    let chain = refusal(&overlay("markdown", &[], Some(CONNECTION)));
    assert_names(&chain, "`catalogs.dict.connection` is read only when `kind` is `rdbms`");
}

#[test]
fn each_required_connection_key_is_refused_when_absent() {
    for key in ["port", "database", "user", "password_file", "transport_mode"] {
        let removal = format!("-{key}");
        let chain = rdbms_refusal(&[], &[removal.as_str()]);
        assert_names(&chain, &format!("`connection.{key}` is required"));
    }
    let chain = rdbms_refusal(&[], &["-host"]);
    assert_names(&chain, "`connection.host_or_unix_socket` is required");
}

#[test]
fn a_connection_with_both_host_and_unix_socket_is_refused() {
    let chain = rdbms_refusal(&[], &["unix_socket: /run/postgresql"]);
    assert_names(&chain, "declares both `host` and `unix_socket`");
}

#[test]
fn a_connection_host_that_is_a_url_is_refused() {
    let chain = rdbms_refusal(&[], &["host: \"postgres://db.example.com\""]);
    assert_names(&chain, "`connection.host` is not a host sutura can dial");
}

#[test]
fn a_relative_connection_path_is_refused() {
    let chain = rdbms_refusal(&[], &["password_file: secrets/dictionary"]);
    assert_names(&chain, "`connection.password_file` is relative");
    let chain = rdbms_refusal(&[], &["-host", "unix_socket: run/postgresql"]);
    assert_names(&chain, "`connection.unix_socket` is relative");
}

#[test]
fn an_unknown_connection_transport_is_refused() {
    let chain = rdbms_refusal(&[], &["transport_mode: tls"]);
    assert_names(&chain, "`connection` declares a transport sutura cannot use");
}

#[test]
fn a_remote_plaintext_connection_is_refused_as_the_sources_arm_refuses_it() {
    let chain = rdbms_refusal(&[], &["host: db.example.com"]);
    assert_names(
        &chain,
        "host `db.example.com` is not a loopback address and `transport_mode` is `plaintext`",
    );
    // The same remote host with a verified channel is what a deployment writes.
    let secured = edit(
        CONNECTION,
        &[
            "host: db.example.com",
            "transport_mode: verified",
            "transport_anchors: system",
        ],
    );
    load(&overlay("rdbms", ENTRY, Some(&secured))).expect("a remote verified dictionary connection loads");
}

#[test]
fn a_tls_connection_over_a_unix_socket_is_refused() {
    let chain = rdbms_refusal(
        &[],
        &[
            "-host",
            "unix_socket: /run/postgresql",
            "transport_mode: verified",
            "transport_anchors: system",
        ],
    );
    assert_names(&chain, "`unix_socket` is set and `transport_mode` is `verified`");
}

#[test]
fn a_predicate_column_that_is_not_a_column_name_is_refused() {
    let chain = rdbms_refusal(
        &["live_row_predicate: {column: \"deleted_at IS NULL OR 1=1\", operator: is_null}"],
        &[],
    );
    assert_names(&chain, "`live_row_predicate.column` is not a column name");
}

#[test]
fn an_unknown_predicate_operator_is_refused() {
    let chain = rdbms_refusal(&["live_row_predicate: {column: status, operator: like, value: x}"], &[]);
    assert_names(
        &chain,
        "`live_row_predicate.operator` is `like`, not one of: is_null, is_not_null, equals",
    );
}

#[test]
fn an_equals_predicate_without_a_value_is_refused() {
    let chain = rdbms_refusal(&["live_row_predicate: {column: status, operator: equals}"], &[]);
    assert_names(&chain, "is `equals` and no `value` was written");
}

#[test]
fn an_is_null_predicate_with_a_value_is_refused() {
    let chain = rdbms_refusal(
        &["live_row_predicate: {column: deleted_at, operator: is_null, value: x}"],
        &[],
    );
    assert_names(&chain, "is `is_null`, which reads no `value`");
}

#[test]
fn a_zero_dictionary_bound_is_refused() {
    for key in ["max_dictionary_rows", "max_dictionary_bytes"] {
        let zero = format!("{key}: 0");
        let chain = rdbms_refusal(&[zero.as_str()], &[]);
        assert_names(&chain, &format!("`{key}` is 0, which would refuse every read"));
    }
}

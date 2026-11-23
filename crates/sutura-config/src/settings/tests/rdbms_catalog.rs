//! A `catalog.kind: rdbms` entry through `Settings::load` - the only path its refusals are reached
//! by. Each cell asserts the refusal's own words through the error's source chain, so a refusal
//! that is neutralised, or reached by a different one, reddens exactly its cell.

use std::error::Error as _;
use std::path::Path;

use crate::catalog::{CatalogConnection, DictionarySource, InvalidCatalogSettings, RdbmsSettings};
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
/// A `dialect: oracle` `connection:` block over loopback `plaintext`: the refused baseline that
/// `a_loopback_oracle_plaintext_catalog_is_refused` loads as-is and the other cells edit.
const ORACLE_CONNECTION: &[&str] = &[
    "dialect: oracle",
    "host: 127.0.0.1",
    "port: 1521",
    "service_name: FREEPDB1",
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
    let connection = connection.map_or_default(|lines| format!("    connection:\n{}", indent(lines, "      ")));
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
#[track_caller]
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

#[track_caller]
fn rdbms_refusal(entry: &[&str], connection: &[&str]) -> String {
    refusal(&overlay("rdbms", &edit(ENTRY, entry), Some(&edit(CONNECTION, connection))))
}

#[track_caller]
fn oracle_refusal(connection: &[&str]) -> String {
    refusal(&overlay("rdbms", ENTRY, Some(&edit(ORACLE_CONNECTION, connection))))
}

#[track_caller]
fn rdbms(settings: &Settings) -> &RdbmsSettings {
    let catalog = settings.catalogs().each().next().expect("one catalog");
    catalog.rdbms().expect("an rdbms entry carries its rdbms settings")
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
fn an_rdbms_catalog_accepts_only_explicit_open_or_nonempty_restricted_audiences() {
    for declaration in ["audience: open", "audience: {restricted: [finance]}"] {
        load(&overlay("rdbms", &edit(ENTRY, &[declaration]), Some(CONNECTION)))
            .expect("a declared audience is parsed through settings");
    }
    for declaration in [
        "audience: everyone",
        "audience: {restricted: []}",
        "audience: {restricted: ['bad audience']}",
    ] {
        assert_names(&rdbms_refusal(&[declaration], &[]), "audience");
    }
    let markdown = overlay("markdown", &["audience: open"], None);
    assert_names(&refusal(&markdown), "audience");
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
fn a_non_rdbms_catalog_without_either_directory_is_refused_at_load() {
    for (directories, expected) in [
        ("    data_dir: data\n", "catalogs[].dir"),
        ("    dir: catalog\n", "catalogs[].data_dir"),
    ] {
        let overlay = format!("catalogs:\n  - name: model\n    kind: markdown\n{directories}    version: test-1\n");
        let error = load(&overlay).expect_err("a non-rdbms catalog needs both directories");
        match error.reason() {
            SettingsError::Catalog {
                cause: InvalidCatalogSettings::EmptyPath { name },
            } => assert_eq!(*name, expected),
            other => panic!("expected a catalog path refusal for {expected}, got {other:?}"),
        }
    }
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
    assert_names(&chain, "`connection.unix_socket` is not a socket directory");
    assert_names(&chain, "`run/postgresql` is relative");
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

#[test]
fn a_complete_oracle_catalog_loads() {
    let verified = edit(
        ORACLE_CONNECTION,
        &[
            "host: db.example.com",
            "transport_mode: verified",
            "transport_anchors: /etc/sutura/dictionary-ca.pem",
        ],
    );
    let settings = load(&overlay("rdbms", ENTRY, Some(&verified))).expect("the entry loads");
    let rdbms = rdbms(&settings);
    let CatalogConnection::Oracle(connection) = rdbms.connection() else {
        panic!("`dialect: oracle` parses as an Oracle connection: {:?}", rdbms.connection());
    };
    assert_eq!(connection.host().as_str(), "db.example.com");
    assert_eq!(connection.port(), 1521);
    assert_eq!(connection.service_name().as_str(), "FREEPDB1");
    assert_eq!(connection.user(), "reader");
    assert_eq!(connection.password_file(), Path::new("/run/secrets/dictionary"));
    assert_eq!(rdbms.dictionary_source(), DictionarySource::DocumentationSchema);
}

#[test]
fn a_loopback_oracle_plaintext_catalog_is_refused() {
    let plaintext = overlay("rdbms", ENTRY, Some(ORACLE_CONNECTION));
    load(&plaintext).expect_err("an Oracle dictionary requires an encrypted channel, loopback or not");
    assert_names(
        &refusal(&plaintext),
        "`transport_mode` is `plaintext`, and an Oracle dictionary requires verified TLS or acknowledged native encryption",
    );
}

#[test]
fn each_required_oracle_connection_key_is_refused_when_absent() {
    for key in ["host", "port", "service_name", "user", "password_file", "transport_mode"] {
        let removal = format!("-{key}");
        assert_names(
            &oracle_refusal(&[removal.as_str()]),
            &format!("`connection.{key}` is required"),
        );
    }
}

#[test]
fn an_oracle_connection_with_a_tls_transport_is_refused() {
    let chain = oracle_refusal(&["transport_mode: verified", "transport_anchors: system"]);
    assert_names(&chain, "the Oracle driver builds its own TLS");
}

#[test]
fn a_remote_oracle_plaintext_connection_is_refused() {
    let chain = oracle_refusal(&["host: db.example.com"]);
    assert_names(
        &chain,
        "host `db.example.com` is not a loopback address and `transport_mode` is `plaintext`",
    );
}

#[test]
fn an_unknown_connection_dialect_is_refused() {
    let chain = rdbms_refusal(&[], &["dialect: mysql"]);
    assert_names(&chain, "`connection.dialect` is `mysql`");
}

#[test]
fn a_native_dictionary_source_is_parsed_as_written() {
    let entry = edit(
        ENTRY,
        &[
            "dictionary_source: native_dictionary",
            "-environment",
            "-live_row_predicate",
            "include: [{schema: public, table: '*'}]",
        ],
    );
    let settings = load(&overlay("rdbms", &entry, Some(CONNECTION))).expect("the entry loads");
    assert_eq!(rdbms(&settings).dictionary_source(), DictionarySource::NativeDictionary);
}

#[test]
fn an_unknown_dictionary_source_is_refused() {
    let chain = rdbms_refusal(&["dictionary_source: wikipedia"], &[]);
    assert_names(&chain, "`dictionary_source` is `wikipedia`");
}

#[test]
fn a_dictionary_source_on_another_kind_is_refused() {
    let chain = refusal(&overlay("markdown", &["dictionary_source: documentation_schema"], None));
    assert_names(
        &chain,
        "`catalogs.dict.dictionary_source` is read only when `kind` is `rdbms`",
    );
}

#[test]
fn a_postgres_only_key_on_an_oracle_connection_is_refused() {
    for (line, key) in [
        ("database: dictionary", "database"),
        ("unix_socket: /run/postgresql", "unix_socket"),
    ] {
        assert_names(
            &oracle_refusal(&[line]),
            &format!("`connection.{key}` is not read when `dialect` is `oracle`"),
        );
    }
}

#[test]
fn a_postgres_connection_with_service_name_is_refused() {
    let chain = rdbms_refusal(&[], &["service_name: FREEPDB1"]);
    assert_names(&chain, "`connection.service_name` is not read when `dialect` is `postgres`");
}

#[test]
fn an_oracle_connection_with_relative_password_file_is_refused() {
    let chain = oracle_refusal(&["password_file: secrets/dictionary"]);
    assert_names(&chain, "`connection.password_file` is relative");
}

#[test]
fn an_oracle_connection_with_plaintext_and_transport_anchors_is_refused() {
    let chain = oracle_refusal(&["transport_anchors: system"]);
    assert_names(
        &chain,
        "`transport_mode: plaintext` does not read it - a control nothing honours is worse than one nobody wrote",
    );
}

#[test]
fn oracle_unbounded_number_policy_is_explicit_and_refuses_other_representations_or_sources() {
    let connection = edit(
        ORACLE_CONNECTION,
        &["transport_mode: verified", "transport_anchors: /etc/sutura/ca.pem"],
    );
    let files = overlay("rdbms", ENTRY, Some(&connection));
    let oracle = files.replace("kind: files\n    data_dir: /srv/data", "kind: oracle\n    host: db.example.com\n    port: 1521\n    service_name: FREEPDB1\n    user: reader\n    password_file: /run/secrets/source\n    transport_mode: verified\n    transport_anchors: /etc/sutura/ca.pem");
    let explicit = oracle.replace("    posture:", "    unbounded_number: utf8\n    posture:");
    let settings = load(&explicit).expect("an exact text policy loads");
    let source = settings
        .sources()
        .get(&sutura_domain::model::SourceName::parse("warehouse").unwrap())
        .unwrap();
    assert_eq!(source.kind(), crate::sources::SourceKind::Oracle);
    assert_eq!(rdbms(&settings).source_alias().as_str(), "warehouse");
    load(&oracle).expect("absence remains a valid fail-closed source declaration");
    for text in [
        explicit.replace("unbounded_number: utf8", "unbounded_number: decimal"),
        files.replace("    posture:", "    unbounded_number: utf8\n    posture:"),
    ] {
        let error = load(&text).expect_err("unknown representations and other data systems are refused");
        assert!(matches!(error.reason(), SettingsError::Sources { .. }), "{error:?}");
    }
}

#[test]
fn description_references_refuse_unknown_self_and_cyclic_sources() {
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    for catalog in ["missing", "dict"] {
        let text = format!("{base}    descriptions_from: [{{catalog: {catalog}}}]\n");
        load(&text).expect_err("an unknown or self reference must refuse");
    }
    let first = format!("{base}    descriptions_from: [{{catalog: second}}]\n");
    let entry = overlay("rdbms", ENTRY, Some(CONNECTION));
    let second = entry
        .split_once("catalogs:\n")
        .unwrap()
        .1
        .replace("name: dict", "name: second");
    let cyclic = format!("{first}{second}    descriptions_from: [{{catalog: dict}}]\n");
    load(&cyclic).expect_err("cyclic prose references must refuse");
    assert_names(
        &refusal(&cyclic),
        "description catalog reference must name another declared rdbms or datahub catalog without a reference cycle",
    );
}

#[test]
fn rdbms_relationship_declarations_require_exact_complete_typed_shapes_and_bounded_checks() {
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    let declaration = "    relationships:\n      - from: {table: public.events, columns: [tenant, app_id]}\n        to: {table: public.apps, columns: [tenant, id]}\n        cardinality: many_to_one\n        check_target_uniqueness: {max_rows: 100, max_bytes: 10000}\n";
    let settings = load(&format!("{base}{declaration}")).unwrap();
    let entry = settings.catalogs().each().next().unwrap();
    let relation = entry.rdbms().unwrap().relationships().first().unwrap();
    assert_eq!(
        relation
            .from()
            .columns()
            .iter()
            .map(sutura_domain::model::ColumnName::as_str)
            .collect::<Vec<_>>(),
        ["tenant", "app_id"]
    );
    assert_eq!(relation.to().table().to_string(), "public.apps");
    assert_eq!(relation.check().unwrap().max_rows().get(), 100);
    for invalid in [
        declaration.replace("many_to_one", "one_to_many"),
        declaration.replace("columns: [tenant, id]", "columns: [id]"),
        declaration.replace("columns: [tenant, id]", "columns: [id, id]"),
        declaration.replace("public.apps", "apps"),
        declaration.replace("max_rows: 100", "max_rows: 0"),
        declaration.replace("cardinality: many_to_one", "cardinality: many_to_one\n        sql: select 1"),
    ] {
        load(&format!("{base}{invalid}")).expect_err("an invalid relationship declaration refuses");
    }
}

// Parsing's default value (`on_missing_endpoint` absent stays `Refuse`) is not pinned by a
// dedicated cell here: the whole `MissingEndpointPolicy` enum is new in this change, so no base
// run exists for a parsing-level pin to redden against. The security default itself - a reading
// identity gets no relationship it cannot resolve unless it opts into `omit` - IS pinned, by
// `default_policy_still_refuses_a_hidden_endpoint_table` in `sutura-catalog-rdbms`'s
// `relationships/tests.rs`, which compiles unchanged on base and carries its own `Claim-Cell`.
#[test]
fn on_missing_endpoint_accepts_the_omit_opt_in() {
    use crate::catalog::MissingEndpointPolicy;
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    let declaration = "    relationships:\n      - from: {table: public.events, columns: [app_id]}\n        to: {table: public.apps, columns: [id]}\n        cardinality: many_to_one\n        on_missing_endpoint: omit\n";
    let settings = load(&format!("{base}{declaration}")).unwrap();
    let entry = settings.catalogs().each().next().unwrap();
    let relation = entry.rdbms().unwrap().relationships().first().unwrap();
    assert_eq!(relation.on_missing_endpoint(), MissingEndpointPolicy::Omit);
}

#[test]
fn an_unknown_on_missing_endpoint_value_is_refused() {
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    let declaration = "    relationships:\n      - from: {table: public.events, columns: [app_id]}\n        to: {table: public.apps, columns: [id]}\n        cardinality: many_to_one\n        on_missing_endpoint: ignore\n";
    assert_names(
        &refusal(&format!("{base}{declaration}")),
        "relationship on_missing_endpoint must be one of refuse, omit",
    );
}

#[test]
fn rdbms_relationship_declarations_are_capped_at_64_declarations_and_32_pairs() {
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    let declarations = |count: usize, pairs: usize| {
        let columns = (0..pairs).map(|column| format!("c{column}")).collect::<Vec<_>>().join(", ");
        let entries = (0..count)
            .map(|index| {
                format!("      - from: {{table: public.events{index}, columns: [{columns}]}}\n        to: {{table: public.apps, columns: [{columns}]}}\n        cardinality: many_to_one\n")
            })
            .collect::<Vec<_>>()
            .concat();
        format!("{base}    relationships:\n{entries}")
    };
    let accepted = |count, pairs| {
        let settings = load(&declarations(count, pairs)).expect("a declaration set at its cap loads");
        let relationships = settings.catalogs().each().next().unwrap().rdbms().unwrap().relationships();
        assert_eq!((relationships.len(), relationships[0].from().columns().len()), (count, pairs));
    };
    accepted(64, 1);
    accepted(1, 32);
    for (count, pairs) in [(65, 1), (1, 33)] {
        let chain = refusal(&declarations(count, pairs));
        assert!(
            chain.contains("at most 64 many_to_one declarations"),
            "{count}x{pairs}: {chain}"
        );
    }
}

/// A catalog-level `on_missing_endpoint` default (`telekom/sutura#2039` ask 2) is what a
/// relationship declaration with no key of its own follows - the per-relationship key, when
/// present, still overrides it.
#[test]
fn a_catalog_level_on_missing_endpoint_default_applies_to_a_declaration_with_no_key_of_its_own() {
    use crate::catalog::MissingEndpointPolicy;
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    let declaration = "    on_missing_endpoint: omit\n    relationships:\n      - from: {table: public.events, columns: [app_id]}\n        to: {table: public.apps, columns: [id]}\n        cardinality: many_to_one\n";
    let settings = load(&format!("{base}{declaration}")).unwrap();
    let entry = settings.catalogs().each().next().unwrap();
    let rdbms = entry.rdbms().unwrap();
    assert_eq!(rdbms.relationships_on_missing_endpoint(), MissingEndpointPolicy::Omit);
    let relation = rdbms.relationships().first().unwrap();
    assert_eq!(relation.on_missing_endpoint(), MissingEndpointPolicy::Omit);
}

#[test]
fn a_per_relationship_on_missing_endpoint_overrides_the_catalog_level_default() {
    use crate::catalog::MissingEndpointPolicy;
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    let declaration = "    on_missing_endpoint: omit\n    relationships:\n      - from: {table: public.events, columns: [app_id]}\n        to: {table: public.apps, columns: [id]}\n        cardinality: many_to_one\n        on_missing_endpoint: refuse\n";
    let settings = load(&format!("{base}{declaration}")).unwrap();
    let entry = settings.catalogs().each().next().unwrap();
    let relation = entry.rdbms().unwrap().relationships().first().unwrap();
    assert_eq!(relation.on_missing_endpoint(), MissingEndpointPolicy::Refuse);
}

/// Absent, the catalog-level default stays `Refuse` - a catalog that names neither key reads
/// exactly as before `on_missing_endpoint` existed at either scope.
#[test]
fn the_catalog_level_on_missing_endpoint_default_stays_refuse_when_absent() {
    use crate::catalog::MissingEndpointPolicy;
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    let settings = load(&base).unwrap();
    let entry = settings.catalogs().each().next().unwrap();
    assert_eq!(
        entry.rdbms().unwrap().relationships_on_missing_endpoint(),
        MissingEndpointPolicy::Refuse
    );
}

#[test]
fn an_unknown_catalog_level_on_missing_endpoint_value_is_refused() {
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    let declaration = "    on_missing_endpoint: ignore\n";
    assert_names(
        &refusal(&format!("{base}{declaration}")),
        "relationship on_missing_endpoint must be one of refuse, omit",
    );
}

/// `on_missing_endpoint` is an rdbms-only key like `relationships_view`; a non-rdbms catalog
/// setting it must be refused the same way, not silently accepted because the raw shape knows the
/// key and `deny_unknown_fields` cannot see past that (`refuse_rdbms_keys` in `settings/catalogs.rs`).
#[test]
fn an_on_missing_endpoint_key_on_another_kind_is_refused() {
    let chain = refusal(&overlay("markdown", &["on_missing_endpoint: omit"], None));
    assert_names(
        &chain,
        "`catalogs.dict.on_missing_endpoint` is read only when `kind` is `rdbms`",
    );
}

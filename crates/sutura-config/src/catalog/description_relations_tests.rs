//! `#[test]`s for `key.via` and `live_row_predicates` (#2040) - new in this diff, not moved from
//! anywhere. Kept in a file of their own, declared by a `mod` line this diff adds, so a base
//! reconstruction that reverts the line never attempts to compile this file either: the whole
//! module disappears rather than failing to build against the production code it exercises.
//! `devco/causality-no-base-exemptions` carries the "not run at base" exemption each test name
//! needs as a result.

use super::tests::{CONNECTION, ENTRY, assert_description_error, load, overlay};
use crate::catalog::rdbms::InvalidDescriptionSource;

#[test]
fn a_description_key_via_joins_a_declared_parent_table_on_declared_columns() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{via: {{table: inventory.object_docs, on: {{obj_id: id}}, schema: schema_name, table_name: object_name}}, column: field_name}}\n        description: prose\n"
    );
    let settings = load(&text).expect("a key.via mapping loads");
    let entry = settings.catalogs().each().next().unwrap();
    let sources = entry.rdbms().unwrap().descriptions_from();
    let crate::catalog::rdbms::DescriptionSource::Table(mapping) = sources.first().unwrap() else {
        panic!("a mapping")
    };
    let crate::catalog::rdbms::DescriptionKeySource::Via(via) = mapping.key_source() else {
        panic!("a via key source")
    };
    assert_eq!(via.table().to_string(), "inventory.object_docs");
    assert_eq!(via.on().len(), 1);
    assert_eq!(via.schema().as_str(), "schema_name");
    assert_eq!(via.table_name().as_str(), "object_name");
}

#[test]
fn description_key_rejects_both_direct_and_via_or_neither() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let both = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, via: {{table: inventory.object_docs, on: {{obj_id: id}}, schema: schema_name, table_name: object_name}}, column: field_name}}\n        description: prose\n"
    );
    assert_description_error(load(&both), &InvalidDescriptionSource::KeyShape);

    let neither = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{column: field_name}}\n        description: prose\n"
    );
    assert_description_error(load(&neither), &InvalidDescriptionSource::KeyShape);
}

#[test]
fn description_key_via_requires_a_nonempty_on_mapping() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{via: {{table: inventory.object_docs, on: {{}}, schema: schema_name, table_name: object_name}}, column: field_name}}\n        description: prose\n"
    );
    assert_description_error(load(&text), &InvalidDescriptionSource::ViaJoin);
}

/// A `key.via` naming the mapped table itself collapses the join back onto its own starting
/// table, so every predicate would resolve to `PredicateTable::Main` and `PredicateTable::Via`
/// could never be produced - refused rather than silently accepted as a no-op join.
#[test]
fn description_key_via_naming_the_mapped_table_itself_is_refused() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{via: {{table: inventory.column_docs, on: {{obj_id: id}}, schema: schema_name, table_name: object_name}}, column: field_name}}\n        description: prose\n"
    );
    assert_description_error(load(&text), &InvalidDescriptionSource::ViaJoin);
}

#[test]
fn live_row_predicates_is_a_closed_conjunction_naming_main_or_via_tables() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{via: {{table: inventory.object_docs, on: {{obj_id: id}}, schema: schema_name, table_name: object_name}}, column: field_name}}\n        description: prose\n        live_row_predicates:\n          - {{table: inventory.column_docs, column: deleted, operator: equals, value: 'N'}}\n          - {{table: inventory.object_docs, column: retired_at, operator: is_null}}\n"
    );
    let settings = load(&text).expect("a plural predicate list on main and via tables loads");
    let entry = settings.catalogs().each().next().unwrap();
    let sources = entry.rdbms().unwrap().descriptions_from();
    let crate::catalog::rdbms::DescriptionSource::Table(mapping) = sources.first().unwrap() else {
        panic!("a mapping")
    };
    let predicates = mapping.predicates();
    assert_eq!(predicates.len(), 2);
    assert_eq!(predicates[0].table(), crate::catalog::rdbms::PredicateTable::Main);
    assert_eq!(predicates[1].table(), crate::catalog::rdbms::PredicateTable::Via);
}

#[test]
fn live_row_predicates_entries_must_name_the_mapped_table_or_its_via_table() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{via: {{table: inventory.object_docs, on: {{obj_id: id}}, schema: schema_name, table_name: object_name}}, column: field_name}}\n        description: prose\n        live_row_predicates:\n          - {{table: inventory.unrelated, column: deleted, operator: equals, value: 'N'}}\n"
    );
    assert_description_error(load(&text), &InvalidDescriptionSource::PredicateTable);
}

#[test]
fn live_row_predicate_and_live_row_predicates_are_mutually_exclusive() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name}}\n        description: prose\n        live_row_predicate: {{column: deleted, operator: equals, value: 'N'}}\n        live_row_predicates:\n          - {{table: inventory.column_docs, column: retired_at, operator: is_null}}\n"
    );
    assert_description_error(load(&text), &InvalidDescriptionSource::PredicateShape);
}

/// `table` is only meaningful inside `live_row_predicates:`'s list form - on the singular
/// `live_row_predicate:` it has no effect, so `deny_unknown_fields` refuses it rather than
/// silently ignoring it (`RawLiveRowPredicate` carries no `table` field at all).
#[test]
fn live_row_predicate_table_field_is_refused_as_unknown() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name}}\n        description: prose\n        live_row_predicate: {{table: inventory.column_docs, column: deleted, operator: equals, value: 'N'}}\n"
    );
    let error = load(&text).expect_err("table on the singular predicate is an unknown field");
    assert!(
        matches!(error.reason(), crate::settings::SettingsError::Source { .. }),
        "{error:?}"
    );
    let rendered = format!("{:?}", core::error::Error::source(&error));
    assert!(
        rendered.contains("unknown field") && rendered.contains("table"),
        "the refusal names the key: {rendered}"
    );

    let dictionary = format!(
        "{}\n    live_row_predicate: {{table: inventory.column_docs, column: deleted_at, operator: is_null}}\n",
        overlay("rdbms", &["environment: prod", "source_alias: warehouse"], Some(CONNECTION))
    );
    let error = load(&dictionary).expect_err("table on the dictionary-level predicate is an unknown field");
    assert!(
        matches!(error.reason(), crate::settings::SettingsError::Source { .. }),
        "{error:?}"
    );
    let rendered = format!("{:?}", core::error::Error::source(&error));
    assert!(
        rendered.contains("unknown field") && rendered.contains("table"),
        "the refusal names the key: {rendered}"
    );
}

/// `live_row_predicates: []` is a declared-but-empty conjunction, which is never what an operator
/// means - either omit the key for "no predicate", or name at least one.
#[test]
fn live_row_predicates_empty_list_is_refused() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name}}\n        description: prose\n        live_row_predicates: []\n"
    );
    assert_description_error(load(&text), &InvalidDescriptionSource::PredicateListEmpty);
}

/// `key.match` (`telekom/sutura#1975`, `telekom/sutura#2040`): opt-in case-insensitive matching of
/// a mapped row's own identity columns against the main dictionary's physical names. Omitting it,
/// or spelling it `exact`, keeps the default case-preserving comparison.
#[test]
fn description_key_match_defaults_to_case_sensitive_and_accepts_case_insensitive() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let default_text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name}}\n        description: prose\n"
    );
    let settings = load(&default_text).expect("a mapping without key.match loads");
    let entry = settings.catalogs().each().next().unwrap();
    let crate::catalog::rdbms::DescriptionSource::Table(mapping) = entry.rdbms().unwrap().descriptions_from().first().unwrap()
    else {
        panic!("a mapping")
    };
    assert!(!mapping.case_insensitive());

    let exact_text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name, match: exact}}\n        description: prose\n"
    );
    let settings = load(&exact_text).expect("key.match: exact loads");
    let entry = settings.catalogs().each().next().unwrap();
    let crate::catalog::rdbms::DescriptionSource::Table(mapping) = entry.rdbms().unwrap().descriptions_from().first().unwrap()
    else {
        panic!("a mapping")
    };
    assert!(!mapping.case_insensitive());

    let folded_text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name, match: case_insensitive}}\n        description: prose\n"
    );
    let settings = load(&folded_text).expect("key.match: case_insensitive loads");
    let entry = settings.catalogs().each().next().unwrap();
    let crate::catalog::rdbms::DescriptionSource::Table(mapping) = entry.rdbms().unwrap().descriptions_from().first().unwrap()
    else {
        panic!("a mapping")
    };
    assert!(mapping.case_insensitive());
}

#[test]
fn description_key_match_rejects_an_unknown_value() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name, match: insensitive}}\n        description: prose\n"
    );
    assert_description_error(load(&text), &InvalidDescriptionSource::KeyMatch);
}

/// `key.match: case_insensitive` must parse through the ordinary entry point, independent of any
/// accessor this diff adds - a `deny_unknown_fields` raw shape that does not yet know the field
/// would refuse this exact text.
#[test]
fn description_key_match_case_insensitive_parses_through_the_ordinary_entry_point() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name, match: case_insensitive}}\n        description: prose\n"
    );
    assert!(load(&text).is_ok(), "key.match: case_insensitive must be accepted");
}

/// `on_invalid_description` (`telekom/sutura#2069`): omitting it keeps the exact pre-existing
/// `refuse` behaviour, mirroring `on_missing_endpoint`'s own default-preserving shape.
#[test]
fn on_invalid_description_defaults_to_refuse_and_accepts_omit() {
    use crate::catalog::rdbms::InvalidDescriptionPolicy;

    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let default_text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name}}\n        description: prose\n"
    );
    let settings = load(&default_text).expect("a mapping without on_invalid_description loads");
    let entry = settings.catalogs().each().next().unwrap();
    let crate::catalog::rdbms::DescriptionSource::Table(mapping) = entry.rdbms().unwrap().descriptions_from().first().unwrap()
    else {
        panic!("a mapping")
    };
    assert_eq!(mapping.on_invalid_description(), InvalidDescriptionPolicy::Refuse);

    let omit_text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name}}\n        description: prose\n        on_invalid_description: omit\n"
    );
    let settings = load(&omit_text).expect("on_invalid_description: omit loads");
    let entry = settings.catalogs().each().next().unwrap();
    let crate::catalog::rdbms::DescriptionSource::Table(mapping) = entry.rdbms().unwrap().descriptions_from().first().unwrap()
    else {
        panic!("a mapping")
    };
    assert_eq!(mapping.on_invalid_description(), InvalidDescriptionPolicy::Omit);
}

#[test]
fn on_invalid_description_rejects_an_unknown_value() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name}}\n        description: prose\n        on_invalid_description: drop\n"
    );
    assert_description_error(load(&text), &InvalidDescriptionSource::InvalidDescriptionPolicy);
}

/// `on_invalid_description` on a `catalog:` reference is a shape error: that source has no mapping
/// fields of its own, and there is no catalog-wide default to carry it the way `on_missing_endpoint`
/// has one.
#[test]
fn on_invalid_description_on_a_catalog_reference_is_refused_as_shape() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!("{text}    descriptions_from:\n      - catalog: documentation\n        on_invalid_description: omit\n");
    assert_description_error(load(&text), &InvalidDescriptionSource::Shape);
}

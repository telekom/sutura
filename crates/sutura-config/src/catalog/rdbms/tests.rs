use std::num::NonZeroU64;
use std::path::Path;

use super::{
    CatalogConnection, InvalidConnection, InvalidDescriptionPolicy, InvalidDocumentationSchema, InvalidRdbmsCatalog,
    PredicateOperator,
};
use crate::environment::Environment;
use crate::raw::RawCatalogConnection;
use crate::settings::{Settings, Sources};
use sutura_domain::model::SourceName;

#[test]
fn a_remote_plaintext_catalog_connection_is_refused() {
    let raw = RawCatalogConnection {
        dialect: None,
        host: Some(String::from("192.0.2.1")),
        unix_socket: None,
        port: Some(5432),
        service_name: None,
        database: Some(String::from("dictionary")),
        user: Some(String::from("reader")),
        password_file: Some(String::from("/run/secrets/dictionary")),
        transport_mode: Some(String::from("plaintext")),
        native_encryption_acknowledged_because: None,
        legacy_password_verifier: None,
        legacy_password_verifier_acknowledged_because: None,
        transport_anchors: None,
        client_certificate: None,
        client_key: None,
    };
    let name = SourceName::parse("dictionary").expect("a catalog name parses");
    let error = CatalogConnection::parse(&name, &raw).expect_err("remote plaintext is unsafe");
    assert!(matches!(error, InvalidConnection::Channel { .. }), "{error:?}");
}

#[test]
fn a_complete_rdbms_entry_reaches_the_catalog_settings() {
    let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
        catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
        environment: prod\n    source_alias: warehouse\n    max_dictionary_rows: 1000\n    \
        live_row_predicate:\n      column: status\n      operator: equals\n      value: live\n    \
        connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
        password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect("a complete rdbms catalog loads");
    let catalog = settings.catalogs().each().next().expect("one catalog");
    let rdbms = catalog.rdbms().expect("an rdbms entry carries its rdbms settings");
    assert_eq!(rdbms.environment().map(super::CatalogEnvironment::as_str), Some("prod"));
    assert_eq!(rdbms.source_alias().as_str(), "warehouse");
    assert_eq!(rdbms.max_dictionary_rows(), NonZeroU64::new(1000));
    assert_eq!(rdbms.max_dictionary_bytes(), None);
    let predicate = rdbms.live_row_predicate().expect("a declared predicate");
    assert_eq!(predicate.column().as_str(), "status");
    assert_eq!(predicate.operator(), PredicateOperator::Equals);
    assert_eq!(predicate.value(), Some("live"));
    let connection = match rdbms.connection() {
        CatalogConnection::Postgres(p) => p,
        CatalogConnection::Oracle(_) => panic!("the overlay declares no dialect"),
    };
    assert_eq!(connection.database(), "dictionary");
    assert_eq!(connection.user(), "reader");
    assert_eq!(connection.password_file(), Path::new("/run/secrets/dictionary"));
    assert_eq!(connection.transport().describe(), "plaintext");
}

/// `telekom/sutura#2082`: absent `on_invalid_comment` and `sanitize_control_characters`
/// keep their defaults - `Refuse` and `false`, both unchanged from before this change.
#[test]
fn on_invalid_comment_and_sanitize_control_characters_default_undeclared() {
    let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
        catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
        environment: prod\n    source_alias: warehouse\n    \
        connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
        password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect("a complete rdbms catalog loads");
    let catalog = settings.catalogs().each().next().expect("one catalog");
    let rdbms = catalog.rdbms().expect("an rdbms entry carries its rdbms settings");
    assert_eq!(rdbms.on_invalid_comment(), InvalidDescriptionPolicy::Refuse);
    assert!(!rdbms.sanitize_control_characters());
}

/// `telekom/sutura#2082`: `on_invalid_comment: omit` and `sanitize_control_characters: true`
/// both parse through `Settings::load`.
#[test]
fn on_invalid_comment_parses_its_omit_spelling_and_sanitize_control_characters_parses_true() {
    let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
        catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
        environment: prod\n    source_alias: warehouse\n    on_invalid_comment: omit\n    sanitize_control_characters: true\n    \
        connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
        password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect("omit and true both parse");
    let catalog = settings.catalogs().each().next().expect("one catalog");
    let rdbms = catalog.rdbms().expect("an rdbms entry carries its rdbms settings");
    assert_eq!(rdbms.on_invalid_comment(), InvalidDescriptionPolicy::Omit);
    assert!(rdbms.sanitize_control_characters());
}

/// `telekom/sutura#2082`: an unknown `on_invalid_comment` spelling is refused as
/// `InvalidRdbmsCatalog::InvalidCommentPolicy` through `Settings::load`.
#[test]
fn an_unknown_on_invalid_comment_spelling_is_refused() {
    let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
        catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
        environment: prod\n    source_alias: warehouse\n    on_invalid_comment: sometimes\n    \
        connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
        password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
    let result = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay));
    assert!(result.is_err(), "an unknown on_invalid_comment spelling is refused");
    let error = result.unwrap_err();
    let mut source: Option<&dyn std::error::Error> = Some(error.reason());
    let mut found = false;
    while let Some(cause) = source {
        if let Some(catalog) = cause.downcast_ref::<InvalidRdbmsCatalog>() {
            assert!(matches!(catalog, InvalidRdbmsCatalog::InvalidCommentPolicy), "{catalog:?}");
            found = true;
        }
        source = cause.source();
    }
    assert!(
        found,
        "the error chain never reached InvalidRdbmsCatalog::InvalidCommentPolicy"
    );
}

/// An empty `dictionary_schema` is refused as `InvalidDocumentationSchema::Empty` through
/// `Settings::load` - the public path a declaration takes.
#[test]
fn an_empty_dictionary_schema_is_refused() {
    let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
        catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
        environment: prod\n    source_alias: warehouse\n    dictionary_schema: \"\"\n    \
        connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
        password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
    let result = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay));
    assert!(result.is_err(), "an empty dictionary schema is refused");
    let error = result.unwrap_err();
    let mut source: Option<&dyn std::error::Error> = Some(error.reason());
    let mut found = false;
    while let Some(cause) = source {
        if let Some(schema) = cause.downcast_ref::<InvalidRdbmsCatalog>() {
            let InvalidRdbmsCatalog::DictionarySchema { cause } = schema else {
                panic!("expected DictionarySchema, got {schema:?}");
            };
            assert!(matches!(cause, InvalidDocumentationSchema::Empty), "{cause:?}");
            found = true;
        }
        source = cause.source();
    }
    assert!(found, "the error chain never reached InvalidRdbmsCatalog::DictionarySchema");
}

/// A `dictionary_schema` that is not an identifier is refused as
/// `InvalidDocumentationSchema::NotIdentifier` through `Settings::load`.
#[test]
fn a_non_identifier_dictionary_schema_is_refused() {
    let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
        catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
        environment: prod\n    source_alias: warehouse\n    dictionary_schema: bad-name\n    \
        connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
        password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
    let result = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay));
    assert!(result.is_err(), "a non-identifier dictionary schema is refused");
    let error = result.unwrap_err();
    let mut source: Option<&dyn std::error::Error> = Some(error.reason());
    let mut found = false;
    while let Some(cause) = source {
        if let Some(schema) = cause.downcast_ref::<InvalidRdbmsCatalog>() {
            let InvalidRdbmsCatalog::DictionarySchema { cause } = schema else {
                panic!("expected DictionarySchema, got {schema:?}");
            };
            assert!(matches!(cause, InvalidDocumentationSchema::NotIdentifier), "{cause:?}");
            found = true;
        }
        source = cause.source();
    }
    assert!(found, "the error chain never reached InvalidRdbmsCatalog::DictionarySchema");
}

/// A valid `dictionary_schema` containing an underscore is accepted through `Settings::load`,
/// pinning the accept side of the same `[A-Za-z0-9_]` rule the two refusal cells above pin the
/// refusal side of.
#[test]
fn a_valid_dictionary_schema_with_an_underscore_is_accepted() {
    let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
        catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
        environment: prod\n    source_alias: warehouse\n    dictionary_schema: docs_v1\n    \
        connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
        password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect("a dictionary_schema with an underscore is accepted");
    let catalog = settings.catalogs().each().next().expect("one catalog");
    let rdbms = catalog.rdbms().expect("an rdbms entry carries its rdbms settings");
    let schema = rdbms.dictionary_schema().expect("dictionary_schema was declared");
    assert_eq!(schema.as_str(), "docs_v1");
}

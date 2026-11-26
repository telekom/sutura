use crate::environment::Environment;
use crate::settings::{Settings, Sources};

use super::{CommentPolicy, InvalidRdbmsCatalog};

macro_rules! load_rdbms {
    (view, $extra:expr) => {{
        let overlay = format!(
            "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
             catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
             environment: prod\n    source_alias: warehouse\n    {}\
             connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
             password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n",
            $extra
        );
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay))
    }};
    ($extra:expr) => {{
        let overlay = format!(
            "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
             catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
             dictionary_source: native_dictionary\n    source_alias: warehouse\n    {}\
             connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
             password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n",
            $extra
        );
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay))
    }};
}

/// Absent `on_invalid_comment` and `sanitize_control_characters` keep their defaults -
/// `Refuse` and `false`, both unchanged from before this change.
#[test]
fn on_invalid_comment_and_sanitize_control_characters_default_undeclared() {
    let settings = load_rdbms!("").expect("a complete rdbms catalog loads");
    let catalog = settings.catalogs().each().next().expect("one catalog");
    let rdbms = catalog.rdbms().expect("an rdbms entry carries its rdbms settings");
    assert_eq!(rdbms.on_invalid_comment(), CommentPolicy::Refuse);
    assert!(!rdbms.sanitize_control_characters());
}

/// `on_invalid_comment: omit` and `sanitize_control_characters: true` both parse through
/// `Settings::load`, on the native-dictionary selection they are scoped to.
#[test]
fn on_invalid_comment_parses_its_omit_spelling_and_sanitize_control_characters_parses_true() {
    let settings =
        load_rdbms!("on_invalid_comment: omit\n    sanitize_control_characters: true\n    ").expect("omit and true both parse");
    let catalog = settings.catalogs().each().next().expect("one catalog");
    let rdbms = catalog.rdbms().expect("an rdbms entry carries its rdbms settings");
    assert_eq!(rdbms.on_invalid_comment(), CommentPolicy::Omit);
    assert!(rdbms.sanitize_control_characters());
}

/// An unknown `on_invalid_comment` spelling is refused as `InvalidRdbmsCatalog::CommentPolicy`
/// through `Settings::load`.
#[test]
fn an_unknown_on_invalid_comment_spelling_is_refused() {
    let result = load_rdbms!("on_invalid_comment: sometimes\n    ");
    assert!(result.is_err(), "an unknown on_invalid_comment spelling is refused");
    let error = result.unwrap_err();
    let mut source: Option<&dyn std::error::Error> = Some(error.reason());
    let mut found = false;
    while let Some(cause) = source {
        if let Some(catalog) = cause.downcast_ref::<InvalidRdbmsCatalog>() {
            assert!(matches!(catalog, InvalidRdbmsCatalog::CommentPolicy), "{catalog:?}");
            found = true;
        }
        source = cause.source();
    }
    assert!(found, "the error chain never reached InvalidRdbmsCatalog::CommentPolicy");
}

/// `telekom/sutura#2082` ask 1's scope: both keys are native-dictionary-only. A
/// `documentation_schema` (view, the default `dictionary_source`) catalog naming either is
/// refused as `NativeKeysOnView`, the same way `on_unsupported_column` already is.
#[test]
fn on_invalid_comment_and_sanitize_control_characters_on_a_view_catalog_are_refused() {
    for declaration in ["on_invalid_comment: omit\n    ", "sanitize_control_characters: true\n    "] {
        let result = load_rdbms!(view, declaration);
        let error = result.expect_err("a view-sourced catalog refuses a native-only key");
        let mut source: Option<&dyn std::error::Error> = Some(error.reason());
        let mut found = false;
        while let Some(cause) = source {
            if let Some(catalog) = cause.downcast_ref::<InvalidRdbmsCatalog>() {
                assert!(matches!(catalog, InvalidRdbmsCatalog::NativeKeysOnView), "{catalog:?}");
                found = true;
            }
            source = cause.source();
        }
        assert!(found, "the error chain never reached InvalidRdbmsCatalog::NativeKeysOnView");
    }
}

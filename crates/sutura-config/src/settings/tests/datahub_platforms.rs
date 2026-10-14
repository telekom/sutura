//! `catalogs[].platforms` through `Settings::load`: a platform mapped to a declared source loads, and
//! one mapped to a source nobody declared is refused at start-up, naming both. The key is refused on
//! any other catalog kind, which would never read it, and written empty, null, with a null source or
//! under a name that is no platform it is refused rather than read as the default.
//!
//! The cells read the settings only through what the base tree already exports - the refusal's
//! `Debug` and `Display`, the catalog's `Debug` - so they build there and fail on its behaviour.

use crate::catalog::InvalidCatalogSettings;
use crate::settings::{Environment, Settings, SettingsError, SettingsLoadError, Sources};

/// A `datahub` catalog mapping the `postgres` platform to `source`, over one declared `files` source
/// named `billing`.
fn overlay(source: &str) -> String {
    format!(
        "security:\n  identity: single-user\n  single_user_because: a test\n\
         catalogs:\n  - name: catalog\n    kind: datahub\n    dir: catalog\n    data_dir: data\n    version: test-1\n    \
         endpoint: https://datahub.example\n    token_file: /nowhere/token\n    metric_property: m\n    \
         platforms:\n      postgres: {source}\n\
         sources:\n  billing:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n"
    )
}

#[test]
fn a_platform_mapped_to_a_declared_source_loads() {
    let sources = Sources::defaults(Environment::Development).with_overlay(overlay("billing"));
    let settings = Settings::load(&sources).expect("a platform naming a declared source loads");
    let shown = format!("{:?}", settings.catalogs());
    assert!(shown.contains("platforms: {DataHubPlatform(\"postgres\"): "), "{shown}");
    assert!(shown.contains("billing"), "{shown}");
}

/// The refusal `Settings::load` gives `overlay`, with `platforms` written as `platforms_yaml` and the
/// sources `billing` and `ledger` declared.
fn refusal(platforms_yaml: &str) -> SettingsLoadError {
    let overlay = format!(
        "security:\n  identity: single-user\n  single_user_because: a test\n\
         catalogs:\n  - name: catalog\n    kind: datahub\n    dir: catalog\n    data_dir: data\n    version: test-1\n    \
         endpoint: https://datahub.example\n    token_file: /nowhere/token\n    metric_property: m\n    {platforms_yaml}\n\
         sources:\n  billing:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n  ledger:\n    kind: files\n    data_dir: /srv/ledger\n    posture: shared-service-user\n"
    );
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect_err("a platforms key that declares nothing is refused")
}

/// The catalog refusal inside `error`.
fn catalog_cause(error: &SettingsLoadError) -> &InvalidCatalogSettings {
    let SettingsError::Catalog { cause } = error.reason() else {
        panic!("expected a catalog refusal, got {error:?}");
    };
    cause
}

/// The catalog refusal inside `error`, as its variant and fields print.
fn cause(error: &SettingsLoadError) -> String {
    format!("{:?}", catalog_cause(error))
}

#[test]
fn platforms_written_empty_or_null_are_refused_rather_than_read_as_the_default() {
    for written in ["platforms: {}", "platforms:", "platforms: null"] {
        let error = refusal(written);
        assert!(cause(&error).starts_with("EmptyPlatforms"), "{written}: {error:?}");
    }
    let error = refusal("platforms:\n      postgres:");
    let shown = cause(&error);
    assert!(
        shown.starts_with("PlatformWithoutSource") && shown.contains("postgres"),
        "{shown}"
    );
}

#[test]
fn a_platform_name_that_is_empty_or_holds_whitespace_is_refused_naming_it() {
    for key in ["\"\"", "\"  \"", "\"two words\""] {
        let error = refusal(&format!("platforms:\n      {key}: billing"));
        assert!(cause(&error).starts_with("InvalidPlatform"), "{key}: {error:?}");
        let shown = catalog_cause(&error).to_string();
        assert!(shown.contains(key.trim_matches('"')), "{key}: {shown}");
    }
}

#[test]
fn two_platform_keys_equal_once_trimmed_are_refused_naming_both_spellings() {
    let error = refusal("platforms:\n      postgres: billing\n      \"postgres \": ledger");
    assert!(cause(&error).starts_with("DuplicatePlatform"), "{error:?}");
    let shown = catalog_cause(&error).to_string();
    assert!(shown.contains("\"postgres\"") && shown.contains("\"postgres \""), "{shown}");
}

#[test]
fn a_platform_mapped_to_an_undeclared_source_is_refused_naming_both() {
    let sources = Sources::defaults(Environment::Development).with_overlay(overlay("ghost"));
    let error = Settings::load(&sources).expect_err("a platform naming an undeclared source is refused");
    let SettingsError::Catalog { ref cause } = *error.reason() else {
        panic!("expected a catalog refusal, got {error:?}");
    };
    let shown = cause.to_string();
    assert!(shown.contains("postgres") && shown.contains("ghost"), "{shown}");
}

#[test]
fn platforms_on_a_markdown_catalog_is_refused_naming_the_catalog_and_key() {
    let overlay = "security:\n  identity: single-user\n  single_user_because: a test\n\
         catalogs:\n  - name: docs\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n    \
         platforms:\n      postgres: billing\n\
         sources:\n  billing:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n";
    let sources = Sources::defaults(Environment::Development).with_overlay(overlay);
    let error = Settings::load(&sources).expect_err("platforms on a markdown catalog is refused");
    let SettingsError::Catalog { ref cause } = *error.reason() else {
        panic!("expected a catalog refusal, got {error:?}");
    };
    assert!(format!("{cause:?}").starts_with("PlatformsOnOtherKind"), "{cause:?}");
    let shown = cause.to_string();
    assert!(shown.contains("docs") && shown.contains("platforms"), "{shown}");
}

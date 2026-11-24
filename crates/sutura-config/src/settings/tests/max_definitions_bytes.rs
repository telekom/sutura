//! `catalogs[].max_definitions_bytes` end to end through `Settings::load` - `#2071`: a declared
//! value reaches `CatalogSettings::max_definitions_bytes`, an absent one resolves to the 1 MiB
//! default, and a `0` or above-ceiling value is refused naming the catalog.
//!
//! A file of its own, the same reason `refresh_seconds` beside it is one: `settings/tests.rs` is
//! at the thousand-line limit `cargo xtask max-lines` enforces.

use sutura_domain::catalog::InvalidDefinitionsByteBound;
use sutura_domain::model::SourceName;

use crate::catalog::InvalidCatalogSettings;
use crate::settings::{Environment, Settings, SettingsError, Sources};

#[test]
fn a_catalog_without_max_definitions_bytes_resolves_to_the_one_mebibyte_default() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: catalog\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n",
    );
    let settings = Settings::load(&sources).expect("a markdown catalog without a declared bound loads");
    let catalog = settings.catalogs().each().next().expect("the declared catalog is present");
    assert_eq!(catalog.max_definitions_bytes().get(), 1024 * 1024);
}

#[test]
fn a_declared_max_definitions_bytes_reaches_the_catalog_settings() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: catalog\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n    max_definitions_bytes: 300000\n",
    );
    let settings = Settings::load(&sources).expect("a markdown catalog with a declared bound loads");
    let catalog = settings.catalogs().each().next().expect("the declared catalog is present");
    assert_eq!(catalog.max_definitions_bytes().get(), 300_000);
}

#[test]
fn a_max_definitions_bytes_of_zero_in_a_settings_file_is_refused_naming_the_catalog() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: catalog\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n    max_definitions_bytes: 0\n",
    );
    let error = Settings::load(&sources).expect_err("a zero bound is not a bound");
    let SettingsError::Catalog { ref cause } = *error.reason() else {
        panic!("expected a catalog refusal, got {error:?}");
    };
    let name = SourceName::parse("catalog").expect("a test source is a source");
    assert_eq!(
        *cause,
        InvalidCatalogSettings::InvalidDefinitionsBound {
            name,
            cause: InvalidDefinitionsByteBound::Zero,
        }
    );
}

#[test]
fn a_max_definitions_bytes_above_the_ceiling_in_a_settings_file_is_refused_naming_the_catalog() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: catalog\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n    max_definitions_bytes: 17000000\n",
    );
    let error = Settings::load(&sources).expect_err("a bound above the ceiling is not a bound");
    let SettingsError::Catalog { ref cause } = *error.reason() else {
        panic!("expected a catalog refusal, got {error:?}");
    };
    let name = SourceName::parse("catalog").expect("a test source is a source");
    assert_eq!(
        *cause,
        InvalidCatalogSettings::InvalidDefinitionsBound {
            name,
            cause: InvalidDefinitionsByteBound::AboveCeiling {
                bytes: 17_000_000,
                ceiling: 16 * 1024 * 1024,
            },
        }
    );
}

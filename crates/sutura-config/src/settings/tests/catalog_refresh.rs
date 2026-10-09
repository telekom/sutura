//! `catalogs[].refresh_seconds` when a settings file leaves it out: the default every entry gets,
//! and a written value that overrides it.
//!
//! A file of its own because `settings/tests.rs` is at the thousand-line limit `cargo xtask
//! max-lines` enforces. The number is spelled out, so a drift in `defaults.yaml` or in
//! `CatalogSettings::DEFAULT_REFRESH_SECONDS` shows here.

use sutura_domain::model::SourceName;

use crate::catalog::{CatalogSettings, InvalidCatalogSettings};
use crate::settings::{Environment, Settings, SettingsError, Sources};

#[test]
fn a_catalog_without_refresh_seconds_refreshes_every_default_interval() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: catalog\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n",
    );
    let settings = Settings::load(&sources).expect("a markdown catalog without a refresh interval loads");
    let catalog = settings.catalogs().each().next().expect("the declared catalog is present");
    assert_eq!(catalog.refresh_seconds(), Some(900));
}

#[test]
fn a_set_refresh_seconds_overrides_the_default_and_leaves_its_neighbour_on_it() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: first\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n    refresh_seconds: 300\n  - name: second\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n",
    );
    let settings = Settings::load(&sources).expect("two markdown catalogs, one with an interval, load");
    let intervals = settings
        .catalogs()
        .each()
        .map(CatalogSettings::refresh_seconds)
        .collect::<Vec<_>>();
    assert_eq!(intervals, vec![Some(300), Some(900)]);
}

#[test]
fn the_embedded_default_catalog_refreshes_every_900_seconds() {
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    let catalog = settings.catalogs().each().next().expect("the default catalog is present");
    assert_eq!(catalog.refresh_seconds(), Some(900));
}

#[test]
fn a_refresh_seconds_of_zero_in_a_settings_file_is_refused_naming_the_catalog() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: catalog\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n    refresh_seconds: 0\n",
    );
    let error = Settings::load(&sources).expect_err("a zero interval is not an interval");
    let SettingsError::Catalog { ref cause } = *error.reason() else {
        panic!("expected a catalog refusal, got {error:?}");
    };
    let name = SourceName::parse("catalog").expect("a test source is a source");
    assert_eq!(*cause, InvalidCatalogSettings::ZeroRefresh { name });
}

#[test]
fn a_null_refresh_seconds_is_refused_naming_the_key_and_the_file() {
    let dir = super::scratch("refresh-null");
    let base = dir.join("base.yaml");
    std::fs::write(
        &base,
        "catalogs:\n  - name: catalog\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n    refresh_seconds: null\n",
    )
    .expect("a scratch file is writable");
    let loaded = Settings::load(&Sources::defaults(Environment::Development).with_directory(dir.clone()));
    drop(std::fs::remove_dir_all(&dir));
    let rendered = loaded.expect_err("a null interval is not an interval").to_string();
    assert!(
        rendered.contains("refresh_seconds"),
        "the error should name the key: {rendered}"
    );
    assert!(
        rendered.contains(&base.display().to_string()),
        "the error should name the file: {rendered}"
    );
}

//! `catalogs[].refresh_seconds` when a settings file leaves it out: the default every entry gets,
//! and a written value that overrides it.
//!
//! A file of its own because `settings/tests.rs` is at the thousand-line limit `cargo xtask
//! max-lines` enforces. The number is spelled out, so a drift in `defaults.yaml` or in
//! `CatalogSettings::DEFAULT_REFRESH_SECONDS` shows here.

use crate::catalog::CatalogSettings;
use crate::settings::{Environment, Settings, Sources};

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
fn the_embedded_default_catalog_refreshes_on_the_interval_the_type_declares() {
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    let catalog = settings.catalogs().each().next().expect("the default catalog is present");
    assert_eq!(catalog.refresh_seconds(), Some(900));
}

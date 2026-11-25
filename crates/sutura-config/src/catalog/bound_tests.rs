//! `max_definitions_bytes` (`github.com/telekom/sutura#2071`): a new file, not inline in
//! `catalog/tests.rs`, because that file is pre-existing and already has real tests of its own - a
//! pure test addition there is kept at HEAD unconditionally by the causality gate, and these tests
//! call `CatalogSettings::with_max_definitions_bytes`/`max_definitions_bytes`, which do not exist on
//! base. Declared instead from `catalog.rs`, whose own implementation change (adding those methods,
//! the `definitions_bound` field, `InvalidCatalogSettings::InvalidDefinitionsBound`) already makes
//! it `impl_only` under the causality gate's partition - reverted whole at base like any
//! implementation file, which cleanly orphans this file rather than failing to compile against a
//! signature the base reconstruction does not have. Every test here is listed by name in
//! `devco/causality-no-base-exemptions`.
//!
//! The four `Settings::load` round-trip tests below were first drafted as a `settings/tests/`
//! sibling of `refresh_seconds.rs`, declared from `settings/tests.rs`'s `mod` line - but that file
//! is itself pure (no implementation change of its own), so the causality gate kept it at HEAD
//! unconditionally while reconstructing base, where `CatalogSettings::max_definitions_bytes` and
//! `InvalidDefinitionsByteBound` do not exist: a compile failure, not a revert. Moved here instead,
//! where the same `catalog.rs` `impl_only` mechanism applies to them too.

use std::path::PathBuf;

use sutura_domain::catalog::InvalidDefinitionsByteBound;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

use super::{CatalogKind, CatalogSettings, InvalidCatalogSettings};
use crate::environment::Environment;
use crate::settings::{Settings, SettingsError, Sources};

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("v1").expect("a test version is a version")
}

fn name(raw: &str) -> SourceName {
    SourceName::parse(raw).expect("a test source name is a name")
}

fn settings(name_raw: &str) -> CatalogSettings {
    CatalogSettings::parse(
        name(name_raw),
        CatalogKind::Markdown,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version(),
    )
    .expect("a directory and a version are a settings")
}

/// `#2071`: absent resolves to the 1 MiB default, and a declared value round-trips on every kind -
/// the same shape `a_refresh_interval_defaults_to_never_and_round_trips_on_every_kind` holds.
#[test]
fn max_definitions_bytes_defaults_to_one_mebibyte_and_round_trips_on_every_kind() {
    for kind in [CatalogKind::Markdown, CatalogKind::Datahub, CatalogKind::Okf] {
        let base = CatalogSettings::parse(
            name("catalog"),
            kind,
            PathBuf::from("/nowhere/catalog"),
            PathBuf::from("/nowhere/data"),
            version(),
        )
        .expect("a directory and a version are a settings");
        assert_eq!(base.max_definitions_bytes().get(), 1024 * 1024, "{kind:?}");
        let declared = base
            .with_max_definitions_bytes(Some(600_000))
            .expect("a bound inside the ceiling is usable");
        assert_eq!(declared.max_definitions_bytes().get(), 600_000, "{kind:?}");
    }
}

/// A declared `0` is refused - a bound of zero can never load a catalog with anything in it.
#[test]
fn a_zero_definitions_bound_is_refused_naming_the_catalog() {
    let error = settings("catalog")
        .with_max_definitions_bytes(Some(0))
        .expect_err("a zero bound is not a bound");
    assert_eq!(
        error,
        InvalidCatalogSettings::InvalidDefinitionsBound {
            name: name("catalog"),
            cause: sutura_domain::catalog::InvalidDefinitionsByteBound::Zero,
        }
    );
}

/// A declared value above `DEFINITIONS_BYTES_CEILING` (16 MiB) is refused, rather than letting a
/// deployment declare away the only thing still bounding the document an agent's prompt and
/// `describe_catalog` carry (`github.com/telekom/sutura#2071`).
#[test]
fn a_definitions_bound_above_the_ceiling_is_refused_naming_the_catalog() {
    let error = settings("catalog")
        .with_max_definitions_bytes(Some(17 * 1024 * 1024))
        .expect_err("a bound above the ceiling is not a bound");
    assert_eq!(
        error,
        InvalidCatalogSettings::InvalidDefinitionsBound {
            name: name("catalog"),
            cause: sutura_domain::catalog::InvalidDefinitionsByteBound::AboveCeiling {
                bytes: 17 * 1024 * 1024,
                ceiling: 16 * 1024 * 1024,
            },
        }
    );
}

/// `#2071`: a catalog with no declared `max_definitions_bytes` resolves to the 1 MiB default,
/// through the full `Settings::load` path (not just `CatalogSettings::parse` directly).
#[test]
fn a_catalog_without_max_definitions_bytes_resolves_to_the_one_mebibyte_default() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: catalog\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n",
    );
    let settings = Settings::load(&sources).expect("a markdown catalog without a declared bound loads");
    let catalog = settings.catalogs().each().next().expect("the declared catalog is present");
    assert_eq!(catalog.max_definitions_bytes().get(), 1024 * 1024);
}

/// A declared `max_definitions_bytes` reaches the loaded `CatalogSettings`.
#[test]
fn a_declared_max_definitions_bytes_reaches_the_catalog_settings() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: catalog\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n    max_definitions_bytes: 300000\n",
    );
    let settings = Settings::load(&sources).expect("a markdown catalog with a declared bound loads");
    let catalog = settings.catalogs().each().next().expect("the declared catalog is present");
    assert_eq!(catalog.max_definitions_bytes().get(), 300_000);
}

/// A declared `0` is refused at `Settings::load`, naming the catalog - not just at
/// `CatalogSettings::with_max_definitions_bytes` directly.
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

/// A declared value above `DEFINITIONS_BYTES_CEILING` is refused at `Settings::load`, naming the
/// catalog.
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

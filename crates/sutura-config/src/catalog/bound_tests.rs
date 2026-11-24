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

use std::path::PathBuf;

use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

use super::{CatalogKind, CatalogSettings, InvalidCatalogSettings};

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

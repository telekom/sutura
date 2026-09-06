//! Catalog opening: how the declared `catalogs:` become the one bundle this build serves.
//!
//! Split out of `main.rs` (a file kept under the thousand-line limit by this very split) and of
//! `sutura-config` (which cannot see which adapter a binary links). What kind each entry is and
//! whether this build can open it is a property of the composition root, so it lives here.

use std::path::PathBuf;

use sutura_app::assemble;
use sutura_catalog_local::LocalCatalog;
use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog as _};

/// Opens every catalog the settings declare, dispatching each kind exhaustively.
///
/// **`sutura_config::CatalogKind` is the metadata side of `SourceKind`, and this is the same
/// exhaustive no-wildcard match that dispatches a source kind in `crate::open_engine`.** A third
/// kind is therefore a compile error here rather than a refusal that reads the same wherever it is
/// written.
///
/// **Each declared catalog is opened, and the metadata assembler composes them.** The settings can
/// declare several sources and a deployment serves the one composed bundle - this function is the
/// metadata side of the source registry, and [`load`] is where they become one bundle.
pub(crate) fn open_catalog(catalogs: &sutura_config::Catalogs) -> Result<Vec<LocalCatalog>, String> {
    catalogs.each().map(open_one_catalog).collect()
}

/// Loads every opened catalog and composes them into one pinned bundle.
///
/// **The bundle this returns is the bundle `LocalService` validates**, because `main` hands the
/// same opened catalogs to `LocalService::start_composed`, which loads them itself - the two
/// bundles are the same values, which is what "validate what you serve" needs.
pub(crate) fn load(catalogs: &[LocalCatalog]) -> Result<PinnedDefinitions, String> {
    let bundles = catalogs
        .iter()
        .map(|catalog| catalog.load().map_err(|cause| flatten(&cause)))
        .collect::<Result<Vec<_>, String>>()?;
    assemble::assemble(bundles).map_err(|cause| cause.to_string())
}

/// Opens one declared catalog, dispatching its kind exhaustively.
fn open_one_catalog(settings: &sutura_config::CatalogSettings) -> Result<LocalCatalog, String> {
    match settings.kind() {
        sutura_config::CatalogKind::Markdown => Ok(LocalCatalog::new(
            settings.name().clone(),
            PathBuf::from(settings.dir()),
            settings.version().clone(),
        )),
        // NO instruction to rebuild, and that is the point rather than a wording preference:
        // `github.com/telekom/sutura#366`. This crate declares no feature that links the adapter and
        // does not depend on it in any form, so "build with the feature that provides it" named
        // nothing an operator could type. What a binary cannot do is a fact about the binary; the
        // second kind IS reachable, so that is what the sentence offers.
        // `check-feature-remedies` is what keeps the next message of this shape honest.
        sutura_config::CatalogKind::Datahub => Err(format!(
            "`catalog.kind: {}` names a metadata adapter no build of this binary links - write \
             `markdown`, the one catalog kind it can open",
            sutura_config::CatalogKind::Datahub.as_str()
        )),
    }
}

/// The catalog adapter's own error and every cause beneath it, on one line each.
fn flatten(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

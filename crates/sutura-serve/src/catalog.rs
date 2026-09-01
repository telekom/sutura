//! Catalog opening: how the declared `catalogs:` become the one catalog this build serves.
//!
//! Split out of `main.rs` (a file kept under the thousand-line limit by this very split) and of
//! `sutura-config` (which cannot see which adapter a binary links). What kind each entry is and
//! whether this build can open it is a property of the composition root, so it lives here.

use std::path::PathBuf;

use sutura_catalog_local::LocalCatalog;

/// Opens the catalog the settings declare, dispatching the kind exhaustively.
///
/// **`sutura_config::CatalogKind` is the metadata side of `SourceKind`, and this is the same
/// exhaustive no-wildcard match that dispatches a source kind in `crate::open_engine`.** A third
/// kind is therefore a compile error here rather than a refusal that reads the same wherever it is
/// written.
///
/// **This build serves exactly ONE catalog.** The settings can now DECLARE several - that is the
/// metadata assembler's input - but the assembler is `sutura-app`'s and this binary does not link
/// it yet, so a deployment that declared more than one is refused HERE rather than silently serving
/// the first. The refusal is deliberately a sentence rather than a number: it tells an operator the
/// capability exists and this artifact has not adopted it, which is the "one contributor in
/// practice" shape the metadata assembler replaces.
pub(crate) fn open_catalog(catalogs: &sutura_config::Catalogs) -> Result<LocalCatalog, String> {
    match catalogs.count() {
        1 => open_one_catalog(catalogs.each().next().ok_or_else(|| {
            // The count said one; as-a-matter-of-state the iterator below returns it. This is a
            // broken invariant rather than an input, so it joins the count mismatch as a refusal.
            String::from("`catalogs` said one and delivered none")
        })?),
        0 => Err(String::from("no catalog is declared - a deployment serves at least one")),
        _ => Err(String::from(
            "this build serves exactly one catalog; N-catalog composition arrives with the metadata assembler",
        )),
    }
}

/// Opens one declared catalog, dispatching its kind exhaustively.
fn open_one_catalog(settings: &sutura_config::CatalogSettings) -> Result<LocalCatalog, String> {
    match settings.kind() {
        sutura_config::CatalogKind::Markdown => Ok(LocalCatalog::new(PathBuf::from(settings.dir()), settings.version().clone())),
        sutura_config::CatalogKind::Datahub => Err(format!(
            "`catalog.kind: {}` names a metadata adapter this build does not link - build the binary \
             with the feature that provides it, or write `markdown`",
            sutura_config::CatalogKind::Datahub.as_str()
        )),
    }
}

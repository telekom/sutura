//! The one permitted mixed catalog shape: native RDBMS declarations plus Markdown.

use super::{OpenedCatalogs, flatten, open_one_markdown_catalog, open_one_rdbms_catalog};
use sutura_domain::pinned::PinnedDefinitions;

pub(super) type Rdbms = sutura_catalog_rdbms::RdbmsCatalog<sutura_catalog_rdbms::AnyDictionaryReader>;

pub(super) fn open(catalogs: &sutura_config::Catalogs) -> Result<OpenedCatalogs, String> {
    super::refuse_oracle_dictionaries(catalogs, cfg!(feature = "oracle"))?;
    let mut rdbms = Vec::new();
    let mut markdown = Vec::new();
    for settings in catalogs.each() {
        match settings.kind() {
            sutura_config::CatalogKind::Markdown => markdown.push(open_one_markdown_catalog(settings)?),
            sutura_config::CatalogKind::Rdbms if settings.rdbms().is_some_and(|settings| settings.native_selection().is_some()) => {
                rdbms.push(open_one_rdbms_catalog(settings)?);
            }
            _ => return Err(String::from("mixed catalogs require only native RDBMS dictionaries and Markdown")),
        }
    }
    Ok(OpenedCatalogs::RdbmsWithMarkdown { rdbms, markdown })
}

/// All physical allowlists are applied to every contributor before the joined bundle is pinned.
pub(super) fn load(rdbms: &[Rdbms], markdown: &[sutura_catalog_local::LocalCatalog]) -> Result<PinnedDefinitions, String> {
    let mut bundles = Vec::with_capacity(rdbms.len());
    let mut allowlists = Vec::new();
    for catalog in rdbms {
        let (bundle, allowlist) = catalog.load_with_allowlist().map_err(|cause| flatten(&cause))?;
        if let Some(allowlist) = allowlist {
            if allowlists.iter().any(|known: &sutura_catalog_rdbms::NativeAllowlist| known.source() == allowlist.source()) {
                return Err(format!("source {} has more than one native dictionary authority", allowlist.source()));
            }
            allowlists.push(allowlist);
        }
        bundles.push(bundle);
    }
    let unlinked = markdown.iter().map(|catalog| catalog.read_unlinked().map_err(|cause| flatten(&cause))).collect::<Result<Vec<_>, _>>()?;
    for allowlist in &allowlists {
        for model in bundles.iter().flat_map(|bundle| bundle.definitions().models().values())
            .chain(unlinked.iter().flat_map(|input| input.models())) {
            allowlist.check_model(model).map_err(|cause| flatten(&cause))?;
        }
    }
    sutura_app::assemble::assemble_linked(&bundles, &unlinked).map_err(|cause| flatten(&cause))
}

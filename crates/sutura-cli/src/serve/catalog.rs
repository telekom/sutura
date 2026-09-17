//! Catalog opening: how the declared `catalogs:` become the one bundle this build serves.
//!
//! Split out of `main.rs` (a file kept under the thousand-line limit by this very split) and of
//! `sutura-config` (which cannot see which adapter a binary links). What kind each entry is and
//! whether this build can open it is a property of the composition root, so it lives here.
//!
//! **Since issue #202's HTTP reader, a deployment may declare `catalog.kind: datahub` instead of
//! `markdown` - never both in one deployment, and the reason is a type-system one rather than a
//! preference.** `sutura_app::Surface::start_composed` takes `&[C]` for one `C: SemanticCatalog`,
//! and `SemanticCatalog::KIND` and `capabilities()` are per-TYPE associated items with no `&self` -
//! so unlike `OpenedSources` (a closed enum over data-system adapters, each reached through the
//! SAME `Warehouse` trait's instance methods), there is no single Rust type that is faithfully "a
//! markdown catalog OR a datahub catalog": an enum wrapping both could not answer its own `KIND` or
//! `capabilities()` without an instance to match on, which the trait's shape does not allow. What
//! this module does instead is decide, from the declared catalogs' SHARED kind, which of two
//! monomorphic vectors to open - [`crate::serve::catalog::OpenedCatalogs`] carries that choice and refuses a mix by name
//! rather than picking one silently.
//!
//! **State the limit next to the claim: a heterogeneous catalog set - one deployment serving BOTH a
//! markdown and a datahub catalog at once - is an architecture decision not taken here.** Wave one
//! is one catalog kind per deployment; `.agents/skills/sutura/crate-map/SKILL.md` and
//! `docs/adr/0016` both carry this same sentence, so it is read from one place rather than pieced
//! together from a mixed-kind refusal's wording.

use std::path::PathBuf;

use sutura_app::assemble;
use sutura_catalog_local::LocalCatalog;
use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog};

/// Which monomorphic vector of catalogs this build opened - see the module header for why this is
/// not one generic catalog type the way `OpenedSources` is one generic `Warehouse`.
#[derive(Debug)]
pub(crate) enum OpenedCatalogs {
    Markdown(Vec<LocalCatalog>),
    #[cfg(feature = "datahub")]
    Datahub(Vec<sutura_catalog_datahub::DataHubCatalog<sutura_catalog_datahub::http::HttpAspectReader>>),
}

/// Opens every catalog the settings declare.
///
/// Every declared entry must share ONE `sutura_config::CatalogKind` - a deployment naming both a
/// `markdown` and a `datahub` catalog is refused here, for the reason the module header gives; the
/// same shape `crate::open_engine`'s `one_kind` already holds for `sources:`.
///
/// **`outbound` is `security.outbound.transport_anchors`, resolved ONCE by `main` at boot and
/// handed here** so the `datahub` catalog reader verifies its endpoint against the SAME CA set the
/// `bigquery` source wire is verified against - the composition-root decision `main.rs` states. A
/// `markdown` catalog reads files and never dials, so only the `Datahub` arm consumes it; `None`
/// leaves the reader on `ureq`'s compiled-in roots, the behaviour before `security.outbound`.
pub(crate) fn open_catalog(
    catalogs: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::LoadedAnchors>,
) -> Result<OpenedCatalogs, String> {
    let mut kinds = catalogs.each().map(sutura_config::CatalogSettings::kind);
    // `Catalogs::parse` refuses an empty list, so there is always a first kind - the `ok_or_else`
    // below is unreachable in practice and named rather than `unwrap`, which this workspace denies.
    let first = kinds
        .next()
        .ok_or_else(|| String::from("this catalog declares no entries, which Catalogs::parse should already have refused"))?;
    if let Some(mixed) = kinds.find(|kind| *kind != first) {
        return Err(format!(
            "this deployment declares catalogs of more than one kind ({} and {}) - one build serves \
             catalogs of one kind today",
            first.as_str(),
            mixed.as_str()
        ));
    }
    match first {
        sutura_config::CatalogKind::Markdown => Ok(OpenedCatalogs::Markdown(
            catalogs.each().map(open_one_markdown_catalog).collect(),
        )),
        sutura_config::CatalogKind::Datahub => open_datahub_catalogs(catalogs, outbound),
    }
}

/// Loads every opened catalog and composes them into one pinned bundle.
///
/// **The bundle this returns is the bundle `LocalService` validates**, because `main` hands the
/// same opened catalogs to `LocalService::start_composed`, which loads them itself - the two
/// bundles are the same values, which is what "validate what you serve" needs.
pub(crate) fn load(catalogs: &OpenedCatalogs) -> Result<PinnedDefinitions, String> {
    match catalogs {
        OpenedCatalogs::Markdown(catalogs) => load_each(catalogs),
        #[cfg(feature = "datahub")]
        OpenedCatalogs::Datahub(catalogs) => load_each(catalogs),
    }
}

/// One kind's worth of catalogs, loaded and composed - the body `load` used to be, generic now
/// because it runs over either monomorphic vector [`OpenedCatalogs`] carries.
fn load_each<C>(catalogs: &[C]) -> Result<PinnedDefinitions, String>
where
    C: SemanticCatalog,
{
    let bundles = catalogs
        .iter()
        .map(|catalog| catalog.load().map_err(|cause| flatten(&cause)))
        .collect::<Result<Vec<_>, String>>()?;
    assemble::assemble(&bundles).map_err(|cause| flatten(&cause))
}

/// Opens one declared markdown catalog.
fn open_one_markdown_catalog(settings: &sutura_config::CatalogSettings) -> LocalCatalog {
    LocalCatalog::new(
        settings.name().clone(),
        PathBuf::from(settings.dir()),
        settings.version().clone(),
    )
}

/// Opens every declared `datahub` catalog, behind this crate's `datahub` feature.
#[cfg(feature = "datahub")]
fn open_datahub_catalogs(
    catalogs: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::LoadedAnchors>,
) -> Result<OpenedCatalogs, String> {
    catalogs
        .each()
        .map(|settings| open_one_datahub_catalog(settings, outbound))
        .collect::<Result<Vec<_>, String>>()
        .map(OpenedCatalogs::Datahub)
}

/// The refusal for a build that did not link the `DataHub` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body** - `crate::bigquery`'s
/// `open_bigquery` precedent - so the dispatcher above has exactly one call and the compiler decides
/// which of these it reaches. The message names the FEATURE, for the same reason
/// `open_bigquery`'s does: an operator can act on "build with `--features datahub`", and
/// `cargo xtask check-feature-remedies` is what keeps that instruction honest.
#[cfg(not(feature = "datahub"))]
fn open_datahub_catalogs(
    _catalogs: &sutura_config::Catalogs,
    _outbound: Option<&sutura_tls::LoadedAnchors>,
) -> Result<OpenedCatalogs, String> {
    Err(String::from(
        "catalog.kind: datahub names a metadata adapter this binary was not built to link - build \
         sutura-cli with --features datahub, or declare markdown catalogs",
    ))
}

/// Reads a `catalog.kind: datahub` entry's declared token file into a `Secret`, at boot rather than
/// on the first question - the same argument `BigQuery`'s `credential_file` is read for. Trimmed, so a
/// file ending in the newline a text editor or `echo` ordinarily writes still reads as one token.
#[cfg(feature = "datahub")]
fn read_token(settings: &sutura_config::CatalogSettings) -> Result<sutura_domain::identity::Secret, String> {
    let path = settings.token_file().ok_or_else(|| {
        format!(
            "`catalogs.{}.token_file` is required for catalog.kind: datahub",
            settings.name()
        )
    })?;
    let raw = std::fs::read_to_string(path)
        .map_err(|cause| format!("`catalogs.{}.token_file` could not be read: {cause}", settings.name()))?;
    Ok(sutura_domain::identity::Secret::new(raw.trim().to_owned()))
}

/// Opens one declared `datahub` catalog: the reader, bounded and authenticated, wrapped in
/// `sutura_catalog_datahub::DataHubCatalog`.
///
/// **The source mapping is a single fixed alias, `bigquery` - a real limit, not a placeholder.**
/// `docs/adr/0016` decision 7 leaves the platform-to-`sources.<alias>` mapping to the deployment, and
/// `sutura_config::CatalogSettings` carries no such mapping for a `datahub` entry today. A
/// deployment whose models live on a platform other than `bigquery` is not representable by this
/// build's `datahub` composition yet - a real gap, not a placeholder, and named here rather than
/// left for a reader to discover from a refusal at load time.
///
/// **The read bounds are `catalogs.<name>.deadline_seconds`/`max_response_bytes`, each defaulting
/// to the reader's own recommended constant when absent.** `docs/adr/0016`'s 2026-09-14 revision
/// asks for a deadline and a response-size cap that are settings with defaults, not constants -
/// `sutura_catalog_datahub::http::{DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES}` stay
/// constants IN THE READER (PR1's own decision: the adapter owns the range), and this composition
/// step is where an absent settings key resolves to them - the same single-owner split
/// `BytesBilledCeiling::parse` holds for `BigQuery`'s ceiling, read here rather than in
/// `sutura-config`, which does not depend on this adapter crate and so cannot name its constants.
///
/// **The endpoint is parsed here too, for the same single-owner reason.** A review on PR1 found
/// `HttpAspectReader::new` took a raw `String` and dialled a bearer over plaintext to any host;
/// PR1's fix is `sutura_catalog_datahub::http::Endpoint::parse` - `https://` for any host, `http://`
/// only for an IP loopback literal - and `HttpAspectReader::new` now REQUIRES one, so this
/// composition step cannot skip the parse even by accident: there is no other way to reach the
/// constructor. `sutura-config` still owns only "was the key written" (`CatalogSettings::endpoint`
/// stays an unparsed `Option<&str>`, since that crate cannot depend on this adapter's `Endpoint`
/// type); the adapter's own refusal (`InvalidEndpoint`) surfaces here as the settings-layer error a
/// deployment sees.
///
/// **`outbound` closes the loop from `main`'s ONE boot-time resolution of
/// `security.outbound.transport_anchors` to the reader's own `ureq` agent** - the same value, cloned,
/// that the `bigquery` wire folds into its `RootCerts::Specific`. `None` (no declared
/// `security.outbound`) hands the reader `ureq`'s compiled-in roots, the behaviour before #125;
/// the reader itself never reads the bundle a second time.
#[cfg(feature = "datahub")]
fn open_one_datahub_catalog(
    settings: &sutura_config::CatalogSettings,
    outbound: Option<&sutura_tls::LoadedAnchors>,
) -> Result<sutura_catalog_datahub::DataHubCatalog<sutura_catalog_datahub::http::HttpAspectReader>, String> {
    use sutura_catalog_datahub::http::{
        DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpAspectReader, ReadBounds,
    };

    let endpoint_raw = settings.endpoint().ok_or_else(|| {
        format!(
            "`catalogs.{}.endpoint` is required for catalog.kind: datahub",
            settings.name()
        )
    })?;
    let endpoint = Endpoint::parse(endpoint_raw)
        .map_err(|cause| format!("`catalogs.{}.endpoint` is not a usable endpoint: {cause}", settings.name()))?;
    let property = settings
        .metric_property()
        .ok_or_else(|| {
            format!(
                "`catalogs.{}.metric_property` is required for catalog.kind: datahub",
                settings.name()
            )
        })?
        .to_owned();
    let token = read_token(settings)?;
    let deadline_seconds = settings.deadline_seconds().unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    let max_response_bytes = settings.max_response_bytes().unwrap_or(DEFAULT_MAX_RESPONSE_BYTES);
    let bounds = ReadBounds::parse(deadline_seconds, max_response_bytes).map_err(|cause| {
        format!(
            "`catalogs.{}.deadline_seconds`/`max_response_bytes` are not usable read bounds: {cause}",
            settings.name()
        )
    })?;
    let reader = HttpAspectReader::new(endpoint, property, token, bounds, outbound.cloned());
    let mut sources = std::collections::BTreeMap::new();
    drop(sources.insert(String::from("bigquery"), settings.name().clone()));
    Ok(sutura_catalog_datahub::DataHubCatalog::new(
        settings.name().clone(),
        settings.version().clone(),
        sources,
        reader,
    ))
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

#[cfg(test)]
mod tests {
    use super::load_each;
    use sutura_catalog_local::LocalCatalog;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::DefinitionVersion;

    /// A scratch directory of this test's own, cleared on the way in - same shape
    /// `sutura_catalog_local::tests::scratch` uses, since `tempfile` is not a dependency here either.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-cli-catalog-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// `LocalCatalog::capabilities` declares `MetadataCapabilities::everything()` regardless of a
    /// given directory's content, so a directory holding one model and no relationships composes
    /// to a `CompositionError::Unfaithful` naming `DefinitionKind::Relationships` - the kind
    /// `UnfaithfulDeclaration::Unprovided` already carries as a field. Before this fix,
    /// `cause.to_string()` rendered only `CompositionError::Unfaithful`'s own message, which does not
    /// interpolate that field, and the kind was reachable only by walking the `#[source]` chain.
    #[test]
    fn a_composition_refusal_names_the_kind_it_already_carries() {
        let root = scratch("unfaithful");
        std::fs::write(
            root.join("model.md"),
            "---\nkind: model\nname: orders\nsource: local\ntable: fct_order\ncolumns: [amount_cents]\n---\n\
             Orders, one row per order.\n",
        )
        .expect("a document is writable");
        let catalog = LocalCatalog::new(
            SourceName::parse("test").expect("a test name is a name"),
            root.clone(),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        );
        let message = load_each(&[catalog]).expect_err("one model with no relationships is not everything");
        drop(std::fs::remove_dir_all(&root));
        assert!(
            message.contains("relationships"),
            "the flattened message should name the undersupplied kind: {message}"
        );
    }
}

//! Parsing the `catalogs:` list into typed `CatalogSettings`, split out of `settings.rs` at the
//! unexemptable 1000-line cap once the `catalog.kind: datahub` fields landed (origin/main sits at
//! 986 lines without them; with them it is 1005). The function it holds is the only catalog logic
//! the settings module owns - the types it builds live in `crate::catalog`.

use std::path::PathBuf;

use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

use crate::catalog::{CatalogKind, CatalogSettings, Catalogs, InvalidCatalogSettings, RdbmsSettings};
use crate::raw::{RawCatalog, RawSettings};
use crate::sources::SourceRegistry;

use super::SettingsError;

/// Reads every declared catalog into a typed [`Catalogs`], in declaration order.
///
/// `name` and `kind` are required here for the same reason a source's alias is: the contribution
/// manifest keys on the name and the composition root dispatches the kind, so an entry that omits
/// either is a declaration that cannot be opened. `kind` is parsed as a closed set; an absent one
/// was already defaulted by the raw shape. `sources` is the parsed registry an rdbms entry's
/// `source_alias` must name.
pub(super) fn parse_catalogs(raw: &RawSettings, sources: &SourceRegistry) -> Result<Catalogs, SettingsError> {
    let mut entries = Vec::with_capacity(raw.catalogs.len());
    for raw_catalog in &raw.catalogs {
        let name = SourceName::parse(&raw_catalog.name).map_err(|cause| SettingsError::CatalogName {
            written: raw_catalog.name.clone(),
            cause,
        })?;
        let kind = CatalogKind::parse(&raw_catalog.kind).map_err(|cause| SettingsError::CatalogKind {
            catalog: raw_catalog.name.clone(),
            cause,
        })?;
        let version = DefinitionVersion::parse(&raw_catalog.version).map_err(|cause| SettingsError::Version {
            catalog: raw_catalog.name.clone(),
            cause,
        })?;
        let settings = CatalogSettings::parse(
            name,
            kind,
            PathBuf::from(&raw_catalog.dir),
            PathBuf::from(&raw_catalog.data_dir),
            version,
        )
        .map_err(|cause| SettingsError::Catalog { cause })?;
        // `datahub` and `rdbms` add their own fields in a separate step so every other kind never
        // has to carry them, and every kind but `rdbms` refuses an rdbms key it would read past.
        // `CatalogKind::parse` already refused any other word, so this match is exhaustive over
        // what `kind` can be at this point.
        if kind != CatalogKind::Rdbms {
            refuse_rdbms_keys(&settings, raw_catalog)?;
        }
        let settings = match kind {
            CatalogKind::Markdown | CatalogKind::Okf | CatalogKind::Openmetadata => settings,
            CatalogKind::Rdbms => {
                let rdbms =
                    RdbmsSettings::parse(settings.name(), raw_catalog, sources).map_err(|cause| SettingsError::Catalog {
                        cause: InvalidCatalogSettings::Rdbms {
                            name: settings.name().clone(),
                            cause,
                        },
                    })?;
                settings.with_rdbms(rdbms)
            }
            CatalogKind::Datahub => {
                let endpoint = raw_catalog.endpoint.clone().unwrap_or_default();
                let token_file = raw_catalog.token_file.clone().unwrap_or_default();
                let metric_property = raw_catalog.metric_property.clone().unwrap_or_default();
                settings
                    .with_datahub_reader(endpoint, PathBuf::from(token_file), metric_property)
                    .map_err(|cause| SettingsError::Catalog { cause })?
                    // Absent means the reader's own recommended default - neither field is
                    // refused here for being absent, only (downstream, in the composition root)
                    // for being an unusable bound if the deployment WROTE a zero.
                    .with_datahub_bounds(raw_catalog.deadline_seconds, raw_catalog.max_response_bytes)
            }
        };
        // Every kind, not `datahub` alone - `#975`. Applied after the kind-specific step above so
        // a `datahub` entry's own refusal (an unusable endpoint, say) is reported first; a zero
        // interval is refused either way.
        let settings = settings
            .with_refresh_seconds(raw_catalog.refresh_seconds)
            .map_err(|cause| SettingsError::Catalog { cause })?;
        entries.push(settings);
    }
    Catalogs::parse(entries).map_err(|cause| SettingsError::Catalog { cause })
}

/// Refuses an rdbms-only key on a catalog of another kind. The raw shape knows these keys, so
/// `deny_unknown_fields` cannot catch one on the wrong kind - this does.
fn refuse_rdbms_keys(settings: &CatalogSettings, raw: &RawCatalog) -> Result<(), SettingsError> {
    let written = [
        ("environment", raw.environment.is_some()),
        ("live_row_predicate", raw.live_row_predicate.is_some()),
        ("source_alias", raw.source_alias.is_some()),
        ("max_dictionary_rows", raw.max_dictionary_rows.is_some()),
        ("max_dictionary_bytes", raw.max_dictionary_bytes.is_some()),
        ("connection", raw.connection.is_some()),
    ];
    match written.into_iter().find(|&(_, written)| written) {
        Some((key, _)) => Err(SettingsError::Catalog {
            cause: InvalidCatalogSettings::RdbmsKeyOnOtherKind {
                name: settings.name().clone(),
                key,
            },
        }),
        None => Ok(()),
    }
}

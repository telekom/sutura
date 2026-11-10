//! The `foundry` entry's own keys: the ontology SQL endpoint and its deployment token.
//!
//! This is deliberately separate from the metadata catalog's `foundry` block. A catalog declares
//! object types; a source connection executes plans over the same ontology. Keeping the two
//! placements separate lets one source be used with a local or another catalog without borrowing a
//! catalog credential at query time.

use std::path::PathBuf;

use super::entry::dialled_source_keys;
use super::placement::SourcePlacement;
use super::{InvalidSourceRegistry, RawSourceEntry, SourceKind};
use sutura_domain::model::SourceName;

/// Reads a `kind: foundry` entry.
pub(super) fn parse_placement(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<SourcePlacement, InvalidSourceRegistry> {
    super::refuse_foreign_keys(
        alias,
        kind,
        [
            ("data_dir", written(entry.data_dir)),
            ("max_bytes_billed", entry.max_bytes_billed.is_some()),
        ],
    )?;
    super::refuse_foreign_keys(alias, kind, dialled_source_keys(entry, written))?;
    let endpoint = super::required(alias, kind, "endpoint", entry.endpoint)?;
    let ontology = super::required(alias, kind, "ontology", entry.ontology)?;
    let token_file = PathBuf::from(super::required(alias, kind, "token_file", entry.token_file)?);
    if token_file.is_relative() {
        return Err(InvalidSourceRegistry::RelativePath {
            alias: alias.clone(),
            key: "token_file",
            path: token_file,
        });
    }
    Ok(SourcePlacement::Foundry {
        endpoint: String::from(endpoint),
        ontology: String::from(ontology),
        token_file,
    })
}

/// Allows `endpoint` and `token_file` only on the kinds that read them.
pub(super) fn refuse_elsewhere(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<(), InvalidSourceRegistry> {
    super::refuse_foreign_keys(
        alias,
        kind,
        [
            ("ontology", kind != SourceKind::Foundry && written(entry.ontology)),
            (
                "token_file",
                !matches!(kind, SourceKind::Cube | SourceKind::Foundry) && written(entry.token_file),
            ),
            (
                "endpoint",
                !matches!(kind, SourceKind::Cube | SourceKind::Node | SourceKind::Foundry) && written(entry.endpoint),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use crate::settings::{Environment, Settings, Sources};
    use crate::sources::SourcePlacement;
    use sutura_domain::model::SourceName;

    fn overlay(kind: &str) -> String {
        format!(
            "security:\n  identity: single-user\n  single_user_because: one operator\nsources:\n  warehouse:\n    kind: {kind}\n    endpoint: https://foundry.example.com\n    ontology: sales\n    token_file: /run/sutura/foundry-token\n    posture: shared-service-user\n"
        )
    }

    #[test]
    fn foundry_ontology_is_read_only_by_the_foundry_source_kind() {
        let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay("foundry")))
            .expect("the Foundry source is declared");
        let source = settings
            .sources()
            .get(&SourceName::parse("warehouse").expect("the alias parses"))
            .expect("the source exists");
        assert!(matches!(source.placement(), SourcePlacement::Foundry { ontology, .. } if ontology == "sales"));
        for kind in ["cube", "node"] {
            let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay(kind)))
                .expect_err("other endpoint sources must refuse ontology");
            assert!(error.to_string().contains("ontology"), "{kind}: {error}");
        }
    }
}

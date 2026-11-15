//! Oracle's explicitly selected representation of an unbounded NUMBER.

use super::{InvalidSourceRegistry, SourceKind, SourceRegistry};
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::schema::UnboundedNumber;

pub(crate) fn declare(registry: &mut SourceRegistry, written: &str, value: &str) -> Result<(), InvalidSourceRegistry> {
    let alias = SourceName::parse(written).map_err(|cause| InvalidSourceRegistry::Alias {
        written: written.to_owned(),
        cause,
    })?;
    if let Some(source) = registry.by_alias.get_mut(&alias) {
        if source.kind() != SourceKind::Oracle {
            return Err(InvalidSourceRegistry::KeyNotForKind {
                alias,
                kind: source.kind(),
                key: "unbounded_number",
            });
        }
        source.unbounded_number = Some(match value {
            "utf8" => UnboundedNumber::Utf8,
            _ => {
                return Err(InvalidSourceRegistry::UnboundedNumber {
                    alias,
                    value: value.to_owned(),
                });
            }
        });
    }
    Ok(())
}

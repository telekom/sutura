//! Native dictionary selection and text policy declarations.

use super::{DictionarySelection, InvalidRdbmsCatalog, RawCatalog};
use sutura_domain::catalog::{DictionaryInclude, DictionaryPattern};

pub(super) fn native_selection(raw: &RawCatalog) -> Result<DictionarySelection, InvalidRdbmsCatalog> {
    let pattern = |text: &str| DictionaryPattern::parse(text).map_err(|cause| InvalidRdbmsCatalog::Pattern { cause });
    let include = raw
        .include
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|pair| Ok(DictionaryInclude::new(pattern(&pair.schema)?, pattern(&pair.table)?)))
        .collect::<Result<Vec<_>, InvalidRdbmsCatalog>>()?;
    let patterns = |written: &Option<Vec<String>>| {
        written
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|text| pattern(text))
            .collect::<Result<Vec<_>, _>>()
    };
    let selected = DictionarySelection::parse(
        include,
        patterns(&raw.exclude_tables)?,
        patterns(&raw.exclude_columns)?,
        patterns(&raw.exclude_types)?,
    )
    .map_err(|cause| InvalidRdbmsCatalog::Selection { cause })?;
    let rules = raw
        .column_rules
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|rule| {
            let policy = sutura_domain::catalog::TextPolicy::parse(rule.max_text_bytes, rule.redact.clone())
                .map_err(|cause| InvalidRdbmsCatalog::TextPolicy { cause })?;
            Ok((pattern(&rule.column)?, policy))
        })
        .collect::<Result<Vec<_>, InvalidRdbmsCatalog>>()?;
    selected
        .with_text_rules(rules)
        .map_err(|cause| InvalidRdbmsCatalog::Selection { cause })
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawColumnRule {
    pub(crate) column: String,
    #[serde(default)]
    pub(crate) max_text_bytes: Option<sutura_domain::catalog::MaxTextBytes>,
    #[serde(default)]
    pub(crate) redact: Vec<sutura_domain::catalog::Redaction>,
}

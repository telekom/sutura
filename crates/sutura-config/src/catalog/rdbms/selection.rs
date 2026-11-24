//! Native dictionary selection and text policy declarations.

use super::{DictionarySelection, InvalidRdbmsCatalog, RawCatalog};
use sutura_domain::catalog::{DictionaryInclude, DictionaryPattern, EmptyTablePolicy, UnsupportedColumnPolicy};

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
            let policy =
                sutura_domain::catalog::TextPolicy::parse(rule.max_text_bytes, rule.redact.clone(), rule.raw_prefilter.clone())
                    .map_err(|cause| InvalidRdbmsCatalog::TextPolicy { cause })?;
            Ok((pattern(&rule.column)?, policy))
        })
        .collect::<Result<Vec<_>, InvalidRdbmsCatalog>>()?;
    let on_unsupported_column = raw
        .on_unsupported_column
        .as_deref()
        .map(|written| UnsupportedColumnPolicy::parse(written).ok_or(InvalidRdbmsCatalog::UnsupportedColumnPolicy))
        .transpose()?
        .unwrap_or_default();
    let on_empty_table = raw
        .on_empty_table
        .as_deref()
        .map(|written| EmptyTablePolicy::parse(written).ok_or(InvalidRdbmsCatalog::EmptyTablePolicy))
        .transpose()?
        .unwrap_or_default();
    selected
        .with_text_rules(rules)
        .map_err(|cause| InvalidRdbmsCatalog::Selection { cause })
        .map(|selected| {
            selected
                .with_unsupported_column_policy(on_unsupported_column)
                .with_empty_table_policy(on_empty_table)
        })
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawColumnRule {
    pub(crate) column: String,
    #[serde(default)]
    pub(crate) max_text_bytes: Option<sutura_domain::catalog::MaxTextBytes>,
    #[serde(default)]
    pub(crate) redact: Vec<sutura_domain::catalog::Redaction>,
    #[serde(default)]
    pub(crate) raw_prefilter: Option<sutura_domain::catalog::RawPrefilter>,
}

#[cfg(test)]
mod tests;

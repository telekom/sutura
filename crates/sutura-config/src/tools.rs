//! The tool surface's own settings: which of the capabilities beside the certified one this
//! deployment turned on.
//!
//! One tool exists here today - `docs/adr/0013`'s raw SQL tool - and this module is where a second
//! one's own key would arrive, per tool, off by default: there is deliberately no group-wide switch,
//! because a tool this deployment never turns on should never be a line item in an operator's
//! decision about a different one.

use std::collections::BTreeSet;

use sutura_domain::model::SourceName;

/// The source aliases `tools.run_sql.sources` lists: at least one, none twice, in the order written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSqlSources(Vec<SourceName>);

impl RunSqlSources {
    /// The list, or [`InvalidRunSqlSources`] for an empty one or an alias written twice.
    pub fn parse(aliases: Vec<SourceName>) -> Result<Self, InvalidRunSqlSources> {
        if aliases.is_empty() {
            return Err(InvalidRunSqlSources::Empty);
        }
        let mut seen = BTreeSet::new();
        if let Some(alias) = aliases.iter().find(|alias| !seen.insert(*alias)) {
            return Err(InvalidRunSqlSources::Repeated { alias: alias.clone() });
        }
        Ok(Self(aliases))
    }

    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[SourceName] {
        &self.0
    }
}

/// Why `tools.run_sql.sources` is no list of sources.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRunSqlSources {
    #[error(
        "tools.run_sql.sources lists no source. List the source aliases the raw SQL tool may run over, \
         or remove the key"
    )]
    Empty,
    #[error("tools.run_sql.sources lists `{alias}` more than once. List each source once")]
    Repeated { alias: SourceName },
}

/// What this deployment turned on beside the certified tool.
///
/// Which combinations are wrong - an enabled tool with no `tools.run_sql.sources` key,
/// `tools.run_sql.enabled` against `security.identity` - are checked in `Settings::refusals`, and
/// whether a listed alias names an opened source that accepts a raw statement is checked by the
/// composition root, which holds the opened adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsSettings {
    enabled: bool,
    federation_allowed: bool,
    sources: Option<RunSqlSources>,
}

impl ToolsSettings {
    #[inline]
    #[must_use]
    pub const fn new(run_sql_enabled: bool) -> Self {
        Self {
            enabled: run_sql_enabled,
            federation_allowed: false,
            sources: None,
        }
    }

    #[inline]
    #[must_use]
    pub fn with_federation_allowed(self, allowed: bool) -> Self {
        Self {
            federation_allowed: allowed,
            ..self
        }
    }

    #[inline]
    #[must_use]
    pub fn with_sources(self, sources: Option<RunSqlSources>) -> Self {
        Self { sources, ..self }
    }

    /// Whether `docs/adr/0013`'s raw SQL tool is turned on. Off unless an operator wrote
    /// `tools.run_sql.enabled: true`.
    #[inline]
    #[must_use]
    pub const fn run_sql_enabled(&self) -> bool {
        self.enabled
    }

    /// Whether one raw statement may name tables in more than one source. Off unless an operator
    /// wrote `tools.run_sql.federation_allowed: true`, and a build without the `sql` feature refuses
    /// `true` at startup.
    #[inline]
    #[must_use]
    pub const fn run_sql_federation_allowed(&self) -> bool {
        self.federation_allowed
    }

    /// The source aliases `tools.run_sql.sources` lists, in the order written. Empty only where the
    /// key is absent, and an enabled tool without it does not start.
    #[inline]
    #[must_use]
    pub fn run_sql_sources(&self) -> &[SourceName] {
        self.sources.as_ref().map_or_default(RunSqlSources::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::ToolsSettings;

    #[test]
    fn off_unless_an_operator_turns_it_on() {
        assert!(!ToolsSettings::new(false).run_sql_enabled());
        assert!(ToolsSettings::new(true).run_sql_enabled());
    }
}

//! The tool surface's own settings: which of the capabilities beside the certified one this
//! deployment turned on.
//!
//! One tool exists here today - `docs/adr/0013`'s raw SQL tool - and this module is where a second
//! one's own key would arrive, per tool, off by default: there is deliberately no group-wide switch,
//! because a tool this deployment never turns on should never be a line item in an operator's
//! decision about a different one.

use sutura_domain::model::SourceName;

/// What this deployment turned on beside the certified tool.
///
/// Infallible to build, like the other settings groups. Which combinations are wrong - an enabled
/// tool with no listed source, `tools.run_sql.enabled` against `security.identity` - are checked in
/// `Settings::refusals`, and whether a listed alias names an opened source that accepts a raw
/// statement is checked by the composition root, which holds the opened adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsSettings {
    enabled: bool,
    federation_allowed: bool,
    sources: Vec<SourceName>,
}

impl ToolsSettings {
    #[inline]
    #[must_use]
    pub const fn new(run_sql_enabled: bool) -> Self {
        Self {
            enabled: run_sql_enabled,
            federation_allowed: false,
            sources: Vec::new(),
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
    pub fn with_sources(self, sources: Vec<SourceName>) -> Self {
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

    /// The source aliases `tools.run_sql.sources` lists, in the order written. Empty unless an
    /// operator wrote some; an enabled tool with an empty list does not start.
    #[inline]
    #[must_use]
    pub fn run_sql_sources(&self) -> &[SourceName] {
        &self.sources
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

//! The tool surface's own settings: which of the capabilities beside the certified one this
//! deployment turned on.
//!
//! One tool exists here today - `docs/adr/0013`'s raw SQL tool - and this module is where a second
//! one's own key would arrive, per tool, off by default: there is deliberately no group-wide switch,
//! because a tool this deployment never turns on should never be a line item in an operator's
//! decision about a different one.

/// What this deployment turned on beside the certified tool.
///
/// Infallible to build, like the other settings groups: an unset key is `false`, and there is no
/// combination of booleans here that is wrong on its own - `tools.run_sql.enabled` is checked
/// against `security.identity` in `Settings::refusals`, which is a cross-group rule and belongs
/// there rather than in this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolsSettings {
    run_sql_enabled: bool,
    run_sql_federation_allowed: bool,
}

impl ToolsSettings {
    #[inline]
    #[must_use]
    pub const fn new(run_sql_enabled: bool) -> Self {
        Self {
            run_sql_enabled,
            run_sql_federation_allowed: false,
        }
    }

    #[inline]
    #[must_use]
    pub const fn with_federation_allowed(self, allowed: bool) -> Self {
        Self {
            run_sql_federation_allowed: allowed,
            ..self
        }
    }

    /// Whether `docs/adr/0013`'s raw SQL tool is turned on. Off unless an operator wrote
    /// `tools.run_sql.enabled: true`.
    #[inline]
    #[must_use]
    pub const fn run_sql_enabled(self) -> bool {
        self.run_sql_enabled
    }

    /// Whether one raw statement may name tables in more than one source. Off unless an operator
    /// wrote `tools.run_sql.federation_allowed: true`. A build without the `sql` feature refuses
    /// `true` at startup; a build with it accepts `true`, but no federated path is built yet, so the
    /// tool still runs over the one open source and refuses a call while more than one is open.
    #[inline]
    #[must_use]
    pub const fn run_sql_federation_allowed(self) -> bool {
        self.run_sql_federation_allowed
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

//! Whether the generated documentation is served, and why the default differs by environment.
//!
//! The `OpenAPI` document and the browser UI over it are generated from the handlers, so they are
//! always *correct*; the question is whether an unauthenticated caller should be handed a map of
//! the surface. On a laptop the answer is obviously yes - it is how the surface is explored at
//! all. In production it is a decision, and the safer default is the one that has to be turned
//! on rather than the one that has to be remembered.
//!
//! It is a default and not a refusal. Serving an interface description is a legitimate choice for
//! a deployment behind a gateway that already authenticates, and refusing to start over it would
//! be this crate overruling an operator on something that leaks no data. What it does instead is
//! log the decision, at a level that shows up.

/// Whether the generated documentation surface is served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiSettings {
    docs_enabled: bool,
    /// Whether [`Self::docs_enabled`] was written down or derived from the environment.
    ///
    /// The startup log distinguishes them, because `docs enabled` in production is worth a
    /// warning when nobody chose it and worth only a note when somebody did.
    docs_were_explicit: bool,
}

impl ApiSettings {
    #[inline]
    pub const fn new(docs_enabled: bool, docs_were_explicit: bool) -> Self {
        Self {
            docs_enabled,
            docs_were_explicit,
        }
    }

    /// The default for an environment: everywhere but production.
    ///
    /// A total match rather than a comparison, so a fourth environment has to state its own
    /// answer instead of inheriting whichever branch it happens to fall into.
    #[inline]
    pub const fn docs_default_for(environment: crate::Environment) -> bool {
        match environment {
            crate::Environment::Production => false,
            crate::Environment::Development | crate::Environment::Test => true,
        }
    }

    #[inline]
    pub const fn docs_enabled(self) -> bool {
        self.docs_enabled
    }

    #[inline]
    pub const fn docs_were_explicit(self) -> bool {
        self.docs_were_explicit
    }
}

#[cfg(test)]
mod tests {
    use super::ApiSettings;
    use crate::Environment;

    #[test]
    fn documentation_is_off_by_default_in_production_and_on_everywhere_else() {
        assert!(!ApiSettings::docs_default_for(Environment::Production));
        assert!(ApiSettings::docs_default_for(Environment::Development));
        assert!(ApiSettings::docs_default_for(Environment::Test));
    }
}

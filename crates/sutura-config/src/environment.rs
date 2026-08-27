//! Which deployment this process believes it is.
//!
//! One value, read before anything else, that decides three things which must not be decided
//! separately: which configuration file is layered on top of the defaults, whether the log is
//! machine-readable or human-readable, and how strict the startup refusals are. Separate
//! switches for those would let a deployment be production for logging and development for
//! safety, which is exactly the combination nobody would choose on purpose.
//!
//! Deliberately three values and not a free string. A typo in an environment name is otherwise
//! the most expensive kind of configuration bug there is: it silently selects the permissive
//! branch of every decision above, and the log line saying so is in the format nobody is
//! collecting.

/// A deployment kind.
///
/// Ordered from most permissive to least, which is also the order the refusals in
/// [`crate::Settings`] tighten in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub enum Environment {
    /// A developer's machine. Human-readable logs, loopback only, no token required.
    Development,
    /// An automated test. Same posture as development, and named separately so a test can
    /// assert on it without pretending to be a laptop.
    Test,
    /// A deployment serving somebody. Machine-readable logs, and every refusal in
    /// [`crate::Settings::parse`] applies.
    Production,
}

/// The string was not one of the three.
///
/// Carries what it found, because the whole point of the type is that a typo fails loudly, and
/// a failure that does not quote the typo makes the operator guess which of the three spellings
/// they got wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` is not a deployment environment - use one of: {}", Environment::NAMES.join(", "))]
pub struct UnknownEnvironment {
    found: String,
}

impl UnknownEnvironment {
    #[inline]
    pub fn found(&self) -> &str {
        &self.found
    }
}

impl Environment {
    /// Every accepted spelling, in the order the enum declares them.
    ///
    /// One list, used by [`Environment::parse`], by the error message above and by the test that
    /// asserts a round trip. A second list is how an added variant becomes unparseable while
    /// still being documented.
    pub const NAMES: &'static [&'static str] = &["development", "test", "production"];

    /// Reads an environment name.
    ///
    /// Case-insensitive and trimmed, because this arrives from a shell variable and
    /// `SUTURA_ENVIRONMENT=Production ` with a trailing space is not a different deployment.
    /// Nothing else is forgiven: `prod` is not accepted, because an abbreviation somebody has to
    /// guess is a spelling this type exists to remove.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownEnvironment> {
        let raw = raw.as_ref().trim();
        match raw.to_ascii_lowercase().as_str() {
            "development" => Ok(Self::Development),
            "test" => Ok(Self::Test),
            "production" => Ok(Self::Production),
            _ => Err(UnknownEnvironment {
                found: String::from(raw),
            }),
        }
    }

    /// The canonical spelling, which is also the file stem this environment layers.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Production => "production",
        }
    }

    /// Is this the environment the strict refusals apply to?
    ///
    /// A method rather than `== Environment::Production` at each call site: there are five
    /// refusals keyed off it, and a fourth variant added later has to answer this question once.
    #[inline]
    pub const fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

impl TryFrom<String> for Environment {
    type Error = UnknownEnvironment;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for Environment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::Environment;

    #[test]
    fn every_declared_name_parses_back_to_the_variant_that_declared_it() {
        // The list in `NAMES` is what the error message offers an operator, so a name in it that
        // does not parse would advertise a spelling nothing accepts.
        for name in Environment::NAMES {
            let parsed = Environment::parse(name).expect("a declared name parses");
            assert_eq!(parsed.as_str(), *name);
        }
    }

    #[test]
    fn a_name_is_read_case_insensitively_and_trimmed() {
        assert_eq!(Environment::parse(" Production\n"), Ok(Environment::Production));
        assert_eq!(Environment::parse("TEST"), Ok(Environment::Test));
    }

    #[test]
    fn an_abbreviation_is_not_an_environment() {
        // The bug this type exists for. `prod` reads as production to a person and, if it were
        // accepted as anything other than an error, would have to fall through to a default -
        // which is the permissive branch of five separate decisions.
        let error = Environment::parse("prod").expect_err("`prod` is not one of the three");
        assert_eq!(error.found(), "prod");
        assert!(error.to_string().contains("production"), "{error}");
    }

    #[test]
    fn an_empty_environment_is_an_error_and_not_a_default() {
        assert_eq!(Environment::parse("").expect_err("empty is not an environment").found(), "");
        assert_eq!(
            Environment::parse("   ")
                .expect_err("whitespace is not an environment")
                .found(),
            ""
        );
    }

    #[test]
    fn only_production_is_production() {
        assert!(Environment::Production.is_production());
        assert!(!Environment::Development.is_production());
        assert!(!Environment::Test.is_production());
    }
}

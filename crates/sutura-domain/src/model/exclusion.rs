//! Which object types a Foundry deployment leaves out, by API name.
//!
//! One list is read by every place an object type is listed - the catalog load, the source's
//! API-name-to-RID directory and each caller's visibility listing - so all three agree on what
//! exists.

/// One pattern: an exact API name, or a prefix written `<segments>.*`.
///
/// Nothing more - no other wildcard, no regex. A segment is ASCII letters, digits, `_` or `-`, never
/// empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiNamePattern {
    Exact(String),
    /// The prefix including its trailing `.`.
    Under(String),
}

/// Why a pattern was refused. Carries the pattern as written, cut to 80 characters.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{value:?} is not an object-type pattern: write an exact API name, or a prefix ending in `.*`")]
pub struct InvalidApiNamePattern {
    value: String,
}

impl InvalidApiNamePattern {
    /// The refused pattern, as written.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl ApiNamePattern {
    /// # Errors
    ///
    /// [`InvalidApiNamePattern`] for anything but an exact API name or `<segments>.*`.
    pub fn parse(raw: &str) -> Result<Self, InvalidApiNamePattern> {
        let (name, under) = raw.strip_suffix(".*").map_or((raw, false), |prefix| (prefix, true));
        let segments_ok = name.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        });
        if !segments_ok {
            return Err(InvalidApiNamePattern {
                value: raw.chars().filter(|c| !c.is_control()).take(80).collect(),
            });
        }
        Ok(if under {
            Self::Under(format!("{name}."))
        } else {
            Self::Exact(String::from(name))
        })
    }

    #[must_use]
    pub fn matches(&self, api_name: &str) -> bool {
        match self {
            Self::Exact(name) => api_name == name,
            Self::Under(prefix) => api_name.len() > prefix.len() && api_name.starts_with(prefix.as_str()),
        }
    }
}

/// The object types left out. [`Self::default`] is `com.palantir.system.*`, the platform's own
/// types the object-type list serves beside an ontology's; an empty list leaves nothing out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectTypeExclusions(Vec<ApiNamePattern>);

/// The default exclusion, as a settings file would write it.
pub const DEFAULT_EXCLUSION: &str = "com.palantir.system.*";

impl Default for ObjectTypeExclusions {
    fn default() -> Self {
        Self(vec![ApiNamePattern::Under(String::from("com.palantir.system."))])
    }
}

impl ObjectTypeExclusions {
    /// Leaves nothing out.
    #[must_use]
    pub const fn none() -> Self {
        Self(Vec::new())
    }

    /// # Errors
    ///
    /// The first pattern [`ApiNamePattern::parse`] refuses.
    pub fn parse<'a>(raw: impl IntoIterator<Item = &'a str>) -> Result<Self, InvalidApiNamePattern> {
        raw.into_iter().map(ApiNamePattern::parse).collect::<Result<_, _>>().map(Self)
    }

    /// Whether `api_name` is left out.
    #[must_use]
    pub fn excludes(&self, api_name: &str) -> bool {
        self.0.iter().any(|pattern| pattern.matches(api_name))
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiNamePattern, DEFAULT_EXCLUSION, ObjectTypeExclusions};

    #[test]
    fn a_pattern_is_an_exact_name_or_a_dotted_prefix_and_nothing_else() {
        let default = ObjectTypeExclusions::parse([DEFAULT_EXCLUSION]).expect("the default parses");
        assert_eq!(default, ObjectTypeExclusions::default());
        assert!(default.excludes("com.palantir.system.EvolutionModelStudioUnslothFineTuningExperiment"));
        assert!(!default.excludes("com.palantir.system"));
        assert!(!default.excludes("com.palantir.systemic.Thing"));
        assert!(!default.excludes("Aircraft"));

        let exact = ObjectTypeExclusions::parse(["Aircraft", "com.example.Internal"]).expect("exact names parse");
        assert!(exact.excludes("Aircraft"));
        assert!(exact.excludes("com.example.Internal"));
        assert!(!exact.excludes("Aircraft2"));
        assert!(!exact.excludes("com.example.InternalTwo"));

        assert!(!ObjectTypeExclusions::none().excludes("com.palantir.system.Evolution"));

        for refused in [
            "",
            "*",
            ".*",
            "com.*.x",
            "com..x",
            "com.palantir*",
            "com.palantir.system.**",
            "a b",
            "com.^x",
        ] {
            let refusal = ApiNamePattern::parse(refused).expect_err(refused);
            assert_eq!(refusal.value(), refused);
        }
    }
}

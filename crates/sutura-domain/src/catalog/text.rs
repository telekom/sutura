//! Operator-authored text transformations, additional to database permissions.

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTextPolicy {
    #[error("max_text_bytes must be between 32 and 1048576")]
    Bound,
    #[error(
        "redaction requires a nonempty pattern up to 512 bytes and a literal replacement up to 512 bytes; only a leading (?i) flag is supported"
    )]
    Rule,
    #[error("a column supports at most 8 redaction rules")]
    Count,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct MaxTextBytes(u32);

impl TryFrom<u32> for MaxTextBytes {
    type Error = InvalidTextPolicy;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if !(32..=0x0010_0000).contains(&value) {
            return Err(InvalidTextPolicy::Bound);
        }
        Ok(Self(value))
    }
}

impl From<MaxTextBytes> for u32 {
    fn from(value: MaxTextBytes) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, try_from = "RawRedaction")]
pub struct Redaction {
    pattern: String,
    replace: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRedaction {
    pattern: String,
    replace: String,
}

impl TryFrom<RawRedaction> for Redaction {
    type Error = InvalidTextPolicy;
    fn try_from(raw: RawRedaction) -> Result<Self, Self::Error> {
        let pattern = raw.pattern.strip_prefix("(?i)").unwrap_or(&raw.pattern);
        if pattern.is_empty()
            || raw.pattern.len() > 512
            || raw.replace.len() > 512
            || pattern.contains("(?")
            || raw.replace.contains(['$', '\\'])
        {
            return Err(InvalidTextPolicy::Rule);
        }
        Ok(Self {
            pattern: raw.pattern,
            replace: raw.replace,
        })
    }
}

impl Redaction {
    pub fn pattern(&self) -> &str {
        &self.pattern
    }
    pub fn replacement(&self) -> &str {
        &self.replace
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, try_from = "RawTextPolicy")]
pub struct TextPolicy {
    max_text_bytes: Option<MaxTextBytes>,
    redact: Vec<Redaction>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTextPolicy {
    #[serde(default)]
    max_text_bytes: Option<MaxTextBytes>,
    #[serde(default)]
    redact: Vec<Redaction>,
}

impl TryFrom<RawTextPolicy> for TextPolicy {
    type Error = InvalidTextPolicy;
    fn try_from(raw: RawTextPolicy) -> Result<Self, Self::Error> {
        if raw.redact.len() > 8 {
            return Err(InvalidTextPolicy::Count);
        }
        Ok(Self {
            max_text_bytes: raw.max_text_bytes,
            redact: raw.redact,
        })
    }
}

impl TextPolicy {
    pub fn parse(max_text_bytes: Option<MaxTextBytes>, redact: Vec<Redaction>) -> Result<Self, InvalidTextPolicy> {
        Self::try_from(RawTextPolicy { max_text_bytes, redact })
    }
    pub const fn empty() -> Self {
        Self {
            max_text_bytes: None,
            redact: Vec::new(),
        }
    }
    pub const fn is_empty(&self) -> bool {
        self.max_text_bytes.is_none() && self.redact.is_empty()
    }
    pub const fn max_text_bytes(&self) -> Option<MaxTextBytes> {
        self.max_text_bytes
    }
    pub fn redactions(&self) -> &[Redaction] {
        &self.redact
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidTextPolicy, MaxTextBytes, TextPolicy};

    #[test]
    fn text_policy_refuses_invalid_bounds_rules_and_capture_replacements() {
        for bound in [0, 31, 0x0010_0001] {
            assert_eq!(MaxTextBytes::try_from(bound), Err(InvalidTextPolicy::Bound));
        }
        for rule in [
            serde_json::json!({"pattern": "", "replace": "masked"}),
            serde_json::json!({"pattern": "(?=secret)", "replace": "masked"}),
            serde_json::json!({"pattern": "secret", "replace": "$1"}),
            serde_json::json!({"pattern": "secret", "replace": "\\1"}),
        ] {
            serde_json::from_value::<TextPolicy>(serde_json::json!({"redact": [rule]})).expect_err("an invalid rule");
        }
        let rule = serde_json::json!({"pattern": "(?i)secret", "replace": "masked"});
        serde_json::from_value::<TextPolicy>(serde_json::json!({"redact": vec![rule; 9]})).expect_err("too many rules");
    }
}

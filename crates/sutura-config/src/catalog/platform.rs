//! The `DataHub` platform name a `catalogs[].platforms` key is.

/// A `DataHub` data platform's name as `platforms:` keys it: what a dataset's platform URN ends in.
///
/// Refuses only shapes that cannot be one - empty, or any whitespace. Whether `DataHub` knows the
/// platform is not this type's question: an unmapped platform is refused where the models load.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DataHubPlatform(String);

/// Why a written `platforms:` key cannot be a `DataHub` platform name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDataHubPlatform {
    /// Nothing was written, or only whitespace was.
    #[error("it is empty")]
    Empty,
    /// A platform name is one token; a space inside it names no platform.
    #[error("it contains whitespace, which cannot be part of a platform name")]
    Whitespace,
}

impl DataHubPlatform {
    /// Parses a written platform name, trimmed.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidDataHubPlatform> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidDataHubPlatform::Empty);
        }
        if trimmed.contains(char::is_whitespace) {
            return Err(InvalidDataHubPlatform::Whitespace);
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The name as `DataHub` spells it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for DataHubPlatform {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

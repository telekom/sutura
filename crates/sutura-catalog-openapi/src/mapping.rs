//! Name-constructor signatures for the later implementation. Both refuse every input here.

const MAX_NAME_BYTES: usize = 128;
const MAX_FALLBACKS: usize = 4;
const MAX_SEGMENT_BYTES: usize = 255;

/// Why a mapping value is not usable. Rendered inside the mapping's parse error, which names where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidMappingValue {
    #[error("a pointer is empty or starts with `/`")]
    PointerStart,
    #[error("a pointer escapes `~` only as `~0` or `~1`")]
    PointerEscape,
    #[error("a description names between one and {MAX_FALLBACKS} pointers")]
    FallbackCount,
    #[error("a name is 1 to {MAX_NAME_BYTES} of `A-Z a-z 0-9 _ . -`")]
    Name,
    #[error("a path parameter value is 1 to {MAX_SEGMENT_BYTES} of `A-Z a-z 0-9 _ . ~ -`, and not `.` or `..`")]
    Segment,
    #[error("a value is longer than {limit} bytes")]
    TooLong { limit: usize },
}

/// An `operationId` as the document spells it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct OperationId(String);

impl OperationId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for OperationId {
    type Error = InvalidMappingValue;

    fn try_from(_raw: String) -> Result<Self, Self::Error> {
        Err(InvalidMappingValue::Name)
    }
}

impl core::fmt::Display for OperationId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A parameter name as the document spells it. Its grammar needs no escaping in a path template or
/// a query string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct ParameterName(String);

impl ParameterName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ParameterName {
    type Error = InvalidMappingValue;

    fn try_from(_raw: String) -> Result<Self, Self::Error> {
        Err(InvalidMappingValue::Name)
    }
}

impl core::fmt::Display for ParameterName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

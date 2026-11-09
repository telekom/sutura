//! The mapping file: which `GET` operations answer for the catalog, and where in each answer a
//! name, a description or a column sits.
//!
//! A closed vocabulary with `deny_unknown_fields` at every depth. Its only expression is an RFC 6901
//! JSON Pointer ([`Pointer`]): no templates, wildcards or path languages, so a mapping can name a
//! place in an answer and nothing else. Every name it gives the document (an operation, a parameter)
//! and every value it puts into a request path is a parsed newtype.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// The most bytes one pointer may hold.
const MAX_POINTER_BYTES: usize = 256;
/// The most fallbacks one description may name.
const MAX_FALLBACKS: usize = 4;
/// The most bytes an operation id or a parameter name may hold.
const MAX_NAME_BYTES: usize = 128;
/// The most bytes a fixed path parameter value may hold.
const MAX_SEGMENT_BYTES: usize = 255;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mapping {
    /// The fixed path parameters, by name.
    #[serde(default)]
    pub(crate) parameters: BTreeMap<ParameterName, PathSegment>,
    pub(crate) models: Models,
    #[serde(default)]
    pub(crate) relationships: Option<Relationships>,
}

/// The operation that lists the models, and where each model's parts sit in one listed entry.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Models {
    pub(crate) operation: OperationId,
    /// The list of entries in one answer.
    pub(crate) items: Pointer,
    #[serde(default)]
    pub(crate) paging: Option<Paging>,
    pub(crate) name: Pointer,
    pub(crate) description: Fallbacks,
    /// A string or a list of strings naming the key columns.
    #[serde(default)]
    pub(crate) primary_key: Option<Pointer>,
    pub(crate) columns: Columns,
}

/// Where a model's columns sit: an object keyed by column name when `name` is absent, else a list
/// whose entries carry the name at `name`.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Columns {
    pub(crate) at: Pointer,
    #[serde(default)]
    pub(crate) name: Option<Pointer>,
    #[serde(default, rename = "type")]
    pub(crate) data_type: Option<Pointer>,
    #[serde(default)]
    pub(crate) description: Option<Fallbacks>,
}

/// The operation that lists one model's outgoing links, asked once per model.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Relationships {
    pub(crate) operation: OperationId,
    /// The path parameter each model's own name is put into.
    pub(crate) model_parameter: ParameterName,
    pub(crate) items: Pointer,
    #[serde(default)]
    pub(crate) paging: Option<Paging>,
    pub(crate) name: Pointer,
    /// The model the link points at. The join is onto that model's single-column primary key.
    pub(crate) target: Pointer,
    /// The column on this side. An entry where it resolves to nothing is a link this vocabulary
    /// does not carry.
    pub(crate) foreign_key: Pointer,
}

/// Cursor paging: the answer carries the next cursor at `next`, and the next request sends it back
/// as the query parameter `cursor`. The only paging style; a "follow this URL" style is not in the
/// vocabulary, so no answer can name a URL to dial.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Paging {
    pub(crate) cursor: ParameterName,
    pub(crate) next: Pointer,
    #[serde(default)]
    pub(crate) size: Option<PageSize>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageSize {
    pub(crate) parameter: ParameterName,
    pub(crate) value: NonZeroU32,
}

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

/// An RFC 6901 JSON Pointer. `""` is the whole answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Pointer(String);

impl Pointer {
    fn parse(raw: String) -> Result<Self, InvalidMappingValue> {
        if raw.len() > MAX_POINTER_BYTES {
            return Err(InvalidMappingValue::TooLong {
                limit: MAX_POINTER_BYTES,
            });
        }
        if !(raw.is_empty() || raw.starts_with('/')) {
            return Err(InvalidMappingValue::PointerStart);
        }
        let mut escapes = raw.split('~').skip(1);
        if escapes.any(|after| !(after.starts_with('0') || after.starts_with('1'))) {
            return Err(InvalidMappingValue::PointerEscape);
        }
        Ok(Self(raw))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The reference tokens, unescaped.
    pub(crate) fn tokens(&self) -> impl Iterator<Item = String> + '_ {
        self.0
            .split('/')
            .skip(1)
            .map(|token| token.replace("~1", "/").replace("~0", "~"))
    }

    pub(crate) fn find<'value>(&self, value: &'value serde_json::Value) -> Option<&'value serde_json::Value> {
        value.pointer(&self.0).filter(|found| !found.is_null())
    }

    pub(crate) fn take(&self, value: &mut serde_json::Value) -> Option<serde_json::Value> {
        value
            .pointer_mut(&self.0)
            .map(serde_json::Value::take)
            .filter(|found| !found.is_null())
    }
}

impl TryFrom<String> for Pointer {
    type Error = InvalidMappingValue;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

/// One to [`MAX_FALLBACKS`] pointers, read in order; the first that holds non-empty text wins.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "Vec<Pointer>")]
pub(crate) struct Fallbacks(Vec<Pointer>);

impl Fallbacks {
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, Pointer> {
        self.0.iter()
    }

    /// The first non-empty text the pointers hold, trimmed.
    pub(crate) fn first_text<'value>(&self, value: &'value serde_json::Value) -> Option<&'value str> {
        self.0
            .iter()
            .filter_map(|pointer| pointer.find(value).and_then(serde_json::Value::as_str))
            .map(str::trim)
            .find(|text| !text.is_empty())
    }
}

impl TryFrom<Vec<Pointer>> for Fallbacks {
    type Error = InvalidMappingValue;

    fn try_from(pointers: Vec<Pointer>) -> Result<Self, Self::Error> {
        if pointers.is_empty() || pointers.len() > MAX_FALLBACKS {
            return Err(InvalidMappingValue::FallbackCount);
        }
        Ok(Self(pointers))
    }
}

const fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
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

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        parse_name(&raw)?;
        Ok(Self(raw))
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

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        parse_name(&raw)?;
        Ok(Self(raw))
    }
}

impl core::fmt::Display for ParameterName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

fn parse_name(raw: &str) -> Result<(), InvalidMappingValue> {
    if raw.is_empty() || raw.len() > MAX_NAME_BYTES || !raw.bytes().all(is_name_byte) {
        return Err(InvalidMappingValue::Name);
    }
    Ok(())
}

/// A fixed path parameter value: one path segment of RFC 3986 unreserved characters, never `.` or
/// `..`, so it cannot climb, add a segment, start a query or change the origin.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct PathSegment(String);

impl PathSegment {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PathSegment {
    type Error = InvalidMappingValue;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        let unreserved = raw.bytes().all(|byte| is_name_byte(byte) || byte == b'~');
        if raw.is_empty() || raw.len() > MAX_SEGMENT_BYTES || !unreserved || raw == "." || raw == ".." {
            return Err(InvalidMappingValue::Segment);
        }
        Ok(Self(raw))
    }
}

//! The alternative to `transit_audience` for a gateway that binds the provider by request path.
//!
//! Some gateways route every provider behind one issuer and sign one assertion shape for all of
//! them, with no `aud` claim a provider can ask to have added. What binds the assertion to *this*
//! provider instead is the `requestPath` claim: the gateway puts the route it dispatched on in the
//! assertion, and that route is this provider's own base path or lies under it - the segment the
//! gateway routes on and a consumer cannot choose. [`RequestPathPrefix`] is that base path, always
//! stored with both a leading and a trailing `/` so a comparison can never be fooled by a sibling
//! route sharing a prefix of characters: `/hub/app/v1/` does not match `/hub/app/v1evil/...`.

use crate::inbound::InvalidInboundValue;
use crate::inbound::primitive::MAX_LENGTH;

const SEGMENT_CHARACTERS: &str = "letters, digits, and any of - . _ ~, with no `.` or `..` segment";

/// An absolute path, stored with a leading and a trailing `/`, that a `requestPath` claim must start
/// with or equal without that trailing `/`.
///
/// **Why the trailing `/` is part of the type and not a comparison detail.** Without it, the prefix
/// `/hub/app/v1` would also match `/hub/app/v1evil/mcp` - a sibling route the gateway dispatches to a
/// different provider, whose name merely extends ours as a string. With the slash stored,
/// [`Self::matches`] is a `starts_with` on it plus one exact comparison for the bare path, and the
/// boundary is never a detail a caller of this type could get wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestPathPrefix(String);

impl RequestPathPrefix {
    /// The key this value is written under.
    pub const KEY: &'static str = "security.inbound.transit_request_path_prefix";

    /// Reads a written prefix: an absolute path, with a leading and a trailing `/`.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        let key = Self::KEY;
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidInboundValue::Empty { key });
        }
        let found = trimmed.chars().count();
        if found > MAX_LENGTH {
            return Err(InvalidInboundValue::TooLong {
                key,
                found,
                limit: MAX_LENGTH,
            });
        }
        if !trimmed.starts_with('/') || !trimmed.ends_with('/') || trimmed.len() < 2 {
            return Err(InvalidInboundValue::NotARequestPathPrefix { key });
        }
        // Between the leading and trailing `/`, every segment must be non-empty (so "//" and the
        // bare "/" - zero segments - are refused), must not be a dot segment, and must hold only
        // unreserved characters: no `%` and nothing else that a path normaliser could read two ways.
        let Some(interior) = trimmed.strip_prefix('/').and_then(|rest| rest.strip_suffix('/')) else {
            // Unreachable given the check above - both ends are `/` - but a typed refusal over a
            // panic either way.
            return Err(InvalidInboundValue::NotARequestPathPrefix { key });
        };
        let segments: Vec<&str> = interior.split('/').collect();
        if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
            return Err(InvalidInboundValue::NotARequestPathPrefix { key });
        }
        for segment in &segments {
            if *segment == "." || *segment == ".." {
                return Err(InvalidInboundValue::NotARequestPathPrefix { key });
            }
            for (position, character) in segment.char_indices() {
                let permitted = character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_' | '~');
                if !permitted {
                    return Err(InvalidInboundValue::NotPermitted {
                        key,
                        position,
                        accepted: SEGMENT_CHARACTERS,
                    });
                }
            }
        }
        Ok(Self(String::from(trimmed)))
    }

    /// Whether a presented `requestPath` claim is under this prefix: it starts with the stored
    /// value, or it equals the stored value without its trailing `/` - the bare form of the base
    /// path, which a gateway's catalog hands to clients (`github.com/telekom/sutura#2098`).
    ///
    /// Equality, not a shorter `starts_with`, keeps the boundary: `/hub/app/v10` is neither.
    #[must_use]
    pub fn matches(&self, request_path: &str) -> bool {
        request_path.starts_with(self.0.as_str()) || self.0.strip_suffix('/') == Some(request_path)
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for RequestPathPrefix {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::RequestPathPrefix;

    #[test]
    fn a_matching_path_matches() {
        let prefix = RequestPathPrefix::parse("/hub/app/v1/").expect("a well-formed prefix parses");
        assert!(prefix.matches("/hub/app/v1/mcp"));
    }

    #[test]
    fn a_sibling_route_extending_the_prefix_as_text_does_not_match() {
        let prefix = RequestPathPrefix::parse("/hub/app/v1/").expect("a well-formed prefix parses");
        assert!(!prefix.matches("/hub/app/v1evil/mcp"));
    }

    #[test]
    fn a_prefix_without_a_trailing_slash_is_refused() {
        RequestPathPrefix::parse("/hub/app/v1").unwrap_err();
    }

    #[test]
    fn a_prefix_without_a_leading_slash_is_refused() {
        RequestPathPrefix::parse("hub/app/v1/").unwrap_err();
    }

    #[test]
    fn the_bare_root_is_refused() {
        RequestPathPrefix::parse("/").unwrap_err();
    }

    #[test]
    fn a_dot_dot_segment_is_refused() {
        RequestPathPrefix::parse("/hub/../v1/").unwrap_err();
    }

    #[test]
    fn a_doubled_slash_is_refused() {
        RequestPathPrefix::parse("/hub//v1/").unwrap_err();
    }

    #[test]
    fn a_percent_sign_is_refused() {
        RequestPathPrefix::parse("/hub/%2e/").unwrap_err();
    }
}

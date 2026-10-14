//! The value a token's `aud` claim must equal.
//!
//! Its own type rather than a [`ResourceIdentifier`](crate::inbound::ResourceIdentifier), because
//! an issuer does not always mint for the identifier this deployment publishes: Entra puts the API's
//! application id in `aud`, and a gateway that routes by path mints its assertion for the route. The
//! comparison is the same either way - one value, ours, byte for byte - so every accepted form is
//! stored exactly as written.

use crate::inbound::InvalidInboundValue;
use crate::inbound::primitive::{MAX_LENGTH, host_of, parse_https_uri, percent_escapes_are_complete};

/// The longest client id accepted. A GUID is 36.
const MAX_CLIENT_ID_LENGTH: usize = 128;

const CLIENT_ID_CHARACTERS: &str = "letters, digits, and any of - . _";

const PATH_CHARACTERS: &str = "letters, digits, and any of - . _ ~ : / @ ! $ & ' ( ) * + , ; = %";

/// What `aud` must equal: an `https` URI, a client id, or - behind a gateway - an absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenAudience(String);

impl TokenAudience {
    /// The key this value is written under in `direct`.
    pub const KEY: &'static str = "security.inbound.audience";
    /// The key this value is written under in `behind-gateway`.
    pub const TRANSIT_KEY: &'static str = "security.inbound.transit_audience";

    /// Reads a `direct` audience: an `https` URI, or a client id.
    ///
    /// A client id holds no `:`, so a value with a scheme is read as a URI and `http://` or `api://`
    /// is refused as [`InvalidInboundValue::NotHttps`] rather than taken for an id.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        let trimmed = raw.as_ref().trim();
        if trimmed.contains(':') {
            return parse_https_uri(Self::KEY, trimmed).map(Self);
        }
        ClientId::parse(Self::KEY, trimmed).map(|id| Self(String::from(id.0)))
    }

    /// Reads a transit audience: an absolute path, or else an `https` URI.
    pub(super) fn parse_transit(raw: &str) -> Result<Self, InvalidInboundValue> {
        let key = Self::TRANSIT_KEY;
        let trimmed = raw.trim();
        if !trimmed.starts_with('/') {
            return parse_https_uri(key, trimmed).map(Self);
        }
        let found = trimmed.chars().count();
        if found > MAX_LENGTH {
            return Err(InvalidInboundValue::TooLong {
                key,
                found,
                limit: MAX_LENGTH,
            });
        }
        for (position, character) in trimmed.chars().enumerate() {
            if character == '#' || character == '?' {
                return Err(InvalidInboundValue::HasDelimiter {
                    key,
                    delimiter: character,
                    position,
                });
            }
            let permitted = character.is_ascii_alphanumeric()
                || matches!(
                    character,
                    '-' | '.'
                        | '_'
                        | '~'
                        | ':'
                        | '/'
                        | '@'
                        | '!'
                        | '$'
                        | '&'
                        | '\''
                        | '('
                        | ')'
                        | '*'
                        | '+'
                        | ','
                        | ';'
                        | '='
                        | '%'
                );
            if !permitted {
                return Err(InvalidInboundValue::NotPermitted {
                    key,
                    position,
                    accepted: PATH_CHARACTERS,
                });
            }
        }
        if trimmed.starts_with("//") || !percent_escapes_are_complete(trimmed) {
            return Err(InvalidInboundValue::NotAnAbsolutePath { key });
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The value of every [`ResourceIdentifier`](crate::inbound::ResourceIdentifier) nobody wrote an audience for.
    pub(super) const fn verbatim(identifier: String) -> Self {
        Self(identifier)
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The client id this audience is, if it is one.
    #[must_use]
    pub fn as_client_id(&self) -> Option<ClientId<'_>> {
        ClientId::parse(Self::KEY, &self.0).ok()
    }

    /// The host an `https` audience names. `None` for a client id or a path, which name no host.
    #[must_use]
    pub fn host(&self) -> Option<crate::server::AllowedHost> {
        host_of(&self.0)
    }
}

impl core::fmt::Display for TokenAudience {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lets a test compare a requirement's audience against the identifier it defaults to.
#[cfg(test)]
impl PartialEq<crate::inbound::ResourceIdentifier> for TokenAudience {
    fn eq(&self, other: &crate::inbound::ResourceIdentifier) -> bool {
        self.0 == other.as_str()
    }
}

/// A client id exactly as written: 1 to 128 letters, digits, `-`, `.` or `_`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientId<'a>(&'a str);

impl<'a> ClientId<'a> {
    /// Reads `raw` untrimmed, so a value with whitespace around it is refused; `key` names it in the error.
    pub fn parse(key: &'static str, raw: &'a str) -> Result<Self, InvalidInboundValue> {
        if raw.is_empty() {
            return Err(InvalidInboundValue::Empty { key });
        }
        let found = raw.chars().count();
        if found > MAX_CLIENT_ID_LENGTH {
            return Err(InvalidInboundValue::TooLong {
                key,
                found,
                limit: MAX_CLIENT_ID_LENGTH,
            });
        }
        if let Some(position) = raw
            .chars()
            .position(|character| !(character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_')))
        {
            return Err(InvalidInboundValue::NotPermitted {
                key,
                position,
                accepted: CLIENT_ID_CHARACTERS,
            });
        }
        Ok(Self(raw))
    }

    /// The same client, ASCII case ignored.
    #[must_use]
    pub const fn is(self, other: ClientId<'_>) -> bool {
        self.0.eq_ignore_ascii_case(other.0)
    }
}

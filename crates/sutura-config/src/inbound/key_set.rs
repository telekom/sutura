//! Where the inbound signing keys come from.

use crate::inbound::primitive::{InvalidInboundValue, KeySetFile, parse_https_uri};

/// Where the signing keys are read from.
///
/// **An enum, so a second source arrives as a variant every reader has to match** rather than as a
/// second optional field a reader can forget. The gate picks its key source out of this value, and
/// the cache it feeds is the same whatever the source is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySetLocation {
    /// A key set on disk at a [`KeySetFile`] path.
    File(KeySetFile),
    /// A JWK set fetched from a [`KeySetUrl`] - an issuer's `jwks_uri`.
    Url(KeySetUrl),
}

/// The `https` URL a [`KeySetLocation::Url`] key set is fetched from.
///
/// The same strict parse as [`crate::inbound::IssuerUrl`]: `https` only, a host, no userinfo, and no
/// `?` or `#`, so an endpoint that needs a query string is refused rather than half-read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySetUrl(String);

impl KeySetUrl {
    /// The key a key set URL is written under.
    pub const KEY: &'static str = "security.inbound.key_set_url";

    /// Reads the URL. Reachability is the composition root's question: it fetches once before the
    /// listener opens, so an unreachable issuer is a refusal to start.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        parse_https_uri(Self::KEY, raw.as_ref()).map(Self)
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for KeySetUrl {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

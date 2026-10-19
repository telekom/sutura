//! Where the inbound signing keys come from, and how long they verify while that source fails.

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

impl KeySetLocation {
    /// Declares the ceiling on the keys from this source.
    pub(crate) const fn set_max_stale(&mut self, max_stale: KeySetMaxStale) {
        match *self {
            Self::File(ref mut file) => file.set_max_stale(max_stale),
            Self::Url(ref mut url) => url.max_stale = max_stale,
        }
    }

    /// How long the keys from this source verify after a look last confirmed them.
    #[must_use]
    pub const fn max_stale(&self) -> KeySetMaxStale {
        match *self {
            Self::File(ref file) => file.max_stale(),
            Self::Url(ref url) => url.max_stale,
        }
    }
}

/// The `https` URL a [`KeySetLocation::Url`] key set is fetched from.
///
/// The same strict parse as [`crate::inbound::IssuerUrl`]: `https` only, a host, no userinfo, and no
/// `?` or `#`, so an endpoint that needs a query string is refused rather than half-read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySetUrl {
    url: String,
    max_stale: KeySetMaxStale,
}

impl KeySetUrl {
    /// The key a key set URL is written under.
    pub const KEY: &'static str = "security.inbound.key_set_url";

    /// Reads the URL. Reachability is the composition root's question: it fetches once before the
    /// listener opens, so an unreachable issuer is a refusal to start.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        parse_https_uri(Self::KEY, raw.as_ref()).map(|url| Self {
            url,
            max_stale: KeySetMaxStale::default_ceiling(),
        })
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.url
    }
}

impl core::fmt::Display for KeySetUrl {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.url)
    }
}

/// How long after a look last came back with a usable document the held keys keep verifying.
///
/// Past it no held key verifies - known or unknown `kid`, an unreadable source or a refused
/// document alike - until a look succeeds. That bounds how long a revoked key verifies while
/// refresh fails, at the cost of answering every caller `503` past it. No off switch: a switch for a
/// security bound only ever gets set wrong, and [`Self::MAX_SECONDS`] is the loosest value there is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySetMaxStale {
    seconds: u64,
}

impl KeySetMaxStale {
    /// The key the ceiling is written under.
    pub const KEY: &'static str = "security.inbound.key_set_max_stale_seconds";

    /// The default, in seconds. An hour: an issuer down that long mints few tokens a held key verifies.
    pub const DEFAULT_SECONDS: u64 = 3600;

    /// The floor, in seconds. A healthy source is up to about two minutes unconfirmed after one failed
    /// read; below five minutes, a few failed reads - a sidecar rewriting the file - refuse every caller.
    pub const MIN_SECONDS: u64 = 300;

    /// The ceiling, in seconds. A day: past it the value is not a ceiling, and a millisecond count
    /// written as seconds is refused rather than read.
    pub const MAX_SECONDS: u64 = 86_400;

    /// Reads a configured ceiling.
    pub const fn parse(seconds: u64) -> Result<Self, InvalidInboundValue> {
        if seconds < Self::MIN_SECONDS || seconds > Self::MAX_SECONDS {
            return Err(InvalidInboundValue::KeySetMaxStaleOutOfRange {
                found: seconds,
                min: Self::MIN_SECONDS,
                max: Self::MAX_SECONDS,
            });
        }
        Ok(Self { seconds })
    }

    /// The default ceiling.
    #[must_use]
    pub const fn default_ceiling() -> Self {
        Self {
            seconds: Self::DEFAULT_SECONDS,
        }
    }

    #[inline]
    #[must_use]
    pub const fn seconds(self) -> u64 {
        self.seconds
    }
}

impl core::fmt::Display for KeySetMaxStale {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}s", self.seconds)
    }
}

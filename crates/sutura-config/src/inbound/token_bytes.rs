//! The bound on token material before JWT decoding or key lookup.

use super::InvalidInboundValue;

/// A nonzero token byte limit with a fixed ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaxTokenBytes(usize);

impl MaxTokenBytes {
    /// The absent setting preserves the original eight-kibibyte bound.
    pub const DEFAULT: Self = Self(8192);
    /// Below the HTTP/2 header-list default (16384), leaving nominal header overhead.
    /// Other headers or intermediaries can still refuse a request before this check.
    pub const CEILING: usize = 12288;

    /// Refuses zero and values above the fixed ceiling at startup.
    pub const fn parse(bytes: usize) -> Result<Self, InvalidInboundValue> {
        if bytes == 0 || bytes > Self::CEILING {
            return Err(InvalidInboundValue::TokenBytesOutOfRange {
                found: bytes,
                limit: Self::CEILING,
            });
        }
        Ok(Self(bytes))
    }

    /// The maximum token size, excluding its header name and scheme.
    #[must_use]
    pub const fn bytes(self) -> usize {
        self.0
    }
}

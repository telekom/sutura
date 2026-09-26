//! What one catalog HTTP read may spend: a request timeout and a response-size cap.

/// The recommended default request timeout, in seconds, for a composition root's settings default.
///
/// Matches `server.request_timeout_seconds`'s own shipped default: a metadata read that outlives
/// the request timeout in front of it cannot answer inside the budget the caller was promised
/// anyway. **Not read by anything in this crate** - a caller passes the number it resolved, through
/// [`ReadBounds::parse`], the same single-owner shape `BytesBilledCeiling::parse` holds for
/// `BigQuery`'s ceiling: this crate owns the range, a settings tree owns that the key was written.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

/// The recommended default response-size cap, in bytes, for a composition root's settings default.
///
/// A metadata page is descriptions, column names and a handful of documents, not query rows, so
/// what this defends against is something that is not the endpoint answering at all - a redirect
/// loop, a proxy gone wrong - rather than a realistic upper bound on a legitimate page.
pub const DEFAULT_MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// Why a declared bound is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidReadBounds {
    /// Zero would refuse every read rather than bounding one.
    #[error("a {what} of zero would refuse every read rather than bounding one")]
    Zero { what: &'static str },
}

/// What one reader's `read()` call may spend: a request timeout and a response-size cap.
///
/// A newtype rather than two loose arguments, so a reader cannot be built with an unchecked pair -
/// a request timeout paired with a response-size cap, and no money bound: a metadata read is not
/// billed.
#[derive(Debug, Clone, Copy)]
pub struct ReadBounds {
    timeout: core::time::Duration,
    max_response_bytes: u64,
}

impl ReadBounds {
    /// Parses a declared timeout and cap, refusing either at zero.
    pub const fn parse(timeout_seconds: u64, max_response_bytes: u64) -> Result<Self, InvalidReadBounds> {
        if timeout_seconds == 0 {
            return Err(InvalidReadBounds::Zero { what: "request timeout" });
        }
        if max_response_bytes == 0 {
            return Err(InvalidReadBounds::Zero {
                what: "response size cap",
            });
        }
        Ok(Self {
            timeout: core::time::Duration::from_secs(timeout_seconds),
            max_response_bytes,
        })
    }

    #[inline]
    #[must_use]
    pub const fn timeout(&self) -> core::time::Duration {
        self.timeout
    }

    #[inline]
    #[must_use]
    pub const fn max_response_bytes(&self) -> u64 {
        self.max_response_bytes
    }
}

//! Credential-shaped values whose `Debug` never reveals them.
//!
//! The redaction is the point, so it has a test. A secret that reaches a log through
//! `{:?}` is not recoverable once shipped, and every structured-logging call site is a
//! chance for it — so the type, not the call site, is where this is fixed.

use std::fmt;

/// An opaque secret. `Debug` prints a placeholder; the value is reachable only by an
/// explicit, greppable call to [`Secret::expose`].
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Named to be conspicuous in review and in a grep. Prefer passing `Secret` around.
    #[inline]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(REDACTED)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("REDACTED")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one test that must exist before there is anything to redact.
    #[test]
    fn debug_does_not_leak_the_secret() {
        let s = Secret::new("hunter2-do-not-log-me");
        assert!(!format!("{s:?}").contains("hunter2"), "Debug leaked: {s:?}");
        assert!(!format!("{s}").contains("hunter2"), "Display leaked: {s}");
        assert_eq!(format!("{s:?}"), "Secret(REDACTED)");
    }

    /// Redaction must not make the value unusable — otherwise people avoid the type.
    #[test]
    fn expose_returns_the_value() {
        assert_eq!(Secret::new("v").expose(), "v");
    }

    /// A struct that merely CONTAINS a Secret must not leak it via derived Debug either.
    #[test]
    fn nested_debug_does_not_leak() {
        #[derive(Debug)]
        struct Config {
            #[expect(
                dead_code,
                reason = "read only through the derived Debug impl, which is what this asserts"
            )]
            token: Secret,
        }
        let c = Config {
            token: Secret::new("hunter2"),
        };
        assert!(
            !format!("{c:?}").contains("hunter2"),
            "nested Debug leaked: {c:?}"
        );
    }
}

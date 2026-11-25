//! A catalog endpoint's own message on a refusal, redacted before it can reach a log line.

/// The endpoint's own message on a refusal.
///
/// Bounded, filtered, and reachable only through [`Self::as_str`] - never through `Debug`, which
/// is the rendering a cause-chain walk uses, so a `Display`-flattened error chain never carries
/// endpoint-owned text.
#[derive(Clone, PartialEq, Eq)]
pub struct EndpointMessage(String);

impl EndpointMessage {
    /// Long enough for the endpoint's own sentences, short enough that a log line stays a line.
    const MAX_DETAIL_CHARS: usize = 400;

    #[must_use]
    pub fn bounded(raw: &str) -> Self {
        Self(
            raw.chars()
                .filter(|c| c.is_ascii_graphic() || *c == ' ')
                .take(Self::MAX_DETAIL_CHARS)
                .collect(),
        )
    }

    /// The message itself, for a caller that has decided it may render it.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for EndpointMessage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "<the endpoint's own message, {} char(s), redacted>", self.0.len())
    }
}

/// The `errorName` of a Conjure-style error answer, such as `Default:PermissionDenied`.
///
/// Kept only in that shape - ASCII letters, digits, `:`, `.`, `_` and `-`, at most
/// [`Self::MAX_BYTES`] - so it names a kind of error and carries no value from the request. The
/// answer's `parameters` are never read: they may quote the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorName(String);

impl ErrorName {
    pub const MAX_BYTES: usize = 128;

    /// The top-level `errorName` of `answer`, when it is JSON and the name has the shape above.
    #[must_use]
    pub fn from_answer(answer: &str) -> Option<Self> {
        let body: serde_json::Value = serde_json::from_str(answer).ok()?;
        let name = body.get("errorName")?.as_str()?;
        let shaped = !name.is_empty()
            && name.len() <= Self::MAX_BYTES
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'_' | b'-'));
        shaped.then(|| Self(String::from(name)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What kind of failure kept a request from an answer, without the failure's own text, which can
/// name the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFault {
    Timeout,
    HostNotFound,
    ConnectionFailed,
    Tls,
    Io,
    Other,
}

impl TransportFault {
    #[must_use]
    pub const fn of(cause: &ureq::Error) -> Self {
        match *cause {
            ureq::Error::Timeout(_) => Self::Timeout,
            ureq::Error::HostNotFound => Self::HostNotFound,
            ureq::Error::ConnectionFailed => Self::ConnectionFailed,
            ureq::Error::Tls(_) | ureq::Error::Pem(_) | ureq::Error::Rustls(_) | ureq::Error::TlsRequired => Self::Tls,
            ureq::Error::Io(_) => Self::Io,
            _ => Self::Other,
        }
    }

    /// The fault of a boxed cause, when it is a transport error.
    #[must_use]
    pub fn within(cause: &(dyn std::error::Error + Send + Sync + 'static)) -> Option<Self> {
        cause.downcast_ref::<ureq::Error>().map(Self::of)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timed out",
            Self::HostNotFound => "host not found",
            Self::ConnectionFailed => "connection failed",
            Self::Tls => "TLS failed",
            Self::Io => "I/O failed",
            Self::Other => "transport failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorName;

    #[test]
    fn only_a_shaped_top_level_error_name_is_kept() {
        let kept = ErrorName::from_answer(
            r#"{"errorCode":"INVALID_ARGUMENT","errorName":"SqlQueries:QueryParseError","parameters":{"query":"SELECT secret"}}"#,
        );
        assert_eq!(kept.as_ref().map(ErrorName::as_str), Some("SqlQueries:QueryParseError"));
        for answer in [
            "not json",
            r#"{"errorName":""}"#,
            r#"{"errorName":"has space"}"#,
            r#"{"errorName":"https://host.example.com/x"}"#,
            r#"{"parameters":{"errorName":"Nested:Name"}}"#,
        ] {
            assert_eq!(ErrorName::from_answer(answer), None, "{answer}");
        }
        assert_eq!(
            ErrorName::from_answer(&format!(r#"{{"errorName":"{}"}}"#, "a".repeat(129))),
            None
        );
    }
}

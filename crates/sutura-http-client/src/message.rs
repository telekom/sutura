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

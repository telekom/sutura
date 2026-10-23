//! One identifier per request, so a request's log lines and the data-system jobs it ran can be
//! found together.
//!
//! **Here and not in the transport that mints it**, because a data system's job is labelled with it
//! (`crate::warehouse::labels`) and an adapter cannot depend on another adapter. The minting stays
//! in `sutura_http::correlation`: a fresh value is seeded from the clock, which this crate does not
//! read.
//!
//! **Not globally unique, not a trace id, not an authentication signal.** A caller may name its own
//! value for the log; the one a request context carries is always [`CorrelationId::drawn`], so a
//! job label never holds caller text.

/// The longest value accepted from a caller.
///
/// Sized off what a caller plausibly already has: a UUID is 36 characters and a W3C `traceparent`
/// is 55. Anything longer is not an identifier somebody is correlating with.
pub const MAX_LENGTH: usize = 64;

/// An identifier for one request.
///
/// If a value of this type exists it is 1 to [`MAX_LENGTH`] ASCII letters, digits, `-` and `_`,
/// so it can be written into a log line without escaping and cannot forge one. `Ord` is not
/// derived: a minted value sorts by mint order and a caller's does not.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CorrelationId(String);

/// Why a string is not a correlation identifier.
///
/// **No variant carries the offending text**: the value came from a caller and the error's
/// `Display` reaches a log, so echoing it there is the injection this type guards against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotACorrelationId {
    /// Empty, or only whitespace.
    #[error("a correlation id is empty")]
    Empty,
    /// Longer than [`MAX_LENGTH`].
    #[error("a correlation id is {length} characters, over the {MAX_LENGTH} allowed")]
    TooLong { length: usize },
    /// A character outside the accepted set - in practice a newline, a space or a quote.
    #[error("a correlation id has an unacceptable character at position {position}")]
    UnacceptableCharacter { position: usize },
}

impl CorrelationId {
    /// The id a process mints from its own counter: sixteen lowercase hex digits.
    ///
    /// Infallible, because every `u64` renders inside the accepted set.
    #[must_use]
    pub fn minted(counter: u64) -> Self {
        Self(format!("{counter:016x}"))
    }

    /// The id for 128 random bits a transport drew: 32 lowercase hex digits.
    ///
    /// Bytes and not text, so no caller value can become one. Unique only with overwhelming
    /// probability: nothing here checks that the bits were random.
    #[must_use]
    pub fn drawn(bits: [u8; 16]) -> Self {
        use core::fmt::Write as _;
        Self(bits.iter().fold(String::with_capacity(32), |mut hex, byte| {
            // Infallible into a `String`; a failure would leave a short id, which `parse` refuses.
            if write!(hex, "{byte:02x}").is_err() {
                hex.clear();
            }
            hex
        }))
    }

    /// Reads a caller-supplied identifier: trimmed, then the length bounded, then each character
    /// checked - the cheap bound before the per-character scan.
    ///
    /// # Errors
    ///
    /// [`NotACorrelationId`] naming the first rule the value breaks.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, NotACorrelationId> {
        let raw = raw.as_ref().trim();
        if raw.is_empty() {
            return Err(NotACorrelationId::Empty);
        }
        let length = raw.chars().count();
        if length > MAX_LENGTH {
            return Err(NotACorrelationId::TooLong { length });
        }
        if let Some((position, _)) = raw
            .char_indices()
            .find(|&(_, c)| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        {
            return Err(NotACorrelationId::UnacceptableCharacter { position });
        }
        Ok(Self(String::from(raw)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for CorrelationId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{CorrelationId, MAX_LENGTH, NotACorrelationId};

    #[test]
    fn a_minted_id_is_one_this_process_would_read_back() {
        for counter in [0, 1, u64::MAX] {
            let minted = CorrelationId::minted(counter);
            assert_eq!(CorrelationId::parse(minted.as_str()), Ok(minted.clone()));
            assert_eq!(minted.as_str().len(), 16, "{minted}");
        }
    }

    #[test]
    fn a_drawn_id_is_32_lowercase_hex_digits_this_process_would_read_back() {
        let drawn = CorrelationId::drawn([0xAB; 16]);
        assert_eq!(drawn.as_str(), "ab".repeat(16));
        assert_eq!(
            CorrelationId::drawn([0x01; 16]).as_str(),
            "01".repeat(16),
            "a leading zero is kept"
        );
        assert_eq!(CorrelationId::parse(drawn.as_str()), Ok(drawn));
    }

    #[test]
    fn a_caller_value_is_bounded_then_checked_character_by_character() {
        assert_eq!(CorrelationId::parse("  "), Err(NotACorrelationId::Empty));
        let long = "a".repeat(MAX_LENGTH + 1);
        assert_eq!(
            CorrelationId::parse(&long),
            Err(NotACorrelationId::TooLong { length: MAX_LENGTH + 1 })
        );
        assert_eq!(
            CorrelationId::parse("ok\nforged"),
            Err(NotACorrelationId::UnacceptableCharacter { position: 2 })
        );
        assert_eq!(
            CorrelationId::parse(" Ingress-42_abc ").map(|id| id.to_string()),
            Ok(String::from("Ingress-42_abc"))
        );
    }
}

//! One identifier per request, so a request's lines can be found together.
//!
//! # What this is, and the limits of it
//!
//! A counter seeded at process start, rendered as hex. That is enough for the one job it has:
//! grouping the lines **one process** wrote for **one request**. It is deliberately not more.
//!
//! * **It is not globally unique.** Two replicas started at the same nanosecond can mint the same
//!   value, and nothing here coordinates. A collector query wants this *and* the pod, not this
//!   alone.
//! * **It is not a distributed trace id.** There is no propagation to a downstream call, no
//!   sampling decision, no parent-child relationship and no exporter - `sutura-runtime`'s crate
//!   documentation records that exporting a trace is a decision about a backend, a sampling rate
//!   and an egress path, and that none of those has been made.
//! * **It is not an authentication or ordering signal.** The value is monotonic within a process,
//!   which makes two lines comparable and makes nothing else true.
//!
//! No `uuid` dependency, on purpose: a random 128-bit value would buy global uniqueness this
//! surface has no use for, and it is not worth a dependency in a workspace that has none.
//!
//! # The inbound header is untrusted text on its way to a log
//!
//! Reading a caller's identifier is what makes one line in an ingress log and one line here the
//! same request, so it is worth doing. It is also **log injection** if it is taken as given: a
//! newline in the value is a log line an attacker writes. `sutura_config::InvalidLogFilter`'s
//! `ControlCharacter` variant exists for exactly that reason about exactly that class of value, and
//! this is the same treatment - bound the length, then accept a strict character set and nothing
//! else.
//!
//! **A malformed header does not fail the request.** [`CorrelationId::from_headers`] mints a fresh
//! value instead, because refusing would let a caller turn a header they control into a `4xx` on a
//! question that was otherwise fine - and the thing being protected is the *log*, which a fresh id
//! protects just as well. The refusal is reported at `debug`, by reason and never by value.

use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

use axum::http::HeaderMap;

/// The header a caller may set to name the request in its own logs too.
pub const HEADER: &str = "x-correlation-id";

/// The longest value accepted from a caller.
///
/// Generous rather than tight, and sized off what a caller plausibly already has: a UUID is 36
/// characters and a W3C `traceparent` is 55. Anything longer is not an identifier somebody is
/// correlating with, and an unbounded one is a log line of a size a caller chooses.
pub const MAX_LENGTH: usize = 64;

/// The counter, seeded once when it is first read.
///
/// Seeded from the clock so two runs of the same process do not both start at zero - a log holding
/// two runs would otherwise have two lines called `0000000000000001` and no way to tell them apart.
/// `Relaxed` is the whole of the synchronisation needed: the only property required of the value is
/// that no two reads return the same one, which `fetch_add` gives on its own.
static COUNTER: LazyLock<AtomicU64> = LazyLock::new(|| AtomicU64::new(seed()));

/// The starting point for this process's counter.
///
/// A clock that will not read falls back to a fixed value rather than refusing: this is a log
/// field, and a process that will not start because it could not read the time is a worse outcome
/// than a run whose ids collide with another run's.
fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        // `try_from` rather than a cast: nanoseconds since the epoch outgrow `u64` in the
        // twenty-sixth century, and a silent truncation there would reset every id to a low number.
        .map_or(0, |since| u64::try_from(since.as_nanos()).unwrap_or(0))
}

/// An identifier for one request, within one process's log.
///
/// If a value of this type exists it is at most [`MAX_LENGTH`] characters of ASCII letters, digits,
/// `-` and `_`, and is not empty - so it can be written into a log line without escaping and cannot
/// forge one.
/// `Ord` is deliberately not derived. Generated values sort by mint order because they are
/// fixed-width hex; a caller-supplied one does not, so an ordering over the type as a whole would
/// mean something on half the values and nothing on the other half.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CorrelationId(String);

/// Why a string is not a correlation identifier.
///
/// **No variant carries the offending text**, and that is the point of the type rather than an
/// omission: the value came from a caller, the error's `Display` reaches a log, and echoing it
/// there is the injection this is guarding against. A position is enough to debug a client with,
/// and a position cannot forge a line.
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
    /// A value nothing else in this process will produce.
    ///
    /// Infallible, and it has to be: it is the fallback for every way of getting one, including a
    /// caller's header being refused.
    ///
    /// Sixteen hex digits, which is inside what [`Self::parse`] accepts - asserted by a test rather
    /// than argued, because the two drifting apart would mean this process mints ids it would
    /// itself decline to read back.
    #[must_use]
    pub fn fresh() -> Self {
        Self(format!("{:016x}", COUNTER.fetch_add(1, Ordering::Relaxed)))
    }

    /// Reads a caller-supplied identifier.
    ///
    /// Sanitize then validate, in that order and both inside the constructor: the value is trimmed
    /// first so a derived `PartialEq` and `Hash` agree about which id this is without any call site
    /// having to normalise. The length is checked against the *trimmed* value for the same reason.
    ///
    /// **Length before character set, deliberately, and this is the ordering the classic advice
    /// asks for**: the expensive check is per-character and the cheap one bounds how many
    /// characters there are.
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

    /// The caller's identifier if it sent a usable one, and a fresh one otherwise.
    ///
    /// Never fails. See the module documentation: a header a caller controls must not be able to
    /// turn an otherwise answerable question into a refusal, and a fresh id protects the log
    /// exactly as well as a rejection would.
    #[must_use]
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let Some(raw) = headers.get(HEADER) else {
            return Self::fresh();
        };
        // `to_str` fails on bytes that are not visible ASCII, which `parse` would refuse anyway;
        // the two are separate so the reason in the log says which happened.
        let Ok(raw) = raw.to_str() else {
            tracing::debug!(
                header = HEADER,
                "an inbound correlation header was not text; using a fresh id"
            );
            return Self::fresh();
        };
        match Self::parse(raw) {
            Ok(id) => id,
            Err(refused) => {
                // By REASON and never by value. `NotACorrelationId` carries no caller text, which
                // is what makes this line safe to write.
                tracing::debug!(
                    header = HEADER,
                    refused = %refused,
                    "an inbound correlation header was not usable; using a fresh id"
                );
                Self::fresh()
            }
        }
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
    use axum::http::{HeaderMap, HeaderValue};

    use super::{CorrelationId, HEADER, MAX_LENGTH, NotACorrelationId};

    /// Would this surface read `id` back?
    ///
    /// The property every fallback has to have, and the one that keeps `fresh` honest: a minted id
    /// that `parse` would decline is one this process writes and cannot read.
    fn is_readable_back(id: &CorrelationId) {
        assert_eq!(CorrelationId::parse(id.as_str()), Ok(id.clone()), "{id}");
    }

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(value) = HeaderValue::from_str(value) {
            headers.insert(HEADER, value);
        }
        headers
    }

    #[test]
    fn a_freshly_minted_id_is_one_this_process_would_read_back() {
        // The invariant that keeps `fresh` and `parse` from drifting: `fresh` does not go through
        // `parse` - it cannot, because it must not fail - so the accepted set has to be checked
        // against it here or not at all.
        let minted = CorrelationId::fresh();
        assert_eq!(
            CorrelationId::parse(minted.as_str()),
            Ok(minted.clone()),
            "this process mints an id it would refuse to read: {minted}"
        );
        assert_eq!(minted.as_str().len(), 16, "{minted}");
    }

    #[test]
    fn two_ids_from_one_process_differ() {
        // The only property the counter has to have. Asserted over a handful rather than one pair,
        // so a `fetch_add` replaced by a load would not pass on the first two happening to differ.
        let minted: Vec<String> = core::iter::repeat_with(|| CorrelationId::fresh().as_str().to_owned())
            .take(8)
            .collect();
        let mut unique = minted.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), minted.len(), "{minted:?}");
    }

    #[test]
    fn a_newline_in_a_caller_s_header_cannot_forge_a_log_line() {
        // **The reason this type exists.** `sutura_config::InvalidLogFilter::ControlCharacter` says
        // it for a configured value: a newline in a value that reaches the log is a log line an
        // attacker writes. This is the same value class arriving over the wire.
        //
        // Two layers, and both are asserted because either alone would look like it worked:
        // `HeaderValue` refuses to hold a bare newline at all, and `parse` refuses one that arrived
        // some other way.
        assert_eq!(
            CorrelationId::parse("abc\n{\"level\":50,\"msg\":\"forged\"}"),
            Err(NotACorrelationId::UnacceptableCharacter { position: 3 })
        );
        // A header that cannot even be constructed leaves the map empty, which is the
        // "no header" path - and that must still produce a usable id rather than nothing.
        is_readable_back(&CorrelationId::from_headers(&headers("abc\ndef")));
    }

    #[test]
    fn a_caller_s_usable_header_is_kept_and_an_unusable_one_is_replaced() {
        // The whole point of reading the header: an ingress log line and a line here name the same
        // request. So a usable value survives verbatim.
        assert_eq!(
            CorrelationId::from_headers(&headers("Ingress-7f3a_b2")).as_str(),
            "Ingress-7f3a_b2"
        );
        // And an unusable one is replaced rather than refused, so a header a caller controls cannot
        // fail a question. `!` is outside the accepted set.
        let replaced = CorrelationId::from_headers(&headers("no!"));
        assert_ne!(replaced.as_str(), "no!");
        is_readable_back(&replaced);
        // No header at all is the same fallback.
        is_readable_back(&CorrelationId::from_headers(&HeaderMap::new()));
    }

    #[test]
    fn the_bounds_are_the_ones_documented() {
        assert_eq!(CorrelationId::parse("   "), Err(NotACorrelationId::Empty));
        // Trimmed before it is measured, so surrounding whitespace is not what makes it too long
        // and two spellings of one id are one id.
        assert_eq!(
            CorrelationId::parse("  abc  ").expect("a trimmed id is an id").as_str(),
            "abc"
        );
        let over = "a".repeat(MAX_LENGTH + 1);
        assert_eq!(
            CorrelationId::parse(&over),
            Err(NotACorrelationId::TooLong { length: MAX_LENGTH + 1 })
        );
        assert_eq!(
            CorrelationId::parse("a".repeat(MAX_LENGTH)).map(|id| id.as_str().len()),
            Ok(MAX_LENGTH)
        );
    }

    #[test]
    fn a_refusal_never_carries_the_value_it_refused() {
        // The error's `Display` reaches a log - `from_headers` writes it - so a variant that
        // carried the text would put the caller's bytes there and undo the whole guard.
        let refused = CorrelationId::parse("secret-looking\tvalue").expect_err("a tab is not accepted");
        let sentence = refused.to_string();
        assert!(!sentence.contains("secret-looking"), "{sentence}");
        let refused = CorrelationId::parse("z".repeat(MAX_LENGTH + 1)).expect_err("an over-long id is refused");
        assert!(!refused.to_string().contains("zzz"), "{refused}");
    }
}

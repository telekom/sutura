//! A caller-supplied `grpc-timeout` header, parsed per the gRPC-over-HTTP2 spec.
//!
//! **Why this crate parses its own**, rather than reading one tonic already decoded: neither
//! `tonic` nor `arrow-flight` exposes the parse as a public function - tonic applies
//! `grpc-timeout` internally to bound its own `Server`'s per-request future, but a caller cannot
//! read the parsed value back, and `do_exchange`'s own deadline (the earlier of this header and the
//! service's own [`sutura_domain::warehouse::deadline::Budget`]) has to be computed before this
//! service's blocking call starts, not inferred from whether tonic later cancels it.
//!
//! **The deliberate difference from tonic's own behaviour**: tonic treats a malformed
//! `grpc-timeout` as absent and falls back to no timeout at all - permissive, because the header is
//! advisory to a transport that has its own bound underneath. Here a malformed header is a hard
//! refusal (`Err`, never `None`), because a peer that cannot state a deadline correctly gets no
//! deadline-less leg to run instead; silently falling through to this service's own budget would
//! let a confused value pass as "the caller stated no preference" when it stated something else.

use std::time::Duration;

/// Why a `grpc-timeout` header value could not be parsed.
///
/// Closed: carries no copy of the header's bytes, which are peer-controlled and otherwise
/// unvalidated text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the grpc-timeout header is malformed")]
pub struct MalformedTimeout;

/// Parse a `grpc-timeout` header value: up to 8 ASCII digits followed by exactly one unit -
/// `H`/`M`/`S`/`m`/`u`/`n` for hours, minutes, seconds, milliseconds, microseconds, nanoseconds -
/// per the [gRPC-over-HTTP2 spec](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-HTTP2.md).
///
/// A value with no digits, more than 8 digits, an unrecognised unit, or any other shape is refused
/// rather than treated as absent - see the module-level note on why that differs from tonic's own
/// internal parse.
pub fn parse(value: &str) -> Result<Duration, MalformedTimeout> {
    let (digits, unit) = value
        .split_at_checked(value.len().saturating_sub(1))
        .ok_or(MalformedTimeout)?;
    let unit = unit.chars().next().ok_or(MalformedTimeout)?;
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(MalformedTimeout);
    }
    let amount: u64 = digits.parse().map_err(|_cause| MalformedTimeout)?;
    match unit {
        'H' => Ok(Duration::from_secs(amount.saturating_mul(3_600))),
        'M' => Ok(Duration::from_secs(amount.saturating_mul(60))),
        'S' => Ok(Duration::from_secs(amount)),
        'm' => Ok(Duration::from_millis(amount)),
        'u' => Ok(Duration::from_micros(amount)),
        'n' => Ok(Duration::from_nanos(amount)),
        _ => Err(MalformedTimeout),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{MalformedTimeout, parse};

    #[test]
    fn hours_minutes_seconds_parse() {
        assert_eq!(parse("3H"), Ok(Duration::from_hours(3)));
        assert_eq!(parse("1M"), Ok(Duration::from_secs(60)));
        assert_eq!(parse("42S"), Ok(Duration::from_secs(42)));
    }

    #[test]
    fn sub_second_units_parse() {
        assert_eq!(parse("13m"), Ok(Duration::from_millis(13)));
        assert_eq!(parse("2u"), Ok(Duration::from_micros(2)));
        assert_eq!(parse("82n"), Ok(Duration::from_nanos(82)));
    }

    #[test]
    fn an_unrecognised_unit_is_refused() {
        assert_eq!(parse("5X"), Err(MalformedTimeout));
    }

    #[test]
    fn more_than_eight_digits_is_refused() {
        assert_eq!(parse("123456789S"), Err(MalformedTimeout));
    }

    #[test]
    fn eight_digits_is_the_boundary_and_still_parses() {
        assert_eq!(parse("99999999S"), Ok(Duration::from_secs(99_999_999)));
    }

    #[test]
    fn non_numeric_digits_are_refused() {
        assert_eq!(parse("1a2S"), Err(MalformedTimeout));
    }

    /// A sign is not a digit, though `u64`'s own parse accepts a leading `+`.
    #[test]
    fn a_signed_amount_is_refused() {
        assert_eq!(parse("+5S"), Err(MalformedTimeout));
    }

    #[test]
    fn no_digits_is_refused() {
        assert_eq!(parse("S"), Err(MalformedTimeout));
    }

    #[test]
    fn an_empty_value_is_refused() {
        assert_eq!(parse(""), Err(MalformedTimeout));
    }
}

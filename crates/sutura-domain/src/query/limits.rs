//! The two bounds behind [`RefusalReason::ResultTooLarge`](crate::query::RefusalReason::ResultTooLarge),
//! and the ceiling that names one of them.
//!
//! Its own module because it is a self-contained concept `sutura-domain/src/query.rs` was carrying
//! at the byte cap `cargo xtask max-lines` enforces: one refusal, honest about two - now three -
//! causes it can fire for.

/// Which bound a result was too large for.
///
/// **The vocabulary exists so one refusal can be honest about two causes.** The answer a caller gets
/// is one sentence - *too much data, ask a narrower question* - and
/// [`RefusalReason::ResultTooLarge`](crate::query::RefusalReason::ResultTooLarge) is that one
/// answer. This is what the deployment knows about why, and the two arms differ in who measured
/// it: the row cap is a number an operator configured here, and the volume bound belongs to the
/// data system and is not one this process was told.
///
/// **Closed, and read by exhaustive matches with no wildcard arm in both transports.** A third bound
/// is a compile error in each of them rather than a case one renders as another - which is what stops
/// a bound with no number being described using somebody else's number. The agent-facing prompt is
/// deliberately NOT one of those matches: `sutura_app::prompt::guide_for` keys on the
/// [`RefusalReason`](crate::query::RefusalReason) variant and never the payload, because what it
/// must tell an agent - *too much data, ask a narrower question* - is the same for both bounds and
/// a guide per bound would give an agent two paragraphs saying one thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ResultBound {
    /// The plan's row cap, in rows.
    ///
    /// Carries the limit and **not** how many rows there would have been, because nobody knows: the
    /// plan asks for one row more than the cap and stops there, so what is known is "more than
    /// this". That is the difference from
    /// [`TooManyDimensions`](crate::query::RefusalReason::TooManyDimensions), which can name what
    /// was requested because the caller sent it.
    ///
    /// **Also what `top.n` is checked against directly**, before anything executes:
    /// [`crate::query::top::TopN::exceeds_the_row_cap`] compares the caller's own count rather than
    /// waiting for a probe row to come back, because a `top` question already states the limit it
    /// wants and there is nothing to detect.
    Rows { limit: u32 },
    /// The data system would not hand this result back in one piece.
    ///
    /// Raised through `Warehouse::result_did_not_fit`, a predicate for the reason that port method's
    /// own documentation gives. It is *not* the row cap: the statement carries `MAX_ROWS + 1` as its
    /// `LIMIT`, so a result reaching this arm was inside the cap and was still more data than the
    /// data system would deliver at once - a wide result rather than a tall one.
    ///
    /// **Carries no number, and that is a decision rather than a field somebody forgot.** The bound
    /// belongs to the data system and is not stated to a client: the endpoint this arm was built for
    /// caps a reply by size and reports neither that cap nor the reply's size, so the only figures
    /// in scope are how much was scanned and how much would be billed - neither of which is the
    /// bound that fired. An `Option<u64>` here would make every reader decide what an absence
    /// permits, and a figure filled in from one of those would be a certified-looking number for a
    /// bound that is not the one that refused. A fabricated limit is worse than an absent one.
    ///
    /// **The limit, stated with the claim:** a caller is told to narrow the question and is not told
    /// by how much. That is the whole of what this deployment honestly knows.
    Volume,
    /// This deployment's own rendered-response ceiling, in bytes.
    ///
    /// **The third arm, and it exists because the first two do not cover a wide-but-short result.**
    /// A result inside `plan::MAX_ROWS` and inside every data system's own reply cap can still carry
    /// [`MAX_DIMENSIONS`](crate::query::MAX_DIMENSIONS) grouped columns of arbitrary text -
    /// `crate::catalog::MAX_DIMENSION_VALUE_CHARS` bounds a caller's filter value and a catalog
    /// author's allowlist entry, and says nothing about the length of a value a data system
    /// actually returns - so the row cap counts rows and the volume bound is the data system's, and
    /// neither measures what a caller's own cells add up to. Raised by
    /// `sutura_app::answer` against [`ResponseByteLimit`], after the row cap has already passed, over
    /// `RowSet::rendered_byte_len` - the same canonical cell text an anchor is compared against, not
    /// the wire bytes a transport wraps it in.
    ///
    /// Carries the ceiling, because unlike [`Self::Volume`] this is a number the deployment chose
    /// rather than one it was never told.
    Encoded { limit_bytes: u64 },
}

/// The most bytes an answer's rendered cells may occupy before this deployment declines to encode
/// it into a response.
///
/// **Parsed rather than a bare constant**, so a caller of this type cannot end up with a zero
/// ceiling that refuses every answer while reading as "no bound was set" - the same distinction
/// [`crate::plan::MAX_ROWS`] does not need to make, because nothing constructs a row cap from
/// outside this crate.
///
/// [`Self::DEFAULT`] is what every deployment is held to today - see its own documentation for the
/// number and the reasoning. **The limit, stated here rather than left for a reader to assume
/// otherwise:** nothing yet reads this from a settings file the way `server.max_body_bytes` bounds
/// the request side: `sutura_app::answer` and `sutura_app::federated::answer_federated` both use
/// [`Self::DEFAULT`] unconditionally. Making it operator-configurable is future work, threaded the
/// same way `working_set_bytes` already is, from a composition root down through
/// `sutura_app::surface::LocalService`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseByteLimit(u64);

/// Why a response byte ceiling is not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidResponseByteLimit {
    /// Zero reads as "no bound was configured" to whoever wrote it, and is the opposite: it refuses
    /// every answer without saying it means to.
    #[error("a response byte limit of zero refuses every answer without saying it means to")]
    Zero,
}

impl ResponseByteLimit {
    /// 8 MiB of rendered cell text.
    ///
    /// **A round number chosen against the TYPICAL case, and stated as one rather than as a derived
    /// bound.** A well-behaved question is nowhere near it: `plan::MAX_ROWS` rows of
    /// [`MAX_DIMENSIONS`](crate::query::MAX_DIMENSIONS) filter-shaped dimension values at
    /// `crate::catalog::MAX_DIMENSION_VALUE_CHARS` characters each, plus one rendered measure per
    /// row, is a small fraction of it. **That arithmetic is not a guarantee, and the gap is the
    /// reason this bound exists at all.** `crate::catalog::MAX_DIMENSION_VALUE_CHARS` bounds a
    /// caller's *filter* value and a catalog author's *allowlist* entry; it says nothing about the
    /// length of a GROUPED column's actual value, which comes back from the data system as
    /// [`crate::warehouse::Value::Text`] with no length carried by the type at all. So a single
    /// oversized column in an otherwise ordinary catalog is exactly the case this ceiling is the
    /// only bound against.
    ///
    /// **Unmeasured, and stated as such rather than dressed up as a derived cost.** 8 MiB is a
    /// round number, not a figure timed against `Outcome::from` building a wire body of that size -
    /// no such measurement exists in this codebase yet. Lowering it trades encode latency on the
    /// async executor thread the wire body is still built on after the blocking closure returns;
    /// raising it trades the other way. Either direction is a decision the next change to this
    /// constant should measure rather than guess at twice.
    ///
    /// **What this bounds is the RENDERED text, and the wire body is up to six times larger.**
    /// [`crate::warehouse::RowSet::new`] validates row WIDTH only, so a returned
    /// [`crate::warehouse::Value::Text`] may hold C0 bytes that `serde_json` must escape as
    /// `\u00XX` - six wire bytes for one rendered byte. Measured once, during review of the change
    /// that added this constant: 1024 rendered bytes serialized as 6146 JSON bytes, a factor of
    /// six. So 8 MiB here admits roughly 48 MiB of HTTP body, and on the agent surface that again
    /// alongside it, because `sutura-mcp` sends a text block AND structured content built from the
    /// same `OutcomeContent`. **Neither number is re-measured by any cell in this workspace** -
    /// the factor is a worst case for one shape of value, recorded so that whoever next changes
    /// this constant knows which side of the encode they are choosing, not a bound this type
    /// enforces on a transport.
    pub const DEFAULT: Self = Self(8 * 1024 * 1024);

    /// Reads a byte ceiling.
    pub const fn parse(bytes: u64) -> Result<Self, InvalidResponseByteLimit> {
        if bytes == 0 {
            return Err(InvalidResponseByteLimit::Zero);
        }
        Ok(Self(bytes))
    }

    #[inline]
    pub const fn bytes(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidResponseByteLimit, ResponseByteLimit};

    #[test]
    fn a_response_byte_limit_of_zero_is_refused() {
        assert_eq!(ResponseByteLimit::parse(0).unwrap_err(), InvalidResponseByteLimit::Zero);
        assert_eq!(ResponseByteLimit::parse(1).expect("one byte is a limit").bytes(), 1);
    }

    #[test]
    fn the_default_response_byte_limit_is_eight_mebibytes() {
        assert_eq!(ResponseByteLimit::DEFAULT.bytes(), 8 * 1024 * 1024);
    }
}

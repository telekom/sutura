//! Bounds behind a result refusal. Source streams, split capacity and final answers have
//! different units and scope; a known threshold is not the complete rejected result size.

use core::num::NonZeroUsize;

use crate::plan::SplitRefusal;
use crate::warehouse::ResultLimit;

/// Which bound refused the result. Both transports match this closed vocabulary exhaustively.
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
    /// A result was refused without a known threshold. No scanned volume, estimated size or
    /// deployment default may stand in for the unknown limit. The fieldless remote protocol
    /// still reaches this fallback; it cannot yet preserve reader-bound provenance.
    Volume,
    /// A source stream crossed this row limit, before the final answer was combined.
    SourceRows { limit: usize },
    /// A source result crossed its materialisation budget, not its rendered response size.
    Materialization { limit_bytes: usize },
    /// The estimate cannot fit the available partition capacity. No overflow was observed.
    SplitCapacity {
        estimated_rows: u64,
        per_stream_rows: NonZeroUsize,
        ready_readers: NonZeroUsize,
    },
    /// The final answer's rendered-cell ceiling, measured after the row cap against
    /// [`ResponseByteLimit`]. This is neither source materialisation nor complete wire size.
    Encoded { limit_bytes: u64 },
}

impl From<ResultLimit> for ResultBound {
    fn from(limit: ResultLimit) -> Self {
        match limit {
            ResultLimit::Unknown => Self::Volume,
            ResultLimit::StreamRows { limit } => Self::SourceRows { limit },
            ResultLimit::MaterializationBytes { limit_bytes } => Self::Materialization { limit_bytes },
        }
    }
}

impl From<SplitRefusal> for ResultBound {
    fn from(refusal: SplitRefusal) -> Self {
        Self::SplitCapacity {
            estimated_rows: refusal.rows().rows(),
            per_stream_rows: refusal.per_stream_bound(),
            ready_readers: refusal.ready_workers(),
        }
    }
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
    /// 8 MiB of rendered cells and reported execution text, including SQL binding values.
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

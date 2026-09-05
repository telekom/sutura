//! What one job may spend: the TIME bound, the MONEY bound, and the per-call budget they are
//! charged against.
//!
//! **Split out of `wire.rs` when that file reached the thousand-line limit `cargo xtask max-lines`
//! enforces and cannot exempt**, at the cut `document.rs` already made once. The seam is the first
//! bullet of `wire.rs`'s own header - *a job is bounded in TIME and in MONEY, and neither bound is a
//! constant here* - and everything in this file is one of those two bounds, the arithmetic that
//! keeps them inside the transport's own request timeout, or the refusal a deployment gets when the
//! number it wrote cannot be used. Nothing here opens a socket, holds a credential or builds a
//! document, which is what makes it the half a test reaches without a project.
//!
//! Why each bound is shaped the way it is stays with the type; `wire.rs`'s header is the one place
//! they are argued together.

use core::time::Duration;

/// How much longer than what is left of a call's budget the socket may wait.
///
/// **The number that stops a pool thread outliving the caller it was answering, and it is charged per
/// OPERATION.** A job is cancelled at the service when the call's remaining budget runs out and the
/// client stops waiting at the same instant, so this covers only connection setup and the last bytes of
/// the answer. Two corrections are recorded here rather than in a commit message, because each was a
/// claim this file made and did not hold:
///
/// - it used to be the other way round - a 70-second socket over a 55-second job wait against a
///   transport whose own default timeout is 30 seconds - which held a blocking-pool thread for up to
///   40 seconds after the request it served had gone;
/// - and then it was charged once per HTTP OPERATION against a whole-budget timeout on the agent, so
///   an answer's four operations could each spend it. [`CallDeadline`] and
///   [`QueryDeadline::within_request_timeout`] are what make the arithmetic add up: a call is
///   `budget + CONNECT_MARGIN`, an answer is [`QueryDeadline::CALLS_PER_ANSWER`] of those, and the
///   deadline a composition root is handed already divides the transport's own timeout by both.
const CONNECT_MARGIN: Duration = Duration::from_secs(5);

/// How long a job may run, and how long the client waits for its answer.
///
/// **A newtype rather than a constant, because the value belongs to the deployment.** The setting that
/// decides it is the one the transport in front of this service already uses -
/// `server.request_timeout_seconds`, which ships as 30 - and a constant in this file would be a second
/// copy of it that drifts the day somebody changes the first.
///
/// **It is a SHARE of that setting rather than the setting itself**, which review had to point out:
/// one answer makes [`Self::CALLS_PER_ANSWER`] calls and each pays [`CONNECT_MARGIN`] on top of its
/// own budget, so filling this with 30 gives a caller who waits 30 seconds a query that may still be
/// running. [`Self::within_request_timeout`] is the constructor that does the division, and it is the
/// one a composition root should reach for; [`Self::parse`] stays for a deployment stating a budget
/// outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryDeadline {
    seconds: u64,
}

/// The most a single job may be billed for scanning.
///
/// **Sent as `maximumBytesBilled`, which is enforced at the service and is what makes it worth
/// more than a client-side check.** A job that would exceed it FAILS and is not charged. Nothing else
/// in this repository bounds bytes scanned: `LIMIT 10001` bounds rows RETURNED, the one-page refusal
/// bounds a page, and `MAX_ANSWER_BYTES` bounds what is read into memory - a question can satisfy
/// all three and still scan a partitioned table end to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytesBilledCeiling {
    bytes: u64,
}

/// Why a bound this adapter was handed is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnusableBound {
    /// Zero, which would refuse every question rather than bounding one.
    #[error("a {what} cannot be zero")]
    Zero { what: &'static str },
    /// Above what the endpoint accepts, or above what a bound is for.
    #[error("a {what} of {given} is above the {cap} this adapter will send")]
    TooLarge { what: &'static str, given: u64, cap: u64 },
    /// A transport's request timeout too short to leave a job any budget at all.
    ///
    /// See [`QueryDeadline::within_request_timeout`]: one answer spends the budget
    /// [`QueryDeadline::CALLS_PER_ANSWER`] times and each spend costs connection setup on top, so a
    /// request timeout below that leaves nothing to bound.
    #[error("a request timeout of {given} seconds leaves no budget for the {calls} calls one answer makes")]
    NoBudget { given: u64, calls: u64 },
}

/// The instant one call into this transport has to be finished by.
///
/// **One absolute deadline for the whole of one call, rather than a timeout per HTTP operation - and
/// that distinction is the correction this type exists to carry.** The previous shape put
/// `timeout_global` on the agent, so EVERY request through it got the full budget independently: a
/// single [`crate::transport::JobTransport::run`] does a token exchange and then a job, and both were allowed
/// `deadline + CONNECT_MARGIN` of their own. A review measured the consequence at the answer level -
/// four HTTP operations, each with its own budget, against a transport whose own request timeout is
/// thirty seconds - and the five-second overrun this module claimed was false.
///
/// So the budget is opened once per call and every operation gets only what is LEFT of it: the token
/// exchange, the socket the job waits on, and the `timeoutMs` and `jobTimeoutMs` the request carries -
/// which is what keeps the service cancelling at the instant the client stops waiting even when the
/// exchange spent half the budget first. When nothing is left, the refusal comes before the send.
///
/// **A monotonic [`std::time::Instant`] and not a wall clock**, because a wall clock can step and a
/// stepped deadline is either a job abandoned early or one that outlives its caller.
///
/// **The limit, and it is the half this type cannot reach:** one ANSWER calls the port twice -
/// `Warehouse::dry_run` and then `Warehouse::execute` - and neither `Warehouse` nor [`crate::transport::JobTransport`]
/// takes a deadline, so the two calls cannot share one. An answer's worst case is therefore
/// `CALLS_PER_ANSWER` budgets rather than one, which is exactly why
/// [`QueryDeadline::within_request_timeout`] exists: it does that arithmetic once so a composition root
/// cannot get it wrong. Carrying one deadline across the port is an architecture decision, not a
/// signature tweak.
#[derive(Debug, Clone, Copy)]
pub struct CallDeadline {
    started: std::time::Instant,
    budget: Duration,
}

impl CallDeadline {
    /// Opens a budget now.
    #[must_use]
    pub fn opened(deadline: QueryDeadline) -> Self {
        Self::opened_at(std::time::Instant::now(), deadline)
    }

    /// Opens a budget that started at a named instant.
    ///
    /// **The canonical constructor, with [`Self::opened`] delegating to it**, and it is public for one
    /// reason: a caller cannot otherwise construct a budget that is already spent, so the refusal at
    /// the end of one could not be reached from a test without sleeping through a real one.
    #[must_use]
    pub const fn opened_at(started: std::time::Instant, deadline: QueryDeadline) -> Self {
        Self {
            started,
            budget: Duration::from_secs(deadline.seconds),
        }
    }

    /// What is left of the budget, or `None` when it is spent.
    ///
    /// `None` rather than a zero duration, because zero means *no timeout* to the client underneath -
    /// so handing it on would turn a spent budget into an unbounded wait, which is the opposite of what
    /// this type is for.
    #[must_use]
    pub fn remaining(self) -> Option<Duration> {
        self.budget.checked_sub(self.started.elapsed()).filter(|left| !left.is_zero())
    }

    /// How long a socket may stay open for an operation with `left` of the budget remaining: that,
    /// plus connection setup.
    #[must_use]
    pub const fn socket(left: Duration) -> Duration {
        left.saturating_add(CONNECT_MARGIN)
    }
}

impl QueryDeadline {
    /// The longest deadline this adapter will send.
    ///
    /// Six hours is the endpoint's own ceiling for a query job. A deployment that wants longer wants
    /// a batch job, which is a different API and a different decision.
    const MAX_SECONDS: u64 = 6 * 60 * 60;

    /// How many times one ANSWER spends this budget: `Warehouse::dry_run`, then `Warehouse::execute`.
    ///
    /// **A constant in this crate that describes `sutura_app::answer`'s call pattern, and nothing
    /// mechanical keeps the two equal** - which is stated here rather than left for somebody to
    /// discover, because it is the one number in [`Self::within_request_timeout`]'s arithmetic that a
    /// change somewhere else could falsify. The alternative - a deadline carried across the
    /// `Warehouse` port - is an architecture decision, and until it is taken this is the honest shape:
    /// a number with its assumption written next to it.
    pub const CALLS_PER_ANSWER: u64 = 2;

    /// The largest deadline that keeps one ANSWER inside a transport's own request timeout.
    ///
    /// **The arithmetic a composition root would otherwise have to remember, and get wrong.** The
    /// number to fill this from is `server.request_timeout_seconds`, which ships as thirty; what a
    /// caller wants is not that number but the share of it one call may spend, because an answer makes
    /// [`Self::CALLS_PER_ANSWER`] calls and each pays [`CONNECT_MARGIN`] on top of its own budget. So
    /// `within_request_timeout(30)` is ten seconds, and two calls of ten plus five is the thirty a
    /// caller was promised.
    ///
    /// A request timeout too short to leave anything is [`UnusableBound::NoBudget`] rather than a
    /// silently clamped value, because a deployment whose timeout cannot fit a query wants to be told
    /// so at startup.
    pub const fn within_request_timeout(request_timeout_seconds: u64) -> Result<Self, UnusableBound> {
        /// The refusal, written once because both arms below reach it.
        const fn no_budget(given: u64) -> UnusableBound {
            UnusableBound::NoBudget {
                given,
                calls: QueryDeadline::CALLS_PER_ANSWER,
            }
        }

        // `checked_div` rather than `/`, because `clippy::integer_division` and
        // `integer_division_remainder_used` are both denied in this workspace - and the named call is
        // the better shape anyway: it makes the truncation deliberate, so a request timeout of 31
        // seconds buys the same budget as 30 and a fraction of a second is never a budget.
        match request_timeout_seconds.checked_div(Self::CALLS_PER_ANSWER) {
            None => Err(no_budget(request_timeout_seconds)),
            Some(share) => match share.checked_sub(CONNECT_MARGIN.as_secs()) {
                None | Some(0) => Err(no_budget(request_timeout_seconds)),
                Some(seconds) => Self::parse(seconds),
            },
        }
    }

    /// Parses a deadline in whole seconds.
    pub const fn parse(seconds: u64) -> Result<Self, UnusableBound> {
        if seconds == 0 {
            return Err(UnusableBound::Zero { what: "query deadline" });
        }
        if seconds > Self::MAX_SECONDS {
            return Err(UnusableBound::TooLarge {
                what: "query deadline",
                given: seconds,
                cap: Self::MAX_SECONDS,
            });
        }
        Ok(Self { seconds })
    }

    /// The deadline in milliseconds, which is the unit both request fields take.
    ///
    /// `saturating_mul` rather than `*`, and it cannot saturate: `Self::MAX_SECONDS` times a
    /// thousand is far inside `u64`. Written that way because a bound that could wrap is a bound that
    /// could become zero, and zero is the one value [`Self::parse`] refuses.
    #[inline]
    #[must_use]
    pub const fn milliseconds(self) -> u64 {
        self.seconds.saturating_mul(1_000)
    }

    /// The whole budget, as a duration.
    #[inline]
    #[must_use]
    pub const fn budget(self) -> Duration {
        Duration::from_secs(self.seconds)
    }

    /// How long a socket may stay open for a call that has spent none of its budget yet.
    ///
    /// **The backstop on the agent rather than the bound that holds.** What a single operation is
    /// really allowed is `CallDeadline::socket(left)` over what is LEFT of the call's budget - see
    /// [`CallDeadline`], and see the module header for why a per-operation timeout was not enough. This
    /// value is what the agent is configured with, so an operation that somehow reached the client
    /// without an override is still bounded.
    #[must_use]
    pub const fn socket(self) -> Duration {
        CallDeadline::socket(self.budget())
    }
}

impl BytesBilledCeiling {
    /// The largest ceiling this adapter will send: one tebibyte.
    ///
    /// Not the endpoint's limit - it has none - but a bound on the bound. A ceiling above this is
    /// indistinguishable from no ceiling, and "no ceiling" is the state this type exists to make
    /// unrepresentable.
    const MAX_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

    /// Parses a ceiling in bytes.
    pub const fn parse(bytes: u64) -> Result<Self, UnusableBound> {
        if bytes == 0 {
            return Err(UnusableBound::Zero {
                what: "bytes-billed ceiling",
            });
        }
        if bytes > Self::MAX_BYTES {
            return Err(UnusableBound::TooLarge {
                what: "bytes-billed ceiling",
                given: bytes,
                cap: Self::MAX_BYTES,
            });
        }
        Ok(Self { bytes })
    }

    /// The ceiling, as the request body writes it.
    ///
    /// **Text, because the endpoint writes and reads 64-bit integers as JSON strings.** A number here
    /// would be silently truncated to a double by a strict reader at the far end.
    #[must_use]
    pub fn as_text(self) -> String {
        self.bytes.to_string()
    }
}

/// What every job this adapter submits is bounded by.
///
/// Two bounds in one value, because they are one decision: *how much of a deployment's time and money
/// may one question spend*. A struct rather than two arguments so a call site cannot supply one and
/// forget the other, and so [`super::WireAgent`] can carry them both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobBounds {
    deadline: QueryDeadline,
    max_bytes_billed: BytesBilledCeiling,
}

impl JobBounds {
    /// Names both bounds. Neither has a default, for the reason `BigQueryWarehouse::new` gives about
    /// its own arguments: a defaulted deadline is a promise nobody made, and a defaulted ceiling is
    /// money somebody else pays.
    #[must_use]
    pub const fn of(deadline: QueryDeadline, max_bytes_billed: BytesBilledCeiling) -> Self {
        Self {
            deadline,
            max_bytes_billed,
        }
    }

    /// How long a job may run.
    #[inline]
    #[must_use]
    pub const fn deadline(self) -> QueryDeadline {
        self.deadline
    }

    /// The most one job may be billed for scanning.
    #[inline]
    #[must_use]
    pub const fn max_bytes_billed(self) -> BytesBilledCeiling {
        self.max_bytes_billed
    }
}

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
///   an answer's four operations could each spend it. [`CallDeadline`] is what makes it one deadline
///   per CALL, opened once and read down by whatever each operation spends. `docs/adr/0029` is what
///   then made it one deadline per ANSWER: the port's own `Deadline` is shared by every call one
///   answer makes, so a slow call shortens the next rather than being followed by one with a full
///   budget of its own - see [`CallDeadline::opened_at_for`].
const CONNECT_MARGIN: Duration = Duration::from_secs(5);

/// How long a job may run when there is no port `Deadline` to read one from, and the ceiling this
/// adapter's socket is pinned to for every call.
///
/// **A newtype rather than a constant, because the value belongs to the deployment.** The setting
/// that decides it is the one the transport in front of this service already uses -
/// `server.request_timeout_seconds`, which ships as 30.
///
/// **It used to be a SHARE of that setting rather than the setting itself, and `docs/adr/0029` is
/// why it no longer is.** One answer made two calls through this transport - `Warehouse::dry_run`
/// then `execute` - and neither took a deadline, so this type had to divide `30` by the two of them
/// and their own connection overhead to keep an answer inside the caller's own wait -
/// `within_request_timeout` was that arithmetic, checked by nothing outside this file. The port now
/// carries ONE `Deadline` shared by every call one answer makes - [`CallDeadline`] opens FROM it at
/// request time - so this type is left with a narrower job: the boot path, which has no `Deadline`
/// to read (`verify_anchor`, a fixture load or drop, the identity read), and the socket ceiling every
/// call is pinned to as a backstop regardless of what a request supplies. [`Self::parse`] is a
/// composition root's one door in, and it takes the setting directly rather than a share of it.
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
/// **The limit this type used to carry is resolved by `docs/adr/0029`, and the record of it stays
/// here rather than being deleted, because the fix is a fact about the type above it and not about
/// this one.** One ANSWER calls the port twice - `Warehouse::dry_run` and then `Warehouse::execute` -
/// and this type alone could never make the two share a budget: it is opened fresh by whoever calls
/// [`Self::opened`]/[`Self::opened_at`], and nothing HERE remembers what an earlier call spent. The
/// port now carries a `sutura_domain::warehouse::deadline::Deadline` - one absolute instant per
/// answer - and `crate::wire::BigQueryWire::submit` opens a `CallDeadline` from what THAT says is
/// left via [`Self::opened_at_for`], so the sharing lives one level up, where the two port calls
/// actually are. The boot path (`verify_anchor`, a fixture load or drop) has no such `Deadline` to
/// read and keeps opening fresh from this adapter's own configured [`QueryDeadline`], exactly as
/// every call did before this record.
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

    /// Opens a budget that started at a named instant, for an amount already known as a
    /// [`Duration`] rather than a whole-second [`QueryDeadline`].
    ///
    /// **`pub(crate)` rather than a third public constructor**, because the one caller is
    /// `super::submit`, deriving this from what a `sutura_domain::warehouse::deadline::Deadline`
    /// says is left at the instant it asks - a value `Deadline` itself will not hand out as a raw
    /// `Instant`, by design, so the amount arrives here as a duration rather than a second instant to
    /// disagree with `started`.
    #[must_use]
    pub(crate) const fn opened_at_for(started: std::time::Instant, budget: Duration) -> Self {
        Self { started, budget }
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

    /// Parses a deadline in whole seconds.
    ///
    /// **The one door in, since `docs/adr/0029` retired `within_request_timeout`'s arithmetic**
    /// (deleted, along with the `CALLS_PER_ANSWER` constant it depended on and the `NoBudget`
    /// refusal it alone produced): a composition root used to have to divide
    /// `server.request_timeout_seconds` by how many calls one answer makes before filling this in,
    /// because neither call carried a budget the other could see. The port now carries one
    /// `sutura_domain::warehouse::deadline::Deadline` shared by every call one answer makes, so what
    /// this type bounds is narrower and needs no division: the boot path, which has no such
    /// `Deadline` to read, and the socket ceiling every call is pinned to regardless. A composition
    /// root fills this from `server.request_timeout_seconds` directly.
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

//! The WIRE: one [`JobTransport`] that speaks to a `BigQuery` endpoint over HTTP.
//!
//! **This is the seam `docs/adr/0017` left open, filled in by the decision `docs/adr/0018` records.**
//! Behind the crate's default-off `wire` feature, because what arrives with it is an outbound TLS
//! stack and two of the four release triples are musl; that manifest argument is on the `ureq` entry
//! in the workspace root and is not repeated here.
//!
//! # What this module claims, and what it does not
//!
//! **This HAS now been run against a real project, and that is new.** On 2026-08-30 the three tests
//! in `crates/sutura-exec-bigquery/tests/acceptance.rs` passed against a real dataset from a
//! developer's machine, under a service-account key: the endpoint accepted a statement this repository
//! generated, answered it as one complete page, and the numbers were the fixture's. **It is the first
//! time anything here has had a statement accepted by `BigQuery`.**
//!
//! **What that does NOT establish, stated first because a green run invites the larger reading.** It
//! is ONE hand-built `SUM` over a two-column fixture - no join, no `COUNT(DISTINCT`, no `CASE WHEN`,
//! no `NULLIF` ratio, no `CAST(... AS FLOAT64)`, no `ISOWEEK` - and `ISOWEEK` plus `DATE_TRUNC`'s
//! argument order are precisely the two things `docs/adr/0017` MEASURED the parse check to be blind
//! about. The leg that record specifies is the corpus compared against the engine, and it is not
//! built. So: *one statement accepted*, not *the corpus accepted*.
//!
//! What the suite beside this module proves is separate and unchanged: that *this code builds the
//! request it says it builds and reads the answer it says it reads*, over documents that are not the
//! service's.
//!
//! So: still *Built And Not Wired* in AGENTS.md, two lines further along. `sutura-serve` links no
//! `BigQuery` adapter and refuses `kind: bigquery` by name, and the `data_systems:` axis of the
//! golden matrix still gains no entry - one live statement is not a registered data system.
//!
//! # What this module decides, and every one of them is pinned by a TYPE or by a test
//!
//! - **A job is bounded in TIME and in MONEY, and neither bound is a constant here.** [`JobBounds`]
//!   carries both, [`WireAgent`] carries the `JobBounds`, and [`BigQueryWire`] can only be built from
//!   a `WireAgent` - so there is no way to submit a job this deployment did not bound. `jobTimeoutMs`
//!   is what cancels a job at the service (`timeoutMs` alone does NOT: it bounds how long the client
//!   waits, and an expired one leaves the job running and billing), and `maximumBytesBilled` is what
//!   stops a question scanning a petabyte - neither the row cap nor the one-page refusal bounds bytes
//!   scanned.
//! - **One page or a refusal.** `jobs.query` answers one page, and completeness is stated as
//!   `totalRows` beside the rows rather than by the rows alone. A `pageToken`, an incomplete job or a
//!   delivered count short of the reported total is refused here - see [`WireError::MoreThanOnePage`]
//!   and [`WireError::NotComplete`] - because to `answer()` a first page would read as *under the
//!   cap, not truncated*, which is the exact row the row-cap invariant exists to hold. **The cost, and
//!   it is a real one:** a wide result reaches a caller as `BigQueryError::Endpoint`, which the
//!   transports answer as a `503` - a status that invites a retry that will produce the same page.
//!   `ResultTooLarge` is what it means, and the port cannot say it: `Warehouse::working_set_exhausted`
//!   is a predicate returning a ceiling in bytes precisely so an adapter cannot mint an arbitrary
//!   `RefusalReason`. Widening that port is an architecture decision, so this is flagged rather than
//!   taken, and AGENTS.md's row says so out loud.
//! - **The service's own result cache is turned OFF.** Not for cost: an anchor that reproduces from a
//!   cache has reproduced the cache, which is `differential.rs`'s own argument. And a cached answer
//!   under a *shared* identity is shared across every asker, so leaving it on would put the
//!   cross-user leak this crate refuses one layer below the code the per-subject step has to change.
//! - **The bearer's DESTINATION is a compile-time constant; its ROUTE is not, and the difference is
//!   worth stating precisely** because an earlier version of this header overstated it.
//!   `HOST` cannot be configured, `https_only` is on and `max_redirects` is `0`, so nothing a
//!   deployment writes can change *which service* receives the credential. What a deployment CAN
//!   change is the path: `ureq`'s default config is `Proxy::try_from_env()`, so `HTTPS_PROXY` routes
//!   these requests through an egress proxy. That is left ON deliberately - an egress proxy is a real
//!   deployment shape here, `docs/enterprise-mirrors.md` is the generic form of it - and it is safe
//!   because the tunnel is still TLS to `HOST` verified against a compiled-in root set, so a proxy
//!   sees a hostname and no bytes. It is written out in [`WireAgent::pinned`] rather than inherited,
//!   so it is a
//!   decision a reviewer can disagree with.
//! - **Failure is derived from the RESULT SHAPE and never from `errors` being non-empty.** The
//!   endpoint documents that array as *"the first errors or warnings encountered"* and says entries
//!   *"do not necessarily mean that the job has completed or was unsuccessful"* - so refusing on it
//!   would decline successful queries that merely warned. What refuses is `jobComplete`, a
//!   `pageToken`, an absent `totalRows`, and a delivered count that is not the reported total; the
//!   reported `reason` is folded into whichever of those fires, because it is the best diagnostic
//!   available at that point. See `complete`, and the limit stated there.
//! - **Every foreign string that reaches an error is bounded and filtered.** The endpoint's
//!   `reason` is kept and its free-text `message` is not, because a reason is a fixed vocabulary an
//!   operator can act on and a message is unbounded text from another service heading for a log.
//!   `credential::bounded` is the one function that does it, shared with the credential module.
//!
//! # What is deliberately absent
//!
//! - **Paging.** A result bigger than one page is refused rather than assembled. `getQueryResults`
//!   needs the job's `location` for a dataset outside the two multi-regions, and
//!   `SourcePlacement::BigQuery` declares no `location` - `docs/adr/0017` says why that field is not
//!   in this repository yet and that the change adding the wire is the one that decides it. **This
//!   change decides it by not needing it**, and the cost is the refusal above.
//! - **A `location` on the request.** Same reason, one size smaller.
//! - **Retries.** A refused job comes back as [`WireError`] and reaches a caller as
//!   `BigQueryError::Endpoint`, whose transport-facing status is a `503`. Retrying inside an adapter
//!   would spend a caller's request timeout on a decision the caller cannot see.
//! - **Surfacing a warning on a result that IS complete.** There is nowhere to put it: `RowSet` has
//!   no field for it and this crate has no logging dependency, so adding one for a line nobody has
//!   ever seen is a dependency decision this change does not take. Stated because a dropped warning
//!   is exactly the kind of absence that reads as "there were none".
//!
use core::time::Duration;

use crate::transport::{Cell, JobRequest, JobRows, JobTransport};
use crate::wire::credential::{AccessTokens, QuotaProject};

pub mod credential;
mod document;

#[cfg(test)]
mod tests;

// The document half, re-exported into this module so `wire.rs` stays the one path callers and tests
// read - the split is a file boundary rather than an API one.
use crate::wire::document::{QueryAnswer, body, complete, refusal, url};

/// The API this module speaks to. A compile-time constant: there is no configuration key for it, so
/// no deployment can choose which service receives the credential. What a deployment CAN choose is
/// the route - see the module header on the proxy.
const HOST: &str = "https://bigquery.googleapis.com";

/// How much longer than the job's own deadline the socket may wait.
///
/// **The number that stops a pool thread outliving the caller it was answering.** A job is cancelled
/// at the service after [`JobBounds::deadline`], and the client stops waiting for the response at the
/// same instant, so this covers only connection setup and the last bytes of the answer. It used to be
/// the other way round - a 70-second socket over a 55-second job wait against a transport whose own
/// default timeout is 30 seconds - which held a blocking-pool thread for up to 40 seconds after the
/// request it served had gone.
const CONNECT_MARGIN: Duration = Duration::from_secs(5);

/// A cap on the answer this module will read into memory.
///
/// One page of `jobs.query` is capped by the endpoint at about ten megabytes, so this is above what
/// it will send and below what would matter; what it actually defends is the case where something
/// that is not the endpoint answers. **Not a bound on bytes SCANNED** - that is
/// [`JobBounds::max_bytes_billed`], and confusing the two is how a bounded-looking question costs a
/// four-figure sum.
const MAX_ANSWER_BYTES: u64 = 32 * 1024 * 1024;

/// A cap on response headers, which are read before any body.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// How much of a foreign short token is kept. `accessDenied` is twelve characters, `invalid_grant`
/// thirteen and `authorized_user` fifteen; anything past this is not one of those.
const MAX_FOREIGN_CHARS: usize = 64;

/// The header that names which project's quota and billing a request is attributed to.
///
/// **Required rather than optional for the credential this build reads.** An application-default
/// credential is an END-USER credential, and the endpoint's own direct-REST guidance is that a
/// user credential needs a quota project stated on the request - without it a perfectly valid token
/// comes back refused, with a message about user credentials not being supported, which reads as an
/// authentication fault and is not one.
const QUOTA_PROJECT_HEADER: &str = "x-goog-user-project";

/// How long a job may run, and how long the client waits for its answer.
///
/// **A newtype rather than a constant, because the value belongs to the deployment.** The number that
/// makes sense here is the one the transport in front of this service already uses -
/// `server.request_timeout_seconds`, which ships as 30 - and a constant in this file would be a
/// second copy of it that drifts the day somebody changes the first. A composition root fills this
/// from that setting; until one links this crate, the acceptance leg is the only caller and it says
/// where its number comes from.
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

impl QueryDeadline {
    /// The longest deadline this adapter will send.
    ///
    /// Six hours is the endpoint's own ceiling for a query job. A deployment that wants longer wants
    /// a batch job, which is a different API and a different decision.
    const MAX_SECONDS: u64 = 6 * 60 * 60;

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

    /// How long the socket may stay open: the job's deadline plus connection setup.
    #[must_use]
    pub const fn socket(self) -> Duration {
        Duration::from_secs(self.seconds.saturating_add(CONNECT_MARGIN.as_secs()))
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
/// forget the other, and so [`WireAgent`] can carry them both.
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

/// The client every request in this crate goes through, with the four settings that matter PINNED BY
/// THE TYPE rather than by a call site.
///
/// **This newtype is the whole mechanism, and it exists because the previous shape was a convention.**
/// The settings below used to live in a free function returning a bare `ureq::Agent`, and both
/// [`BigQueryWire::new`] and `credential::ApplicationDefault::read` accepted any agent - so a
/// composition root writing `ureq::Agent::new_with_defaults()` got redirects on, plaintext allowed
/// and no timeout, while every test passed because the tests all called the right function. A private
/// field with one constructor is what *a newtype parses rather than validates* asks for: if an
/// instance of this exists, the pins hold.
///
/// It also carries the [`JobBounds`], so the deadline that shapes the socket timeout and the deadline
/// that goes into the request body are **the same value**. Two arguments could have disagreed.
#[derive(Debug, Clone)]
pub struct WireAgent {
    agent: ureq::Agent,
    bounds: JobBounds,
}

impl WireAgent {
    /// The one constructor, and every non-default setting below is a decision:
    ///
    /// - `http_status_as_error(false)`, because the client's default turns a `4xx` into an error and
    ///   discards the body - and the body is where the endpoint says *which* refusal this is. Status is
    ///   read explicitly instead, in `refusal`.
    /// - `https_only(true)`, so a bearer token cannot leave over plaintext even if a URL somewhere
    ///   loses its scheme. `HOST` is already `https`; this is the second lock.
    /// - `max_redirects(0)`, so the credential has no second host to reach. `ureq-proto` also strips
    ///   `authorization` on a redirect, which was verified rather than assumed - so this is belt and
    ///   braces, and the belt is ours.
    /// - `timeout_global`, at the job's deadline plus `CONNECT_MARGIN`, so the socket cannot outlive
    ///   the job it is waiting for by more than connection setup.
    /// - `max_response_header_size`, because headers are read before the body's own limit applies.
    /// - `proxy(Proxy::try_from_env())`, which is the client's own default WRITTEN OUT rather than
    ///   inherited. An egress proxy is a legitimate deployment shape and the tunnel is still TLS to
    ///   `HOST` against a compiled-in root set, so what the environment chooses is the route and not
    ///   the destination. The module header states that distinction, because a previous version of it
    ///   claimed the stronger thing.
    #[must_use]
    pub fn pinned(bounds: JobBounds) -> Self {
        Self {
            agent: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .http_status_as_error(false)
                    .https_only(true)
                    .max_redirects(0)
                    .timeout_global(Some(bounds.deadline().socket()))
                    .max_response_header_size(MAX_HEADER_BYTES)
                    .proxy(ureq::Proxy::try_from_env())
                    .build(),
            ),
            bounds,
        }
    }

    /// The client, for the two modules in this crate that send a request.
    ///
    /// `pub(crate)`, so nothing outside can take the agent out of its wrapper and reconfigure it.
    #[inline]
    pub(crate) const fn agent(&self) -> &ureq::Agent {
        &self.agent
    }

    /// What every job through this client is bounded by.
    #[inline]
    #[must_use]
    pub const fn bounds(&self) -> JobBounds {
        self.bounds
    }
}

/// One fallible step of the wire.
///
/// Named for the reason `crate::Mapped` is named: `Result<V, WireError<C::Error>>` is over the
/// `type_complexity` threshold this workspace tightened, and erasing the generic would lose which
/// credential source failed.
type Wired<V, C> = Result<V, WireError<C>>;

/// The cells one page delivered, in the shape [`JobRows::of`] takes them.
///
/// Named for the same reason [`Wired`] is: `Wired<Vec<Vec<Cell>>, C>` is over the threshold, and this
/// is the one place in the crate where the nesting is unavoidable - a page is rows of cells.
type Grid = Vec<Vec<Cell>>;

/// Whether a submission reads data or only validates.
///
/// **An enum rather than a `bool`, because `submit(request, true)` at a call site says nothing.** The
/// two calls have different costs - one is billed and one is documented as using no slots and not
/// being charged - which is exactly the distinction a reader needs at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DryRun {
    /// Validate only. Not billed, no slots, no rows.
    Yes,
    /// Run the job.
    No,
}

impl DryRun {
    /// The flag as the request body writes it.
    const fn asked(self) -> bool {
        matches!(self, Self::Yes)
    }
}

/// Why the endpoint did not answer with rows.
///
/// Generic in the credential source's own error, for the reason [`crate::BigQueryError`] is generic
/// in this one: a caller that knows which credential source is installed can still tell a missing
/// file from a refused refresh, and erasing it here would be the information this whole chain of
/// generics exists to keep.
///
/// **`ureq::Error` appears as a `#[source]` and never as a variant this type re-exports**, which is
/// the shape *Structured Errors* asks for at a boundary: the variant is ours, the chain still walks,
/// and a caller who knows the transport can downcast. It is boxed because it is much larger than
/// every other variant and `clippy::result_large_err` is on.
#[derive(Debug, thiserror::Error)]
pub enum WireError<C>
where
    C: core::error::Error + 'static,
{
    /// No token could be produced, so nothing was sent.
    #[error("no credential was available to submit the job with")]
    Credential {
        #[source]
        cause: C,
    },
    /// A token was produced and its deadline had already passed.
    ///
    /// **Checked here rather than trusted, and it is the one check that would be pointless if
    /// anything were cached.** A source that hands back an expired token is a source with a clock
    /// problem, and presenting it turns that into a `401` from the data system - which reads to an
    /// operator as a permissions fault.
    #[error("the credential expired {at} seconds after the epoch, and it is now {now}")]
    Expired { at: u64, now: u64 },
    /// This process could not read a wall clock.
    ///
    /// Reachable only on a machine whose clock is before the epoch. It is a variant rather than a
    /// fallback because the alternative is presenting a token whose deadline nothing compared.
    #[error("this process could not read the time, so no credential deadline could be compared")]
    NoClock {
        #[source]
        cause: std::time::SystemTimeError,
    },
    /// The request could not be serialized.
    ///
    /// **A defect-only path, and it is named rather than unwrapped.** Everything in the body is a
    /// string, a bool or a number, so nothing here can fail the serializer; the variant exists
    /// because `unwrap` is denied and a silent `unwrap_or_default` would send a different query.
    #[error("the request body could not be serialized, which is a defect in this adapter")]
    RequestNotSerializable {
        #[source]
        cause: serde_json::Error,
    },
    /// The endpoint was not reached.
    #[error("the endpoint was not reached")]
    Unreachable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The endpoint answered and the answer could not be read.
    #[error("the endpoint's answer could not be read")]
    Unreadable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The endpoint refused.
    ///
    /// The status and the endpoint's own `reason`, and deliberately not its message - see the module
    /// header. An absent or unparseable error document leaves `named` empty, which is honest: the
    /// status is what is guaranteed.
    #[error("the endpoint refused the job with {status}: {named}: {detail}")]
    Refused { status: u16, named: String, detail: String },
    /// The answer was not the document a query response is.
    #[error("the endpoint's answer was not a query response")]
    NotADocument {
        #[source]
        cause: serde_json::Error,
    },
    /// The job had not finished when the endpoint answered.
    ///
    /// **Refused rather than polled.** The alternative is `getQueryResults`, which needs a `location`
    /// this deployment does not declare - see the module header - and a partial answer is a wrong
    /// number under a certified name.
    ///
    /// **It should now be reachable only through a defect or a cancellation**, because the request
    /// carries `jobTimeoutMs` equal to the client's own wait: the service cancels the job at the same
    /// instant the client stops waiting for it, so an incomplete answer is no longer a live job this
    /// adapter walked away from. `named` carries whatever reason the endpoint reported, which for a
    /// cancelled job is the useful half.
    #[error("the job had not finished when the endpoint answered: {named}")]
    NotComplete { named: String },
    /// The answer is one page of more than one.
    #[error("the endpoint answered with one page of a larger result")]
    MoreThanOnePage,
    /// A complete job that stated no total.
    ///
    /// **Refused rather than read as zero**, because zero is what a complete empty result and a
    /// missing field both look like, and only one of them is an answer this adapter may certify.
    ///
    /// This is also where a FAILED job lands: the endpoint reports one as complete with no total, so
    /// `named` carries the reason it gave and is the whole diagnostic. That is why failure is derived
    /// from the shape here rather than from `errors` being non-empty - see `reported`.
    #[error("the endpoint reported the job complete and stated no total row count: {named}")]
    NoTotal { named: String },
    /// The total was not a number.
    ///
    /// It arrives as text, because the endpoint writes 64-bit integers as JSON strings.
    #[error("the endpoint's total row count was not a number")]
    NotATotal {
        #[source]
        cause: core::num::ParseIntError,
    },
    /// A complete job with rows and no schema to read them against.
    #[error("the endpoint returned {rows} rows and no schema")]
    NoSchema { rows: usize },
    /// A cell that is neither a string nor a null.
    ///
    /// Every scalar the endpoint returns is JSON text whatever its declared type; an array or an
    /// object is a `REPEATED` or `RECORD` column, which is outside `crate::transport::FieldType`'s closed
    /// vocabulary. It names the position rather than the value, because the value is a row.
    #[error("the cell at row {row}, column {column} is not a scalar")]
    NotAScalar { row: usize, column: usize },
}

/// A short token another service sent us, bounded and filtered.
///
/// **One function for every foreign string in this crate that reaches an error**, because there were
/// two and they had drifted by one character in their allowed set. Both callers want the same thing:
/// an endpoint's `reason`, an `OAuth` error code and a credential file's `type` are each a fixed
/// vocabulary spelled in the same characters, and what has to be impossible is any of them writing a
/// newline, an escape sequence or sixteen kilobytes into a log.
///
/// **Not a slice**, because `clippy::string_slice` is denied and because a byte slice of foreign text
/// can land inside a multi-byte character. Taking characters is both correct and what the ban is for.
pub(crate) fn bounded(named: Option<String>) -> String {
    named
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .take(MAX_FOREIGN_CHARS)
        .collect()
}

/// A `BigQuery` endpoint, reached over HTTP.
///
/// Generic in its credential source rather than holding a boxed one, for the reason
/// [`crate::BigQueryWarehouse`] is generic in its transport: there is one per process, it is chosen
/// at composition, and a generic keeps the source's own error type visible in [`WireError`].
///
/// It holds a [`WireAgent`] and not a `ureq::Agent`, which is what makes the module header's claims
/// properties of this type rather than of whichever function a composition root happened to call.
#[derive(Debug)]
pub struct BigQueryWire<C> {
    agent: WireAgent,
    credentials: C,
}

impl<C> BigQueryWire<C>
where
    C: AccessTokens,
{
    /// Opens a transport.
    ///
    /// The [`WireAgent`] is a parameter rather than something built here so it can be the same one
    /// the credential source refreshes through - one connection pool, one set of pins, and one
    /// [`JobBounds`] shared by the socket timeout and the request body.
    #[must_use]
    pub const fn new(agent: WireAgent, credentials: C) -> Self {
        Self { agent, credentials }
    }

    /// Seconds since the epoch, or a named refusal.
    ///
    /// The one clock read in this crate. It is here rather than in the credential source because
    /// [`AccessTokens::bearer`] takes the instant as an argument, which is what makes an expiry
    /// testable without one.
    fn now() -> Wired<u64, C::Error> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
            .map_err(|cause| WireError::NoClock { cause })
    }

    /// Sends one job and returns the endpoint's answer, checked as far as *the service accepted
    /// this*.
    ///
    /// The completeness and row checks are NOT here, because a dry run has neither rows nor a total
    /// and would fail them - see [`Self::validate_job`] and [`Self::run_job`], which are the two
    /// callers and hold the two different conclusions.
    fn submit(&self, request: &JobRequest<'_>, dry_run: DryRun) -> Wired<QueryAnswer, C::Error> {
        let now = Self::now()?;
        let bearer = self
            .credentials
            .bearer(now)
            .map_err(|cause| WireError::Credential { cause })?;
        // Checked BEFORE anything is built or sent, which is why an expired credential costs no round
        // trip - and why it is the one guard in this function a test can reach with no socket.
        if let Some(at) = bearer.not_after().passed_by(now) {
            return Err(WireError::Expired { at, now });
        }
        let document = serde_json::to_vec(&body(request, dry_run, self.agent.bounds()))
            .map_err(|cause| WireError::RequestNotSerializable { cause })?;
        // `Secret::expose` is the one greppable call that lets the token out, and it lets it out into
        // a header value the client parses rather than into a string it concatenates - so a token
        // carrying a newline is a refused request at `send` rather than a second header. That is the
        // library's guarantee and not ours, which is why it is a comment here and not a row in
        // AGENTS.md's table.
        let mut sending = self
            .agent
            .agent()
            .post(url(request))
            .header("authorization", format!("Bearer {}", bearer.token().expose()))
            .header("content-type", "application/json");
        // **The quota project, and whether to send it AT ALL is the credential's answer rather than
        // this function's** - which is the correction that made `AccessTokens::quota_project` a
        // required method. An END-USER credential without this header is refused with a message about
        // user credentials not being supported; a SERVICE ACCOUNT with it needs
        // `serviceusage.services.use` on the project, which one holding only dataset grants does not
        // have. So the header that makes the first work breaks the second.
        //
        // The value, where it is sent, is the source's DECLARED billing project - this deployment's own
        // statement about who pays. The credential file's own `quota_project_id` is deliberately not
        // read, because two answers to that question which can disagree silently is worse than one a
        // reviewer can see in a settings file.
        match self.credentials.quota_project() {
            QuotaProject::Required => {
                sending = sending.header(QUOTA_PROJECT_HEADER, request.billing_project().as_str());
            }
            QuotaProject::FromTheCredential => {}
        }
        let mut answer = sending
            .send(&document)
            .map_err(|cause| WireError::Unreachable { cause: Box::new(cause) })?;
        let status = answer.status();
        let text = answer
            .body_mut()
            .with_config()
            .limit(MAX_ANSWER_BYTES)
            .read_to_string()
            .map_err(|cause| WireError::Unreadable { cause: Box::new(cause) })?;
        if !status.is_success() {
            return Err(refusal(status.as_u16(), &text));
        }
        serde_json::from_str(&text).map_err(|cause| WireError::NotADocument { cause })
    }

    /// What a dry run may conclude, which is *the endpoint accepted this statement* and nothing more.
    ///
    /// **A `2xx` is the whole test, and both of the things it deliberately does not check are worth
    /// naming.** `jobComplete` is not required, because a dry run creates no job and what that field
    /// means for one is not something this repository can verify; requiring it would risk refusing a
    /// validation that succeeded. And `errors` is not read, because a dry run that FAILS is an HTTP
    /// error rather than a `200` carrying a reason - the endpoint's own shape - while a `200` carrying
    /// entries is a warning, and refusing on those is the defect the module header describes.
    ///
    /// **What it does not do, and could:** a dry run returns `statistics.totalBytesProcessed`, and
    /// this discards it. The bound that matters is already on the request as `maximumBytesBilled`,
    /// enforced by the service, so a client-side comparison here would be a second and weaker copy of
    /// it. `PreFlight::Accepted` also carries no field for an estimate, so there is nowhere to put it.
    fn validate_job(&self, request: &JobRequest<'_>) -> Wired<(), C::Error> {
        self.submit(request, DryRun::Yes).map(|_| ())
    }

    /// The rows one job produced, or the reason they are not a complete result.
    fn run_job(&self, request: &JobRequest<'_>) -> Wired<JobRows, C::Error> {
        complete(self.submit(request, DryRun::No)?)
    }
}

impl<C> JobTransport for BigQueryWire<C>
where
    C: AccessTokens,
{
    type Error = WireError<C::Error>;

    fn run(&self, request: &JobRequest<'_>) -> Result<JobRows, Self::Error> {
        self.run_job(request)
    }

    fn validate(&self, request: &JobRequest<'_>) -> Result<(), Self::Error> {
        self.validate_job(request)
    }
}

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
//! So: one live statement is not a registered data system, and the `data_systems:` axis of the
//! golden matrix still gains no entry. `sutura` DOES link this adapter and dispatch
//! `kind: bigquery` behind its default-off `bigquery` feature; the sentence that used to stand here
//! said it linked none, which `docs/adr/0017`'s second amendment had already spent.
//!
//! # What this module decides, and every one of them is pinned by a TYPE or by a test
//!
//! - **A job is bounded in TIME and in MONEY, and neither bound is a constant here.** [`JobBounds`]
//!   carries both, [`WireAgent`] carries the `JobBounds`, and [`BigQueryWire`] can only be built from
//!   a `WireAgent`. `jobTimeoutMs` is what cancels a job (`timeoutMs` alone does NOT: it bounds the
//!   client's own wait, and an expired one leaves the job running and billing); `maximumBytesBilled`
//!   stops a question scanning a petabyte, which neither the row cap nor the one-page refusal does.
//! - **The time bound is ONE ABSOLUTE DEADLINE PER ANSWER, opened by the port and not by this
//!   adapter, and this bullet exists because the earlier two shapes were each the second thing while
//!   claiming the first.** `timeout_global` on the agent once gave every HTTP operation a full budget
//!   of its own, so a review measured one ANSWER at four independent budgets against a transport
//!   whose own request timeout is thirty seconds. [`CallDeadline`], opened once per CALL, fixed that
//!   leak - and then could not fix the next one, because neither `Warehouse` nor [`JobTransport`]
//!   took a deadline, so the two calls one answer makes still could not share one; a composition
//!   root's own configured job bounds substituted an arithmetic that divided the request timeout by
//!   how many calls one answer makes, checked by nothing outside this crate. **`docs/adr/0029` now
//!   carries a `Deadline` across the port itself** - one absolute instant,
//!   opened by the transport at the answer's arrival and shared by every leg. `submit` reads what it
//!   says is left via [`crate::transport::JobRequest::deadline`] and opens a [`CallDeadline`] FROM
//!   that via [`CallDeadline::opened_at_for`], so a slow token exchange shortens the job that
//!   follows it rather than being followed by one with a full budget of its own, and `timeoutMs`/
//!   `jobTimeoutMs` are what is left of THAT rather than of this adapter's own configured job bounds.
//!   A budget spent before the job is [`WireError::DeadlineSpent`] rather than a send - checked
//!   BEFORE the credential exchange too, since a spent caller should not spend it on an exchange
//!   nobody waits for. **The boot path has no port `Deadline` to read** (`verify_anchor`, a fixture
//!   load or drop, the identity read) and opens a fresh window from this adapter's own configured
//!   [`JobBounds`] instead, exactly as every call did before this record. Pinned at [`call_body`];
//!   that `submit` hands it the exchange's own `call` is READ, not measured (`HOST` is unreachable).
//! - **One page or a refusal.** `jobs.query` answers one page, and completeness is stated as
//!   `totalRows` beside the rows rather than by the rows alone. The wire refuses a `pageToken`
//!   (`WireError::MoreThanOnePage`) and a job that did not finish (`WireError::NotComplete`); the
//!   delivered count that is not the reported total is refused one port further out, in the adapter's
//!   `BigQueryWarehouse::rows` as `BigQueryError::Incomplete` - `complete` here compares nothing, it
//!   hands the rows and the total to the adapter - because to `answer()` a first page would read as
//!   *under the cap, not truncated*, which is the exact row the row-cap invariant exists to hold.
//!   **And a wide
//!   result now leaves as a REFUSAL rather than as a `503`, which is a correction to what this header
//!   used to say was the cost.** It used to reach a caller as `BigQueryError::Endpoint`, which both
//!   transports answer as the status a dead endpoint produces - inviting a retry that returns the same
//!   page. `ResultTooLarge` is what it means, and the port can now say it: `result_did_not_fit` on
//!   [`crate::transport::JobTransport`] answers it for [`WireError::MoreThanOnePage`], the adapter
//!   passes it up through `Warehouse::result_did_not_fit`, and a caller gets `413 result_too_large`
//!   carrying `ResultBound::Volume` - a bound with no number, because the reply cap is the service's
//!   and it reports neither that nor the size of the reply that hit it. `NotComplete` deliberately
//!   answers `false`: a job that ran out of time may finish on a retry.
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
//!   because the tunnel is still TLS to `HOST`, so a proxy sees a hostname and no bytes. It is
//!   written out in [`WireAgent::pinned`] rather than inherited, so it is a decision a reviewer can
//!   disagree with.
//! - **Which roots verify `HOST` is `ureq`'s compiled-in set by default, and a deployment MAY declare
//!   its own instead - `github.com/telekom/sutura#125`.** [`WireAgent::pinned`] is unchanged: it
//!   verifies against `ureq`'s own `RootCerts::WebPki`, exactly as before this change.
//!   [`WireAgent::secured`] is the second constructor `security.outbound.transport_anchors` reaches: a
//!   composition root resolves the declaration through `sutura_tls::load_anchors` once at boot and
//!   hands the loaded certificates here, which replaces `RootCerts::WebPki` with `RootCerts::Specific`
//!   built from exactly that bundle or host store - never both. `crate::wire::tls` is the one place
//!   either constructor turns `sutura_tls`'s `CertificateDer` output into `ureq`'s own certificate
//!   type, so the conversion is written once rather than at every call site. **No client identity
//!   travels this way**: `security.outbound` is anchors only - Google's endpoints take a bearer
//!   token, not mTLS, so there is no `ClientCert` this module ever builds.
//! - **Failure is derived from the RESULT SHAPE and never from `errors` being non-empty.** The
//!   endpoint documents that array as *"the first errors or warnings encountered"* and says entries
//!   *"do not necessarily mean that the job has completed or was unsuccessful"* - so refusing on it
//!   would decline successful queries that merely warned. What refuses is `jobComplete`, a
//!   `pageToken`, an absent `totalRows`, and a delivered count that is not the reported total - the
//!   last of those in the adapter (`BigQueryWarehouse::rows`, `BigQueryError::Incomplete`), not here;
//!   the reported
//!   `reason` is folded into whichever of those fires, because it is the best diagnostic
//!   available at that point. See `complete`, and the limit stated there.
//! - **Refusal text is closed, not passed through.** [`ReasonCode`] maps the endpoint's reason to a
//!   fixed local vocabulary and turns an unrecognized value into a static marker;
//!   [`EndpointMessage`] retains the free-text `message` and redacts it under `Debug`. `Display` on
//!   the refusal renders the status and the local reason code and never the message, so a cause-chain
//!   walk cannot carry endpoint text either - see [`WireError::Refused`] for the limit.
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
use crate::transport::{Cell, DatasetAddress, HeldTables, JobRequest, JobRows, JobTransport};
use crate::wire::credential::{AccessTokens, QuotaProject};

// The bounds are re-exported flat, so `wire::JobBounds` stays the path every call site reads:
// they are handed to `WireAgent::pinned` on the line beside it, and a second segment there would
// buy nothing. The module is PUBLIC rather than private, which is the half worth writing down -
// `docs/.tools/rustdoc_to_markdown.py` renders a `use` item by its own name, and a re-export has
// none, so a `pub use` out of a private module reaches the generated page as an undocumented
// `use None` stub. Measured on `pub use sts::StsOverHttp`, which is on that page as exactly that.
pub mod bounds;
mod budget;
pub mod credential;
pub mod document;
mod iamcredentials;
mod sts;
mod tables;
mod tls;
pub use bounds::{BytesBilledCeiling, CallDeadline, JobBounds, QueryDeadline, UnusableBound};
pub use iamcredentials::IamCredentialsOverHttp;
pub use sts::StsOverHttp;

#[cfg(test)]
mod tests;

// The document half, re-exported into this module so `wire.rs` stays the one path callers and tests
// read - the split is a file boundary rather than an API one.
use crate::wire::budget::{call_body, configured_budget_seconds, remaining_of_the_ports_deadline};
#[cfg(feature = "fixtures")]
use crate::wire::document::applied;
use crate::wire::document::{QueryAnswer, QueryBody, complete, estimated_bytes, refusal, url};

/// The API this module speaks to. A compile-time constant: there is no configuration key for it, so
/// no deployment can choose which service receives the credential. What a deployment CAN choose is
/// the route - see the module header on the proxy.
const HOST: &str = "https://bigquery.googleapis.com";

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
    agent: sutura_tls::Rotating<ureq::Agent>,
    bounds: JobBounds,
}

impl WireAgent {
    /// The compiled-in-roots constructor: [`Self::secured`] with no declared anchors.
    ///
    /// This is every deployment's behaviour before `github.com/telekom/sutura#125` and stays the
    /// default for one with no `security.outbound.transport_anchors` block - see [`Self::secured`]
    /// for the one setting that differs when a deployment declares one.
    #[must_use]
    pub fn pinned(bounds: JobBounds) -> Self {
        Self::secured(bounds, None)
    }

    /// The one place every non-default setting is decided, and every one of them is a decision:
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
    ///   `HOST`, so what the environment chooses is the route and not the destination. The module
    ///   header states that distinction, because a previous version of it claimed the stronger thing.
    /// - `tls_config`, over [`crate::wire::tls::config`] - `RootCerts::WebPki` (`ureq`'s own default)
    ///   for `anchors: None`, which is every call [`Self::pinned`] makes and every deployment before
    ///   `#125`; `RootCerts::Specific` built from `anchors` for `Some`, which is what
    ///   `security.outbound.transport_anchors` resolves to. No client identity: `security.outbound`
    ///   is anchors only, so there is no `ClientCert` in either arm.
    ///
    /// The agent is wrapped in a never-rotating [`sutura_tls::Rotating`] - this constructor has no
    /// declaration to re-read. The rotation lane is [`Self::rotating`], fed by
    /// [`Self::rotating_agent`].
    #[must_use]
    pub fn secured(bounds: JobBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self {
        Self::rotating(
            bounds,
            sutura_tls::Rotating::fixed(tls::agent_from_tls(bounds.deadline().socket(), tls::config(anchors))),
        )
    }

    /// The rotation-lane constructor over a handle built by [`Self::rotating_agent`].
    #[must_use]
    pub const fn rotating(bounds: JobBounds, agent: sutura_tls::Rotating<ureq::Agent>) -> Self {
        Self { agent, bounds }
    }

    /// Builds the wire's rotating agent handle for a declared `security.outbound.transport_anchors`
    /// set (and, when one is declared, the poll handle the composition root drives on
    /// [`sutura_tls::POLL_INTERVAL`]). `None` returns a fixed handle over `ureq`'s compiled-in roots
    /// (the pre-`#125` behaviour, nothing to re-read); `Some` rebuilds `RootCerts::Specific` from each
    /// freshly loaded bundle, adopted by the next request.
    ///
    /// # Errors
    ///
    /// The declared bundle cannot be loaded at boot.
    pub fn rotating_agent(
        bounds: JobBounds,
        anchors: Option<sutura_tls::Anchors>,
    ) -> Result<tls::OutboundAgent, sutura_tls::LoadError> {
        let Some(anchors) = anchors else {
            return Ok((
                sutura_tls::Rotating::fixed(tls::agent_from_tls(bounds.deadline().socket(), tls::config(None))),
                None,
            ));
        };
        let timeout = bounds.deadline().socket();
        let rebuild = move |loaded, _identity: Option<sutura_tls::LoadedIdentity>| {
            Ok::<_, sutura_tls::LoadError>(tls::agent_from_tls(timeout, tls::config(Some(loaded))))
        };
        let initial = tls::agent_from_tls(timeout, tls::config(Some(sutura_tls::load_anchors(&anchors)?)));
        let rotator = sutura_tls::Rotator::new(anchors, None, rebuild, initial);
        Ok((rotator.rotating(), Some(rotator)))
    }

    /// The client, for the two modules in this crate that send a request, as an `Arc` clone resolved
    /// from the rotating handle - the per-request read that adopts a rotation on the next request.
    ///
    /// `pub(crate)`, so nothing outside can take the agent out of its wrapper and reconfigure it.
    #[inline]
    pub(crate) fn agent(&self) -> std::sync::Arc<ureq::Agent> {
        self.agent.current()
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

/// One call's request body, and what was LEFT of its budget when it was built.
///
/// Named for the same reason [`Wired`]/[`Grid`] are: the tuple is over the `type_complexity`
/// threshold. The duration travels beside the body so `submit` reads the socket's timeout from the
/// SAME value the body was built from, not a second `call.remaining()`.
type CallBody<'job> = (QueryBody<'job>, core::time::Duration);

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

/// The endpoint's own message on a refusal: free text, and the one field here that can name an
/// account.
///
/// **A type rather than a `String`, because the rule it carries is about RENDERING and a rule about
/// rendering cannot be held at call sites.** `Display` on the whole refusal omits this field;
/// `Debug` redacts it; only an explicit accessor produces the raw value. That is the whole
/// mechanism, and it is here because the alternative was asking fourteen acceptance legs to
/// remember which formatter they used.
///
/// **Measured, which is why this exists.** A leg ending `.expect("the endpoint answered")` formats
/// its error with `Debug`, and `Debug` walks the struct: on a real refusal that printed
/// `Access Denied: ... permission: <an account>` into a public workflow log. Ten of the fourteen
/// legs `nix run .#bigquery-acceptance` invokes were in exactly that shape, and the job's
/// `::add-mask::` step covers the project, the dataset and the table - **not an account**.
///
/// **The refusal's own `Display` used to interpolate this field, and that made the type's
/// redaction narrower than it read.** A cause-chain walk that flattens every link with `Display` -
/// which is what the transports' sinks do - carried the message into a deployment's own log. That
/// no longer happens: [`WireError::Refused`]'s `Display` renders the status and the closed reason
/// code and never this field. The limit, stated next to the claim: the endpoint's message
/// remains a queryable string on the error TYPE, reached only by an explicit call - so a caller
/// that deliberately opts in to rendering it can. The free text is bounded and stripped on the way
/// in regardless - see [`Self::bounded`].
///
/// **Why a caller would never reach the raw value by accident, and the cost of that shape:** there
/// is no `Display` and no `Debug` here that prints the sentence - the only door is [`Self::as_str`],
/// named on purpose - so any formatter that would have leaked it cannot be written without naming
/// the field and calling that accessor. Keeping the raw sentence out of every ordinary rendering is
/// the one control this type holds; it does not change what the endpoint itself records on its side.

#[derive(Clone, PartialEq, Eq)]
pub struct EndpointMessage(String);

impl EndpointMessage {
    /// The endpoint's message, capped and stripped of anything that could forge a log line.
    ///
    /// Infallible: an absent message is an empty one, which is honest - the status is what is
    /// guaranteed.
    #[must_use]
    pub fn bounded(message: Option<String>) -> Self {
        /// Long enough for the endpoint's own sentences, short enough that a log line stays a line.
        const MAX_DETAIL_CHARS: usize = 400;

        Self(
            message
                .unwrap_or_default()
                .chars()
                .filter(|c| c.is_ascii_graphic() || *c == ' ')
                .take(MAX_DETAIL_CHARS)
                .collect(),
        )
    }

    /// The message itself, for a caller that has decided it may render it.
    ///
    /// Named rather than reached through `Deref`, which `cargo xtask check-newtype-leaks` refuses:
    /// a wrapper you can forget you are holding is not a wrapper.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for EndpointMessage {
    /// Redacted, and it says how much it is hiding so a reader knows the field was populated.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "<the endpoint's own message, {} char(s), redacted>", self.0.len())
    }
}

/// The fixed reason vocabulary this adapter exposes from an endpoint response.
///
/// Provider text is parsed into this type before it reaches an error. The endpoint may add a reason
/// this adapter does not know; that value becomes [`Self::Unrecognized`] and its text is discarded.
/// This keeps ordinary error rendering useful for known conditions without allowing provider-owned
/// text to become a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonCode {
    /// No reason was present in the response.
    Absent,
    /// The endpoint returned a reason outside this adapter's vocabulary.
    Unrecognized,
    /// The caller was not authorized.
    AccessDenied,
    /// The request was not valid for the service.
    InvalidQuery,
    /// The requested resource was not found.
    NotFound,
    /// The request exceeded a short-term service rate limit.
    RateLimitExceeded,
    /// The request exceeded a service quota.
    QuotaExceeded,
    /// The response exceeded the service's maximum response size.
    ResponseTooLarge,
    /// The service reported a temporary backend failure.
    BackendError,
}

impl ReasonCode {
    /// Maps provider text to the closed local vocabulary.
    pub(crate) fn from_provider(reason: Option<&str>) -> Self {
        match reason {
            None => Self::Absent,
            Some("accessDenied") => Self::AccessDenied,
            Some("invalidQuery") => Self::InvalidQuery,
            Some("notFound") => Self::NotFound,
            Some("rateLimitExceeded") => Self::RateLimitExceeded,
            Some("quotaExceeded") => Self::QuotaExceeded,
            Some("responseTooLarge") => Self::ResponseTooLarge,
            Some("backendError" | "jobBackendError" | "internalError") => Self::BackendError,
            Some(_) => Self::Unrecognized,
        }
    }

    /// The stable text this adapter renders for the code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "no reason supplied",
            Self::Unrecognized => "unrecognized reason",
            Self::AccessDenied => "accessDenied",
            Self::InvalidQuery => "invalidQuery",
            Self::NotFound => "notFound",
            Self::RateLimitExceeded => "rateLimitExceeded",
            Self::QuotaExceeded => "quotaExceeded",
            Self::ResponseTooLarge => "responseTooLarge",
            Self::BackendError => "backendError",
        }
    }
}

impl core::fmt::Display for ReasonCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
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
    /// This call's budget was gone before the job could be submitted.
    ///
    /// **Refused rather than sent with whatever budget was left, because there was none. Reachable
    /// two ways, and the first is new with `docs/adr/0029`:** the port's own `Deadline` was already
    /// spent when `submit` was asked to do anything at all - the exchange included, so a
    /// caller that ran out of time before this adapter was even reached does not spend it on an
    /// exchange nobody is still waiting for - and the older way, a token exchange slow enough to spend
    /// what was left of it before the job could be built. Submitting anyway would either mean an
    /// unbounded wait or a job the service keeps running after the client has stopped waiting, which
    /// is the pair of failures this whole shape exists to rule out. `budget_seconds` is the port's own
    /// configured budget where a request carried a `Deadline`, and this adapter's own configured
    /// [`JobBounds`] at the boot path, where there is no caller's budget to name.
    #[error("this call's {budget_seconds}-second budget was spent before the job could be submitted")]
    DeadlineSpent { budget_seconds: u64 },
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
    /// The status, the endpoint's reason mapped to [`ReasonCode`] in `named`, and its MESSAGE in
    /// `detail`. An absent or unrecognized reason carries a static local marker rather than provider
    /// text, which is honest: the status is what is guaranteed.
    ///
    /// **This used to say the message was deliberately not carried, and the field beside it was
    /// built from `error.message`.** The wrong half mattered: `detail` is free text the endpoint
    /// writes, it quotes the resource and the principal it refused, and `Display` interpolated it -
    /// so anything that rendered this variant into a public log leaked both. `ci.yml`'s masking step
    /// exists because of exactly that, and `tests/exchanged_identity.rs` prints `status` and `named`
    /// and never `detail` for the same reason.
    ///
    /// **`Display` does NOT render `detail`.** It prints the status and the closed reason code, and
    /// nothing else: a cause-chain walk that flattens every link with `Display` - which is what the
    /// transports' sinks do - carries the same pair and never endpoint-owned text.
    /// `detail` is an [`EndpointMessage`], whose `Debug` is redacted and whose raw value is reached
    /// only through an explicit accessor a caller has to opt into. Each of the three renderings
    /// this error can meet is therefore one of those, and the one that leaks is the one a caller
    /// cannot write by accident. `docs/adr/0018` carries this decision and its limit.
    #[error("the endpoint refused the job with {status}: {named}")]
    Refused {
        status: u16,
        named: ReasonCode,
        detail: EndpointMessage,
    },
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
    /// adapter walked away from. `named` carries the endpoint's reason as a closed [`ReasonCode`],
    /// which for a cancelled job is the useful half.
    ///
    /// **This is the DOCUMENTED shape [`crate::transport::JobTransport::deadline_exceeded`] answers
    /// `true` for, and it is stated as documented rather than measured because that is exactly what
    /// it is.** `timeoutMs` and `jobTimeoutMs` travel as the same number by construction - see
    /// [`crate::wire::document::body`] - so a synchronous `jobs.query` reply cannot say *the wait
    /// expired* without also saying *the service was asked to cancel at the same instant*: the
    /// endpoint's own documentation of `timeoutMs` is that an expired one answers `jobComplete:
    /// false`, which is this variant.
    ///
    /// **Not yet measured against a real endpoint, and that is stated here rather than implied.** An
    /// acceptance cell that raced a statement against a real deadline to reach exactly this reply was
    /// tried and reverted - the corpus fixture is a handful of rows, so the round trip reliably
    /// finishes before any budget short enough to matter, and a budget picked to "usually" lose that
    /// race flakes against a project that bills for it. `tests/tests/deadline.rs`'s cell proves the
    /// narrower claim instead - a spent port deadline refuses through the REAL wire and credential
    /// before a request is sent - and leaves this variant's own shape open, closed only by a
    /// statement that reliably outruns a real budget without depending on fixture size or jitter.
    #[error("the job had not finished when the endpoint answered: {named}")]
    NotComplete { named: ReasonCode },
    /// The answer is one page of more than one.
    #[error("the endpoint answered with one page of a larger result")]
    MoreThanOnePage,
    /// A complete job that stated no total.
    ///
    /// **Refused rather than read as zero**, because zero is what a complete empty result and a
    /// missing field both look like, and only one of them is an answer this adapter may certify.
    ///
    /// This is also where a FAILED job lands: the endpoint reports one as complete with no total, so
    /// `named` carries the endpoint's reason as a closed [`ReasonCode`] and is the whole diagnostic.
    /// That is why failure is derived from the shape here rather than from `errors` being non-empty -
    /// see `reported`.
    #[error("the endpoint reported the job complete and stated no total row count: {named}")]
    NoTotal { named: ReasonCode },
    /// The total was not a number.
    ///
    /// It arrives as text, because the endpoint writes 64-bit integers as JSON strings.
    #[error("the endpoint's total row count was not a number")]
    NotATotal {
        #[source]
        cause: core::num::ParseIntError,
    },
    /// `totalBytesProcessed` was present and not a number.
    ///
    /// Same reason as [`Self::NotATotal`]: the endpoint writes this 64-bit count as a JSON string
    /// too. Refused rather than read as [`None`] - which is reserved for the field being ABSENT -
    /// because a value that arrived and did not parse is a shape this adapter does not understand,
    /// not a dry run that declined to price.
    #[error("the endpoint's total bytes processed was not a number")]
    NotAnEstimate {
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
    /// The answer to a table listing was not one. Distinct from [`Self::NotADocument`], the same
    /// failure for a query answer: two documents, two shapes, and one message per request.
    #[error("the endpoint's answer was not a table listing")]
    NotAListing {
        #[source]
        cause: serde_json::Error,
    },
    /// The service handed back a page token this transport will not write into a URL. **Refused
    /// rather than filtered**, and `tables::usable_token` carries the argument; the token travels
    /// through [`bounded`], which keeps a foreign string out of a log unbounded.
    #[error("the endpoint's next page token is not one this transport can send: {named}")]
    UnusablePageToken { named: String },
    /// A dataset that did not finish listing inside the page bound. **A failure rather than a short
    /// listing**: this feeds *these tables are absent*, so a cut-off listing reports a table that is
    /// there as missing.
    #[error("the dataset had not finished listing after {pages} pages")]
    ListingDidNotFinish { pages: usize },
}

/// A short textual diagnostic another service sent us, bounded and filtered.
///
/// **This is not the endpoint-reason decoder.** `errors[].reason` is mapped to [`ReasonCode`] before
/// it reaches an error, so an unknown provider value becomes a static local marker. This helper is
/// for values that remain textual diagnostics - an unusable page token, an `OAuth` error code or a
/// credential file's `type` - and keeps any of them from writing a newline, an escape sequence or
/// sixteen kilobytes into a log.
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
    /// The one clock read on the JOB path - not in the crate, which has three. It is here rather
    /// than in the credential source because [`AccessTokens::bearer`] takes the instant as an
    /// argument, which is what makes an expiry testable without one.
    fn now() -> Wired<u64, C::Error> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
            .map_err(|cause| WireError::NoClock { cause })
    }

    /// The header value this source's OWN credential authorizes a call with.
    ///
    /// **Factored out of [`Self::submit`] when a second call site arrived** - the pre-flight's
    /// listing, which has no asking subject by construction, because it runs at boot. The expiry
    /// guard is here rather than at either call site, so neither can forget it, and it is checked
    /// BEFORE anything is sent. A subject's own token deliberately does NOT come through here: the
    /// broker that minted it already checked it.
    #[expect(
        clippy::disallowed_methods,
        reason = "a bearer has to reach the wire as text; the exposure here builds the one header \
                  value the client parses, which is the whole reason the token exists"
    )]
    fn source_bearer(&self, now: u64, call: CallDeadline) -> Wired<String, C::Error> {
        let bearer = self
            .credentials
            .bearer(now, call)
            .map_err(|cause| WireError::Credential { cause })?;
        if let Some(at) = bearer.not_after().passed_by(now) {
            return Err(WireError::Expired { at, now });
        }
        Ok(format!("Bearer {}", bearer.token().expose_secret()))
    }

    /// Sends one job and returns the endpoint's answer, checked as far as *the service accepted
    /// this*.
    ///
    /// The completeness and row checks are NOT here, because a dry run has neither rows nor a total
    /// and would fail them - see [`Self::validate_job`] and [`Self::run_job`], which are the two
    /// callers and hold the two different conclusions.
    fn submit(&self, request: &JobRequest<'_>, dry_run: DryRun) -> Wired<QueryAnswer, C::Error> {
        // **One absolute deadline for everything below, and it is the PORT's where a request carries
        // one.** `docs/adr/0029`: a request-time call reads what `Warehouse::dry_run`/`execute`'s own
        // `Deadline` says is left, so the token exchange and the job share the same instant every leg
        // of the answer does - a slow exchange shortens the job rather than being followed by one with
        // a full budget of its own. The boot path (`verify_anchor`, a fixture load or drop, the
        // identity read) carries no such `Deadline` and opens a fresh window from this adapter's own
        // configured `JobBounds` instead, unchanged from before this record. ONE clock read for
        // either branch, reused by both checks below.
        let now_instant = std::time::Instant::now();
        let budget_seconds = configured_budget_seconds(request.deadline(), self.agent.bounds());
        let whole = remaining_of_the_ports_deadline(request.deadline(), self.agent.bounds(), now_instant)
            .ok_or(WireError::DeadlineSpent { budget_seconds })?;
        let call = CallDeadline::opened_at_for(now_instant, whole);
        let now = Self::now()?;
        // **Which bearer authorizes this job is decided HERE, once.** A leg that carries the asking
        // subject's own exchanged credential sends THAT - the whole point of this change, and the
        // half that makes a dataset evaluate under the asker. A leg carrying none (the shared posture)
        // reaches the credential source as before. The two are never both sent: a subject bearer is
        // the asker's, and blending the deployment's identity into the same header would be the
        // cross-subject leak this crate refuses.
        #[expect(
            clippy::disallowed_methods,
            reason = "a bearer has to reach the wire as text; the exposure here builds the one \
                      header value the client parses, which is the whole reason the token exists"
        )]
        let sending_bearer: String = match request.subject_bearer() {
            Some(subject) => format!("Bearer {}", subject.expose_secret()),
            None => self.source_bearer(now, call)?,
        };
        // What the exchange left, read ONCE via `call_body` - see its own doc and [`CallBody`] for
        // why `left` comes back alongside the body rather than a second `call.remaining()` below.
        let (built, left) = call_body(request, dry_run, self.agent.bounds(), call)?;
        let serialized = serde_json::to_vec(&built).map_err(|cause| WireError::RequestNotSerializable { cause })?;
        // `Secret::expose_secret` is the one greppable call that lets the token out, and it lets it out into
        // a header value the client parses rather than into a string it concatenates - so a token
        // carrying a newline is a refused request at `send` rather than a second header. That is the
        // library's guarantee and not ours, which is why it is a comment here and not a row in
        // AGENTS.md's table.
        let mut sending = self
            .agent
            .agent()
            .post(url(request))
            // **The agent's own `timeout_global` is a backstop and this is the bound that holds.** It
            // is what is LEFT of the call's budget plus connection setup, so the socket cannot outlive
            // the job the service was asked to cancel at the same instant.
            .config()
            .timeout_global(Some(CallDeadline::socket(left)))
            .build()
            .header("authorization", sending_bearer)
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
            .send(&serialized)
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

    /// What a dry run may conclude: *the endpoint accepted this statement*, and its own estimate of
    /// the bytes it would scan, when it priced one.
    ///
    /// **A `2xx` is the whole acceptance test, and both of the things it deliberately does not check
    /// are worth naming.** `jobComplete` is not required, because a dry run creates no job and what
    /// that field means for one is not something this repository can verify; requiring it would risk
    /// refusing a validation that succeeded. And `errors` is not read, because a dry run that FAILS
    /// is an HTTP error rather than a `200` carrying a reason - the endpoint's own shape - while a
    /// `200` carrying entries is a warning, and refusing on those is the defect the module header
    /// describes.
    ///
    /// **The estimate is read but not enforced here.** `document::estimated_bytes` carries
    /// `totalBytesProcessed` out of the answer, typed and refusing a non-numeric value rather than
    /// panicking, but nothing in this function compares it against anything - the bound that matters
    /// is already on the request as `maximumBytesBilled`, enforced by the service, so a client-side
    /// comparison here would be a second and weaker copy of it. `docs/adr/0030` is where the estimate
    /// this carries starts being useful for something other than display.
    fn validate_job(&self, request: &JobRequest<'_>) -> Wired<crate::transport::DryRunEstimate, C::Error> {
        let answer = self.submit(request, DryRun::Yes)?;
        estimated_bytes(&answer)
    }

    /// The rows one job produced, or the reason they are not a complete result.
    fn run_job(&self, request: &JobRequest<'_>) -> Wired<JobRows, C::Error> {
        complete(self.submit(request, DryRun::No)?)
    }

    /// That one statement with no result set finished. `document::applied` says what it checks and
    /// what it deliberately does not.
    #[cfg(feature = "fixtures")]
    fn apply_job(&self, request: &JobRequest<'_>) -> Wired<(), C::Error> {
        applied(&self.submit(request, DryRun::No)?)
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

    fn validate(&self, request: &JobRequest<'_>) -> Result<crate::transport::DryRunEstimate, Self::Error> {
        self.validate_job(request)
    }

    /// The dataset's table ids, over `tables.list`, paged. `tables::list` is the whole of it.
    fn list_tables(&self, at: &DatasetAddress) -> Result<HeldTables, Self::Error> {
        tables::list(self, at)
    }

    /// Whether the endpoint REFUSED the listing. `tables::was_refused` holds the match.
    fn listing_was_refused(&self, error: &Self::Error) -> bool {
        tables::was_refused(error)
    }

    /// Whether the endpoint REFUSED the statement at the identity/authorization level.
    ///
    /// **Delegates to the SAME classifier [`Self::listing_was_refused`] does**, so the boot-time and
    /// query-time splits cannot drift apart: a refused status is refused the same way whether the
    /// refused read was a listing or a job. `tables::was_refused` holds the match.
    fn job_was_refused(&self, error: &Self::Error) -> bool {
        tables::was_refused(error)
    }

    /// Whether this JOB failure was the port's own `Deadline` running out.
    ///
    /// Two arms, and only the first is unconditionally true: [`WireError::DeadlineSpent`] is a fact
    /// about this adapter's own clock, checked before anything was sent, and cannot be anything else.
    /// [`WireError::NotComplete`] is the DOCUMENTED shape a job stopped at `jobTimeoutMs` answers
    /// with - see that variant's own doc for the argument and the open measurement. Every other
    /// variant is `false`, exhaustively.
    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(*error, WireError::DeadlineSpent { .. } | WireError::NotComplete { .. })
    }

    /// One arm, and it is the wire's own reading of the endpoint's page contract.
    ///
    /// `MoreThanOnePage` is raised by `complete` when the answer carries a `pageToken`, which is the
    /// endpoint saying *there is more of this than fits one reply*. That is a governance outcome above
    /// the domain port - `ResultTooLarge` carrying `ResultBound::Volume` - and it used to reach a
    /// caller as `503`. A refusal whose reason is `responseTooLarge` says the same thing and answers
    /// `true` too, so which spelling the service used does not decide the answer a caller gets.
    ///
    /// Every other variant is `false`, exhaustively rather than through a wildcard, and each of them
    /// really is a failure: an unreachable host, an unreadable body, a job that did not finish, an
    /// absent or unparsable total, a missing schema, a non-scalar cell. **`NotComplete` is the one
    /// worth naming as deliberately `false`:** a job that did not finish inside `jobTimeoutMs` may
    /// well finish on a retry, so telling a caller not to retry it would be the mistake this
    /// predicate's documentation warns about, pointed the other way. **`bytesBilledLimitExceeded` is
    /// the OTHER name this endpoint can refuse with, and it deliberately is NOT this bound:** that is
    /// the deployment's own `max_bytes_billed` ceiling refusing - `ResourcesExhausted`, a different
    /// predicate one port up, and one `dry_run` bypasses - so answering `true` here would certify a
    /// number as "too much data" that is actually a configured spend bound.
    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        match *error {
            // The endpoint's own name for the same shape, arriving as an HTTP error instead of a
            // page token: `responseTooLarge` (403) is documented as *the query results are larger
            // than the maximum response size*, which IS the volume bound. Without this arm, the same
            // reply leaves as `413` when it comes back with a `pageToken` and as `503` when it comes
            // back as a refusal - which answer a caller gets then depends on the service's mood. A
            // non-matching reason still falls through to `false`, so this cannot make things worse
            // than the `503` it replaces.
            WireError::MoreThanOnePage
            | WireError::Refused {
                named: ReasonCode::ResponseTooLarge,
                ..
            } => true,
            WireError::Credential { .. }
            | WireError::Expired { .. }
            | WireError::NoClock { .. }
            // A call that spent its budget before the job ran did not learn how big the result is,
            // so it is not this bound - and it is one of the few failures here a caller CAN
            // usefully retry.
            | WireError::DeadlineSpent { .. }
            | WireError::RequestNotSerializable { .. }
            | WireError::Unreachable { .. }
            | WireError::Unreadable { .. }
            | WireError::Refused { .. }
            | WireError::NotADocument { .. }
            | WireError::NotComplete { .. }
            | WireError::NoTotal { .. }
            | WireError::NotATotal { .. }
            | WireError::NotAnEstimate { .. }
            | WireError::NoSchema { .. }
            | WireError::NotAScalar { .. }
            // The three listing failures. None of them is about a result: the pre-flight reads no
            // rows, and it runs before a listener is bound - so there is no caller to tell anything
            // about how much data came back.
            | WireError::NotAListing { .. }
            | WireError::UnusablePageToken { .. }
            | WireError::ListingDidNotFinish { .. } => false,
        }
    }

    #[cfg(feature = "fixtures")]
    fn apply(&self, request: &JobRequest<'_>) -> Result<(), Self::Error> {
        self.apply_job(request)
    }
}

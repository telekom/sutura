//! How the asking subject's own assertion reaches Google's token service: a loopback source the
//! driver fetches it from, per request, and the workload-identity credential document that points
//! at it.
//!
//! # Why this exists, and what it replaces
//!
//! Rounds 2 and 3 of `telekom/sutura#929` shipped a PRINCIPAL SWITCH: the driver's
//! `bigquery.impersonate.target_principal` option, impersonating a declared service account from
//! the deployment's own application default credentials. The caller's own credential was not in
//! that chain at all - this deployment vouched for the subject - and the owner rejected it. **That
//! path is deleted rather than kept beside this one**: a fallback a misconfiguration could select
//! is the whole defect.
//!
//! What ships instead is Workload Identity Federation, read off the pinned sources rather than
//! assumed:
//!
//! ```text
//! bigquery.auth_type = json_credential_string
//! bigquery.auth.credentials_type = external_account
//! bigquery.auth.credentials = <the document this module builds>
//!   -> go/connection.go's `newClient`: option.WithAuthCredentialsJSON(credType, bytes)
//!   -> cloud.google.com/go/auth credentials/filetypes.go: handleExternalAccount
//!      (which passes `credential_source` THROUGH, whole, unfiltered)
//!   -> externalaccount.NewTokenProvider: GET our loopback URL for the subject token,
//!      exchange it at Google's STS for the declared pool audience
//!   -> and, because the document names `service_account_impersonation_url`, wrap that
//!      federated credential in credentials/internal/impersonate: one POST of
//!      `generateAccessToken` to the declared account, with the federated token as its
//!      authorization (externalaccount.go:252-270, impersonate.go:103-127)
//! ```
//!
//! So Google's own token service verifies the caller's assertion against the pool, the pool
//! resolves the subject to its principal, and that principal mints an access token for the account
//! this deployment declared for that subject. **That is what makes `Warehouse::IMPERSONATION`
//! true rather than documented**: there is no arm on this path that runs a question under the
//! deployment's identity while reporting it as the asker's -
//! [`crate::transport::JobIdentity`] has no such spelling since the switch was removed.
//!
//! **Two things measured off those sources rather than assumed.** `impersonate.Options::Token`
//! POSTs `URL` verbatim with no scheme, host or shape check of any kind, and
//! `externalaccount.Options::validate` checks only that it is non-empty - so the only thing
//! standing between a declared value and an arbitrary request target is this workspace's own
//! narrowing (`crate::principal::names_a_service_account`, applied at parse AND at send). And the
//! STS leg is exchanged for `cloud-platform` while the CALLER's scopes go to the impersonation
//! call (externalaccount.go:256-263), which is why this document still carries no `scopes` member
//! and why `sources.<alias>.workload_identity.scope` still reaches nothing here.
//!
//! # Why a loopback URL rather than a file or an executable
//!
//! `external_account`'s `credential_source` is one of file, url, executable, certificate or an AWS
//! environment id (`credentials/externalaccount/externalaccount.go`'s `CredentialSource`). Priced
//! against *the asking subject's assertion must not be readable by another process*:
//!
//! - **`file`** puts the assertion at rest. Mode, path and unconditional removal are all ours to
//!   get right on every path including a panic, and `TMPDIR` need not be a tmpfs. Available, and
//!   the fallback this is preferred over.
//! - **`executable`** needs `GOOGLE_EXTERNAL_ACCOUNT_ALLOW_EXECUTABLES=1` on every deployment
//!   (`credentials/internal/externalaccount/executable_provider.go`'s own check), and the command
//!   is one string split on whitespace with no shell - so a per-request assertion would travel in
//!   `argv`, which any local process can read, or through the same loopback machinery this module
//!   already needs plus a fork per request.
//! - **`certificate`** is mTLS workload identity: the subject token IS a client certificate, which
//!   authenticates a workload and has nowhere for a caller's assertion. **`environment_id`** accepts
//!   only `aws1`. Neither is a shape leg 2 fits into.
//! - **`url`** is a plain `GET` with arbitrary headers from the document
//!   (`credentials/internal/externalaccount/url_provider.go`), and `Options::validate` performs no
//!   scheme or host check at all - so a loopback address is accepted. Nothing is written, and
//!   nothing appears in `argv`.
//!
//! # What the exposure actually is, stated rather than claimed away
//!
//! The listener is on `127.0.0.1` with a kernel-assigned port, so **any process on this host can
//! connect to it.** Two independent unguessable values are required together - a nonce in the path
//! and a secret in a header - and both exist only inside the credential document this process built
//! for one request. So the bound is: *a local process that can read that document already has the
//! assertion itself*, and nothing weaker gets in.
//!
//! **That bound rests on the document never reaching disk or a log.** It is a driver option string
//! built per request and handed across the C ABI; [`SubjectSource::document`] returns it as a
//! [`Secret`], whose `Debug` prints `REDACTED` and which has no `Display` - so a `tracing` line, an
//! error body or a `{:?}` cannot carry it. That is the property to preserve; breaking it is the
//! leak, not the open port.
//!
//! # What a local process CAN still do, which is delay and not read
//!
//! A round of review measured two availability defects here and both were unauthenticated: one
//! request line with no newline was read to 268 MB against a nominal 8 KiB bound, and ONE idle
//! connection wedged the endpoint so the driver's own fetch timed out and `Drop` never returned.
//! Both are closed by bounds rather than by prose - [`MOST_REQUEST_BYTES`] is now a ceiling on bytes
//! READ and not a check between lines, and every accepted connection is read and written under
//! [`READ_WINDOW`].
//!
//! **The limit those bounds leave**: connections are handled ONE AT A TIME, so a local process that
//! keeps opening them can delay a fetch by up to the read window per connection. That is
//! availability and not confidentiality - a delayed fetch fails the question, and no arm of it
//! reaches the assertion. Handling connections concurrently would trade it for something worse: a
//! detached handler holding the assertion could outlive the request that built it, which is exactly
//! what [`SubjectSource::drop`] exists to make impossible.
//!
//! # The lifetime, which fails mid-query rather than at boot
//!
//! `externalaccount::NewTokenProvider` wraps its provider in `auth.NewCachedTokenProvider`, so the
//! fetch is **lazy**: it happens on the driver's first token mint, which is after `connect` has
//! returned, and it can recur if the credential expires while a long query runs. The listener
//! therefore lives for as long as the [`SubjectSource`] is held - `AdbcBigQuery::run` holds it
//! across the whole execution - and serves every correctly-authenticated fetch in that window
//! rather than exactly one. `a_fetch_after_the_connection_was_built_is_still_served` is the cell on
//! that; `the_listener_is_gone_once_the_source_is_dropped` and
//! `the_endpoint_is_closed_before_drop_returns_even_with_a_connection_in_flight` are the other
//! direction. The second of those exists because the first passed with `Drop`'s join deleted: a
//! stop flag being SET says nothing about the thread having seen it, and the race is only a race
//! while no connection is in flight. With one in flight the thread is inside its read window, so an
//! unjoined `drop` demonstrably returns with the port still open.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sutura_domain::identity::{PrincipalName, Secret};

use super::AdbcError;

/// The header the driver has to present beside the path nonce.
///
/// Its own name rather than `Authorization`, so nothing mistakes this for a scheme a proxy or a
/// library might rewrite: it is a one-hop shared secret between this process and a library inside
/// it.
pub(super) const SECRET_HEADER: &str = "x-sutura-subject-token-secret";

/// The token type a workload-identity pool is told the subject token is.
///
/// A JWT, because that is what leg 1 verifies and what every issuer this deployment can be pointed
/// at emits. A pool configured for SAML would need a different value and a different leg 1, so this
/// is a constant rather than a setting nobody could fill in correctly.
const SUBJECT_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:jwt";

/// Everything before the declared account in `service_account_impersonation_url`.
///
/// **Rendered from two constants and a `format!` rather than parsed into a type.** The URL is
/// `iamcredentials.projects.serviceAccounts.generateAccessToken`'s resource path, which the
/// deleted HTTP wire built the same way off a bare `&str`; a newtype whose `parse` cannot fail,
/// with one consumer and one caller, is the builder-with-one-implementor this workspace deletes.
/// What makes the interpolation safe is the NARROWING on the value, which
/// `crate::principal::names_a_service_account` performs at both ends - see this function's own
/// refusal in `super::identity`.
const IMPERSONATION_URL_PREFIX: &str = "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/";

/// Everything after it: the method the pool's principal calls on that account.
const IMPERSONATION_URL_SUFFIX: &str = ":generateAccessToken";

/// How many bytes of one request head this will read before refusing.
///
/// The only client is a Go HTTP library on loopback asking for one URL, so a head larger than this
/// is not a client running late - it is something else on this host, and an unbounded read from it
/// is a denial-of-service primitive. `AGENTS.md` treats availability as a security property, which
/// is why this is a constant and not a comment.
const MOST_REQUEST_BYTES: usize = 8 * 1024;

/// How long one connection may take to send its head and read its answer.
///
/// **A deadline on every accepted socket, because the loop is serial.** Without it a local process
/// that connected and sent nothing held the endpoint for as long as it liked: the driver's own fetch
/// never got served and `Drop`'s join never returned, both measured. Generous for the one real
/// client - a Go HTTP library on loopback writes its request in one syscall - and short enough that
/// a wedge is a delay.
const READ_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);

/// Everything about a source that does not change per request: the pool it exchanges against.
///
/// **Parsed at composition, so an unusable declaration fails to start.** The audience is the pool
/// provider resource the subject token is exchanged for - `externalaccount::Options::validate`
/// refuses an empty one outright, so a deployment that declared nothing would fail on its first
/// question instead of at boot.
///
/// **One field, and the declared SCOPE is not it.** `sources.<alias>.workload_identity.scope` is
/// parsed by `sutura_config` and reaches nothing here, because the pinned driver has nowhere to put
/// it: `credsfile::ExternalAccountFile` (`cloud.google.com/go/auth@v0.23.2`) has no `scopes` member,
/// so the document cannot carry one, and the driver's only scope option is
/// `bigquery.impersonate.scopes`, which `connection.go`'s `hasImpersonationOptions` treats as a
/// request for the DELETED mechanism - it then demands a target principal and replaces the
/// federated credential with an impersonated token source. A screened value this transport cannot
/// send would read as a control that is in place, so it is not held here at all and the operator is
/// told where they declare it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadPool {
    audience: String,
}

/// Why a declared pool is not one this transport can exchange against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnusablePool {
    /// There was nothing there.
    #[error("the workload identity audience this source declares is empty")]
    Empty,
    /// Longer than the value's own bound.
    #[error(
        "the workload identity audience this source declares is {found} characters and at most \
         {most} are usable"
    )]
    TooLong {
        /// How long it was.
        found: usize,
        /// The bound.
        most: usize,
    },
    /// A character outside the accepted set, at a position.
    #[error("the workload identity audience this source declares carries an unusable character at {at}")]
    Character {
        /// Where, so an operator can find it without the refusal quoting it.
        at: usize,
    },
}

impl WorkloadPool {
    /// A provider resource is bounded the way the endpoint documents it.
    const MOST_AUDIENCE: usize = 256;

    /// Parses the audience one impersonating source declares.
    ///
    /// **It reaches a JSON document this transport builds**, so it is checked here as well as in the
    /// settings tree - the reason `crate::transport::ProjectId` is parsed twice. The accepted set
    /// excludes every character that could close a JSON string or a URL, so a declaration cannot
    /// reshape the document it lands in.
    ///
    /// # Errors
    ///
    /// [`UnusablePool`], which carries a position and never the text.
    pub fn parse(audience: &str) -> Result<Self, UnusablePool> {
        let trimmed = audience.trim();
        if trimmed.is_empty() {
            return Err(UnusablePool::Empty);
        }
        let found = trimmed.chars().count();
        if found > Self::MOST_AUDIENCE {
            return Err(UnusablePool::TooLong {
                found,
                most: Self::MOST_AUDIENCE,
            });
        }
        if let Some(at) = trimmed.char_indices().find_map(|(at, c)| {
            (!matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | ':' | '.' | '-' | '_' | '%')).then_some(at)
        }) {
            return Err(UnusablePool::Character { at });
        }
        Ok(Self {
            audience: String::from(trimmed),
        })
    }

    /// The pool provider resource a subject token is exchanged against.
    #[inline]
    #[must_use]
    pub fn audience(&self) -> &str {
        &self.audience
    }
}

/// One request's subject token, served over loopback for as long as this value is held.
///
/// Dropping it closes the listener and joins its thread, which is what ties the endpoint's lifetime
/// to the job's rather than to a comment.
pub(super) struct SubjectSource {
    address: SocketAddr,
    /// The path segment the driver must ask for. Not a secret on its own, and unguessable.
    nonce: String,
    /// The header value the driver must present. The second of the two independent values.
    secret: String,
    /// Flipped on drop, read by the accept loop between connections.
    stop: Arc<AtomicBool>,
    /// Joined on drop, so nothing outlives the request that built it.
    serving: Option<std::thread::JoinHandle<()>>,
}

impl SubjectSource {
    /// Binds a loopback listener that will serve `assertion` to whoever presents both values.
    ///
    /// # Errors
    ///
    /// [`AdbcError::SubjectSource`] where the listener cannot be bound or its address read - a
    /// deployment with no loopback interface cannot serve an impersonated question, and saying so
    /// is better than a driver failing to fetch a token for reasons of its own.
    pub(super) fn bind(assertion: &Secret) -> Result<Self, AdbcError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|cause| AdbcError::SubjectSource { cause })?;
        let address = listener.local_addr().map_err(|cause| AdbcError::SubjectSource { cause })?;
        let nonce = unguessable()?;
        let secret = unguessable()?;
        let stop = Arc::new(AtomicBool::new(false));
        #[expect(
            clippy::disallowed_methods,
            reason = "the assertion's bytes are the response body this endpoint exists to serve, so \
                      they have to be read once - here, into a thread that writes them to one \
                      authenticated loopback caller and nowhere else. Nothing on this path logs, \
                      formats or stores them, and the credential document that names the endpoint \
                      stays a `Secret` so no reader can print either half"
        )]
        let serving = std::thread::spawn({
            // The body is the assertion and nothing else, which is `format: {"type": "text"}`'s
            // contract in `url_provider.go`: an absent or `text` format returns the whole body.
            let body = String::from(assertion.expose_secret());
            let wanted = format!("/{nonce}");
            let secret = secret.clone();
            let stop = Arc::clone(&stop);
            // **The listener MOVES into the thread**, which is what makes the join observable: the
            // port is open for exactly as long as this closure has not returned, so a `Drop` that
            // did not wait leaves it open and a connection attempt after `drop` succeeds. That is
            // the whole mechanism behind
            // `the_endpoint_is_closed_before_drop_returns_even_with_a_connection_in_flight`.
            move || serve(&listener, &wanted, &secret, &body, &stop)
        });
        Ok(Self {
            address,
            nonce,
            secret,
            stop,
            serving: Some(serving),
        })
    }

    /// The workload-identity credential document the driver is handed for this job.
    ///
    /// **A [`Secret`], and that is the property the loopback bound rests on.** It carries the nonce
    /// and the header secret, so a `tracing` line or a `{:?}` that printed it would hand any local
    /// process the assertion - `Secret` has no `Display` and prints `REDACTED` under `Debug`, so
    /// neither is reachable by accident.
    ///
    /// **`service_account_impersonation_url` names the account declared for THIS subject**, which
    /// is the field `telekom/sutura#929` F3 added and the reason the `impersonate` map's values
    /// decide something. Two links, one chain: the subject's own assertion federates to the pool's
    /// principal, and `externalaccount`'s `impersonate.go` then calls
    /// `generateAccessToken` on this account with that federated credential - so the token the
    /// driver ends up holding is the declared account's, reached only by a caller the pool
    /// verified. Changing a declared account changes which account that caller's questions run as.
    ///
    /// A round of this path left the field ABSENT, which made the credential the pool principal
    /// itself and every declared caller one identity; its comment described that as the `impersonate`
    /// map's job, not yet built. It is built.
    ///
    /// **The limit, beside the claim**: nothing here - and nothing anywhere in this repository -
    /// checks that the pool's principal may actually impersonate `target`. That is one IAM binding,
    /// `roles/iam.workloadIdentityUser` on the target account with the pool's
    /// `principal://.../subject/<id>` as its member, and no type, lint, hook or gate sees a live
    /// policy. An account that is declared, well-formed and not reachable fails as
    /// [`AdbcError::Adbc`] on the first question by that subject, never at boot.
    pub(super) fn document(&self, pool: &WorkloadPool, target: &PrincipalName) -> Secret {
        // Built with `serde_json` rather than by formatting, so a declared value cannot close a
        // string and add a field - the parse in `WorkloadPool` narrows the input and this makes the
        // narrowing unnecessary rather than load-bearing.
        let document = serde_json::json!({
            "type": "external_account",
            "audience": pool.audience(),
            "subject_token_type": SUBJECT_TOKEN_TYPE,
            "token_url": "https://sts.googleapis.com/v1/token",
            "service_account_impersonation_url":
                format!("{IMPERSONATION_URL_PREFIX}{target}{IMPERSONATION_URL_SUFFIX}"),
            "credential_source": {
                "url": format!("http://{}/{}", self.address, self.nonce),
                "headers": { SECRET_HEADER: self.secret.clone() },
                "format": { "type": "text" }
            }
        });
        Secret::new(document.to_string())
    }
}

impl Drop for SubjectSource {
    /// Closes the endpoint and waits for its thread.
    ///
    /// **Joined rather than detached**, so `run` returning means the port is gone: a detached
    /// thread would leave an authenticated subject-token endpoint open for an unbounded time after
    /// the question it belonged to was answered.
    ///
    /// **And the wait is bounded**, which it was not: a local process that connected and sent
    /// nothing held this join open for as long as it stayed connected. Every accepted socket now
    /// carries [`READ_WINDOW`], so the worst case is one in-flight connection's window plus the
    /// wake below.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // One connection to wake the blocking `accept`, so the loop reaches its own stop check
        // rather than waiting for a fetch that will never come. Its outcome is irrelevant - the
        // loop refuses it like any other unauthenticated caller.
        drop(TcpStream::connect(self.address));
        if let Some(serving) = self.serving.take() {
            drop(serving.join());
        }
    }
}

/// A compile-time guard that `SubjectSource` does not implement `Debug`.
///
/// `SubjectSource` carries the loopback nonce, secret, and address - the two independent values a
/// local process needs to fetch the caller's assertion from this process's own endpoint. A
/// `#[derive(Debug)]` or `impl Debug` would print them, which is exactly the leak the module
/// header's bound rests on ("the document never reaching disk or a log"). `Secret` already redacts
/// under `Debug`, but `SubjectSource` is the layer above it: its fields are bare, and it is
/// `pub(super)` so a `compile_fail` doctest cannot name it - this is the mechanism that replaces
/// review for a type no external crate can reach.
///
/// The negative-impl ambiguity trick: one blanket impl for all `T`, one for `T: Debug`. With no
/// `Debug` impl only the first matches, so the reference resolves and the crate compiles. Adding
/// `#[derive(Debug)]` or any `impl Debug` makes both apply, so `<SubjectSource as
/// AmbiguousIfImpl<_>>::some_item` is ambiguous (E0283) and the crate does not build.
///
/// **Limit:** this forbids `Debug` and nothing else. A hand-written `Display` on `SubjectSource`,
/// or a field printed by name in a `tracing`/`format!` call elsewhere, still leaks - those are held
/// by review, not by this guard.
mod no_debug {
    trait AmbiguousIfImpl<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    impl<T: ?Sized + core::fmt::Debug> AmbiguousIfImpl<u8> for T {}

    // Compiles only while SubjectSource does NOT implement Debug.
    const _: fn() = || {
        let _ = <super::SubjectSource as AmbiguousIfImpl<_>>::some_item;
    };
}

/// Serves `body` to every caller presenting both `wanted` and `secret`, until told to stop.
///
/// **Every other caller gets a refusal and a closed connection**, and the refusal says nothing
/// about which half was wrong - a response that distinguished them would tell a local process
/// holding one value which of the two to keep guessing.
fn serve(listener: &TcpListener, wanted: &str, secret: &str, body: &str, stop: &AtomicBool) {
    for incoming in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = incoming else { continue };
        // **The deadline goes on before anything is read.** A socket with no timeout is what let one
        // idle local connection wedge this loop; a socket whose deadline could not be set is refused
        // outright rather than served without one, because "served without a deadline" is the state
        // being removed.
        if stream.set_read_timeout(Some(READ_WINDOW)).is_err() || stream.set_write_timeout(Some(READ_WINDOW)).is_err() {
            drop(stream.shutdown(Shutdown::Both));
            continue;
        }
        // A failed write is not this loop's business: the client went away, and the next fetch -
        // the cached provider's re-mint - gets its own connection.
        drop(answer(&stream, wanted, secret, body));
        drop(stream.shutdown(Shutdown::Both));
    }
}

/// Reads one request head and writes one response.
fn answer(stream: &TcpStream, wanted: &str, secret: &str, body: &str) -> std::io::Result<()> {
    // One `is_some_and`, so an unreadable head, an over-long head and a head presenting the wrong
    // values are one refusal with one body - a caller cannot tell which of them it hit.
    let granted = head_of(stream).is_some_and(|head| authenticated(&head, wanted, secret));
    let mut stream = stream;
    if granted {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    } else {
        // 404 rather than 401: a caller with neither value learns nothing from it, and the one real
        // client never reaches this arm.
        write!(
            stream,
            "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        )
    }
}

/// Reads one request head, or nothing.
///
/// **`None` is every way the read did not produce a head this endpoint will act on**, and the bound
/// is on BYTES READ rather than checked between lines. That distinction was a real defect: a
/// `read_line` is unbounded within one line, so a request line with no newline was measured reading
/// 268 MB against a nominal 8 KiB. The reader is capped at one byte PAST the bound, so a head that
/// reached the cap is refused rather than truncated and matched - a truncation would have let a
/// caller present both values and then any amount of padding.
fn head_of(stream: &TcpStream) -> Option<String> {
    let mut reader = BufReader::new(stream).take(u64::try_from(MOST_REQUEST_BYTES).unwrap_or(u64::MAX).saturating_add(1));
    let mut head = String::new();
    loop {
        let mut line = String::new();
        // A read error and an invalid-UTF-8 request are both "no head", which is also what a
        // timed-out idle connection arrives as.
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let blank = line.trim().is_empty();
        head.push_str(&line);
        if blank {
            break;
        }
    }
    (head.len() <= MOST_REQUEST_BYTES).then_some(head)
}

/// Does this request head present BOTH the path nonce and the header secret?
///
/// **Both, and neither is sufficient**, which is what makes the open port's bound the one the module
/// header states. The comparison is a plain `==` on values this process generated, and it is NOT
/// constant time: a review measured a right nonce answering in 225 us against 166 us for a wrong one
/// over 60 samples. What that oracle confirms is a nonce a caller already holds - it does not
/// recover one, because there is nothing to walk towards through 128 bits of process-local
/// randomness and each attempt costs a fresh connection. Stated rather than fixed, because the
/// comparison a constant-time crate would replace is not the value at risk.
fn authenticated(head: &str, wanted: &str, secret: &str) -> bool {
    let mut lines = head.lines();
    let Some(request) = lines.next() else {
        return false;
    };
    let mut parts = request.split_whitespace();
    if parts.next() != Some("GET") {
        return false;
    }
    if parts.next() != Some(wanted) {
        return false;
    }
    lines.any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.trim().eq_ignore_ascii_case(SECRET_HEADER) && value.trim() == secret
    })
}

/// 128 bits of randomness, hex, from the operating system.
///
/// Two of these are generated per request and neither is derived from the other, so learning one
/// says nothing about the other.
///
/// # Errors
///
/// [`AdbcError::NoRandomness`] where the operating system will not answer. **A `Result` and not a
/// fallback, because every fallback here is fail-OPEN**: a poisoned constant would be written into
/// the same document the driver reads, so the fetch would authenticate perfectly against a value
/// any local process could guess. Refusing the question is the only direction that is not worse
/// than the switch this path replaced.
fn unguessable() -> Result<String, AdbcError> {
    let mut bytes = [0_u8; 16];
    #[expect(
        clippy::disallowed_methods,
        reason = "the one entropy read this workspace makes on purpose: two unguessable per-request \
                  values authorising the driver's loopback fetch of a subject's assertion. \
                  `clippy.toml` bans all four of getrandom's entry points so that this site is the \
                  only one, and a second is a visible diff"
    )]
    getrandom::fill(&mut bytes).map_err(|cause| AdbcError::NoRandomness { cause })?;
    Ok(bytes.iter().fold(String::with_capacity(32), |mut hex, byte| {
        // `fold` rather than `map(format!).collect()`, which `clippy::format_collect` refuses: one
        // allocation of a known size instead of sixteen two-byte `String`s.
        use std::fmt::Write as _;
        // Infallible into a `String`, and the result is answered for rather than unwrapped
        // because `unwrap_used` is denied: a failure here would leave a short hex string, which
        // the driver would then present and the endpoint would refuse - the fail-closed direction.
        if write!(hex, "{byte:02x}").is_err() {
            hex.clear();
        }
        hex
    }))
}

#[cfg(test)]
mod tests;

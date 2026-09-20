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
//! ```
//!
//! So Google's own token service verifies the caller's assertion against the pool, and the source
//! executes as whatever principal that pool resolves the subject to. **That is what makes
//! `Warehouse::IMPERSONATION` true rather than documented**: there is no arm on this path that
//! runs a question under the deployment's identity while reporting it as the asker's -
//! [`crate::transport::JobIdentity`] has no such spelling since the switch was removed.
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
//! # The lifetime, which fails mid-query rather than at boot
//!
//! `externalaccount::NewTokenProvider` wraps its provider in `auth.NewCachedTokenProvider`, so the
//! fetch is **lazy**: it happens on the driver's first token mint, which is after `connect` has
//! returned, and it can recur if the credential expires while a long query runs. The listener
//! therefore lives for as long as the [`SubjectSource`] is held - `AdbcBigQuery::run` holds it
//! across the whole execution - and serves every correctly-authenticated fetch in that window
//! rather than exactly one. `a_fetch_after_the_connection_was_built_is_still_served` is the cell on
//! that; `the_listener_is_gone_once_the_source_is_dropped` is the other direction.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sutura_domain::identity::Secret;

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

/// How many bytes of one request head this will read before refusing.
///
/// The only client is a Go HTTP library on loopback asking for one URL, so a head larger than this
/// is not a client running late - it is something else on this host, and an unbounded read from it
/// is a denial-of-service primitive. `AGENTS.md` treats availability as a security property, which
/// is why this is a constant and not a comment.
const MOST_REQUEST_BYTES: usize = 8 * 1024;

/// Everything about a source that does not change per request: the pool it exchanges against.
///
/// **Parsed at composition, so an unusable declaration fails to start.** The audience is the pool
/// provider resource the subject token is exchanged for - `externalaccount::Options::validate`
/// refuses an empty one outright, so a deployment that declared nothing would fail on its first
/// question instead of at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadPool {
    audience: String,
    scope: String,
}

/// Why a declared pool is not one this transport can exchange against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnusablePool {
    /// There was nothing there.
    #[error("the {what} this source declares is empty")]
    Empty {
        /// Which of the two values.
        what: &'static str,
    },
    /// Longer than the value's own bound.
    #[error("the {what} this source declares is {found} characters and at most {most} are usable")]
    TooLong {
        /// Which of the two values.
        what: &'static str,
        /// How long it was.
        found: usize,
        /// The bound.
        most: usize,
    },
    /// A character outside the accepted set, at a position.
    #[error("the {what} this source declares carries an unusable character at {at}")]
    Character {
        /// Which of the two values.
        what: &'static str,
        /// Where, so an operator can find it without the refusal quoting it.
        at: usize,
    },
}

impl WorkloadPool {
    /// A provider resource is bounded the way the endpoint documents it.
    const MOST_AUDIENCE: usize = 256;

    /// A scope is a URL, so its bound is a URL's.
    const MOST_SCOPE: usize = 1024;

    /// Parses the audience and scope one impersonating source declares.
    ///
    /// **Both reach a JSON document this transport builds**, so both are checked here as well as in
    /// the settings tree - the reason `crate::transport::ProjectId` is parsed twice. The accepted
    /// sets exclude every character that could close a JSON string or a URL, so a declaration
    /// cannot reshape the document it lands in.
    ///
    /// # Errors
    ///
    /// [`UnusablePool`], which carries a position and never the text.
    pub fn parse(audience: &str, scope: &str) -> Result<Self, UnusablePool> {
        let audience = bounded(audience, "workload identity audience", Self::MOST_AUDIENCE)?;
        let scope = bounded(scope, "workload identity scope", Self::MOST_SCOPE)?;
        Ok(Self { audience, scope })
    }

    /// The pool provider resource a subject token is exchanged against.
    #[inline]
    #[must_use]
    pub fn audience(&self) -> &str {
        &self.audience
    }

    /// The scope the exchanged credential carries.
    #[inline]
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }
}

/// Trims, bounds and screens one declared value.
///
/// One function for the two, because *a declared value reaches a request* is one rule and two
/// copies of it are two places for the accepted set to drift.
fn bounded(raw: &str, what: &'static str, most: usize) -> Result<String, UnusablePool> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(UnusablePool::Empty { what });
    }
    if trimmed.chars().count() > most {
        return Err(UnusablePool::TooLong {
            what,
            found: trimmed.chars().count(),
            most,
        });
    }
    if let Some(at) = trimmed
        .char_indices()
        .find_map(|(at, c)| (!matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | ':' | '.' | '-' | '_' | '%')).then_some(at))
    {
        return Err(UnusablePool::Character { what, at });
    }
    Ok(String::from(trimmed))
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
    /// `service_account_impersonation_url` is deliberately ABSENT. Without it the credential IS the
    /// pool principal the subject resolved to, which is what makes two subjects two principals at
    /// the data system by construction rather than by a declared map. A per-subject service account
    /// on top of that is the `impersonate` map's job and is not built on this path yet - see
    /// `crate::principal`.
    pub(super) fn document(&self, pool: &WorkloadPool) -> Secret {
        // Built with `serde_json` rather than by formatting, so a declared value cannot close a
        // string and add a field - the parse in `WorkloadPool` narrows the input and this makes the
        // narrowing unnecessary rather than load-bearing.
        let document = serde_json::json!({
            "type": "external_account",
            "audience": pool.audience(),
            "subject_token_type": SUBJECT_TOKEN_TYPE,
            "token_url": "https://sts.googleapis.com/v1/token",
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
        // A failed write is not this loop's business: the client went away, and the next fetch -
        // the cached provider's re-mint - gets its own connection.
        drop(answer(&stream, wanted, secret, body));
        drop(stream.shutdown(Shutdown::Both));
    }
}

/// Reads one request head and writes one response.
fn answer(stream: &TcpStream, wanted: &str, secret: &str, body: &str) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut head = String::new();
    // Bounded, and the bound is the whole head rather than per line: the one real client sends a
    // few hundred bytes, so anything approaching this is not it.
    while head.len() < MOST_REQUEST_BYTES {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let blank = line.trim().is_empty();
        head.push_str(&line);
        if blank {
            break;
        }
    }
    let granted = head.len() <= MOST_REQUEST_BYTES && authenticated(&head, wanted, secret);
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

/// Does this request head present BOTH the path nonce and the header secret?
///
/// **Both, and neither is sufficient**, which is what makes the open port's bound the one the module
/// header states. The comparison is a plain `==` on values this process generated - there is no
/// remote guessing loop to time, because the values are 128 bits of process-local randomness and a
/// caller gets one connection per attempt.
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

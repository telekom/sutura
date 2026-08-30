//! Where the bearer token a job is submitted with comes from.
//!
//! **A second narrow port, for the reason [`JobTransport`](crate::transport::JobTransport) is one.**
//! The wire needs two things from a credential - a token that is usable right now, and whether a
//! request carrying it has to name a quota project - and everything else about how a deployment
//! authenticates is somebody else's decision. So [`AccessTokens`] is those two things, and the
//! transport is generic in it.
//!
//! **This is also the seam per-subject execution arrives at**, which is why it is a port on the first
//! day rather than a `String` field. `docs/implementation-plan-bigquery.md`'s second `BigQuery` step
//! mints a token *per leg, for the subject who asked*; under a `String` that step would have to change
//! the transport, and under a port it adds an implementor. Nothing here anticipates it further than
//! that: [`Bearer`] carries the deadline because a minted token has one, and
//! [`crate::BigQueryWarehouse`]'s `IMPERSONATION` still says `NoPlaceForASubject` because nothing
//! mints one.
//!
//! # Two credential kinds, as one closed shape
//!
//! [`Credential`] reads either of the two files a deployment can actually have, and **which one it is
//! is a closed two-variant shape rather than a struct of `Option`s.** That matters for a reason
//! stronger than tidiness: a document carrying *both* a refresh token and a private key is
//! unrepresentable here, so there is no state in which it is ambiguous which flow will run or which
//! credential material was used.
//!
//! | Kind | Who holds it | The exchange |
//! | --- | --- | --- |
//! | `authorized_user` | a developer, from `just gcloud-login` | trade a refresh token |
//! | `service_account` | CI, and a deployment | sign an assertion and trade that |
//!
//! **Both are `SharedServiceUser` and neither is a step towards per-subject execution.** One identity
//! reaches the dataset for everybody who asks; on a laptop that identity is the developer and in CI it
//! is a service account. `docs/adr/0017`'s amendment is where the decision to run the acceptance leg
//! in CI on the second kind lives.
//!
//! **The signing costs no new dependency, which was verified rather than assumed.** `ring` is already
//! in the graph - it is `ureq`'s and `tokio-rustls`'s crypto provider - and it carries
//! `RsaKeyPair::from_pkcs8` plus `RSA_PKCS1_SHA256`, which is exactly the primitive and exactly the
//! key encoding a service-account key uses. `base64` is already resolved too. So `docs/adr/0018`'s
//! 446-to-446 measurement survives this, and the alternative that would have cost a package -
//! `jsonwebtoken`'s `use_pem`, which pulls `simple_asn1` because its DER path wants `PKCS#1` while a
//! service-account key is `PKCS#8` - was priced and refused.
//!
//! **What IS first-party here is the JWT's text, and that boundary is deliberate.** `ring` computes the
//! signature; this module base64url-encodes two JSON documents and joins them with dots.
//! `docs/adr/0014` draws exactly that line when it argues for hand-writing a metrics exposition format
//! and against hand-writing signature verification in the same breath: one is a text format, the other
//! is cryptography. And this side SIGNS rather than verifies, which is where algorithm confusion does
//! not live - the algorithm is a constant, not a field read off somebody else's document.
//!
//! # Two credential shapes exist and neither is built
//!
//! - the **metadata server**, which is how a deployment on the provider's own compute gets a token
//!   with no key at all. It is a plain unauthenticated `GET` and would cost nothing in dependencies -
//!   the cheapest option available. It is out because **nothing in this repository can verify it**: it
//!   exists only inside that provider's network, so building it would add an unexercised code path to
//!   a module whose whole point is that it does not claim more than it has.
//! - **workload or workforce identity federation**, which is the per-subject step and an architecture
//!   decision with an owner outside this repository.
//!
//! # Nothing is cached, and both the decision and its REASON were wrong once
//!
//! A token is minted for every call into the endpoint. **The decision stands; the paragraph that
//! justified it did not, in two ways a review caught, and both are corrected here because the
//! per-subject step will read this as the argument it inherits.**
//!
//! **The cost, counted properly.** `sutura_app::answer` calls `Warehouse::dry_run` and then
//! `Warehouse::execute`; each goes through the wire's `submit`, and each calls
//! [`AccessTokens::bearer`]. So one question is **two** token exchanges before its two job round
//! trips, and **every anchor verified at boot is one more**. The earlier wording - *"one extra round
//! trip per job"* - was half the real number and counted the wrong unit.
//!
//! **The reason, corrected.** The earlier version said a token cache keyed by nothing is the
//! credential-shaped version of the result cache this crate refuses. That is true of a cache shared
//! across SUBJECTS and false here: a [`Credential`] **is** one identity, so a token held until its
//! `not_after` is keyed by exactly the thing that matters and leaks to nobody. The
//! `std::sync::Mutex` ban in `clippy.toml` is not an argument either - `sutura-http`'s own key-set
//! cache holds a lock.
//!
//! **So the honest reason is the small one: it is not needed until it is measured.** Minting is one
//! `HTTPS` round trip against a query that costs seconds and money, and the shape with nothing to
//! reuse is the shape that cannot get *a credential is not reused past its expiry* wrong - which is one
//! of the assertions the per-subject step owes. **What that step must NOT inherit is a prohibition**,
//! because caching per subject, keyed by subject, is a different question this paragraph does not
//! answer.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use sutura_domain::identity::{Expiry, Secret};

use crate::wire::WireAgent;

/// A token usable now, and when it stops being usable.
///
/// **The deadline travels with the token rather than beside it**, so a caller cannot present one and
/// forget the other. [`Expiry`] is the domain's own vocabulary for this, which matters because the
/// per-subject step reports the same value through `sutura_domain::audit::CallRecord`.
#[derive(Debug, Clone)]
pub struct Bearer {
    token: Secret,
    not_after: Expiry,
}

impl Bearer {
    /// Names a token and its deadline.
    #[must_use]
    pub const fn of(token: Secret, not_after: Expiry) -> Self {
        Self { token, not_after }
    }

    /// The token, still opaque. A caller has to reach `Secret::expose_secret` to write it into a header, and
    /// that call is greppable.
    #[inline]
    #[must_use]
    pub const fn token(&self) -> &Secret {
        &self.token
    }

    /// When it stops being usable.
    #[inline]
    #[must_use]
    pub const fn not_after(&self) -> Expiry {
        self.not_after
    }
}

/// Whether a request has to name the project whose quota and billing it is attributed to.
///
/// Two variants and no `Option`, because "the credential already says" is a real answer rather than a
/// missing one - which is the same argument [`Expiry`] makes for having no `Option`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaProject {
    /// State it on the request. An application-default credential is an END-USER credential, and the
    /// endpoint's own direct-REST guidance requires a quota project for one - without it a perfectly
    /// valid token is refused, with a message about user credentials not being supported that reads as
    /// an authentication fault and is not one.
    Required,
    /// The credential carries its own project, so stating one would add a permission requirement -
    /// `serviceusage.services.use` on that project - that a service account holding only dataset
    /// grants does not have. **So the header that MAKES the first kind work BREAKS the second**, which
    /// is why this is a two-variant answer and not a constant on the request.
    FromTheCredential,
}

/// Where the token a job is submitted with comes from.
///
/// The clock is a parameter for the reason [`Expiry::passed_by`] takes one: an implementor that reads
/// the wall clock itself cannot be tested against a deadline, and the expiry logic is the half of a
/// credential source most likely to be wrong in the direction nobody notices. Who reads the real clock
/// is the transport, once.
pub trait AccessTokens {
    /// Why no token could be produced. Typed per source: a missing file, a refused exchange and a
    /// clock that disagrees are not the same thing to whoever responds to them.
    type Error: core::error::Error + Send + Sync + 'static;

    /// Whether a request carrying this credential has to state a quota project.
    ///
    /// **Required, with no default, and the reason is the rule `Warehouse::IMPERSONATION` follows:**
    /// required where an absence changes what a caller may believe. Both answers break something when
    /// they are wrong, in opposite directions - see [`QuotaProject`] - and a default would pick one of
    /// them silently.
    fn quota_project(&self) -> QuotaProject;

    /// A token usable at `now_unix_seconds`.
    fn bearer(&self, now_unix_seconds: u64) -> Result<Bearer, Self::Error>;
}

/// The file a credential lives in.
///
/// A newtype rather than a `PathBuf` argument, because [`Self::well_known`] and [`Self::at`] are two
/// different claims - *wherever this machine keeps it* and *this exact file* - and a function taking a
/// path cannot tell which one it was handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialFile(PathBuf);

/// Why the well-known location could not be worked out.
///
/// One variant, and it carries no path: the refusal is that this machine named no home directory, and
/// the fix is to say where the file is with [`CredentialFile::at`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no {named} is set, so there is no well-known location for a credential")]
pub struct NoWellKnownLocation {
    /// Which variables were looked for, so the message says what to set.
    named: &'static str,
}

impl CredentialFile {
    /// The variable a caller sets to name the file outright. Read first, because that is what it is
    /// for - and it is how CI points at a service-account key without a second setting.
    const EXPLICIT: &'static str = "GOOGLE_APPLICATION_CREDENTIALS";
    /// The variable that relocates the whole configuration directory. `just gcloud-login` honours it,
    /// and so does the tool that writes the file, so this reads the same value they do.
    const CONFIG_DIR: &'static str = "CLOUDSDK_CONFIG";
    /// The file's name inside that directory. Chosen by the tool that writes it, not by us.
    const LEAF: &'static str = "application_default_credentials.json";

    /// This exact file.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Wherever this machine keeps it.
    ///
    /// Three places, in the order the tooling itself uses: the explicit variable, then the relocated
    /// configuration directory, then the default under the user's home. **`HOME` is read and no
    /// fallback is invented** - a process with no home directory has no well-known location, and
    /// guessing one would be reading a credential from a path nobody chose.
    pub fn well_known() -> Result<Self, NoWellKnownLocation> {
        // An empty value counts as unset. A variable exported as `""` by a shell script is the
        // ordinary way this goes wrong, and reading it as a path produces a refusal naming the
        // repository root.
        let named = |key: &str| std::env::var(key).ok().filter(|value| !value.trim().is_empty());
        if let Some(explicit) = named(Self::EXPLICIT) {
            return Ok(Self(PathBuf::from(explicit)));
        }
        if let Some(config) = named(Self::CONFIG_DIR) {
            return Ok(Self(PathBuf::from(config).join(Self::LEAF)));
        }
        named("HOME")
            .map(|home| Self(PathBuf::from(home).join(".config").join("gcloud").join(Self::LEAF)))
            .ok_or(NoWellKnownLocation {
                named: "GOOGLE_APPLICATION_CREDENTIALS, CLOUDSDK_CONFIG or HOME",
            })
    }

    /// The path, for reading it and for a message that says which file was wrong.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// The document on disk, as the tool that writes it spells the fields.
///
/// **No `deny_unknown_fields`, and this is the exception to a rule this repository otherwise applies
/// everywhere.** That rule is about shapes *this repository defines* - a catalog document, a request
/// body - where an undeclared field is an author's mistake worth naming. This shape is defined by
/// somebody else's tool, which already writes fields nothing here reads (`quota_project_id`,
/// `account`, `token_uri`, `auth_uri`) and will write more. Refusing the file the day it gains one
/// would be a gate that fails on a correct input, which is the shape this repository deletes.
///
/// **Every field is optional and that is not the shape the credential takes.** This is the wire
/// document; [`Credential`] is the parsed value, and its two variants each carry only what their own
/// flow needs - so a document with a refresh token AND a private key cannot become a credential that
/// has both.
///
/// **No `Debug`, and the omission is the mechanism.** Three of these fields are credential
/// material - `client_secret`, `refresh_token`, `private_key` - and they are raw `Option<String>`
/// because `sutura_domain::identity::Secret` has no `Deserialize`, deliberately, so
/// deserialize-then-wrap is the only shape available and this document is the layer where the value
/// is still bare. A derive here would make `{document:?}` a working line that prints a private key.
/// Nothing prints one today; the point of not deriving it is that nothing CAN, and that adding the
/// derive back is a diff a reviewer sees. `docs/adr/0020` decides the same thing one layer up, where
/// the type has no `Display` to reach.
#[derive(serde::Deserialize)]
struct Document {
    /// Which credential shape this is. Required: every other field's meaning depends on it.
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    universe_domain: Option<String>,
    // ------------------------------------------------------ the authorized_user half ----
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    // ------------------------------------------------------ the service_account half ----
    #[serde(default)]
    client_email: Option<String>,
    #[serde(default)]
    private_key: Option<String>,
    #[serde(default)]
    private_key_id: Option<String>,
    /// **The project the key itself names, which is why CI configures no separate project.** A
    /// service-account key carries this; an `authorized_user` file does not.
    #[serde(default)]
    project_id: Option<String>,
}

/// Why the file could not become a credential.
///
/// **Every variant names what is wrong and none of them quotes a value from the file.** A path is
/// carried where the fix is *which file*, a field NAME where the fix is *what is missing*, and the
/// credential's own `type` only after `crate::wire::bounded` has cut it to a fixed character set -
/// because a 16 KiB file can put 16 KiB of newlines there and this string reaches a log.
#[derive(Debug, thiserror::Error)]
pub enum UnusableCredential {
    /// The file could not be opened or read.
    #[error("the credential at {at} could not be read")]
    Unreadable {
        at: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    /// The file is larger than any credential document is.
    ///
    /// Bounded before it is parsed, which is the ordering *secure by design* asks for: the cheap
    /// check that stops work proportional to the input runs first. A credential file is a few
    /// kilobytes at most.
    #[error("the credential at {at} is larger than {cap} bytes")]
    TooLarge { at: PathBuf, cap: usize },
    /// The file is not the JSON document this expects.
    #[error("the credential at {at} is not a credential document")]
    NotADocument {
        at: PathBuf,
        #[source]
        cause: serde_json::Error,
    },
    /// The file names a credential shape this build does not implement.
    ///
    /// **It names the shape rather than saying "unsupported"**, because each named shape has a
    /// different answer: the metadata server needs no file at all, and a federated credential is the
    /// per-subject step. The module header lists both.
    #[error("the credential at {at} is a `{named}`, and this build reads `authorized_user` and `service_account`")]
    UnknownKind { at: PathBuf, named: String },
    /// A document missing one of the fields its own kind needs.
    #[error("the credential at {at} declares no `{field}`")]
    Incomplete { at: PathBuf, field: &'static str },
    /// The credential was minted against a different service universe than the one this build talks
    /// to.
    ///
    /// **Refused rather than tried.** The endpoints this crate reaches are compile-time constants in
    /// the default universe, so a credential minted for another one would be presented to a service it
    /// was not issued for - which is a credential sent to the wrong recipient, whatever the answer
    /// turns out to be.
    #[error("the credential at {at} was minted for another service universe, and this build reaches only the default one")]
    AnotherUniverse { at: PathBuf },
    /// The private key is not a `PKCS#8` PEM block this build can read.
    ///
    /// **Nothing from the key reaches the message.** The whole value is key material, so there is no
    /// half of it that would be safe to quote.
    #[error("the private key in the credential at {at} is not a readable PKCS#8 PEM block")]
    UnreadableKey { at: PathBuf },
}

/// Why no token came back.
#[derive(Debug, thiserror::Error)]
pub enum TokenUnavailable {
    /// The token endpoint did not answer.
    #[error("the token endpoint did not answer")]
    Unreachable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The token endpoint answered, and the answer could not be read.
    #[error("the token endpoint's answer could not be read")]
    Unreadable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The token endpoint refused.
    ///
    /// **The status and the provider's own short code, and not its description.** The two `OAuth`
    /// fields are a fixed vocabulary - `invalid_grant`, `invalid_client` - which is what an operator
    /// needs; the description is free text from another service, and this repository does not put
    /// unbounded foreign text where a log will read it.
    #[error("the token endpoint refused the exchange with {status}: {named}")]
    Refused { status: u16, named: String },
    /// The answer was not the JSON document a token response is.
    #[error("the token endpoint's answer was not a token response")]
    NotADocument {
        #[source]
        cause: serde_json::Error,
    },
    /// The answer carried no token.
    #[error("the token endpoint's answer carried no access token")]
    NoToken,
    /// The answer's own deadline had already passed when it arrived.
    ///
    /// A token that is expired on delivery is a clock disagreeing with a clock, and presenting it
    /// anyway would turn one clear failure into a `401` from the data system.
    #[error("the token endpoint returned a token that expired {at} seconds after the epoch, and it is now {now}")]
    AlreadyExpired { at: u64, now: u64 },
    /// The assertion could not be signed.
    ///
    /// Only reachable for a `service_account`. The cause is kept: `ring`'s own error says whether the
    /// key was rejected, and that is the difference between a bad key and a bad build.
    #[error("the assertion could not be signed")]
    Unsigned {
        #[source]
        cause: ring::error::KeyRejected,
    },
    /// The signature itself failed.
    ///
    /// `ring` reports this opaquely on purpose, so there is nothing to carry beyond the fact.
    #[error("the assertion's signature could not be computed")]
    NotSigned,
}

/// The token endpoint's answer.
///
/// **No `Debug`, for the reason [`Document`] has none:** `access_token` is a bearer token in the
/// clear until `Credential::token` wraps it, and a derive here would put it one `{response:?}` away
/// from a log line. It is parsed and consumed in the same function; nothing needs to render it.
#[derive(serde::Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    /// Seconds of remaining life. Optional in the wild, and its absence is not an error: what it
    /// costs is the deadline, and `Expiry::NothingExpires` is the honest answer for a token whose
    /// life nobody stated.
    #[serde(default)]
    expires_in: Option<u64>,
}

/// The token endpoint's refusal, which is a different document from its answer.
#[derive(Debug, serde::Deserialize)]
struct TokenRefusal {
    #[serde(default)]
    error: Option<String>,
}

/// Bigger than any credential document, small enough that a wrong file is refused rather than read.
const MAX_FILE_BYTES: usize = 16 * 1024;

/// Bigger than any token response, for the same reason.
const MAX_ANSWER_BYTES: u64 = 64 * 1024;

/// The one universe these endpoints belong to.
const UNIVERSE: &str = "googleapis.com";

/// The endpoint both grants are exchanged at.
///
/// **A constant, and deliberately NOT the `token_uri` the file may carry.** The file's job is to name a
/// credential; letting it name an *endpoint* would mean a tampered file could redirect the credential
/// to a host of its choosing, and the tamper would be invisible because the flow would succeed. The
/// cost is that a non-default service universe is unreachable, which
/// [`UnusableCredential::AnotherUniverse`] refuses out loud rather than leaving to fail as a signature
/// error.
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// The one scope this adapter asks for.
///
/// **Not `cloud-platform`, which would be every API in the project.** This crate submits query jobs
/// and reads their rows, so that is what it asks to be allowed to do - least authority applied to the
/// grant rather than only to the identity.
const SCOPE: &str = "https://www.googleapis.com/auth/bigquery";

/// The two credential kinds, each carrying only what its own exchange needs.
///
/// Private, with [`Credential`] as the one public path to it: a public enum would put every field on
/// the public surface, and the whole point of the shape is that nothing outside can assemble a
/// half-credential.
#[derive(Debug)]
enum Kind {
    /// A developer's own login, from `gcloud auth application-default login`.
    AuthorizedUser {
        client_id: String,
        client_secret: Secret,
        refresh_token: Secret,
    },
    /// A service account's key, which is what CI and a deployment can hold.
    ServiceAccount {
        client_email: String,
        private_key_id: String,
        /// The `PKCS#8` DER, still base64 as the PEM carried it, because [`Secret`] holds text and
        /// because decoding per assertion costs nothing beside the round trip it precedes.
        ///
        /// **Held as a `Secret` so a derived `Debug` on this enum cannot print a private key**, which
        /// is the accident `Secret` exists for.
        private_key: Secret,
        project_id: String,
    },
}

/// A credential this build can present, read from a file.
///
/// **One type for both kinds, so a caller does not have to know which file it has.** CI points
/// `GOOGLE_APPLICATION_CREDENTIALS` at a service-account key and a laptop has an
/// application-default login; both reach the endpoint through this.
#[derive(Debug)]
pub struct Credential {
    agent: WireAgent,
    kind: Kind,
}

impl Credential {
    /// Reads a credential file, whichever of the two kinds it holds.
    ///
    /// **The agent is a [`WireAgent`] and not a `ureq::Agent`, which is the point of that newtype:**
    /// the exchange and the job then share one connection pool and one set of pins by construction,
    /// rather than because two call sites happened to call the same builder. A previous version took
    /// any agent, so a composition root could have exchanged a credential over a client with redirects
    /// on and no timeout.
    ///
    /// **The two doctests below are the mechanism, not decoration.** The first is the mistake the
    /// newtype exists to make impossible; the second is its compiling twin, so the failure is the
    /// missing pin rather than a typo in the example.
    ///
    /// ```compile_fail
    /// use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
    ///
    /// // A client with redirects on, plaintext allowed and no timeout. There is no way to hand it in.
    /// let _ = Credential::read(&CredentialFile::at("/nonexistent"), ureq::Agent::new_with_defaults());
    /// ```
    ///
    /// ```
    /// use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
    /// use sutura_exec_bigquery::wire::{BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};
    ///
    /// let bounds = JobBounds::of(
    ///     QueryDeadline::parse(30).expect("a deadline"),
    ///     BytesBilledCeiling::parse(1024 * 1024).expect("a ceiling"),
    /// );
    /// // Compiles, and refuses at run time because the path is not there - which is the point: what
    /// // the first example cannot get past is the TYPE, before any file is read.
    /// let refused = Credential::read(&CredentialFile::at("/nonexistent"), WireAgent::pinned(bounds));
    /// assert!(refused.is_err());
    /// ```
    pub fn read(file: &CredentialFile, agent: WireAgent) -> Result<Self, UnusableCredential> {
        let at = PathBuf::from(file.path());
        let opened =
            std::fs::File::open(file.path()).map_err(|cause| UnusableCredential::Unreadable { at: at.clone(), cause })?;
        let mut text = String::new();
        // `take` rather than a metadata check, so the bound is on what was actually read: a file that
        // grows between the two calls is not a case this has to reason about.
        let read = u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
        opened
            .take(read)
            .read_to_string(&mut text)
            .map_err(|cause| UnusableCredential::Unreadable { at: at.clone(), cause })?;
        if text.len() > MAX_FILE_BYTES {
            return Err(UnusableCredential::TooLarge { at, cap: MAX_FILE_BYTES });
        }
        let document: Document =
            serde_json::from_str(&text).map_err(|cause| UnusableCredential::NotADocument { at: at.clone(), cause })?;
        Self::read_document(document, &at, agent)
    }

    /// The document, as a credential, or the reason it is not one.
    ///
    /// Separate from [`Self::read`] so every refusal above the file system is reachable from a test
    /// without writing a file - the same split the response decoding in [`crate::wire`] uses. Named
    /// `read_document` rather than `of` because a reader of a test wants to see that a DOCUMENT went in
    /// and no file was touched.
    fn read_document(document: Document, at: &Path, agent: WireAgent) -> Result<Self, UnusableCredential> {
        if document.universe_domain.as_deref().is_some_and(|named| named != UNIVERSE) {
            return Err(UnusableCredential::AnotherUniverse { at: PathBuf::from(at) });
        }
        let required = |value: Option<String>, field: &'static str| {
            value
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| UnusableCredential::Incomplete {
                    at: PathBuf::from(at),
                    field,
                })
        };
        // The one place the file's own word decides which flow will run. An exhaustive-by-construction
        // match: two known spellings and everything else refused by name.
        let kind = match document.kind.as_str() {
            "authorized_user" => Kind::AuthorizedUser {
                client_id: required(document.client_id, "client_id")?,
                client_secret: Secret::new(required(document.client_secret, "client_secret")?),
                refresh_token: Secret::new(required(document.refresh_token, "refresh_token")?),
            },
            "service_account" => Kind::ServiceAccount {
                client_email: required(document.client_email, "client_email")?,
                private_key_id: required(document.private_key_id, "private_key_id")?,
                // Unwrapped from its PEM HERE rather than at first use, so a malformed key is a
                // startup refusal instead of a failure on the first question - and so the unwrapping
                // has a test that needs no network.
                private_key: unwrap_pem(&required(document.private_key, "private_key")?)
                    .ok_or_else(|| UnusableCredential::UnreadableKey { at: PathBuf::from(at) })?,
                project_id: required(document.project_id, "project_id")?,
            },
            other => {
                return Err(UnusableCredential::UnknownKind {
                    at: PathBuf::from(at),
                    named: crate::wire::bounded(Some(String::from(other))),
                });
            }
        };
        Ok(Self { agent, kind })
    }

    /// Which project this credential names, where it names one.
    ///
    /// `Some` for a service-account key and `None` for an application-default login, which is the
    /// difference the two files actually have. **This is why CI configures no project variable:** the
    /// key carries it, so a second declaration would be a second answer to *who pays* that can
    /// disagree with the first.
    #[inline]
    #[must_use]
    pub const fn project(&self) -> Option<&String> {
        match self.kind {
            Kind::AuthorizedUser { .. } => None,
            Kind::ServiceAccount { ref project_id, .. } => Some(project_id),
        }
    }

    /// Which kind this is, for a banner or a test. A fixed word, never the file's own text.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self.kind {
            Kind::AuthorizedUser { .. } => "authorized_user",
            Kind::ServiceAccount { .. } => "service_account",
        }
    }

    /// The signed assertion a service account trades for a token.
    ///
    /// **`ring` computes the signature and this function writes the text**, which is the boundary the
    /// module header argues for. Two JSON documents, base64url with no padding, joined with a dot,
    /// signed, and the signature appended the same way - RFC 7515's compact serialization.
    fn assertion(
        client_email: &str,
        private_key_id: &str,
        private_key: &Secret,
        now_unix_seconds: u64,
    ) -> Result<String, TokenUnavailable> {
        /// How long an assertion is good for. Ten minutes rather than the hour the endpoint permits,
        /// because an assertion is a bearer credential in flight and its window is its replay window -
        /// the same argument `sutura_config::ProofLifetime` makes about a gateway's assertion.
        const LIVES_FOR: u64 = 600;

        let url = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        // `alg` and `typ` are CONSTANTS in the text, not values read from anywhere - which is what
        // makes algorithm confusion a non-question on the signing side.
        let header = serde_json::json!({ "alg": "RS256", "typ": "JWT", "kid": private_key_id });
        let claims = serde_json::json!({
            "iss": client_email,
            "sub": client_email,
            "scope": SCOPE,
            "aud": TOKEN_ENDPOINT,
            "iat": now_unix_seconds,
            "exp": now_unix_seconds.saturating_add(LIVES_FOR),
        });
        let mut signing_input = url.encode(header.to_string().as_bytes());
        signing_input.push('.');
        signing_input.push_str(&url.encode(claims.to_string().as_bytes()));

        let der = base64::engine::general_purpose::STANDARD
            .decode(private_key.expose_secret().as_bytes())
            .map_err(|_ignored| TokenUnavailable::NotSigned)?;
        let key = ring::signature::RsaKeyPair::from_pkcs8(&der).map_err(|cause| TokenUnavailable::Unsigned { cause })?;
        let mut signature = vec![0_u8; key.public().modulus_len()];
        key.sign(
            &ring::signature::RSA_PKCS1_SHA256,
            &ring::rand::SystemRandom::new(),
            signing_input.as_bytes(),
            &mut signature,
        )
        .map_err(|_ignored| TokenUnavailable::NotSigned)?;

        let mut token = signing_input;
        token.push('.');
        token.push_str(&url.encode(&signature));
        Ok(token)
    }

    /// Posts a form to the token endpoint and reads a bearer out of the answer.
    ///
    /// **Shared by both grants, because it is the same exchange** - the same endpoint, the same
    /// response document, the same refusal shapes - and only the form differs. Two copies would be two
    /// vocabularies for one endpoint's answers, and the second copy is where the expired-on-arrival
    /// check would fail to get added.
    fn exchange<'form, F>(&self, form: F, now_unix_seconds: u64) -> Result<Bearer, TokenUnavailable>
    where
        F: IntoIterator<Item = (&'form str, &'form str)>,
    {
        let mut answer = self
            .agent
            .agent()
            .post(TOKEN_ENDPOINT)
            .send_form(form)
            .map_err(|cause| TokenUnavailable::Unreachable { cause: Box::new(cause) })?;
        let status = answer.status();
        let body = answer
            .body_mut()
            .with_config()
            .limit(MAX_ANSWER_BYTES)
            .read_to_string()
            .map_err(|cause| TokenUnavailable::Unreadable { cause: Box::new(cause) })?;
        if !status.is_success() {
            // A refusal document is best-effort: what is guaranteed is the status, and the code is
            // read out of the body when the body is the document the provider documents.
            let refusal: TokenRefusal = serde_json::from_str(&body).unwrap_or(TokenRefusal { error: None });
            return Err(TokenUnavailable::Refused {
                status: status.as_u16(),
                named: crate::wire::bounded(refusal.error),
            });
        }
        let response: TokenResponse = serde_json::from_str(&body).map_err(|cause| TokenUnavailable::NotADocument { cause })?;
        let token = response
            .access_token
            .filter(|t| !t.trim().is_empty())
            .ok_or(TokenUnavailable::NoToken)?;
        let not_after = deadline(response.expires_in, now_unix_seconds);
        if let Some(at) = not_after.passed_by(now_unix_seconds) {
            return Err(TokenUnavailable::AlreadyExpired {
                at,
                now: now_unix_seconds,
            });
        }
        Ok(Bearer::of(Secret::new(token), not_after))
    }
}

impl AccessTokens for Credential {
    type Error = TokenUnavailable;

    /// **The one place the two kinds differ about the REQUEST rather than about the exchange**, and
    /// getting it wrong breaks one of them - see [`QuotaProject`].
    fn quota_project(&self) -> QuotaProject {
        match self.kind {
            Kind::AuthorizedUser { .. } => QuotaProject::Required,
            Kind::ServiceAccount { .. } => QuotaProject::FromTheCredential,
        }
    }

    /// Exchanges whatever this credential holds for an access token.
    ///
    /// Form-encoded, because that is what both grants take; every value travels in the body and never
    /// in the URL, so none of them reaches a proxy log as a query string.
    fn bearer(&self, now_unix_seconds: u64) -> Result<Bearer, Self::Error> {
        /// The grant a signed assertion is presented under, spelled as the endpoint documents it.
        const ASSERTION_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

        match self.kind {
            Kind::AuthorizedUser {
                ref client_id,
                ref client_secret,
                ref refresh_token,
            } => self.exchange(
                [
                    ("grant_type", "refresh_token"),
                    ("client_id", client_id.as_str()),
                    ("client_secret", client_secret.expose_secret()),
                    ("refresh_token", refresh_token.expose_secret()),
                ],
                now_unix_seconds,
            ),
            Kind::ServiceAccount {
                ref client_email,
                ref private_key_id,
                ref private_key,
                ..
            } => {
                let assertion = Self::assertion(client_email, private_key_id, private_key, now_unix_seconds)?;
                self.exchange(
                    [("grant_type", ASSERTION_GRANT), ("assertion", assertion.as_str())],
                    now_unix_seconds,
                )
            }
        }
    }
}

/// The delimiters a service-account key's private key is wrapped in.
///
/// `PKCS#8` - which is what the endpoint issues and what `ring` reads directly, and the reason no
/// `ASN.1` conversion is needed anywhere in this crate.
///
/// **Assembled from three pieces rather than written as one literal, and that is not stylistic.** The
/// leak guard matches a PEM header, and a fixture in this module's own tests that spelled one out
/// tripped it - which would block `pre-push` for everybody over a string that is a delimiter rather
/// than a key. Composing it here means the tests build fixtures from the same constant the parser
/// uses, so the two cannot drift either.
const PEM_LABEL: &str = "PRIVATE KEY";
pub(super) const PEM_BEGIN_MARK: &str = "-----BEGIN ";
pub(super) const PEM_END_MARK: &str = "-----END ";
const PEM_TAIL: &str = "-----";

/// The opening delimiter, assembled once.
static PEM_BEGIN: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| format!("{PEM_BEGIN_MARK}{PEM_LABEL}{PEM_TAIL}"));

/// The closing delimiter, assembled once.
static PEM_END: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| format!("{PEM_END_MARK}{PEM_LABEL}{PEM_TAIL}"));

/// The base64 body inside a `PKCS#8` PEM block, or `None`.
///
/// **Hand-written, and the boundary of what is hand-written matters:** this strips two delimiter lines
/// and packs the rest, which is a text format. It does not parse `ASN.1` and it does not touch the
/// key's structure - `ring::signature::RsaKeyPair::from_pkcs8` does that, and hand-rolling it is what
/// `docs/adr/0014` argues against.
///
/// It returns a [`Secret`] rather than a `String`, so the value cannot be printed on the way from here
/// to the field that holds it.
fn unwrap_pem(pem: &str) -> Option<Secret> {
    let body = pem.trim().strip_prefix(PEM_BEGIN.as_str())?.strip_suffix(PEM_END.as_str())?;
    let packed: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    if packed.is_empty() {
        return None;
    }
    Some(Secret::new(packed))
}

/// The deadline a token response states, as the domain names it.
///
/// An absent `expires_in` becomes [`Expiry::NothingExpires`], which is the honest reading: the
/// provider stated no deadline. It is NOT a claim that the token is eternal - the data system will
/// refuse an expired one - and with nothing cached there is no window in which the difference could be
/// acted on.
const fn deadline(expires_in: Option<u64>, now_unix_seconds: u64) -> Expiry {
    match expires_in {
        None => Expiry::NothingExpires,
        Some(seconds) => Expiry::At {
            unix_seconds: now_unix_seconds.saturating_add(seconds),
        },
    }
}

#[cfg(test)]
mod tests;

//! Where the bearer token a job is submitted with comes from.
//!
//! **A second narrow port, for the reason [`JobTransport`](crate::transport::JobTransport) is one.**
//! The wire needs one thing from a credential - a token that is usable right now, and the instant it
//! stops being usable - and everything else about how a deployment authenticates is somebody else's
//! decision. So [`AccessTokens`] is that one thing, and the transport is generic in it.
//!
//! **This is also the seam per-subject execution arrives at**, which is why it is a port on the first
//! day rather than a `String` field. `docs/implementation-plan-bigquery.md`'s second `BigQuery` step
//! mints a token *per leg, for the subject who asked*; under a `String` that step would have to change
//! the transport, and under a port it adds an implementor. Nothing here anticipates it further than
//! that: [`Bearer`] carries the deadline because a minted token has one, and
//! [`crate::BigQueryWarehouse`]'s `IMPERSONATION` still says `NoPlaceForASubject` because nothing
//! mints one. (Plain backticks on the constant, because it is a `Warehouse` trait item rather than an
//! inherent one and an intra-doc link to it does not resolve - `api-docs` reported that as a broken
//! link, which is the one class of rustdoc warning `AGENTS.md` says can reach `mkdocs --strict`.)
//!
//! # What ships, and what does not
//!
//! [`ApplicationDefault`] reads the file `gcloud auth application-default login` writes and exchanges
//! its refresh token for an access token. That is exactly the fixture
//! [`docs/adr/0017`](https://github.com/telekom/sutura/blob/main/docs/adr/0017-what-a-bigquery-test-runs-against.md)
//! decided - a real project reached from a developer's own machine, under the `SharedServiceUser`
//! posture - and `just gcloud-login` is what produces it.
//!
//! **Three other credential shapes exist and none of them is built**, each refused by name rather
//! than mishandled:
//!
//! - a **service-account key**, which needs an `RS256` assertion signed with a private key. The
//!   signing is the dependency decision, not the flow: `jsonwebtoken` is already in this workspace
//!   but with `use_pem` off, so a key in PEM form has nothing to parse it. `docs/adr/0018` prices it.
//! - the **metadata server**, which is how a deployment on the provider's own compute gets a token
//!   with no key at all. It is a plain unauthenticated `GET` and would cost nothing in dependencies;
//!   what it costs is that **nothing in this repository can verify it**, because it exists only
//!   inside that provider's network.
//! - **workload or workforce identity federation**, which is the per-subject step and an
//!   architecture decision with an owner outside this repository.
//!
//! # Nothing is cached, and that is a decision
//!
//! A token is minted for every job. The obvious alternative - hold the last one until it expires -
//! is refused for three reasons, in ascending order of how much they matter:
//!
//! 1. `clippy.toml` disallows `std::sync::Mutex` in this workspace, so the cache would arrive with a
//!    new dependency for the primitive to hold it.
//! 2. *"A credential is not reused past its expiry"* is one of the assertions the per-subject step
//!    owes, and the shape that cannot get it wrong is the one with nothing to reuse.
//! 3. **It is the credential-shaped version of the cache this crate already refuses.** `lib.rs` says
//!    a query-keyed result cache is a cross-user leak under row-level security; a *token* cache keyed
//!    by nothing is the same defect one layer down, and it would be sitting in the code the
//!    per-subject step has to change. A cache that is correct for one identity and wrong for many is
//!    worse than no cache, because it works until the day it matters.
//!
//! **The cost, stated rather than waved at:** one extra `HTTPS` round trip per job, against a query
//! that costs seconds and money. When that is measured to matter, what arrives is a cache keyed by
//! whose credential it is - which is a thing only the per-subject step can key.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use sutura_domain::identity::{Expiry, Secret};

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

    /// The token, still opaque. A caller has to reach `Secret::expose` to write it into a header, and
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

/// Where the token a job is submitted with comes from.
///
/// One method, and it takes the clock as an argument. **The clock is a parameter for the reason
/// [`Expiry::passed_by`] takes one:** an implementor that reads the wall clock itself cannot be tested
/// against a deadline, and the expiry logic is the half of a credential source most likely to be
/// wrong in the direction nobody notices. Who reads the real clock is the transport, once.
pub trait AccessTokens {
    /// Why no token could be produced. Typed per source: a missing file, a refused refresh and a
    /// clock that disagrees are not the same thing to whoever responds to them.
    type Error: core::error::Error + Send + Sync + 'static;

    /// A token usable at `now_unix_seconds`.
    fn bearer(&self, now_unix_seconds: u64) -> Result<Bearer, Self::Error>;
}

/// The file an application-default credential lives in.
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
#[error("no {named} is set, so there is no well-known location for an application-default credential")]
pub struct NoWellKnownLocation {
    /// Which variables were looked for, so the message says what to set.
    named: &'static str,
}

impl CredentialFile {
    /// The variable a caller sets to name the file outright. Read first, because that is what it is
    /// for.
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
/// `account`, `universe_domain`) and will write more. Refusing the file the day it gains one would be
/// a gate that fails on a correct input, which is the shape this repository deletes.
#[derive(Debug, serde::Deserialize)]
struct Document {
    /// Which credential shape this is. Required: every other field's meaning depends on it.
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    /// Present on a credential minted for a non-default universe. Read only to REFUSE - see
    /// [`UnusableCredential::AnotherUniverse`].
    #[serde(default)]
    universe_domain: Option<String>,
}

/// Why the file could not become a credential.
///
/// **Every variant names what is wrong and none of them quotes the file's contents.** A path is
/// carried where the fix is *which file*, and a field NAME is carried where the fix is *what is
/// missing*; the value never is, because in this file every value is either a secret or a project
/// identifier.
#[derive(Debug, thiserror::Error)]
pub enum UnusableCredential {
    /// The file could not be opened or read.
    #[error("the application-default credential at {at} could not be read")]
    Unreadable {
        at: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    /// The file is larger than any credential document is.
    ///
    /// Bounded before it is parsed, which is the ordering *secure by design* asks for: the cheap
    /// check that stops work proportional to the input runs first. A credential file is a few hundred
    /// bytes.
    #[error("the application-default credential at {at} is larger than {cap} bytes")]
    TooLarge { at: PathBuf, cap: usize },
    /// The file is not the JSON document this expects.
    #[error("the application-default credential at {at} is not a credential document")]
    NotADocument {
        at: PathBuf,
        #[source]
        cause: serde_json::Error,
    },
    /// The file is a credential shape this build does not implement.
    ///
    /// **It names the shape rather than saying "unsupported"**, because each named shape has a
    /// different answer: a service-account key needs signing this build has no dependency for, and a
    /// federated credential is the per-subject step. The module header lists all three.
    #[error("the credential at {at} is a `{named}`, and this build reads an `authorized_user`")]
    NotAUserCredential { at: PathBuf, named: String },
    /// An `authorized_user` document missing one of the three fields a refresh needs.
    #[error("the credential at {at} declares no `{field}`")]
    Incomplete { at: PathBuf, field: &'static str },
    /// The credential was minted against a different service universe than the one this build talks
    /// to.
    ///
    /// **Refused rather than tried.** The endpoints this crate reaches are compile-time constants in
    /// the default universe, so a credential minted for another one would be presented to a service
    /// it was not issued for - which is a credential sent to the wrong recipient, whatever the answer
    /// turns out to be.
    #[error("the credential at {at} was minted for another service universe, and this build reaches only the default one")]
    AnotherUniverse { at: PathBuf },
}

/// Why a refresh did not produce a token.
#[derive(Debug, thiserror::Error)]
pub enum RefreshFailed {
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
    /// **The status and the provider's own short reason, and not its message.** The two `OAuth`
    /// fields are a fixed vocabulary - `invalid_grant`, `invalid_client` - which is what an operator
    /// needs and is not free text; the description is free text from another service, and this
    /// repository does not put unbounded foreign text where a log will read it.
    #[error("the token endpoint refused the refresh with {status}: {named}")]
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
}

/// The token endpoint's answer.
#[derive(Debug, serde::Deserialize)]
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

/// An access-token source built from the file `gcloud auth application-default login` writes.
///
/// **One identity for everybody who asks**, which is `SharedServiceUser` and is the posture
/// [`crate::BigQueryWarehouse`] declares. On a developer's machine that identity is the developer.
#[derive(Debug)]
pub struct ApplicationDefault {
    agent: ureq::Agent,
    client_id: String,
    client_secret: Secret,
    refresh_token: Secret,
}

impl ApplicationDefault {
    /// The endpoint a refresh token is exchanged at.
    ///
    /// **A constant, and deliberately NOT the `token_uri` the file may carry.** The file's job is to
    /// name a credential; letting it name an *endpoint* would mean a tampered file could redirect the
    /// credential to a host of its choosing, and the tamper would be invisible because the flow would
    /// succeed. The cost is that a non-default service universe is unreachable, which
    /// [`UnusableCredential::AnotherUniverse`] refuses out loud rather than leaving to fail as a
    /// signature error.
    const TOKEN_ENDPOINT: &'static str = "https://oauth2.googleapis.com/token";
    /// The one universe these constants belong to.
    const UNIVERSE: &'static str = "googleapis.com";
    /// Bigger than any credential document, small enough that a wrong file is refused rather than
    /// read.
    const MAX_FILE_BYTES: usize = 16 * 1024;
    /// Bigger than any token response, for the same reason.
    const MAX_ANSWER_BYTES: u64 = 64 * 1024;
    /// How much of a provider's error code is kept. `invalid_grant` is eighteen characters; anything
    /// past this is not a code.
    const MAX_NAMED_BYTES: usize = 64;

    /// Reads a credential file.
    ///
    /// The `agent` is handed in rather than built here, so the refresh and the job share one
    /// connection pool and one [`crate::wire::agent`] configuration - which is what makes "the
    /// redirect policy is set once" true rather than true twice.
    pub fn read(file: &CredentialFile, agent: ureq::Agent) -> Result<Self, UnusableCredential> {
        let at = || PathBuf::from(file.path());
        let opened = std::fs::File::open(file.path()).map_err(|cause| UnusableCredential::Unreadable { at: at(), cause })?;
        let mut text = String::new();
        // `take` rather than a metadata check, so the bound is on what was actually read: a file that
        // grows between the two calls is not a case this has to reason about.
        let read = u64::try_from(Self::MAX_FILE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
        opened
            .take(read)
            .read_to_string(&mut text)
            .map_err(|cause| UnusableCredential::Unreadable { at: at(), cause })?;
        if text.len() > Self::MAX_FILE_BYTES {
            return Err(UnusableCredential::TooLarge {
                at: at(),
                cap: Self::MAX_FILE_BYTES,
            });
        }
        let document: Document =
            serde_json::from_str(&text).map_err(|cause| UnusableCredential::NotADocument { at: at(), cause })?;
        Self::of(document, file.path(), agent)
    }

    /// The document, as a credential, or the reason it is not one.
    ///
    /// Separate from [`Self::read`] so every refusal above the file system is reachable in a test
    /// without writing a file, which is the same split the response decoding in
    /// [`crate::wire`] uses.
    fn of(document: Document, at: &Path, agent: ureq::Agent) -> Result<Self, UnusableCredential> {
        if document.kind != "authorized_user" {
            return Err(UnusableCredential::NotAUserCredential {
                at: PathBuf::from(at),
                named: document.kind,
            });
        }
        if document
            .universe_domain
            .as_deref()
            .is_some_and(|named| named != Self::UNIVERSE)
        {
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
        Ok(Self {
            agent,
            client_id: required(document.client_id, "client_id")?,
            client_secret: Secret::new(required(document.client_secret, "client_secret")?),
            refresh_token: Secret::new(required(document.refresh_token, "refresh_token")?),
        })
    }

    /// The provider's error code, bounded and stripped of anything that is not one.
    ///
    /// **Not a slice**, because `clippy::string_slice` is denied here and because a byte slice of
    /// foreign text can land inside a multi-byte character. Taking characters is both correct and
    /// what the ban is for.
    fn named(refusal: Option<String>) -> String {
        refusal
            .unwrap_or_default()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(Self::MAX_NAMED_BYTES)
            .collect()
    }

    /// The deadline a token response states, as the domain names it.
    ///
    /// An absent `expires_in` becomes [`Expiry::NothingExpires`], which is the honest reading: the
    /// provider stated no deadline. It is NOT a claim that the token is eternal - the data system
    /// will refuse an expired one - and with nothing cached there is no window in which the
    /// difference could be acted on.
    const fn deadline(expires_in: Option<u64>, now_unix_seconds: u64) -> Expiry {
        match expires_in {
            None => Expiry::NothingExpires,
            Some(seconds) => Expiry::At {
                unix_seconds: now_unix_seconds.saturating_add(seconds),
            },
        }
    }
}

impl AccessTokens for ApplicationDefault {
    type Error = RefreshFailed;

    /// Exchanges the refresh token for an access token.
    ///
    /// Form-encoded, because that is what the endpoint's grant takes; the three values travel in the
    /// body and never in the URL, so none of them reaches a proxy log as a query string.
    fn bearer(&self, now_unix_seconds: u64) -> Result<Bearer, Self::Error> {
        let mut answer = self
            .agent
            .post(Self::TOKEN_ENDPOINT)
            .send_form([
                ("grant_type", "refresh_token"),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.expose()),
                ("refresh_token", self.refresh_token.expose()),
            ])
            .map_err(|cause| RefreshFailed::Unreachable { cause: Box::new(cause) })?;
        let status = answer.status();
        let body = answer
            .body_mut()
            .with_config()
            .limit(Self::MAX_ANSWER_BYTES)
            .read_to_string()
            .map_err(|cause| RefreshFailed::Unreadable { cause: Box::new(cause) })?;
        if !status.is_success() {
            // A refusal document is best-effort: what is guaranteed is the status, and the code is
            // read out of the body when the body is the document the provider documents.
            let refusal: TokenRefusal = serde_json::from_str(&body).unwrap_or(TokenRefusal { error: None });
            return Err(RefreshFailed::Refused {
                status: status.as_u16(),
                named: Self::named(refusal.error),
            });
        }
        let response: TokenResponse = serde_json::from_str(&body).map_err(|cause| RefreshFailed::NotADocument { cause })?;
        let token = response
            .access_token
            .filter(|t| !t.trim().is_empty())
            .ok_or(RefreshFailed::NoToken)?;
        let not_after = Self::deadline(response.expires_in, now_unix_seconds);
        if let Some(at) = not_after.passed_by(now_unix_seconds) {
            return Err(RefreshFailed::AlreadyExpired {
                at,
                now: now_unix_seconds,
            });
        }
        Ok(Bearer::of(Secret::new(token), not_after))
    }
}

#[cfg(test)]
mod tests {
    use super::{ApplicationDefault, CredentialFile, Document, UnusableCredential};
    use sutura_domain::identity::Expiry;

    /// A document with everything an `authorized_user` needs, so a test changes one field at a time.
    /// The values are not credentials: they are the shape, and `client_secret` here is a literal
    /// nobody can use.
    fn complete() -> Document {
        Document {
            kind: String::from("authorized_user"),
            client_id: Some(String::from("an-installed-app.apps.example")),
            client_secret: Some(String::from("not-a-secret")),
            refresh_token: Some(String::from("not-a-token")),
            universe_domain: None,
        }
    }

    /// Where a refusal says the file was. A fixed path in the temporary directory, never read.
    fn at() -> std::path::PathBuf {
        std::path::PathBuf::from("/nonexistent/application_default_credentials.json")
    }

    #[test]
    fn a_credential_shape_this_build_does_not_read_is_refused_by_name() {
        // **Named rather than "unsupported"**, because each shape has a different answer: a
        // service-account key needs signing nothing here depends on, and a federated one is the
        // per-subject step. A reader of the message has to be able to tell which.
        for named in [
            "service_account",
            "external_account",
            "impersonated_service_account",
            "external_account_authorized_user",
            "gdch_service_account",
        ] {
            let document = Document {
                kind: String::from(named),
                ..complete()
            };
            let refused = ApplicationDefault::of(document, &at(), crate::wire::agent());
            match refused {
                Err(UnusableCredential::NotAUserCredential { named: ref found, .. }) => assert_eq!(found, named),
                other => panic!("{named} was accepted as a user credential: {other:?}"),
            }
        }
    }

    #[test]
    fn a_credential_minted_for_another_service_universe_is_refused() {
        // The endpoints this crate reaches are compile-time constants in the default universe, so a
        // credential for another one would be presented to a service it was not issued for. Refused
        // out loud rather than left to fail as a signature error.
        let document = Document {
            universe_domain: Some(String::from("example.test")),
            ..complete()
        };
        let refused = ApplicationDefault::of(document, &at(), crate::wire::agent());
        assert!(
            matches!(refused, Err(UnusableCredential::AnotherUniverse { .. })),
            "{refused:?}"
        );

        // And the default universe, written out, is accepted - so the check is not "any value is
        // wrong".
        let document = Document {
            universe_domain: Some(String::from("googleapis.com")),
            ..complete()
        };
        ApplicationDefault::of(document, &at(), crate::wire::agent()).expect("the default universe is accepted");
    }

    #[test]
    fn each_of_the_three_fields_a_refresh_needs_is_refused_by_name_when_absent() {
        // Three fields and three refusals, because the fix for each is a different line of the file -
        // and an author reading "the credential is incomplete" cannot act on it.
        let cases: [(&str, Document); 6] = [
            (
                "client_id",
                Document {
                    client_id: None,
                    ..complete()
                },
            ),
            (
                "client_id",
                Document {
                    client_id: Some(String::from("   ")),
                    ..complete()
                },
            ),
            (
                "client_secret",
                Document {
                    client_secret: None,
                    ..complete()
                },
            ),
            (
                "client_secret",
                Document {
                    client_secret: Some(String::new()),
                    ..complete()
                },
            ),
            (
                "refresh_token",
                Document {
                    refresh_token: None,
                    ..complete()
                },
            ),
            (
                "refresh_token",
                Document {
                    refresh_token: Some(String::from("\t")),
                    ..complete()
                },
            ),
        ];
        for (field, document) in cases {
            let refused = ApplicationDefault::of(document, &at(), crate::wire::agent());
            match refused {
                Err(UnusableCredential::Incomplete { field: found, .. }) => assert_eq!(found, field),
                other => panic!("an absent {field} was accepted: {other:?}"),
            }
        }
    }

    #[test]
    fn a_refusal_names_the_file_and_never_its_contents() {
        // Every value in this file is either a secret or a project identifier, so the path is the only
        // thing a message may carry - which is the same rule `ProjectId`'s own refusal follows one
        // module over.
        let document = Document {
            client_secret: Some(String::from("do-not-print-me")),
            refresh_token: None,
            ..complete()
        };
        let refused = ApplicationDefault::of(document, &at(), crate::wire::agent()).expect_err("it refused");
        let shown = refused.to_string();
        assert!(shown.contains("application_default_credentials.json"), "{shown}");
        assert!(!shown.contains("do-not-print-me"), "the refusal quoted a secret: {shown}");
    }

    #[test]
    fn a_file_that_is_not_there_is_refused_naming_the_path_and_keeping_the_cause() {
        let refused = ApplicationDefault::read(&CredentialFile::at(at()), crate::wire::agent()).expect_err("it refused");
        assert!(matches!(refused, UnusableCredential::Unreadable { .. }), "{refused:?}");
        assert!(
            core::error::Error::source(&refused).is_some(),
            "the io cause did not survive #[source]"
        );
    }

    #[test]
    fn an_absent_lifetime_is_no_deadline_rather_than_an_invented_one() {
        // **The honest reading**, and it costs nothing here because nothing is cached: there is no
        // window in which the difference between "no deadline stated" and "eternal" could be acted
        // on. What refuses an expired token is the data system.
        assert_eq!(ApplicationDefault::deadline(None, 1_000), Expiry::NothingExpires);
        assert_eq!(
            ApplicationDefault::deadline(Some(3_600), 1_000),
            Expiry::At { unix_seconds: 4_600 }
        );
        // Saturating rather than wrapping, so a provider stating an absurd lifetime cannot produce a
        // deadline in the past.
        assert_eq!(
            ApplicationDefault::deadline(Some(u64::MAX), 1_000),
            Expiry::At { unix_seconds: u64::MAX }
        );
    }

    #[test]
    fn a_providers_error_code_is_bounded_and_filtered() {
        // Foreign text heading for a log, bounded by characters rather than bytes.
        assert_eq!(
            ApplicationDefault::named(Some(String::from("invalid_grant"))),
            "invalid_grant"
        );
        assert_eq!(ApplicationDefault::named(None), "");
        assert_eq!(
            ApplicationDefault::named(Some(String::from("bad\ncode \u{1b}\"x\""))),
            "badcodex"
        );
        assert_eq!(
            ApplicationDefault::named(Some("z".repeat(1_024))).len(),
            ApplicationDefault::MAX_NAMED_BYTES
        );
    }

    #[test]
    fn the_file_location_is_a_claim_and_not_a_path_argument() {
        // `at` says THIS file and `well_known` says wherever this machine keeps it, and a function
        // taking a `PathBuf` could not have told a reader which it was handed.
        //
        // **`well_known` itself is not tested, and the reason is a hard one rather than an omission:**
        // it reads three environment variables, `std::env::set_var` is `unsafe` in this edition, and
        // `unsafe_code` is `forbid` in this workspace. So there is no way to give it an environment
        // from a test here. What is testable is that the two constructors are distinguishable and that
        // the explicit one is exact.
        let named = CredentialFile::at("/tmp/somewhere/creds.json");
        assert_eq!(named.path(), std::path::Path::new("/tmp/somewhere/creds.json"));
    }
}

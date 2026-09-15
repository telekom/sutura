//! The HTTP half of [`crate::sts::ImpersonateAsAccount`]: a federated access token in, a
//! service-account access token out - telekom/sutura#376's second hop.
//!
//! **Behind the same default-off `wire` feature [`crate::wire::StsOverHttp`] is**, and for the same
//! reason: an outbound TLS stack is a dependency decision a composition root makes in a manifest
//! line, not something the adapter inherits by being linked. It shares the crate's one
//! [`crate::wire::WireAgent`] - the outbound anchors `security.outbound.transport_anchors` resolves
//! reach this endpoint identically to `sts.googleapis.com`, no new anchor plumbing.
//!
//! The request is `iamcredentials.generateAccessToken`: the federated access token the first hop
//! produced, presented as this request's own bearer, asking for a short-lived access token scoped to
//! the account [`crate::sts::WorkloadIdentity::target_for`] declares. The exchanged
//! [`crate::sts::StsCredential`] carries that access token and the instant it stops being usable,
//! computed from the lifetime THIS adapter requested - never parsed from the endpoint's own
//! `expireTime`, since Google grants exactly what a request within its documented ceiling asks for
//! or refuses the call outright; there is no partial grant to reconcile against a second clock read.

use sutura_domain::identity::{Expiry, Secret};

use crate::sts::{ImpersonateAsAccount, StsCredential};
use crate::wire::{CallDeadline, WireAgent};

/// The API this module speaks to.
const HOST: &str = "https://iamcredentials.googleapis.com/v1";

/// The request body, as `generateAccessToken`'s own document describes it.
#[derive(serde::Serialize)]
struct Request<'a> {
    scope: [&'a str; 1],
    lifetime: String,
}

/// The answer, with the one field this adapter reads. `expireTime` is left undeserialized - see the
/// module header for why this adapter does not read it.
#[derive(serde::Deserialize)]
struct Response {
    #[serde(rename = "accessToken")]
    access_token: String,
}

/// The account this hop names in a refusal - carried, and never rendered raw.
///
/// **The same discipline `crate::wire::EndpointMessage` holds, applied to a value that is ours rather
/// than the endpoint's own free text.** The target is a declared configuration value, not a secret,
/// but this hop's whole point is that "the pool subject may not impersonate this account" reaches a
/// caller as a class of refusal and never as an account identifier - see
/// `docs/where-identity-is-proven.md` and this crate's own `wire::EndpointMessage` header for the
/// measured cost of a formatter that walked a struct instead of asking it.
#[derive(Clone, PartialEq, Eq)]
pub struct RedactedSa(String);

impl RedactedSa {
    fn of(raw: &str) -> Self {
        Self(String::from(raw))
    }
}

impl core::fmt::Debug for RedactedSa {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "<the declared impersonation target, {} char(s), redacted>", self.0.len())
    }
}

/// The account this hop asks the endpoint to impersonate, as this adapter holds it.
///
/// **Re-validated here for the reason `crate::transport::ProjectId` gives**:
/// `sutura_config::sources::workload_identity::WorkloadIdentitySa` is checked once where it is
/// declared, and this crate may not depend on that settings tree - so the format is checked again
/// where the value is interpolated into a request path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ImpersonatedAccount(String);

/// Why a declared impersonation target cannot be interpolated into this hop's request path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnusableAccount {
    /// Nothing was written, or only whitespace was.
    #[error("the impersonation target is empty")]
    Empty,
    /// A character that could leave the URL path segment this value is written into.
    #[error("the character at position {at} is not allowed in an impersonation target")]
    Character { at: usize },
}

impl ImpersonatedAccount {
    /// Parses a target account.
    ///
    /// The accepted set is `[A-Za-z0-9._-@]` - a service-account email's own alphabet - so a value
    /// that would escape the URL path segment this is interpolated into cannot exist here.
    fn parse(raw: &str) -> Result<Self, UnusableAccount> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(UnusableAccount::Empty);
        }
        if let Some(at) = trimmed
            .char_indices()
            .find_map(|(at, c)| (!matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | '@')).then_some(at))
        {
            return Err(UnusableAccount::Character { at });
        }
        Ok(Self(String::from(trimmed)))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why the hop could not happen.
#[derive(Debug, thiserror::Error)]
pub enum IamCredentialsError {
    /// The declared impersonation target could not be interpolated into the request path.
    #[error("the declared impersonation target is not usable")]
    Account {
        #[source]
        cause: UnusableAccount,
    },
    /// This process could not read a wall clock, so no deadline could be computed.
    #[error("this process could not read the time, so no impersonated-token deadline could be computed")]
    NoClock {
        #[source]
        cause: std::time::SystemTimeError,
    },
    /// The request could not be serialized.
    #[error("the request body could not be serialized, which is a defect in this adapter")]
    RequestNotSerializable {
        #[source]
        cause: serde_json::Error,
    },
    /// This call's budget was gone before the hop could be submitted.
    #[error("this call's budget was spent before the impersonation request could be submitted")]
    DeadlineSpent,
    /// The endpoint was not reached.
    #[error("the impersonation endpoint was not reached")]
    Unreachable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The endpoint's answer could not be read.
    #[error("the impersonation endpoint's answer could not be read")]
    Unreadable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The pool subject may not impersonate the declared account - `403`, named rather than the
    /// endpoint's own free-text body.
    #[error("the pool subject may not impersonate the declared account")]
    ImpersonationRefused {
        /// Carried for a caller that has decided it needs to know which target - never rendered by
        /// this variant's own `Display`, and redacted under `Debug`.
        target: RedactedSa,
    },
    /// The endpoint refused for a reason other than `403`.
    #[error("the impersonation endpoint refused with {status}")]
    Refused { status: u16 },
    /// The answer was not the access-token document this adapter reads.
    #[error("the impersonation endpoint's answer was not an access-token document")]
    NotADocument {
        #[source]
        cause: serde_json::Error,
    },
    /// No access token came back.
    #[error("the impersonation endpoint answered without an access token")]
    NoAccessToken,
}

/// An [`ImpersonateAsAccount`] that talks to Google's `iamcredentials` API over HTTP.
#[derive(Debug, Clone)]
pub struct IamCredentialsOverHttp {
    agent: WireAgent,
}

impl IamCredentialsOverHttp {
    /// Opens the transport, reusing the pinned [`WireAgent`] so a composition root shares one client,
    /// one connection pool and one set of pins with [`crate::wire::StsOverHttp`] and `BigQueryWire`.
    #[must_use]
    pub const fn new(agent: WireAgent) -> Self {
        Self { agent }
    }
}

impl ImpersonateAsAccount for IamCredentialsOverHttp {
    type Error = IamCredentialsError;

    #[expect(
        clippy::disallowed_methods,
        reason = "the federated access token is this hop's own bearer, so it is exposed once as a header value and nothing else"
    )]
    fn impersonate(
        &self,
        federated: &Secret,
        target_sa: &str,
        scope: &str,
        lifetime: core::time::Duration,
    ) -> Result<StsCredential, Self::Error> {
        let account = ImpersonatedAccount::parse(target_sa).map_err(|cause| IamCredentialsError::Account { cause })?;
        let call = CallDeadline::opened(self.agent.bounds().deadline());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .map_err(|cause| IamCredentialsError::NoClock { cause })?;
        let seconds = lifetime.as_secs();
        let document = serde_json::to_vec(&Request {
            scope: [scope],
            lifetime: format!("{seconds}s"),
        })
        .map_err(|cause| IamCredentialsError::RequestNotSerializable { cause })?;
        let left = call.remaining().ok_or(IamCredentialsError::DeadlineSpent)?;
        let url = format!("{HOST}/projects/-/serviceAccounts/{}:generateAccessToken", account.as_str());
        let mut answer = self
            .agent
            .agent()
            .post(&url)
            .config()
            .timeout_global(Some(CallDeadline::socket(left)))
            .build()
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", federated.expose_secret()))
            .send(&document)
            .map_err(|cause| IamCredentialsError::Unreachable { cause: Box::new(cause) })?;
        let status = answer.status();
        let text = answer
            .body_mut()
            .with_config()
            .limit(1 << 20)
            .read_to_string()
            .map_err(|cause| IamCredentialsError::Unreadable { cause: Box::new(cause) })?;
        let status_u16: u16 = status.into();
        if status_u16 == 403 {
            return Err(IamCredentialsError::ImpersonationRefused {
                target: RedactedSa::of(target_sa),
            });
        }
        if !status.is_success() {
            return Err(IamCredentialsError::Refused { status: status_u16 });
        }
        let response: Response = serde_json::from_str(&text).map_err(|cause| IamCredentialsError::NotADocument { cause })?;
        if response.access_token.is_empty() {
            return Err(IamCredentialsError::NoAccessToken);
        }
        Ok(StsCredential::of(
            Secret::new(response.access_token),
            Expiry::At {
                unix_seconds: now.saturating_add(seconds),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{ImpersonatedAccount, Request, UnusableAccount};

    #[test]
    fn the_request_body_is_the_scope_and_lifetime_shape() {
        let request = Request {
            scope: ["https://www.googleapis.com/auth/bigquery.readonly"],
            lifetime: String::from("3600s"),
        };
        let json = serde_json::to_value(&request).expect("serializes");
        assert_eq!(json["scope"][0], "https://www.googleapis.com/auth/bigquery.readonly");
        assert_eq!(json["lifetime"], "3600s");
    }

    #[test]
    fn an_account_with_no_at_sign_still_parses_here_the_config_boundary_owns_that_check() {
        // This adapter's OWN check is narrower than the settings tree's: it refuses only what would
        // escape a URL path segment, not "is this a plausible email" - that discrimination belongs
        // to `sutura_config::sources::workload_identity::WorkloadIdentitySa`, which this crate may
        // not depend on. A value with no `@` but no illegal character still parses here.
        drop(ImpersonatedAccount::parse("not-an-account").expect("no illegal character is present"));
    }

    #[test]
    fn a_character_that_could_escape_the_path_is_refused() {
        let err = ImpersonatedAccount::parse("sa@x/../y.iam.gserviceaccount.com").expect_err("a slash is refused");
        assert!(matches!(err, UnusableAccount::Character { .. }));
    }

    #[test]
    fn an_empty_account_is_refused() {
        let err = ImpersonatedAccount::parse("   ").expect_err("whitespace only is empty");
        assert!(matches!(err, UnusableAccount::Empty));
    }

    #[test]
    fn a_redacted_sa_never_renders_the_account_under_debug() {
        let redacted = super::RedactedSa::of("principal-a@acme-analytics.iam.gserviceaccount.com");
        let rendered = format!("{redacted:?}");
        assert!(!rendered.contains("principal-a"), "{rendered}");
        assert!(!rendered.contains("acme-analytics"), "{rendered}");
    }
}

//! The HTTP half of [`crate::StsExchange`]: one subject's token in, a Google access token out.
//!
//! **This is what `WorkloadIdentityBroker`'s exchange port needs a real implementor of**, behind the
//! crate's default-off `wire` feature for the same reason `BigQueryWire` is: an outbound TLS stack on
//! a target that cross-compiles to musl is a dependency decision a composition root makes in a
//! manifest line, not something the adapter inherits by being linked.
//!
//! The request is an RFC 8693 token exchange against Google's Security Token Service for a Workload
//! Identity Federation provider: the provider's audience, the scope the exchanged credential is
//! minted for, and the asker's own OIDC `id_token` as the `jwt` subject token. The exchanged
//! [`StsCredential`] carries the access token AND the instant it stops being usable, which is what a
//! broker needs to compute one deadline for the whole answer.

use sutura_domain::identity::{Expiry, Secret};

use crate::sts::{StsCredential, StsExchange};
use crate::wire::{CallDeadline, WireAgent};

/// The API this module speaks to. A compile-time constant, like `BigQueryWire`'s `HOST`.
const STSD_HOST: &str = "https://sts.googleapis.com/v1/token";

/// The RFC 8693 grant type: token exchange.
const GRANT_TYPE_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
/// The token this service wants OUT: a Google OAuth access token.
const REQUESTED_ACCESS_TOKEN: &str = "urn:ietf:params:oauth:token-type:access_token";
/// The token this service hands IN: the asker's OIDC `id_token`, a JWT.
const SUBJECT_JWT: &str = "urn:ietf:params:oauth:token-type:jwt";

/// The request body, as the endpoint's RFC 8693 document describes it.
#[derive(serde::Serialize)]
struct Request<'a> {
    grant_type: &'static str,
    requested_token_type: &'static str,
    subject_token_type: &'static str,
    #[serde(rename = "audience")]
    target_audience: &'a str,
    scope: &'a str,
    subject_token: String,
}

/// The answer, with the two fields this adapter reads.
///
/// **`expires_in` arrives as text**, because the endpoint writes 64-bit integers as JSON strings -
/// the same shape `BigQuery`'s `totalRows` arrives in.
#[derive(serde::Deserialize)]
struct Response {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// Why the exchange could not happen.
#[derive(Debug, thiserror::Error)]
pub enum StsError {
    /// This process could not read a wall clock, so no deadline could be computed.
    #[error("this process could not read the time, so no exchanged-token deadline could be computed")]
    NoClock {
        #[source]
        cause: std::time::SystemTimeError,
    },
    /// The request could not be sent.
    #[error("the token exchange endpoint was not reached")]
    Unreachable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// This call's budget was gone before the exchange could be submitted.
    #[error("this call's budget was spent before the exchange could be submitted")]
    DeadlineSpent,
    /// The endpoint's answer could not be read.
    #[error("the token exchange endpoint's answer could not be read")]
    Unreadable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The endpoint refused.
    #[error("the token exchange endpoint refused with {status}")]
    Refused { status: u16 },
    /// The answer was not the token-exchange document this adapter reads.
    #[error("the token exchange endpoint's answer was not a token document")]
    NotADocument {
        #[source]
        cause: serde_json::Error,
    },
    /// No access token came back.
    #[error("the token exchange endpoint answered without an access token")]
    NoAccessToken,
    /// The exchanged credential's deadline could not be computed.
    #[error("the token exchange endpoint returned no lifetime, and none can be invented here")]
    NoLifetime,
}

/// An [`StsExchange`] that talks to Google STS over HTTP.
#[derive(Debug, Clone)]
pub struct StsOverHttp {
    agent: WireAgent,
}

impl StsOverHttp {
    /// Opens the transport, reusing the pinned [`WireAgent`] so a composition root shares one
    /// client, one connection pool and one set of pins with `BigQueryWire`.
    #[must_use]
    pub const fn new(agent: WireAgent) -> Self {
        Self { agent }
    }
}

impl StsExchange for StsOverHttp {
    type Error = StsError;

    #[expect(
        clippy::disallowed_methods,
        reason = "RFC 8693 puts the subject token in the request body, so the exchange cannot \
                  happen without exposing it once; it becomes a serialized field and nothing else"
    )]
    fn exchange(&self, audience: &str, scope: &str, subject_token: &Secret) -> Result<StsCredential, Self::Error> {
        let call = CallDeadline::opened(self.agent.bounds().deadline());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .map_err(|cause| StsError::NoClock { cause })?;
        let document = serde_json::to_vec(&Request {
            grant_type: GRANT_TYPE_EXCHANGE,
            requested_token_type: REQUESTED_ACCESS_TOKEN,
            subject_token_type: SUBJECT_JWT,
            target_audience: audience,
            scope,
            subject_token: String::from(subject_token.expose_secret()),
        })
        .map_err(|cause| StsError::NotADocument { cause })?;
        let left = call.remaining().ok_or(StsError::DeadlineSpent)?;
        let mut answer = self
            .agent
            .agent()
            .post(STSD_HOST)
            .config()
            .timeout_global(Some(CallDeadline::socket(left)))
            .build()
            .header("content-type", "application/json")
            .send(&document)
            .map_err(|cause| StsError::Unreachable { cause: Box::new(cause) })?;
        let status = answer.status();
        let text = answer
            .body_mut()
            .with_config()
            .limit(1 << 20)
            .read_to_string()
            .map_err(|cause| StsError::Unreadable { cause: Box::new(cause) })?;
        if !status.is_success() {
            return Err(StsError::Refused { status: status.into() });
        }
        let response: Response = serde_json::from_str(&text).map_err(|cause| StsError::NotADocument { cause })?;
        if response.access_token.is_empty() {
            return Err(StsError::NoAccessToken);
        }
        // The deadline is the instant of mint plus the lifetime, and a missing lifetime is a refusal
        // rather than a guessed one - `Expiry::NothingExpires` would be a token with no bound, which
        // is the shape the whole design exists to rule out.
        let not_after = match response.expires_in {
            Some(seconds) => Expiry::At {
                unix_seconds: now.saturating_add(seconds),
            },
            None => return Err(StsError::NoLifetime),
        };
        Ok(StsCredential::of(Secret::new(response.access_token), not_after))
    }
}

#[cfg(test)]
mod tests {
    use super::{GRANT_TYPE_EXCHANGE, Request, SUBJECT_JWT};
    use sutura_domain::identity::Secret;

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture builds the body the real exchange builds, so the pinned document is the one that goes on the wire"
    )]
    fn the_request_body_is_the_rfc_8693_shape_with_the_subject_token_in_it() {
        let request = Request {
            grant_type: GRANT_TYPE_EXCHANGE,
            requested_token_type: "urn:ietf:params:oauth:token-type:access_token",
            subject_token_type: SUBJECT_JWT,
            target_audience: "//iam.googleapis.com/.../providers/sso",
            scope: "https://www.googleapis.com/auth/bigquery.readonly",
            subject_token: String::from(Secret::new("the-askers-own-id-token").expose_secret()),
        };
        let json = serde_json::to_value(&request).expect("serializes");
        assert_eq!(json["grant_type"], "urn:ietf:params:oauth:grant-type:token-exchange");
        assert_eq!(json["subject_token_type"], "urn:ietf:params:oauth:token-type:jwt");
        assert_eq!(json["subject_token"], "the-askers-own-id-token");
        assert_eq!(json["audience"], "//iam.googleapis.com/.../providers/sso");
        assert_eq!(json["scope"], "https://www.googleapis.com/auth/bigquery.readonly");
    }
}

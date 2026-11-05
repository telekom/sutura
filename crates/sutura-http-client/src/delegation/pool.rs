//! The real [`PoolExchange`]: Google's security token service, asked the way the pinned
//! `cloud.google.com/go/auth` external-account provider asks it when a credential names no
//! impersonation URL - `credentials/internal/stsexchange/sts_exchange.go`'s form, and
//! `credentials/internal/externalaccount/externalaccount.go`'s scope and answer checks.
//!
//! What is checked on the answer: a success status, JSON, an `issued_token_type` of the access
//! token asked for, a `token_type` of `Bearer`, a non-empty `access_token` and a positive integer
//! `expires_in`. **The token is opaque and never decoded**: Google's token service verified the
//! assertion against the pool, and `BigQuery` verifies the token, so the answer is shape-checked
//! and not verified here.
//!
//! **No client authentication and no `options` member**: the provider sends neither without a
//! declared client id and secret or a workforce user project, and a source here declares none.

use sutura_domain::identity::{Delegated, PoolExchange, PoolExchangeFailed, PoolScope, Secret, WorkloadPool};

use super::{ACCESS_TOKEN, TOKEN_EXCHANGE, TokenEndpoint, Unsent};
use crate::ReadBounds;

/// Google's token service, the provider's default for the `googleapis.com` universe.
pub const GOOGLE_STS: &str = "https://sts.googleapis.com/v1/token";

/// What the caller's assertion is: leg 1 verified a JWT.
const JWT: &str = "urn:ietf:params:oauth:token-type:jwt";

/// The `BigQuery` client's own scope. The driver's client prepends it to its options, and a
/// federated token with no impersonation hop is asked for the client's scopes - so this is what the
/// driver asked for before this process performed the exchange. **Both [`PoolScope`]s ask for it
/// today:** the read-only scope lands with the forwarded token, the next change of the `BigQuery`
/// reading worker.
const BIGQUERY_SCOPE: &str = "https://www.googleapis.com/auth/bigquery";

/// [`PoolExchange`] over HTTP, at [`GOOGLE_STS`] or a stand-in for it.
#[derive(Debug)]
pub struct GoogleSts {
    endpoint: TokenEndpoint,
    agent: sutura_tls::Rotating<ureq::Agent>,
    bounds: ReadBounds,
}

impl GoogleSts {
    #[must_use]
    pub const fn new(endpoint: TokenEndpoint, agent: sutura_tls::Rotating<ureq::Agent>, bounds: ReadBounds) -> Self {
        Self { endpoint, agent, bounds }
    }
}

impl PoolExchange for GoogleSts {
    fn exchange(&self, assertion: &Secret, pool: &WorkloadPool, scope: PoolScope) -> Result<Delegated, PoolExchangeFailed> {
        let scope = match scope {
            PoolScope::Query | PoolScope::ReadOnly => BIGQUERY_SCOPE,
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "the assertion is the value this request exists to send; it goes into one form \
                      body over the declared endpoint and nowhere else, and no error built below carries it"
        )]
        let form = [
            ("audience", pool.audience()),
            ("grant_type", TOKEN_EXCHANGE),
            ("requested_token_type", ACCESS_TOKEN),
            ("subject_token_type", JWT),
            ("subject_token", assertion.expose_secret()),
            ("scope", scope),
        ];
        let (status, text) = super::post(&self.agent, &self.endpoint, self.bounds, &form).map_err(|unsent| match unsent {
            Unsent::Unreachable(cause) => PoolExchangeFailed::Unreachable { cause: Box::new(cause) },
            Unsent::TooLarge(cap) => PoolExchangeFailed::TooLarge { cap },
        })?;
        #[expect(
            clippy::disallowed_methods,
            reason = "the answer is read here and nowhere else; no error built from it carries the token"
        )]
        answered(status, text.expose_secret(), super::now())
    }
}

/// Validates the token service's answer at `now`. Pure, so every refusal has a cell without a socket.
fn answered(status: u16, text: &str, now: u64) -> Result<Delegated, PoolExchangeFailed> {
    let body: Option<serde_json::Value> = serde_json::from_str(text).ok();
    let field = |name: &str| body.as_ref()?.get(name);
    let text_of = |name: &str| field(name)?.as_str();
    if !(200..300).contains(&status) {
        return Err(PoolExchangeFailed::Refused {
            status,
            error: super::registered(text_of("error")),
        });
    }
    if body.is_none() {
        return Err(PoolExchangeFailed::Malformed { what: "not JSON" });
    }
    if text_of("issued_token_type") != Some(ACCESS_TOKEN)
        || !text_of("token_type").is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"))
    {
        return Err(PoolExchangeFailed::WrongTokenType);
    }
    let token = text_of("access_token")
        .filter(|token| !token.is_empty())
        .ok_or(PoolExchangeFailed::Malformed { what: "no access_token" })?;
    let lifetime = field("expires_in")
        .and_then(serde_json::Value::as_u64)
        .filter(|&seconds| seconds > 0)
        .ok_or(PoolExchangeFailed::Malformed {
            what: "no positive integer expires_in",
        })?;
    Ok(Delegated::new(Secret::new(token), now.saturating_add(lifetime)))
}

#[cfg(test)]
mod tests;

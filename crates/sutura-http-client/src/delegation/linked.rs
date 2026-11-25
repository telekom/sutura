//! The real [`LinkedLogin`]: Keycloak's Identity Brokering API v2, whose answer is the token the
//! brokered provider issued when the caller signed in there.
//!
//! **The token is opaque**, so nothing in it is checked: the data system verifies it. What is
//! checked is the answer - a success status, JSON, an `access_token`, a `token_type` of `Bearer`
//! when one is present, and an `accessTokenExpiration` after now. That field is the identity
//! provider's absolute expiry; `expires_in` is not read, because the answer is the response stored
//! at sign-in and counts from then.

use sutura_domain::identity::{Delegated, DelegationFailed, LinkedLogin, Secret};

use super::{ExchangeClient, OverHttp, TokenEndpoint};
use crate::ReadBounds;
use crate::token::Scopes;

/// [`LinkedLogin`] over HTTP, at a `/realms/<realm>/broker/<alias>/token` endpoint.
#[derive(Debug)]
pub struct BrokeredLogin(OverHttp);

impl BrokeredLogin {
    #[must_use]
    pub const fn new(
        endpoint: TokenEndpoint,
        client: ExchangeClient,
        agent: sutura_tls::Rotating<ureq::Agent>,
        bounds: ReadBounds,
    ) -> Self {
        Self(OverHttp::new(endpoint, client, agent, bounds))
    }

    /// The stored answer for `subject`, its expiry not yet held to anything: what an operator's
    /// check reports. [`LinkedLogin::retrieve`] is this, then [`StoredAnswer::delegated_at`].
    ///
    /// # Errors
    ///
    /// [`DelegationFailed`] for an answer `retrieve` refuses for any reason but its expiry.
    #[expect(
        clippy::disallowed_methods,
        reason = "the subject token and the client secret go into one form body over the declared \
                  endpoint, and the answer is read here once; no error built below carries any of them"
    )]
    pub fn stored(&self, subject: &Secret) -> Result<StoredAnswer, DelegationFailed> {
        let client = &self.0.client;
        let (status, text) = self.0.send(&[
            ("token", subject.expose_secret()),
            ("client_id", client.id.as_str()),
            ("client_secret", client.secret.expose_secret()),
        ])?;
        stored_answer(status, text.expose_secret())
    }
}

impl LinkedLogin for BrokeredLogin {
    fn retrieve(&self, subject: &Secret) -> Result<Delegated, DelegationFailed> {
        self.stored(subject)?.delegated_at(super::now())
    }
}

/// A broker answer that passed every check but its expiry.
#[derive(Debug)]
pub struct StoredAnswer {
    token: Secret,
    not_after: Option<u64>,
    scope: Option<Scopes>,
}

impl StoredAnswer {
    /// The answer's `accessTokenExpiration`, in Unix seconds, when it carries one.
    #[must_use]
    pub const fn not_after_unix_seconds(&self) -> Option<u64> {
        self.not_after
    }

    /// The answer's `scope`, when it carries one made of scope tokens.
    #[must_use]
    pub const fn scope(&self) -> Option<&Scopes> {
        self.scope.as_ref()
    }

    /// The token, held to an `accessTokenExpiration` after `now`.
    ///
    /// # Errors
    ///
    /// [`DelegationFailed::Malformed`] without one, [`DelegationFailed::AlreadyExpired`] at or
    /// before `now`.
    pub fn delegated_at(self, now: u64) -> Result<Delegated, DelegationFailed> {
        let not_after = self.not_after.ok_or(DelegationFailed::Malformed {
            what: "no accessTokenExpiration",
        })?;
        if not_after <= now {
            return Err(DelegationFailed::AlreadyExpired);
        }
        Ok(Delegated::new(self.token, not_after))
    }
}

/// The stored token response, with a field for nothing but what is checked or reported: its
/// refresh token and ID token bind to nothing.
#[derive(serde::Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct StoredToken {
    access_token: Option<String>,
    token_type: Option<String>,
    #[serde(rename = "accessTokenExpiration")]
    access_token_expiration: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

/// Validates the broker's answer at `now` as [`LinkedLogin::retrieve`] does. Pure, so every refusal
/// has a cell without a socket.
#[cfg(test)]
fn opaque_answered(status: u16, text: &str, now: u64) -> Result<Delegated, DelegationFailed> {
    stored_answer(status, text)?.delegated_at(now)
}

/// Validates the broker's answer, all but its expiry.
fn stored_answer(status: u16, text: &str) -> Result<StoredAnswer, DelegationFailed> {
    if !(200..300).contains(&status) {
        return Err(super::refusal(status, serde_json::from_str(text).ok().as_ref()));
    }
    let stored: StoredToken = serde_json::from_str(text).map_err(|_cause: serde_json::Error| DelegationFailed::Malformed {
        what: "not a token response",
    })?;
    if stored.token_type.is_some_and(|kind| !kind.eq_ignore_ascii_case("bearer")) {
        return Err(DelegationFailed::WrongTokenType);
    }
    let token = stored
        .access_token
        .filter(|token| !token.is_empty())
        .ok_or(DelegationFailed::Malformed { what: "no access_token" })?;
    Ok(StoredAnswer {
        token: Secret::new(token),
        not_after: stored.access_token_expiration,
        scope: stored
            .scope
            .and_then(|scope| Scopes::parse(scope.split_whitespace()).ok())
            .filter(|scopes| !scopes.as_str().is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::DelegationFailed;

    use super::{StoredToken, opaque_answered};

    const NOW: u64 = 4_000_000_000;

    fn stored(fields: &str) -> String {
        format!(r#"{{"access_token":"ya29.opaque-google-token","token_type":"Bearer",{fields}}}"#)
    }

    #[test]
    fn an_opaque_broker_answer_without_access_token_expiration_is_refused() {
        // `expires_in` alone counts from the caller's sign-in, not from now, so it is no expiry.
        let refused = opaque_answered(200, &stored(r#""expires_in":3599"#), NOW).expect_err("no absolute expiry");
        assert!(
            matches!(
                refused,
                DelegationFailed::Malformed {
                    what: "no accessTokenExpiration"
                }
            ),
            "{refused:?}"
        );
        let answered = opaque_answered(
            200,
            &stored(&format!(r#""expires_in":1,"accessTokenExpiration":{}"#, NOW + 60)),
            NOW,
        )
        .expect("an absolute expiry after now is usable");
        assert_eq!(answered.not_after_unix_seconds(), NOW + 60);
    }

    #[test]
    fn an_expired_opaque_broker_answer_is_refused() {
        for at in [NOW, NOW - 1] {
            let refused = opaque_answered(200, &stored(&format!(r#""accessTokenExpiration":{at}"#)), NOW)
                .expect_err("an expiry at or before now is refused");
            assert!(matches!(refused, DelegationFailed::AlreadyExpired), "{refused:?}");
        }
    }

    #[test]
    fn a_broker_answer_of_another_token_type_is_refused() {
        let answer = format!(
            r#"{{"access_token":"ya29.opaque-google-token","token_type":"DPoP","accessTokenExpiration":{}}}"#,
            NOW + 60
        );
        let refused = opaque_answered(200, &answer, NOW).expect_err("only a bearer token is presented");
        assert!(matches!(refused, DelegationFailed::WrongTokenType), "{refused:?}");
    }

    #[test]
    fn a_broker_answer_with_an_empty_access_token_is_refused() {
        let answer = format!(r#"{{"access_token":"","accessTokenExpiration":{}}}"#, NOW + 60);
        let refused = opaque_answered(200, &answer, NOW).expect_err("an empty token is no token");
        assert!(
            matches!(refused, DelegationFailed::Malformed { what: "no access_token" }),
            "{refused:?}"
        );
    }

    #[test]
    fn a_refresh_token_in_the_broker_answer_reaches_no_field() {
        let answer = stored(&format!(
            r#""accessTokenExpiration":{},"refresh_token":"1//refresh-never-kept","id_token":"id.never.kept""#,
            NOW + 60
        ));
        // What the answer decodes into, written back out: a field for either token would show here.
        let decoded: StoredToken = serde_json::from_str(&answer).expect("a stored response decodes");
        let kept = serde_json::to_string(&decoded).expect("the decoded value serialises");
        assert!(!kept.contains("never-kept") && !kept.contains("never.kept"), "{kept}");
        let answered = opaque_answered(200, &answer, NOW).expect("a stored response is usable");
        #[expect(clippy::disallowed_methods, reason = "the cell reads the one token the answer keeps")]
        let token = answered.into_token().expose_secret().to_owned();
        assert_eq!(token, "ya29.opaque-google-token");
    }

    #[test]
    fn a_stored_answer_reports_its_expiry_and_scope_before_either_is_held_to_anything() {
        let without = super::stored_answer(200, &stored(r#""scope":"api:sql-queries-execute api:ontologies-read""#))
            .expect("an answer without an expiry is still a stored answer");
        assert_eq!(without.not_after_unix_seconds(), None);
        assert_eq!(
            without.scope().map(crate::token::Scopes::as_str),
            Some("api:sql-queries-execute api:ontologies-read")
        );
        let with = super::stored_answer(
            200,
            &stored(&format!(r#""accessTokenExpiration":{},"scope":"bad\"scope""#, NOW + 60)),
        )
        .expect("a stored answer");
        assert_eq!(with.not_after_unix_seconds(), Some(NOW + 60));
        assert!(with.scope().is_none(), "a scope that is not scope tokens is not reported");
    }

    #[test]
    fn a_caller_who_linked_no_account_is_told_so_and_no_other_refusal_is() {
        let not_linked = r#"{"error":"invalid_request","error_description":"User not associated to identity provider"}"#;
        assert!(matches!(
            opaque_answered(400, not_linked, NOW),
            Err(DelegationFailed::NotLinked)
        ));
        for other in [
            r#"{"error":"invalid_request","error_description":"Invalid identity provider"}"#,
            r#"{"error":"invalid_request"}"#,
        ] {
            let refused = opaque_answered(400, other, NOW).expect_err("a refusal");
            assert!(
                matches!(refused, DelegationFailed::Refused { status: 400, error: Some(ref code) } if code == "invalid_request"),
                "{refused:?}"
            );
        }
    }
}

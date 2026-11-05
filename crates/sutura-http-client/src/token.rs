//! The bearer a catalog reader presents: a fixed token, or one minted by OAuth 2.0 client
//! credentials (RFC 6749 section 4.4) and re-minted before it expires.
//!
//! **Either way the reader acts as the deployment.** The token is this process's own, not a
//! caller's, so every caller sees what it sees.
//!
//! What a minted answer is held to: a success status, JSON, `token_type` `Bearer`, and an
//! `access_token` that is a bearer value (printable ASCII, no space, at most
//! [`MAX_TOKEN_BYTES`]). The token is opaque here - nothing decodes it - so its lifetime is the
//! answer's `expires_in`. An answer without one is used for one read and minted again on the next.

use std::time::{Duration, Instant};

use sutura_domain::identity::Secret;

use crate::delegation::{ExchangeClient, TokenEndpoint, Unsent};
use crate::{Budget, Endpoint, RotatingAgent};

/// The longest token, in bytes, either variant accepts.
pub const MAX_TOKEN_BYTES: usize = 16 * 1024;

/// The cap on a token endpoint's answer: a token, its type, its lifetime and its scope.
pub const MAX_TOKEN_RESPONSE_BYTES: u64 = 64 * 1024;

/// How long before its `expires_in` a minted token is replaced, so a read that starts on a token
/// never ends on a dead one.
pub const REFRESH_MARGIN: Duration = Duration::from_secs(60);

const CLIENT_CREDENTIALS: &str = "client_credentials";

/// Why a token is not a bearer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnusableToken {
    /// Empty, over [`MAX_TOKEN_BYTES`], or carrying a space or a byte that is not printable ASCII -
    /// which is what could end the header it is written into.
    #[error("the bearer token is empty, over {MAX_TOKEN_BYTES} bytes, or not printable ASCII without spaces")]
    NotABearerValue,
}

/// Why a declared scope list is unusable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidScope {
    /// Empty, or carrying a space, a `"` or a `\`, or a byte outside printable ASCII
    /// (RFC 6749 section 3.3's `scope-token`).
    #[error("the scope {given:?} is not an OAuth 2.0 scope token")]
    NotAScopeToken { given: String },
}

/// The scopes a client-credentials grant asks for, space-joined. Empty asks for none, and the
/// identity provider's default applies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scopes(String);

impl Scopes {
    /// # Errors
    ///
    /// [`InvalidScope`], naming the first entry that is not a scope token.
    pub fn parse<'a>(scopes: impl IntoIterator<Item = &'a str>) -> Result<Self, InvalidScope> {
        let mut joined = String::new();
        for scope in scopes {
            let token = !scope.is_empty()
                && scope
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\');
            if !token {
                return Err(InvalidScope::NotAScopeToken {
                    given: String::from(scope),
                });
            }
            if !joined.is_empty() {
                joined.push(' ');
            }
            joined.push_str(scope);
        }
        Ok(Self(joined))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A token endpoint that is not on the origin the reader was declared over.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the token endpoint {token_endpoint} is not on the declared endpoint {origin}")]
pub struct OffOrigin {
    token_endpoint: String,
    origin: String,
}

/// Why no bearer could be presented.
#[derive(Debug, thiserror::Error)]
pub enum TokenUnavailable {
    /// The read's own deadline was spent before a token was asked for.
    #[error("the read's deadline was spent before a token was minted")]
    DeadlineSpent,
    #[error("the token endpoint was not reached")]
    Unreachable(#[source] Box<ureq::Error>),
    #[error("the token endpoint's answer is over the {cap}-byte cap")]
    TooLarge { cap: u64 },
    /// A non-success status, with the RFC 6749 `error` code only when it has the registered shape.
    #[error("the token endpoint refused the client ({status}, {})", error.as_deref().unwrap_or("no error code"))]
    Refused { status: u16, error: Option<String> },
    #[error("the token endpoint's answer is not usable: {what}")]
    Malformed { what: &'static str },
    #[error("the token endpoint answered a token type other than Bearer")]
    WrongTokenType,
}

/// What one request presents: `Authorization: Bearer <token>`.
#[derive(Debug, Clone)]
pub struct Bearer(Secret);

impl Bearer {
    /// The header value, built per request so the token lives as text only as long as the request.
    #[must_use]
    #[expect(
        clippy::disallowed_methods,
        reason = "a bearer has to reach the wire as text; this is the one header value a reader sends"
    )]
    pub fn header(&self) -> String {
        format!("Bearer {}", self.0.expose_secret())
    }
}

/// Where a reader's bearer comes from.
#[derive(Debug)]
pub enum TokenSource {
    /// One token, read at boot and presented on every request until the process restarts.
    Fixed(Bearer),
    /// Minted at the declared token endpoint, and minted again once it nears its expiry.
    ClientCredentials(ClientCredentials),
}

impl TokenSource {
    /// # Errors
    ///
    /// [`UnusableToken`] for a token no header can carry.
    pub fn fixed(token: Secret) -> Result<Self, UnusableToken> {
        #[expect(clippy::disallowed_methods, reason = "only the shape is read")]
        let usable = bearer_value(token.expose_secret());
        if !usable {
            return Err(UnusableToken::NotABearerValue);
        }
        Ok(Self::Fixed(Bearer(token)))
    }

    /// The bearer for one request, minted inside what is left of `budget` when one is due.
    ///
    /// # Errors
    ///
    /// [`TokenUnavailable`]; never for [`Self::Fixed`].
    pub fn bearer(&self, budget: Budget) -> Result<Bearer, TokenUnavailable> {
        match self {
            Self::Fixed(bearer) => Ok(bearer.clone()),
            Self::ClientCredentials(grant) => grant.bearer(budget),
        }
    }
}

/// A token still worth presenting, and until when.
#[derive(Debug)]
struct Minted {
    bearer: Bearer,
    fresh_until: Instant,
}

/// The client-credentials grant: this deployment's client, posted to a token endpoint on the
/// reader's own origin.
#[derive(Debug)]
pub struct ClientCredentials {
    endpoint: TokenEndpoint,
    client: ExchangeClient,
    scopes: Scopes,
    agent: RotatingAgent,
    minted: parking_lot::Mutex<Option<Minted>>,
}

impl ClientCredentials {
    /// A grant at `endpoint`, which must be on `origin` - the endpoint the reader reads - so the
    /// client secret goes to no host the reader would not already send the token to.
    ///
    /// # Errors
    ///
    /// [`OffOrigin`] for a token endpoint on another scheme, host or port.
    pub fn new(
        origin: &Endpoint,
        endpoint: TokenEndpoint,
        client: ExchangeClient,
        scopes: Scopes,
        agent: RotatingAgent,
    ) -> Result<Self, OffOrigin> {
        if !endpoint.is_on(origin) {
            return Err(OffOrigin {
                token_endpoint: String::from(endpoint.as_str()),
                origin: String::from(origin.as_str()),
            });
        }
        Ok(Self {
            endpoint,
            client,
            scopes,
            agent,
            minted: parking_lot::Mutex::new(None),
        })
    }

    /// The held token while it is fresh, else a new one. The lock is held across the mint, so
    /// concurrent reads wait for one answer rather than each asking.
    fn bearer(&self, budget: Budget) -> Result<Bearer, TokenUnavailable> {
        let mut held = self.minted.lock();
        if let Some(minted) = held.as_ref().filter(|minted| Instant::now() < minted.fresh_until) {
            return Ok(minted.bearer.clone());
        }
        let granted = budget
            .remaining()
            .ok_or(TokenUnavailable::DeadlineSpent)
            .and_then(|left| self.mint(left));
        *held = granted.as_ref().ok().and_then(Granted::held);
        drop(held);
        granted.map(|granted| granted.bearer)
    }

    fn mint(&self, left: Duration) -> Result<Granted, TokenUnavailable> {
        let mut form = vec![("grant_type", CLIENT_CREDENTIALS), ("client_id", self.client.id())];
        #[expect(
            clippy::disallowed_methods,
            reason = "the client secret is the value this request exists to send; it goes into one form \
                      body to the declared endpoint and into no error"
        )]
        form.push(("client_secret", self.client.secret().expose_secret()));
        if !self.scopes.as_str().is_empty() {
            form.push(("scope", self.scopes.as_str()));
        }
        let (status, text) = crate::delegation::post_capped(&self.agent, &self.endpoint, left, MAX_TOKEN_RESPONSE_BYTES, &form)
            .map_err(|unsent| match unsent {
            Unsent::Unreachable(cause) => TokenUnavailable::Unreachable(Box::new(cause)),
            Unsent::TooLarge(cap) => TokenUnavailable::TooLarge { cap },
        })?;
        #[expect(
            clippy::disallowed_methods,
            reason = "the answer is read here and nowhere else; no error built from it carries the token"
        )]
        answered(status, text.expose_secret())
    }
}

/// A minted token and the lifetime its answer declared.
#[derive(Debug)]
struct Granted {
    bearer: Bearer,
    lifetime: Option<Duration>,
}

impl Granted {
    /// What to hold for the next read: nothing for a token without a lifetime, or one inside
    /// [`REFRESH_MARGIN`] of it.
    fn held(&self) -> Option<Minted> {
        let fresh_for = self.lifetime?.checked_sub(REFRESH_MARGIN)?;
        Some(Minted {
            bearer: self.bearer.clone(),
            fresh_until: Instant::now().checked_add(fresh_for)?,
        })
    }
}

/// Validates a token endpoint's answer. Pure, so every refusal has a cell without a socket.
fn answered(status: u16, text: &str) -> Result<Granted, TokenUnavailable> {
    let body: Option<serde_json::Value> = serde_json::from_str(text).ok();
    let field = |name: &str| body.as_ref()?.get(name);
    let text_of = |name: &str| field(name)?.as_str();
    if !(200..300).contains(&status) {
        return Err(TokenUnavailable::Refused {
            status,
            error: crate::delegation::registered(text_of("error")),
        });
    }
    if body.is_none() {
        return Err(TokenUnavailable::Malformed { what: "not JSON" });
    }
    if !text_of("token_type").is_some_and(|kind| kind.eq_ignore_ascii_case("bearer")) {
        return Err(TokenUnavailable::WrongTokenType);
    }
    let token = text_of("access_token").ok_or(TokenUnavailable::Malformed { what: "no access_token" })?;
    if !bearer_value(token) {
        return Err(TokenUnavailable::Malformed {
            what: "the access_token is not a bearer value",
        });
    }
    let lifetime = field("expires_in")
        .and_then(serde_json::Value::as_u64)
        .map(Duration::from_secs);
    Ok(Granted {
        bearer: Bearer(Secret::new(token)),
        lifetime,
    })
}

fn bearer_value(token: &str) -> bool {
    !token.is_empty() && token.len() <= MAX_TOKEN_BYTES && token.bytes().all(|byte| byte.is_ascii_graphic())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sutura_domain::identity::Secret;

    use super::{ClientCredentials, InvalidScope, MAX_TOKEN_RESPONSE_BYTES, Scopes, TokenSource, TokenUnavailable, answered};
    use crate::delegation::{ExchangeClient, TokenEndpoint};
    use crate::test_support::{FakeServer, Scripted};
    use crate::{Budget, Endpoint, ReadBounds};

    const CLIENT_SECRET: &str = "client-secret-never-logged";
    const TOKEN_PATH: &str = "/multipass/api/oauth2/token";

    fn budget() -> Budget {
        Budget::opened(Duration::from_secs(5))
    }

    fn granted(token: &str, expires_in: u64) -> Scripted {
        Scripted::ok(&serde_json::json!({
            "access_token": token,
            "token_type": "bearer",
            "expires_in": expires_in,
            "scope": "api:ontologies-read",
        }))
    }

    fn grant_at(server: &FakeServer) -> TokenSource {
        let bounds = ReadBounds::parse(5, 1024).expect("test bounds are nonzero");
        let origin = Endpoint::parse(&server.endpoint()).expect("a loopback origin parses");
        let endpoint = TokenEndpoint::parse(&format!("{}{TOKEN_PATH}", server.endpoint())).expect("a loopback endpoint parses");
        let client = ExchangeClient::new("catalog-reader", Secret::new(CLIENT_SECRET)).expect("a client id parses");
        let scopes = Scopes::parse(["api:ontologies-read", "api:admin-read"]).expect("two scope tokens");
        TokenSource::ClientCredentials(
            ClientCredentials::new(&origin, endpoint, client, scopes, crate::fixed(bounds, None)).expect("one origin"),
        )
    }

    #[test]
    fn a_fixed_token_is_presented_as_a_bearer() {
        let source = TokenSource::fixed(Secret::new("deployment-token")).expect("a bearer value");
        let bearer = source.bearer(budget()).expect("a fixed token is always available");
        assert_eq!(bearer.header(), "Bearer deployment-token");
    }

    #[test]
    fn a_token_that_could_end_its_header_is_refused() {
        for token in [
            "",
            "two words",
            "split\r\nx-injected: yes",
            &"a".repeat(super::MAX_TOKEN_BYTES + 1),
        ] {
            assert!(TokenSource::fixed(Secret::new(token)).is_err(), "{token:?} was accepted");
        }
    }

    #[test]
    fn the_client_and_its_scopes_are_posted_to_the_token_endpoint() {
        let server = FakeServer::start(vec![granted("minted-token", 3600)]);
        let source = grant_at(&server);
        let bearer = source.bearer(budget()).expect("the grant answers");
        let requests = server.finish();
        assert_eq!(bearer.header(), "Bearer minted-token");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request_line(), format!("POST {TOKEN_PATH} HTTP/1.1"));
        let body = requests[0].body();
        for field in [
            "grant_type=client_credentials",
            "client_id=catalog-reader",
            "client_secret=client-secret-never-logged",
            "scope=api%3Aontologies-read+api%3Aadmin-read",
        ] {
            assert!(body.split('&').any(|pair| pair == field), "{field} missing from {body}");
        }
    }

    #[test]
    fn a_fresh_token_is_reused_and_one_near_expiry_is_minted_again() {
        let server = FakeServer::start(vec![granted("long-lived", 3600)]);
        let source = grant_at(&server);
        let first = source.bearer(budget()).expect("the grant answers");
        let second = source.bearer(budget()).expect("the held token is fresh");
        assert_eq!(server.finish().len(), 1, "a fresh token was minted again");
        assert_eq!(first.header(), second.header());

        let server = FakeServer::start(vec![granted("short-lived", 30), granted("next", 30)]);
        let source = grant_at(&server);
        let first = source.bearer(budget()).expect("the grant answers");
        let second = source.bearer(budget()).expect("the grant answers again");
        assert_eq!(server.finish().len(), 2, "a token inside the refresh margin was reused");
        assert_eq!(
            (first.header().as_str(), second.header().as_str()),
            ("Bearer short-lived", "Bearer next")
        );
    }

    #[test]
    fn a_spent_deadline_mints_nothing() {
        let server = FakeServer::start(Vec::new());
        let source = grant_at(&server);
        let spent = source.bearer(Budget::opened(Duration::ZERO));
        assert!(matches!(spent, Err(TokenUnavailable::DeadlineSpent)), "{spent:?}");
        assert_eq!(server.finish().len(), 0, "a spent deadline dialled the token endpoint");
    }

    #[test]
    fn a_refused_client_keeps_the_registered_code_and_never_the_secret() {
        let body = format!(r#"{{"error":"invalid_client","error_description":"bad secret {CLIENT_SECRET}"}}"#);
        let server = FakeServer::start(vec![Scripted::status(401, &body)]);
        let source = grant_at(&server);
        let refused = source.bearer(budget()).expect_err("a 401 is a refusal");
        drop(server.finish());
        assert!(matches!(&refused, TokenUnavailable::Refused { status: 401, error: Some(code) } if code == "invalid_client"));
        for rendered in [refused.to_string(), format!("{refused:?}"), format!("{source:?}")] {
            assert!(!rendered.contains(CLIENT_SECRET), "the secret reached {rendered}");
        }
    }

    #[test]
    fn an_answer_over_the_cap_is_refused() {
        let oversized = vec![b' '; usize::try_from(MAX_TOKEN_RESPONSE_BYTES).expect("fits") + 1];
        let server = FakeServer::start(vec![Scripted::raw(200, oversized)]);
        let refused = grant_at(&server).bearer(budget());
        drop(server.finish());
        assert!(matches!(refused, Err(TokenUnavailable::TooLarge { .. })), "{refused:?}");
    }

    #[test]
    fn an_answer_that_is_no_bearer_token_is_refused() {
        let refused = |body: &str| answered(200, body).map(|granted| granted.bearer.header());
        assert!(matches!(
            refused("not json"),
            Err(TokenUnavailable::Malformed { what: "not JSON" })
        ));
        assert!(matches!(
            refused(r#"{"access_token":"t","token_type":"mac"}"#),
            Err(TokenUnavailable::WrongTokenType)
        ));
        assert!(matches!(
            refused(r#"{"token_type":"Bearer"}"#),
            Err(TokenUnavailable::Malformed { what: "no access_token" })
        ));
        assert!(matches!(
            refused(r#"{"access_token":"a b","token_type":"Bearer"}"#),
            Err(TokenUnavailable::Malformed { .. })
        ));
        let granted = answered(200, r#"{"access_token":"t","token_type":"Bearer"}"#).expect("no lifetime is usable");
        assert_eq!(granted.lifetime, None);
    }

    #[test]
    fn a_token_endpoint_off_the_declared_origin_is_refused() {
        let bounds = ReadBounds::parse(5, 1024).expect("test bounds are nonzero");
        let origin = Endpoint::parse("https://ontology.example.com").expect("an origin");
        for elsewhere in [
            "https://identity.example.com/oauth2/token",
            "https://ontology.example.com:8443/oauth2/token",
            "http://127.0.0.1/oauth2/token",
            "https://ontology.example.com.example.net/oauth2/token",
        ] {
            let endpoint = TokenEndpoint::parse(elsewhere).expect("a token endpoint");
            let client = ExchangeClient::new("catalog-reader", Secret::new(CLIENT_SECRET)).expect("a client id");
            let grant = ClientCredentials::new(&origin, endpoint, client, Scopes::default(), crate::fixed(bounds, None));
            assert!(grant.is_err(), "{elsewhere} was accepted for {}", origin.as_str());
        }
    }

    #[test]
    fn a_scope_that_is_no_scope_token_is_refused() {
        for scope in ["", "two words", "quo\"te", "back\\slash"] {
            assert_eq!(
                Scopes::parse([scope]),
                Err(InvalidScope::NotAScopeToken {
                    given: String::from(scope)
                })
            );
        }
        assert_eq!(Scopes::parse([]).expect("no scope is a scope list").as_str(), "");
    }
}

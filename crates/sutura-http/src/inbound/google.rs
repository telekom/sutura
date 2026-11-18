//! Opaque-token verification. Cache entries hold checked identity and a digest, never a token.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sutura_config::inbound::google::GoogleTokenInfo;
use sutura_domain::identity::{
    AdmittingVerifier, AssertionDigest, ClientClaim, ClientClaims, PrincipalChain, Secret, Subject, VerifiedAddress, VerifierId,
};
use sutura_http_client::ReadBounds;

use super::{Groups, Scopes, TokenRejected, VerifiedCaller};

const MAX_RESPONSE_BYTES: u64 = 16 * 1024;
const TIMEOUT_SECONDS: u64 = 10;
const CACHE_CAPACITY: usize = 2048;

#[derive(Debug, thiserror::Error)]
pub enum GoogleClientUnusable {
    #[error("Google tokeninfo read bounds are not usable")]
    Bounds(#[source] sutura_http_client::InvalidReadBounds),
    #[error("Google tokeninfo trust material is not usable")]
    Trust(#[source] sutura_tls::LoadError),
}

/// No source carries a response body or request: either can contain personal data or a token.
#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum TokenInfoFailed {
    #[error("Google refused the access token")]
    InvalidToken,
    #[error("Google tokeninfo did not produce a usable response")]
    Unavailable,
}

trait TokenInfoSource: Send + Sync {
    fn read(&self, token: &Secret) -> Result<RawTokenInfo, TokenInfoFailed>;
}

/// Google's response is an external extensible shape. Only these fields influence admission.
#[derive(serde::Deserialize)]
struct RawTokenInfo {
    aud: String,
    azp: String,
    sub: String,
    scope: String,
    expires_in: WireSeconds,
    email: String,
    email_verified: WireBool,
    #[serde(default)]
    hd: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum WireSeconds {
    Number(u64),
    Text(String),
}
impl WireSeconds {
    fn seconds(&self) -> Option<u64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(value) => value.parse().ok(),
        }
    }
}
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum WireBool {
    Bool(bool),
    Text(String),
}
impl WireBool {
    fn is_true(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::Text(value) => value == "true",
        }
    }
}

struct RemoteTokenInfo {
    endpoint: sutura_config::google_endpoints::GoogleApiEndpoint,
    agent: sutura_tls::Rotating<ureq::Agent>,
}
impl TokenInfoSource for RemoteTokenInfo {
    #[expect(
        clippy::disallowed_methods,
        reason = "the verified endpoint receives the access token in a POST body, never a URL or error"
    )]
    fn read(&self, token: &Secret) -> Result<RawTokenInfo, TokenInfoFailed> {
        let mut response = self
            .agent
            .current()
            .post(self.endpoint.as_str())
            .config()
            .timeout_global(Some(Duration::from_secs(TIMEOUT_SECONDS)))
            .build()
            .send_form([("access_token", token.expose_secret())])
            .map_err(|_cause| TokenInfoFailed::Unavailable)?;
        match response.status().as_u16() {
            200 => {}
            400 | 401 | 403 => return Err(TokenInfoFailed::InvalidToken),
            _ => return Err(TokenInfoFailed::Unavailable),
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(|_cause| TokenInfoFailed::Unavailable)?;
        serde_json::from_slice(&bytes).map_err(|_cause| TokenInfoFailed::Unavailable)
    }
}

#[derive(Clone)]
struct CachedIdentity {
    subject: Subject,
    address: VerifiedAddress,
    scopes: Scopes,
    expires: u64,
    until: Instant,
}

pub(crate) struct GoogleVerifier {
    policy: GoogleTokenInfo,
    verifier: AdmittingVerifier,
    source: Arc<dyn TokenInfoSource>,
    positive: Mutex<HashMap<AssertionDigest, CachedIdentity>>,
}
impl GoogleVerifier {
    pub(crate) fn trusting(
        id: VerifierId,
        policy: &GoogleTokenInfo,
        outbound: Option<&sutura_tls::Declared>,
    ) -> Result<Self, GoogleClientUnusable> {
        let bounds = ReadBounds::parse(TIMEOUT_SECONDS, MAX_RESPONSE_BYTES).map_err(GoogleClientUnusable::Bounds)?;
        // Introspection trust is fixed at startup, including any declared client identity.
        let (agent, rotator) =
            sutura_http_client::rotating_agent(bounds, outbound.cloned()).map_err(GoogleClientUnusable::Trust)?;
        drop(rotator);
        Ok(Self::over(
            id,
            policy.clone(),
            Arc::new(RemoteTokenInfo {
                endpoint: policy.endpoint().clone(),
                agent,
            }),
        ))
    }
    fn over(id: VerifierId, policy: GoogleTokenInfo, source: Arc<dyn TokenInfoSource>) -> Self {
        Self {
            policy,
            verifier: AdmittingVerifier::google_tokeninfo(id),
            source,
            positive: Mutex::new(HashMap::new()),
        }
    }
    pub(crate) const fn max_token_bytes(&self) -> usize {
        self.policy.max_token_bytes().bytes()
    }
    pub(crate) async fn verify(&self, presented: &str, now: Instant) -> Result<VerifiedCaller, TokenRejected> {
        let secret = Secret::new(presented);
        let digest = AssertionDigest::of(&secret);
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_cause| TokenRejected::GoogleUnavailable)?
            .as_secs();
        let cached = {
            let mut positive = self.positive.lock();
            positive.retain(|_, value| value.until > now && value.expires > wall);
            positive.get(&digest).cloned()
        };
        let checked = if let Some(cached) = cached {
            cached
        } else {
            // Only this blocking request owns a temporary token clone; neither source nor cache retains it.
            let source = Arc::clone(&self.source);
            let sent = secret.clone();
            let started = Instant::now();
            let response = sutura_runtime::spawn_carrying_span(move || source.read(&sent))
                .await
                .map_err(|_cause| TokenRejected::GoogleUnavailable)?
                .map_err(|cause| match cause {
                    TokenInfoFailed::InvalidToken => TokenRejected::GoogleRejected,
                    TokenInfoFailed::Unavailable => TokenRejected::GoogleUnavailable,
                })?;
            let checked = self.check(&response, wall, now, started.elapsed())?;
            let mut positive = self.positive.lock();
            if positive.len() >= CACHE_CAPACITY
                && let Some(key) = positive.keys().next().cloned()
            {
                drop(positive.remove(&key));
            }
            drop(positive.insert(digest, checked.clone()));
            drop(positive);
            checked
        };
        let client = ClientClaims::of(Some(ClientClaim::read(Some(self.policy.client().as_str()))), None);
        Ok(VerifiedCaller::established(
            PrincipalChain::of(checked.subject),
            checked.scopes,
            Groups::none(),
            secret,
            checked.expires,
            client,
            self.verifier.clone(),
        )
        .addressed(Some(checked.address)))
    }
    fn check(
        &self,
        response: &RawTokenInfo,
        wall: u64,
        now: Instant,
        elapsed: Duration,
    ) -> Result<CachedIdentity, TokenRejected> {
        if response.aud != self.policy.client().as_str()
            || response.azp != self.policy.client().as_str()
            || !response.email_verified.is_true()
        {
            return Err(TokenRejected::GoogleRejected);
        }
        let seconds = response
            .expires_in
            .seconds()
            .filter(|seconds| *seconds > 0)
            .ok_or(TokenRejected::GoogleRejected)?;
        let lifetime = Duration::from_secs(seconds)
            .checked_sub(elapsed)
            .filter(|duration| !duration.is_zero())
            .ok_or(TokenRejected::GoogleRejected)?;
        let expires = wall
            .checked_add(lifetime.as_secs())
            .filter(|expiry| *expiry > wall)
            .ok_or(TokenRejected::GoogleRejected)?;
        let until = now.checked_add(lifetime).ok_or(TokenRejected::GoogleRejected)?;
        let scopes = Scopes::parse(&response.scope).map_err(|cause| TokenRejected::UnusableScope { cause })?;
        if self
            .policy
            .required_scopes()
            .iter()
            .any(|scope| !scopes.iter().any(|granted| granted == scope))
        {
            return Err(TokenRejected::GoogleRejected);
        }
        let address = VerifiedAddress::parse(&response.email).map_err(|_cause| TokenRejected::GoogleRejected)?;
        if let Some(domains) = self.policy.hosted_domains() {
            let hosted = response
                .hd
                .as_deref()
                .and_then(|hosted| VerifiedAddress::parse(&format!("domain-check@{hosted}")).ok());
            if !hosted.as_ref().is_some_and(|hosted| domains.admits(hosted)) {
                return Err(TokenRejected::GoogleRejected);
            }
        }
        let subject = Subject::verified_by_address(self.verifier.id().clone(), &response.sub, &address)
            .map_err(|cause| TokenRejected::UnusableSubject { cause })?;
        // Google cannot mint sutura scopes. Only the checked read-only grant maps to surface reads.
        // Never copy arbitrary tokeninfo scope text into the surface grant.
        let operations = if scopes
            .iter()
            .any(|scope| scope == "https://www.googleapis.com/auth/bigquery.readonly")
        {
            use sutura_app::Capability;
            Scopes::parse(
                &[Capability::DescribeCatalog, Capability::AskMetric, Capability::RunSql]
                    .map(Capability::scope)
                    .join(" "),
            )
            .map_err(|cause| TokenRejected::UnusableScope { cause })?
        } else {
            Scopes::none()
        };
        Ok(CachedIdentity {
            subject,
            address,
            scopes: operations,
            expires,
            until,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use sutura_config::{Environment, Settings, Sources};

    struct FixtureTokenInfo {
        response: serde_json::Value,
        failure: Option<TokenInfoFailed>,
        reads: AtomicUsize,
    }
    impl TokenInfoSource for FixtureTokenInfo {
        fn read(&self, _token: &Secret) -> Result<RawTokenInfo, TokenInfoFailed> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if let Some(failure) = self.failure {
                return Err(failure);
            }
            serde_json::from_value(self.response.clone()).map_err(|_cause| TokenInfoFailed::Unavailable)
        }
    }
    fn declaration() -> sutura_config::InboundVerifier {
        let settings=Settings::load(&Sources::defaults(Environment::Development).with_overlay("security:\n  inbound:\n    - id: google\n      path: /google/mcp\n      mode: direct\n      verifier: google_tokeninfo\n      resource: https://sutura.example.com/google/mcp\n      authorization_server: https://accounts.google.com\n      client_id: dedicated.apps.googleusercontent.com\n      hosted_domains: [example.com]\n      required_scopes: [https://www.googleapis.com/auth/bigquery.readonly]\n")).expect("Google policy");
        settings.security().inbound()[0].clone()
    }
    fn response() -> serde_json::Value {
        serde_json::json!({"aud":"dedicated.apps.googleusercontent.com","azp":"dedicated.apps.googleusercontent.com","sub":"subject-123","scope":"openid email https://www.googleapis.com/auth/bigquery.readonly","expires_in":"60","email":"person@example.com","email_verified":"true","hd":"example.com"})
    }
    fn over(response: serde_json::Value, failure: Option<TokenInfoFailed>) -> (GoogleVerifier, Arc<FixtureTokenInfo>) {
        let declaration = declaration();
        let source = Arc::new(FixtureTokenInfo {
            response,
            failure,
            reads: AtomicUsize::new(0),
        });
        let shared = Arc::clone(&source);
        (
            GoogleVerifier::over(
                declaration.id().clone(),
                declaration.google().expect("Google policy").clone(),
                shared,
            ),
            source,
        )
    }
    async fn refuses_without_caching(key: &str, value: serde_json::Value) -> bool {
        let mut answer = response();
        answer[key] = value;
        let (verifier, source) = over(answer, None);
        for _ in 0..2 {
            if !matches!(
                verifier.verify("opaque-fixture-token", Instant::now()).await,
                Err(TokenRejected::GoogleRejected)
            ) {
                return false;
            }
        }
        source.reads.load(Ordering::SeqCst) == 2 && verifier.positive.lock().is_empty()
    }
    #[tokio::test]
    async fn google_tokeninfo_refuses_another_audience() {
        assert!(refuses_without_caching("aud", serde_json::json!("other.apps.googleusercontent.com")).await);
    }
    #[tokio::test]
    async fn google_tokeninfo_refuses_another_authorized_party() {
        assert!(refuses_without_caching("azp", serde_json::json!("other.apps.googleusercontent.com")).await);
    }
    #[tokio::test]
    async fn google_tokeninfo_refuses_an_expired_token() {
        assert!(refuses_without_caching("expires_in", serde_json::json!("0")).await);
    }
    #[tokio::test]
    async fn google_tokeninfo_refuses_a_missing_scope() {
        assert!(refuses_without_caching("scope", serde_json::json!("openid email")).await);
    }
    #[tokio::test]
    async fn google_tokeninfo_refuses_an_unverified_email() {
        assert!(refuses_without_caching("email_verified", serde_json::json!(false)).await);
    }
    #[tokio::test]
    async fn google_tokeninfo_refuses_an_unlisted_hosted_domain() {
        assert!(refuses_without_caching("hd", serde_json::json!("other.example.com")).await);
    }
    #[tokio::test]
    async fn google_tokeninfo_unavailability_is_not_cached() {
        let (verifier, source) = over(response(), Some(TokenInfoFailed::Unavailable));
        for _ in 0..2 {
            assert!(matches!(
                verifier.verify("opaque-fixture-token", Instant::now()).await,
                Err(TokenRejected::GoogleUnavailable)
            ));
        }
        assert_eq!(source.reads.load(Ordering::SeqCst), 2);
        assert!(verifier.positive.lock().is_empty());
    }
    #[tokio::test]
    async fn google_tokeninfo_caches_a_digest_and_scopes_identity_to_the_verifier() {
        let (verifier, source) = over(response(), None);
        let now = Instant::now();
        let caller = verifier.verify("opaque-fixture-token", now).await.expect("verified token");
        verifier.verify("opaque-fixture-token", now).await.expect("cached positive");
        assert_eq!(source.reads.load(Ordering::SeqCst), 1);
        assert_eq!(caller.chain().subject().signed_sub(), Some("subject-123"));
        assert_eq!(caller.chain().subject().verifier_id().expect("verifier").as_str(), "google");
        assert_ne!(
            caller.chain().subject().id().expect("masked email").as_str(),
            "person@example.com"
        );
        assert!(caller.chain().subject().id().expect("masked email").as_str().contains('@'));
        assert!(!format!("{caller:?}").contains("opaque-fixture-token"));
        assert!(caller.verifier().is_google_tokeninfo());
        let cache = verifier.positive.lock();
        assert!(cache.contains_key(&AssertionDigest::of(&Secret::new("opaque-fixture-token"))));
        drop(cache);
    }
    #[tokio::test]
    async fn google_tokeninfo_positive_cache_expires_and_stays_bounded() {
        let (verifier, source) = over(response(), None);
        let now = Instant::now();
        verifier.verify("first-token", now).await.expect("first positive");
        verifier
            .verify("first-token", now + Duration::from_secs(61))
            .await
            .expect("expired entry is revalidated");
        assert_eq!(source.reads.load(Ordering::SeqCst), 2);
        for index in 0..=CACHE_CAPACITY {
            verifier
                .verify(&format!("distinct-token-{index}"), now)
                .await
                .expect("positive within bounded cache");
        }
        assert_eq!(verifier.positive.lock().len(), CACHE_CAPACITY);
    }
    #[tokio::test]
    async fn a_verified_google_readonly_grant_maps_to_three_surface_reads_and_never_remote_legs() {
        use sutura_app::{Capability, Permitted};
        let mut answer = response();
        answer["scope"] = serde_json::json!("openid email https://www.googleapis.com/auth/bigquery.readonly sutura:legs.run");
        let (verifier, _) = over(answer, None);
        let caller = verifier
            .verify("opaque-fixture-token", Instant::now())
            .await
            .expect("Google readonly caller");
        let permitted = Permitted::granted_by(caller.scopes().iter());
        for capability in [Capability::DescribeCatalog, Capability::AskMetric, Capability::RunSql] {
            assert!(
                permitted.includes(capability),
                "{capability:?} is the declared Google read policy"
            );
        }
        assert!(
            !permitted.includes(Capability::RunRemoteLeg),
            "even foreign sutura scope text cannot authorize remote execution"
        );
        assert_eq!(caller.scopes().count(), 3);
    }

    #[tokio::test]
    async fn tokeninfo_unreachable_answers_503_without_a_challenge_or_token_text() {
        let (verifier, _) = over(response(), Some(TokenInfoFailed::Unavailable));
        let gate = Arc::new(super::super::gate::InboundGate::over_google(&declaration(), verifier));
        let (status, body, challenged) = super::super::tests::guarded_answer(&gate, "opaque-fixture-token").await;
        assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, super::super::tests::IDENTITY_UNAVAILABLE);
        assert!(!challenged);
        assert!(!body.contains("opaque-fixture-token"));
    }

    #[tokio::test]
    async fn google_discovery_names_google_and_the_client_scopes_on_its_mount() {
        use tower::ServiceExt as _;
        let (verifier, _) = over(response(), None);
        let gate = super::super::gate::InboundGate::over_google(&declaration(), verifier);
        let resource = gate.protected_resources().next().expect("Google discovery");
        let app: axum::Router = resource.router();
        let request = axum::http::Request::builder()
            .uri("/.well-known/oauth-protected-resource/google/mcp")
            .header("host", "sutura.example.com")
            .body(axum::body::Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("discovery answers");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024)
            .await
            .expect("metadata body");
        let metadata: serde_json::Value = serde_json::from_slice(&bytes).expect("metadata");
        assert_eq!(
            metadata["authorization_servers"],
            serde_json::json!(["https://accounts.google.com"])
        );
        assert_eq!(
            metadata["scopes_supported"],
            serde_json::json!(["openid", "email", "https://www.googleapis.com/auth/bigquery.readonly"])
        );
        assert_eq!(metadata["resource"], "https://sutura.example.com/google/mcp");
    }

    #[tokio::test]
    async fn an_opaque_google_token_uses_its_mounted_verifier_without_a_jwt_issuer_peek() {
        let (verifier, source) = over(response(), None);
        let declaration = declaration();
        let (jwt, jwt_token) = super::super::tests::fixtures::jwt_neighbour();
        let gate = super::super::gate::InboundGate::over_google(&declaration, verifier)
            .with_also_accept(std::collections::BTreeMap::from([(jwt.issuer().to_owned(), jwt)]));
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer opaque-fixture-token"),
        );
        let caller = gate
            .establish_on(declaration.path(), &headers, Instant::now())
            .await
            .expect("opaque token is introspected on its own mount");
        assert!(caller.verifier().is_google_tokeninfo());
        assert_eq!(source.reads.load(Ordering::SeqCst), 1);
        let wrong = sutura_config::McpPath::parse("/other/mcp").expect("other mount");
        let rejected = gate
            .establish_on(&wrong, &headers, Instant::now())
            .await
            .expect_err("wrong mount has no eligible verifier");
        assert!(matches!(rejected, TokenRejected::Absent { .. }));
        assert_eq!(
            source.reads.load(Ordering::SeqCst),
            1,
            "another mount never introspects this token"
        );
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_str(&format!("Bearer {jwt_token}")).expect("signed JWT header"),
        );
        let jwt_caller = gate
            .establish_on(&sutura_config::McpPath::conventional(), &headers, Instant::now())
            .await
            .expect("the JWT mount still verifies its signed token");
        assert!(!jwt_caller.verifier().is_google_tokeninfo());
        assert_eq!(
            source.reads.load(Ordering::SeqCst),
            1,
            "the JWT mount does not introspect at Google"
        );
    }
}

//! The fixture tier's deployment token, for the golden matrix in `sutura-app`'s tests.
//!
//! Behind the default-off `fixtures` feature, like `sutura_exec_clickhouse::fixture`: nothing here
//! belongs in a composition root.

use std::time::{SystemTime, UNIX_EPOCH};

use sutura_domain::identity::Secret;

/// `CUBEJS_API_SECRET` in `compose.services.yaml`'s `cube` block: a fixture, not a secret.
const TIER_SECRET: &[u8] = b"sutura-dev-cube-secret-with-256-bits";

/// Why the tier's token was not signed.
#[derive(Debug, thiserror::Error)]
pub enum UnsignedToken {
    #[error("the clock reads before 1970")]
    Clock(#[source] std::time::SystemTimeError),
    #[error("the HS256 token did not sign")]
    Sign(#[source] jsonwebtoken::errors::Error),
}

/// An HS256 token over the tier's secret, valid for one hour - the deployment token Cube's default
/// `check_auth` verifies.
///
/// # Errors
///
/// [`UnsignedToken`] when the clock reads before 1970 or the token does not sign.
pub fn tier_token() -> Result<Secret, UnsignedToken> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(UnsignedToken::Clock)?
        .as_secs();
    let claims = serde_json::json!({ "sub": "sutura-matrix", "iat": now, "exp": now.saturating_add(3600) });
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(TIER_SECRET),
    )
    .map(Secret::new)
    .map_err(UnsignedToken::Sign)
}

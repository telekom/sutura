//! `sutura-dev mint-pat` - the docker DataHub tier signs its own personal-access token.
//!
//! The headless GMS this tier runs exposes no token-minting surface: `/auth/authenticate` and
//! `/auth/accessTokens` live in the React frontend, which `compose.services.yaml` deliberately omits
//! (both return `404` on the pinned 1.7.0 GMS - measured). So the *tier* mints the PAT the wave-one
//! E2E serves, offline, with the tier's OWN `DATAHUB_TOKEN_SERVICE_SIGNING_KEY` - the symmetric key
//! GMS validates access tokens against. The served binary presents the result via `token_file`
//! exactly as it does the fake's token, and `datahub-acceptance` presents the same PAT as its
//! bearer; `METADATA_SERVICE_AUTH_ENABLED` stays `true`, so every read against the tier is an
//! enforced one.
//!
//! # The claim set, measured not guessed
//!
//! Read out of `com.datahub.authentication.token.StatefulTokenService` (+ its `StatelessTokenService`
//! parent and `DataHubTokenAuthenticator`) at the pinned 1.7.0, then confirmed against a live tier
//! (build-lane probe: the minted PAT -> `GET /openapi/v3/entity/dataset?count=0` returned `200`
//! with `Bearer`; a bearer-less fetch of the same path returned `401`):
//!
//! * `version` **`1`**, not `2`. The stateful service only hash-and-looks-up *stored* tokens for
//!   version 2 - `validateAccessToken` returns `revoked` for a v2 token the store never saw. Version
//!   1 skips that store lookup, which is exactly the honest offline case: a self-minted token the
//!   tier trusts but that was never registered as a revocable PAT.
//! * `type` `PERSONAL`, `actorType` `USER`, `actorId` the seeded corpuser id, `sub`
//!   `urn:li:corpuser:<id>`. `actorType`/`actorId` are read back into the actor after validation, so
//!   they must parse as GMS's enums.
//! * `exp` in **seconds** since the epoch: jjwt serializes `setExpiration(Date)` as a `NumericDate`.
//!   (`iat` is set by nothing here and required by nothing here.)
//!
//! ## The limits, next to the claim
//!
//! A self-minted PAT is the tier trusting the symmetric key it owns - **not** DataHub's full
//! token-service flow: there is no DB-backed PAT entity, so no per-token revocation or audit, and
//! no login/session flow is exercised. `version: 1` is deliberate: it is what makes an offline mint
//! acceptable to GMS's stateful validator. None of this is a production DataHub; it is the docker
//! tier.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long the self-minted PAT stays valid. Bounded, but far past any single E2E run; nothing in
/// the tier looks past this number.
pub(crate) const DEFAULT_LIFETIME: Duration = Duration::from_hours(12);

/// The one thing that can go wrong in [`sign`], said once.
///
/// `Display` by hand the way `crate::discovery` does it: one failure mode does not earn an error
/// derive, and a correct caller cannot reach it.
#[derive(Debug)]
pub(crate) struct Defect {
    told: &'static str,
}

impl core::fmt::Display for Defect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.told)
    }
}

impl std::error::Error for Defect {}

/// The seconds since the epoch, or [`Defect`] if the clock is before 1970.
fn now_seconds() -> Result<u64, Defect> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_clock| Defect {
            told: "the system clock predates 1970",
        })
}

/// Sign an HS256 access token with the tier's own signing key, for the seeded corpuser `actor_id`.
///
/// A pure function - the whole point (key, actor, lifetime) -> JWT - so the unit test below pins the
/// exact claim set and re-verifies the signature under the key. A caller can mint with whatever it
/// needs (the mutation suite's wrong-key case mints with a different key than GMS holds).
///
/// The claims are built as a `serde_json::Value` rather than a named struct: the crate's only
/// serialization dependency is `serde_json` (this module lives behind the `mock-issuer` feature,
/// which must not add `serde` to `sutura-dev`'s non-optional graph), and the claim set is small and
/// fixed enough that the unit test reads it back through the same `Value` shape.
pub(crate) fn sign(key: &str, actor_id: &str, lifetime: Duration) -> Result<String, Defect> {
    let now = now_seconds()?;
    let claims = serde_json::json!({
        "version": "1",
        "type": "PERSONAL",
        "actorType": "USER",
        "actorId": actor_id,
        "sub": format!("urn:li:corpuser:{actor_id}"),
        "exp": now.saturating_add(lifetime.as_secs()),
    });
    let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    let encoding = jsonwebtoken::EncodingKey::from_secret(key.as_bytes());
    jsonwebtoken::encode(&header, &claims, &encoding).map_err(|_library| Defect {
        told: "the JWT library refused the claim set",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The claims GMS reads back, exactly as the validator reads them: the four it requires plus the
    /// standard `sub` and `exp`. Read off the wire as JSON so the assertion is about the doc, not
    /// about a struct this module reuses.
    #[test]
    fn the_minted_pat_decodes_to_the_documented_claim_set() {
        let key = "sutura-dev-signing-key";
        let actor = "datahub";
        let pat = sign(key, actor, DEFAULT_LIFETIME).expect("a valid key signs");

        // Re-verify under the SAME key, through the library's real verify path.
        let data: jsonwebtoken::TokenData<serde_json::Value> = jsonwebtoken::decode(
            &pat,
            &jsonwebtoken::DecodingKey::from_secret(key.as_bytes()),
            &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256),
        )
        .expect("the PAT verifies under the tier's own signing key");

        let claims = &data.claims;
        assert_eq!(
            claims["version"], "1",
            "stateless version - the stateful store lookup is skipped"
        );
        assert_eq!(claims["type"], "PERSONAL");
        assert_eq!(claims["actorType"], "USER");
        assert_eq!(claims["actorId"], actor);
        assert_eq!(claims["sub"], format!("urn:li:corpuser:{actor}"));
        let exp = claims["exp"].as_u64().expect("exp is a number");
        let now = now_seconds().expect("the clock is sane");
        assert!(exp > now, "the PAT expires in the future ({exp} > {now})");
        assert!(
            exp <= now + DEFAULT_LIFETIME.as_secs(),
            "the PAT does not outlive its chosen lifetime"
        );

        // And a WRONG key must not verify - the mutation that makes the tier cell fail.
        let wrong = jsonwebtoken::decode::<serde_json::Value>(
            &pat,
            &jsonwebtoken::DecodingKey::from_secret(b"a-different-key"),
            &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256),
        );
        assert!(
            wrong.is_err(),
            "a PAT signed by one key must not verify under a different one"
        );
    }
}

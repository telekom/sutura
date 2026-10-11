#![forbid(unsafe_code)]
//! The live `cube` tier (`just dev-up-cube`), read through the real reader.
//!
//! `#[ignore]`d, behind `just cube-acceptance`: the tier is a docker service, and `just test` and the
//! nix sandbox do not start one. `sutura_dev::provisioned::here` panics under
//! `SUTURA_DEV_REQUIRE_TIER` and prints a notice otherwise when no tier is up.
#![cfg(feature = "http")]

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use sutura_catalog_cube::http::{Endpoint, HttpMetaReader, HttpReaderError, ReadBounds};
    use sutura_catalog_cube::{CubeCatalog, CubeError, fixture};
    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    /// `CUBEJS_API_SECRET` in `compose.services.yaml`'s `cube` block: a fixture, not a secret.
    const TIER_SECRET: &[u8] = b"sutura-dev-cube-secret-with-256-bits";

    fn endpoint() -> Option<String> {
        let inside = Path::new(env!("CARGO_MANIFEST_DIR"));
        sutura_dev::provisioned::here(inside, "cube")
            .endpoint()
            .map(ToString::to_string)
    }

    /// An HS256 token over `key`, valid for ten minutes - the deployment token Cube's default
    /// `check_auth` verifies with its API secret.
    fn token(key: &[u8]) -> Secret {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs();
        let claims = serde_json::json!({ "sub": "sutura-catalog", "iat": now, "exp": now + 600 });
        let signed = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(key),
        )
        .expect("an HS256 token signs");
        Secret::new(signed)
    }

    fn catalog<R>(reader: R) -> CubeCatalog<R>
    where
        R: sutura_catalog_cube::MetaReader,
    {
        let name = SourceName::parse("metrics").expect("a name");
        CubeCatalog::new(
            name.clone(),
            DefinitionVersion::parse("test").expect("a version"),
            name,
            reader,
        )
    }

    fn reader(endpoint: &str, token: Secret) -> HttpMetaReader {
        HttpMetaReader::new(
            Endpoint::parse(&format!("http://{endpoint}")).expect("the loopback endpoint parses"),
            token,
            ReadBounds::parse(30, 1 << 20).expect("usable bounds"),
            None,
        )
    }

    #[test]
    #[ignore = "needs `just dev-up-cube` - run `just cube-acceptance`"]
    fn the_live_cube_serves_the_recorded_definitions() {
        let Some(endpoint) = endpoint() else { return };
        let live = catalog(reader(&endpoint, token(TIER_SECRET)))
            .load()
            .expect("the live tier's answer loads");
        let recorded = catalog(fixture::FixtureReader).load().expect("the recorded answer loads");
        assert_eq!(live.definitions(), recorded.definitions());
        assert_eq!(live.digest(), recorded.digest());
    }

    #[test]
    #[ignore = "needs `just dev-up-cube` - run `just cube-acceptance`"]
    fn a_token_the_tier_did_not_sign_is_refused() {
        let Some(endpoint) = endpoint() else { return };
        let refused = catalog(reader(&endpoint, token(b"a-key-the-tier-never-saw-0123456789")))
            .load()
            .expect_err("Cube refuses a token its secret did not sign");
        let CubeError::Read(cause) = &refused else {
            panic!("a refused read is a read error: {refused:?}")
        };
        assert!(
            matches!(
                cause.downcast_ref::<HttpReaderError>(),
                Some(HttpReaderError::Refused { status: 403, .. })
            ),
            "{cause:?}"
        );
    }
}

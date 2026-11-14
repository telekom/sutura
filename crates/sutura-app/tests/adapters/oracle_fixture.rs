//! Oracle's two live venues: verified TLS by default, or the native probe's allocated loopback port.

use std::num::NonZeroU16;
use std::path::Path;
use std::sync::OnceLock;

use sutura_dev::discovery::Endpoint;
use sutura_dev::provisioned::{self, Provisioned};
use sutura_exec_oracle::fixture::{FixtureCredential, UnconfiguredFixture};

pub(super) enum OracleFixture {
    Verified(Endpoint),
    NativeEncryption(NonZeroU16),
}

impl OracleFixture {
    pub(super) fn host(&self) -> &str {
        match self {
            Self::Verified(endpoint) => endpoint.host(),
            Self::NativeEncryption(_) => "127.0.0.1",
        }
    }

    pub(super) fn port(&self) -> u16 {
        match self {
            Self::Verified(endpoint) => endpoint.port(),
            Self::NativeEncryption(port) => port.get(),
        }
    }

    pub(super) fn credential(&self) -> Result<FixtureCredential, UnconfiguredFixture> {
        match self {
            Self::Verified(_) => FixtureCredential::from_env(),
            Self::NativeEncryption(_) => FixtureCredential::native_from_env(),
        }
    }
}

impl core::fmt::Display for OracleFixture {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}", self.host(), self.port())
    }
}

/// A requested venue must exist: neither mode falls back to another server or silently skips.
pub(super) fn here() -> Option<&'static OracleFixture> {
    static CACHED: OnceLock<Option<OracleFixture>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            super::set("SUTURA_ORACLE_TIER")?;
            match super::set("SUTURA_ORACLE_FIXTURE_TRANSPORT").as_deref() {
                Some("native-encryption") => {
                    let port = super::set("SUTURA_ORACLE_ACCEPTANCE_PORT")
                        .expect("the native probe must publish its allocated Oracle port")
                        .parse::<NonZeroU16>()
                        .expect("the native probe's allocated Oracle port must be a nonzero u16");
                    Some(OracleFixture::NativeEncryption(port))
                }
                None | Some("verified") => match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle") {
                    Provisioned::At(endpoint) => Some(OracleFixture::Verified(endpoint)),
                    Provisioned::Skipped(absent) => {
                        panic!("`SUTURA_ORACLE_TIER` is set and no Oracle tier answered discovery: {absent}")
                    }
                },
                Some(unknown) => panic!("unsupported Oracle fixture transport: {unknown}"),
            }
        })
        .as_ref()
}

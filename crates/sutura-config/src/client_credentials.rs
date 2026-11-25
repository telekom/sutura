//! One OAuth 2.0 client-credentials block, as every entry that declares one writes it.
//!
//! `client_id`, `client_secret_file`, `scopes` and `token_path` - a PATH on the declaring entry's
//! own `endpoint`, so a settings file cannot send the client secret to another host. The Foundry
//! catalog's `foundry.client_credentials`, a Foundry source's `client_credentials` and its
//! `verification_client_credentials` are all read here: one parser, one set of refusals, each
//! naming the key it was written under. The scopes are parsed here by the one grammar the HTTP
//! client sends them with ([`sutura_domain::oauth::Scopes`]), and the token path into its own
//! [`TokenPath`]; the client identifier is parsed by the HTTP client's own type at composition
//! (`sutura_http_client::token::ClientCredentials::declared`).

use sutura_domain::oauth::{InvalidScope, Scopes};

use std::path::{Path, PathBuf};

/// Foundry's documented token endpoint, under the entry's own `endpoint`.
pub const DEFAULT_TOKEN_PATH: &str = "/multipass/api/oauth2/token";

/// A client-credentials block, as read.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawClientCredentials {
    #[serde(default)]
    pub(crate) token_path: Option<String>,
    #[serde(default)]
    pub(crate) client_id: Option<String>,
    #[serde(default)]
    pub(crate) client_secret_file: Option<String>,
    #[serde(default)]
    pub(crate) scopes: Vec<String>,
}

/// A token endpoint's path on the declaring entry's own `endpoint`: a leading `/`, not `//`, and
/// printable ASCII with no `?`, `#`, `@` or `\` - so it names no other host, no query and no
/// credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenPath(String);

impl TokenPath {
    fn parse(raw: &str) -> Option<Self> {
        let path_only = raw.starts_with('/')
            && !raw.starts_with("//")
            && raw
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'?' | b'#' | b'@' | b'\\'));
        path_only.then(|| Self(String::from(raw)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A client-credentials grant as declared. Its secret is a file, read at boot by the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCredentialsSettings {
    token_path: TokenPath,
    client_id: String,
    client_secret_file: PathBuf,
    scopes: Scopes,
}

/// Why a declared client-credentials block is not usable. `key` is the block's own key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidClientCredentials {
    #[error("`{key}` is required, and is empty or absent")]
    Missing { key: String },
    /// Not a path on the entry's own endpoint: no leading `/`, a leading `//`, or a `?`, `#`, `@`,
    /// `\`, space or non-ASCII byte.
    #[error("`{key}.token_path` is not a path on the entry's endpoint - write `/` and a path, no query, no host")]
    TokenPath { key: String },
    #[error("`{key}.scopes` holds a value that is not an OAuth 2.0 scope token")]
    Scope {
        key: String,
        #[source]
        cause: InvalidScope,
    },
    #[error("`{key}.client_secret_file` is relative - write an absolute path")]
    RelativeSecretFile { key: String },
}

impl ClientCredentialsSettings {
    /// Reads the block written under `key`.
    ///
    /// # Errors
    ///
    /// [`InvalidClientCredentials`], naming `key`.
    pub(crate) fn parse(raw: &RawClientCredentials, key: &str) -> Result<Self, InvalidClientCredentials> {
        let token_path = TokenPath::parse(written(raw.token_path.as_deref()).unwrap_or(DEFAULT_TOKEN_PATH))
            .ok_or_else(|| InvalidClientCredentials::TokenPath { key: String::from(key) })?;
        let client_id = written(raw.client_id.as_deref()).ok_or_else(|| InvalidClientCredentials::Missing {
            key: format!("{key}.client_id"),
        })?;
        let client_secret_file = written(raw.client_secret_file.as_deref()).ok_or_else(|| InvalidClientCredentials::Missing {
            key: format!("{key}.client_secret_file"),
        })?;
        let client_secret_file = PathBuf::from(client_secret_file);
        if client_secret_file.is_relative() {
            return Err(InvalidClientCredentials::RelativeSecretFile { key: String::from(key) });
        }
        let scopes = Scopes::parse(raw.scopes.iter().map(String::as_str)).map_err(|cause| InvalidClientCredentials::Scope {
            key: String::from(key),
            cause,
        })?;
        Ok(Self {
            token_path,
            client_id: String::from(client_id),
            client_secret_file,
            scopes,
        })
    }

    /// The token endpoint's path on the entry's `endpoint`; [`DEFAULT_TOKEN_PATH`] when unwritten.
    #[inline]
    #[must_use]
    pub const fn token_path(&self) -> &TokenPath {
        &self.token_path
    }

    #[inline]
    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// The file the client secret is read from at boot. Never the secret itself.
    #[inline]
    #[must_use]
    pub fn client_secret_file(&self) -> &Path {
        &self.client_secret_file
    }

    #[inline]
    #[must_use]
    pub const fn scopes(&self) -> &Scopes {
        &self.scopes
    }
}

fn written(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|text| !text.is_empty())
}

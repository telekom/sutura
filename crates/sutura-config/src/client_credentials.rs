//! One OAuth 2.0 client-credentials block, as every entry that declares one writes it.
//!
//! `client_id`, `client_secret_file`, `scopes` and `token_path` - a PATH on the declaring entry's
//! own `endpoint`, so a settings file cannot send the client secret to another host. The Foundry
//! catalog's `foundry.client_credentials`, a Foundry source's `client_credentials` and its
//! `verification_client_credentials` are all read here: one parser, one set of refusals, each
//! naming the key it was written under. The client identifier and the scopes are parsed into the
//! HTTP client's own types by the composition root, which owns those grammars
//! (`sutura_http_client::token::ClientCredentials::declared`).

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

/// A client-credentials grant as declared. Its secret is a file, read at boot by the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCredentialsSettings {
    token_path: String,
    client_id: String,
    client_secret_file: PathBuf,
    scopes: Vec<String>,
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
    #[error("`{key}.scopes` holds an empty scope")]
    EmptyScope { key: String },
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
        let token_path = written(raw.token_path.as_deref()).unwrap_or(DEFAULT_TOKEN_PATH);
        let path_only = token_path.starts_with('/')
            && !token_path.starts_with("//")
            && token_path
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'?' | b'#' | b'@' | b'\\'));
        if !path_only {
            return Err(InvalidClientCredentials::TokenPath { key: String::from(key) });
        }
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
        if raw.scopes.iter().any(|scope| scope.trim().is_empty()) {
            return Err(InvalidClientCredentials::EmptyScope { key: String::from(key) });
        }
        Ok(Self {
            token_path: String::from(token_path),
            client_id: String::from(client_id),
            client_secret_file,
            scopes: raw.scopes.clone(),
        })
    }

    /// The token endpoint's path on the entry's `endpoint`; [`DEFAULT_TOKEN_PATH`] when unwritten.
    #[inline]
    #[must_use]
    pub fn token_path(&self) -> &str {
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
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

fn written(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|text| !text.is_empty())
}

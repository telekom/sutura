//! The `catalog.kind: foundry` entry's own settings. **Experimental.**
//!
//! The ontology to read and how the reader authenticates: the entry's `token_file`, or an OAuth 2.0
//! client-credentials grant whose token endpoint is a PATH on the entry's own `endpoint`, so a
//! settings file cannot send the client secret to another host. Checked here: that each key was
//! written and has its shape. Parsed into the adapter's own types by the composition root, which
//! owns their grammars: the ontology identifier, the client identifier and the scope tokens.

use std::path::{Path, PathBuf};

/// Foundry's documented token endpoint, under the entry's own `endpoint`.
pub const DEFAULT_TOKEN_PATH: &str = "/multipass/api/oauth2/token";

/// The `foundry:` block of a catalog entry, as read.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawFoundry {
    /// The ontology's API name or resource identifier.
    #[serde(default)]
    pub(crate) ontology: Option<String>,
    #[serde(default)]
    pub(crate) client_credentials: Option<RawClientCredentials>,
}

/// The `foundry.client_credentials:` block, as read.
#[derive(Debug, Default, serde::Deserialize)]
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

/// How a Foundry reader authenticates. Either is the deployment's own identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoundryAuth {
    /// The entry's `token_file`, read once at boot.
    Token { token_file: PathBuf },
    /// A client-credentials grant, minted again before each token expires.
    ClientCredentials(ClientCredentialsSettings),
}

/// A client-credentials grant as declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCredentialsSettings {
    token_path: String,
    client_id: String,
    client_secret_file: PathBuf,
    scopes: Vec<String>,
}

impl ClientCredentialsSettings {
    fn parse(raw: &RawClientCredentials) -> Result<Self, InvalidFoundryCatalog> {
        let token_path = written(raw.token_path.as_deref()).unwrap_or(DEFAULT_TOKEN_PATH);
        let path_only = token_path.starts_with('/')
            && !token_path.starts_with("//")
            && token_path
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'?' | b'#' | b'@' | b'\\'));
        if !path_only {
            return Err(InvalidFoundryCatalog::TokenPath);
        }
        let client_id = written(raw.client_id.as_deref()).ok_or(InvalidFoundryCatalog::Missing {
            key: "foundry.client_credentials.client_id",
        })?;
        let client_secret_file = written(raw.client_secret_file.as_deref()).ok_or(InvalidFoundryCatalog::Missing {
            key: "foundry.client_credentials.client_secret_file",
        })?;
        if raw.scopes.iter().any(|scope| scope.trim().is_empty()) {
            return Err(InvalidFoundryCatalog::EmptyScope);
        }
        Ok(Self {
            token_path: String::from(token_path),
            client_id: String::from(client_id),
            client_secret_file: PathBuf::from(client_secret_file),
            scopes: raw.scopes.clone(),
        })
    }

    /// The token endpoint's path on the entry's `endpoint`; [`DEFAULT_TOKEN_PATH`] when unwritten.
    #[inline]
    pub fn token_path(&self) -> &str {
        &self.token_path
    }

    #[inline]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// The file the client secret is read from at boot. Never the secret itself.
    #[inline]
    pub fn client_secret_file(&self) -> &Path {
        &self.client_secret_file
    }

    #[inline]
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

/// The `kind: foundry`-only half of a catalog entry, all or nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundrySettings {
    ontology: String,
    auth: FoundryAuth,
}

impl FoundrySettings {
    /// Reads the `foundry:` block and the entry's `token_file`.
    pub(crate) fn parse(raw: Option<&RawFoundry>, token_file: Option<&str>) -> Result<Self, InvalidFoundryCatalog> {
        let raw = raw.ok_or(InvalidFoundryCatalog::Missing { key: "foundry" })?;
        let ontology = written(raw.ontology.as_deref()).ok_or(InvalidFoundryCatalog::Missing { key: "foundry.ontology" })?;
        let token_file = written(token_file);
        let auth = match (token_file, raw.client_credentials.as_ref()) {
            (Some(_), Some(_)) => return Err(InvalidFoundryCatalog::TwoAuths),
            (None, None) => return Err(InvalidFoundryCatalog::NoAuth),
            (Some(token_file), None) => FoundryAuth::Token {
                token_file: PathBuf::from(token_file),
            },
            (None, Some(grant)) => FoundryAuth::ClientCredentials(ClientCredentialsSettings::parse(grant)?),
        };
        Ok(Self {
            ontology: String::from(ontology),
            auth,
        })
    }

    #[inline]
    pub fn ontology(&self) -> &str {
        &self.ontology
    }

    #[inline]
    pub const fn auth(&self) -> &FoundryAuth {
        &self.auth
    }
}

/// Why a `kind: foundry` entry's own keys are not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidFoundryCatalog {
    #[error("`{key}` is required for catalog.kind: foundry, and is empty or absent")]
    Missing { key: &'static str },
    #[error("catalog.kind: foundry needs one of `token_file` or `foundry.client_credentials`, and neither is written")]
    NoAuth,
    #[error("catalog.kind: foundry takes one of `token_file` or `foundry.client_credentials`, and both are written")]
    TwoAuths,
    /// Not a path on the entry's own endpoint: no leading `/`, a leading `//`, or a `?`, `#`, `@`,
    /// `\`, space or non-ASCII byte.
    #[error(
        "`foundry.client_credentials.token_path` is not a path on the entry's endpoint - write `/` and a path, no query, no host"
    )]
    TokenPath,
    #[error("`foundry.client_credentials.scopes` holds an empty scope")]
    EmptyScope,
}

fn written(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|text| !text.is_empty())
}

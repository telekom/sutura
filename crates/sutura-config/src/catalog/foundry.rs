//! The `catalog.kind: foundry` entry's own settings. **Experimental.**
//!
//! The ontology to read and how the reader authenticates: the entry's `token_file`, or an OAuth 2.0
//! client-credentials grant whose token endpoint is a PATH on the entry's own `endpoint`, so a
//! settings file cannot send the client secret to another host. Checked here: that each key was
//! written and has its shape. Parsed into the adapter's own types by the composition root, which
//! owns their grammars: the ontology identifier, the client identifier and the scope tokens.

use std::path::PathBuf;

use sutura_domain::model::ObjectTypeExclusions;

pub use crate::client_credentials::DEFAULT_TOKEN_PATH;
use crate::client_credentials::{ClientCredentialsSettings, InvalidClientCredentials, RawClientCredentials};

/// The `foundry:` block of a catalog entry, as read.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawFoundry {
    /// The ontology's API name or resource identifier.
    #[serde(default)]
    pub(crate) ontology: Option<String>,
    #[serde(default)]
    pub(crate) client_credentials: Option<RawClientCredentials>,
    /// The object types left out, by pattern. Absent is the default; `[]` leaves nothing out.
    #[serde(default)]
    pub(crate) exclude_object_types: Option<Vec<String>>,
}

/// How a Foundry reader authenticates. Either is the deployment's own identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoundryAuth {
    /// The entry's `token_file`, read once at boot.
    Token { token_file: PathBuf },
    /// A client-credentials grant, minted again before each token expires.
    ClientCredentials(ClientCredentialsSettings),
}

/// The `kind: foundry`-only half of a catalog entry, all or nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundrySettings {
    ontology: String,
    auth: FoundryAuth,
    exclusions: ObjectTypeExclusions,
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
            (None, Some(grant)) => FoundryAuth::ClientCredentials(
                ClientCredentialsSettings::parse(grant, "foundry.client_credentials")
                    .map_err(InvalidFoundryCatalog::ClientCredentials)?,
            ),
        };
        let exclusions = match raw.exclude_object_types.as_deref() {
            None => ObjectTypeExclusions::default(),
            Some(patterns) => ObjectTypeExclusions::parse(patterns.iter().map(String::as_str)).map_err(|cause| {
                InvalidFoundryCatalog::ExcludeObjectTypes {
                    value: String::from(cause.value()),
                }
            })?,
        };
        Ok(Self {
            ontology: String::from(ontology),
            auth,
            exclusions,
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

    /// The object types this catalog, and the source of the same alias, leave out.
    #[inline]
    pub const fn exclusions(&self) -> &ObjectTypeExclusions {
        &self.exclusions
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
    #[error("`foundry.client_credentials` is not usable")]
    ClientCredentials(#[source] InvalidClientCredentials),
    #[error("`foundry.exclude_object_types` holds {value:?}, which is neither an exact API name nor a prefix ending in `.*`")]
    ExcludeObjectTypes { value: String },
}

fn written(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|text| !text.is_empty())
}

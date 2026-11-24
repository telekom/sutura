//! `catalog.kind: entropy`: how sutura signs in to a data product marketplace, and which declared
//! source serves each server its data contracts and output ports name.
//!
//! The endpoint and `metric_property` are [`super::CatalogSettings`]' own fields, checked there the
//! way a `datahub` entry's are. The token endpoint and `scope` are held as written and parsed by the
//! crate that sends them - **so an unusable one loads here and is refused only by a build that links
//! the reader**, which no build does yet.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sutura_domain::model::SourceName;

use crate::raw::{RawCatalog, RawEntropyBinding, RawEntropyOidc};
use crate::sources::SourceRegistry;

/// A `kind: entropy` entry's sign-in and bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntropySettings {
    auth: EntropyAuth,
    bindings: Vec<EntropyBinding>,
}

/// How sutura signs in. A deployment declares exactly one, and either is presented as
/// `Authorization: Bearer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntropyAuth {
    /// `api_key_file`: the key is the bearer token. It reads whatever its owner - an organization, a
    /// team or a user - can see.
    ApiKey { file: PathBuf },
    /// `oidc`: the client credentials grant, the client authenticated by HTTP Basic, against the
    /// marketplace's own token endpoint or an external issuer's. An agent client reads only the
    /// endpoints open to agents, data products and contracts among them. **An external issuer's `aud`
    /// is checked only when the marketplace registers that issuer with an audience**, which nothing
    /// here can see.
    Oidc(EntropyOidc),
}

/// The `oidc` block. The secret is read from `client_secret_file` at boot, never written here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntropyOidc {
    token_endpoint: String,
    client_id: String,
    client_secret_file: PathBuf,
    scope: Option<String>,
}

/// One server a data contract or an output port may name, and the declared source that serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntropyBinding {
    server: ServerMatch,
    source: SourceName,
}

/// A binding's `match:`: a server `type` and at least one field that picks its servers out.
///
/// A server matches when its type is the match's and every listed field equals the server's field of
/// that name rendered as text, an integer `port` as its decimal digits. Fields the match does not
/// list are ignored, so one shape reads every server type and an output port's `server` object,
/// whose type is the port's.
///
/// **Exact string equality**: no case folding and no host normalization, so `DB.example.com` is not
/// `db.example.com` and `postgresql` is not `postgres`. **Nothing checks that the source's kind can
/// serve the type**: a `bigquery` server bound to a `duckdb` source loads and is read from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerMatch {
    kind: FieldValue,
    fields: BTreeMap<FieldName, FieldValue>,
}

/// A server field's name, spelled as the contract spells it (`serviceName`). Never blank.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldName(String);

/// A server `type` or field value, as written. Never blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldValue(String);

/// Why a `kind: entropy` entry's own keys are not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidEntropyCatalog {
    /// A key the entropy reader does not read, refused rather than left in the file doing nothing.
    #[error("`{key}` is not read when `kind` is `entropy` - remove it")]
    Unread { key: &'static str },
    /// Both sign-ins, or neither.
    #[error("write exactly one of `api_key_file` and `oidc`")]
    AuthMode,
    #[error("`{key}` is empty")]
    Missing { key: &'static str },
    /// An `@` in a URL is userinfo however a parser splits it, so it is never quoted back.
    #[error("`{key}` carries an `@` - credentials in the URL are refused")]
    CredentialsInUrl { key: &'static str },
    /// A file named relative to wherever the process happens to start.
    #[error("`{key}` is `{path}`, which is not an absolute path")]
    RelativePath { key: &'static str, path: String },
    #[error("`bindings` is absent or empty - bind at least one server to a declared source")]
    NoBindings,
    #[error("`bindings[{position}]` is not usable")]
    Binding {
        position: usize,
        #[source]
        cause: InvalidBinding,
    },
    /// Two matches of one type that list no field with different values: a server carrying every
    /// field of both matches both. Refused here, so a loaded deployment never has a server two
    /// bindings claim.
    #[error("`bindings[{first}]` and `bindings[{second}]` can match one server - give them a field with different values")]
    OverlappingBindings { first: usize, second: usize },
}

/// Why one `bindings:` entry is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidBinding {
    #[error("`match` has no `type` - write the server type its contract or port names")]
    NoType,
    /// A match on the type alone would bind every server of it to one source.
    #[error("`match` lists only `type` - add a field that picks its servers out")]
    NoField,
    #[error("`match.{field}` is blank")]
    Blank { field: String },
    #[error("`source: {source_alias}` is not declared under `sources:`")]
    UnknownSource { source_alias: String },
}

impl EntropySettings {
    /// Reads the entropy keys of one `catalogs:` entry. `sources` is the parsed registry every
    /// binding's `source` must name.
    pub(crate) fn parse(raw: &RawCatalog, sources: &SourceRegistry) -> Result<Self, InvalidEntropyCatalog> {
        let unread = [
            ("token_file", raw.token_file.is_some()),
            ("dir", raw.dir.is_some()),
            ("data_dir", raw.data_dir.is_some()),
        ];
        if let Some((key, _)) = unread.into_iter().find(|&(_, written)| written) {
            return Err(InvalidEntropyCatalog::Unread { key });
        }
        let auth = match (raw.api_key_file.as_deref(), raw.oidc.as_ref()) {
            (Some(file), None) => EntropyAuth::ApiKey {
                file: absolute("api_key_file", file)?,
            },
            (None, Some(oidc)) => EntropyAuth::Oidc(EntropyOidc::parse(oidc)?),
            _ => return Err(InvalidEntropyCatalog::AuthMode),
        };
        let written = raw.bindings.as_deref().unwrap_or_default();
        if written.is_empty() {
            return Err(InvalidEntropyCatalog::NoBindings);
        }
        let mut bindings: Vec<EntropyBinding> = Vec::with_capacity(written.len());
        for (position, binding) in written.iter().enumerate() {
            let binding =
                EntropyBinding::parse(binding, sources).map_err(|cause| InvalidEntropyCatalog::Binding { position, cause })?;
            if let Some(first) = bindings.iter().position(|seen| seen.server.overlaps(&binding.server)) {
                return Err(InvalidEntropyCatalog::OverlappingBindings { first, second: position });
            }
            bindings.push(binding);
        }
        Ok(Self { auth, bindings })
    }

    pub const fn auth(&self) -> &EntropyAuth {
        &self.auth
    }

    /// Every binding, in declaration order; never empty, and no two can match one server.
    pub fn bindings(&self) -> &[EntropyBinding] {
        &self.bindings
    }
}

impl EntropyOidc {
    fn parse(raw: &RawEntropyOidc) -> Result<Self, InvalidEntropyCatalog> {
        let token_endpoint = written("oidc.token_endpoint", &raw.token_endpoint)?;
        if token_endpoint.contains('@') {
            return Err(InvalidEntropyCatalog::CredentialsInUrl {
                key: "oidc.token_endpoint",
            });
        }
        Ok(Self {
            token_endpoint,
            client_id: written("oidc.client_id", &raw.client_id)?,
            client_secret_file: absolute("oidc.client_secret_file", &raw.client_secret_file)?,
            scope: raw.scope.clone(),
        })
    }

    /// As written, never derived from the catalog's `endpoint`: the marketplace's authorization
    /// server is on another host than its API.
    pub fn token_endpoint(&self) -> &str {
        &self.token_endpoint
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn client_secret_file(&self) -> &Path {
        &self.client_secret_file
    }

    /// The `scope` asked for, as written; `None` asks for the issuer's default. The marketplace's own
    /// authorization server needs none: the scopes it lists are for its MCP endpoint.
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }
}

impl EntropyBinding {
    fn parse(raw: &RawEntropyBinding, sources: &SourceRegistry) -> Result<Self, InvalidBinding> {
        let server = ServerMatch::parse(&raw.server)?;
        let source = SourceName::parse(raw.source.trim())
            .ok()
            .filter(|alias| sources.get(alias).is_some())
            .ok_or_else(|| InvalidBinding::UnknownSource {
                source_alias: raw.source.clone(),
            })?;
        Ok(Self { server, source })
    }

    pub const fn server(&self) -> &ServerMatch {
        &self.server
    }

    pub const fn source(&self) -> &SourceName {
        &self.source
    }
}

impl ServerMatch {
    fn parse(raw: &BTreeMap<String, String>) -> Result<Self, InvalidBinding> {
        let mut kind = None;
        let mut fields = BTreeMap::new();
        for (name, value) in raw {
            if name.trim().is_empty() || value.trim().is_empty() {
                return Err(InvalidBinding::Blank { field: name.clone() });
            }
            let value = FieldValue(value.clone());
            if name == "type" {
                kind = Some(value);
            } else {
                fields.insert(FieldName(name.clone()), value);
            }
        }
        let kind = kind.ok_or(InvalidBinding::NoType)?;
        if fields.is_empty() {
            return Err(InvalidBinding::NoField);
        }
        Ok(Self { kind, fields })
    }

    /// Whether a server of type `kind` matches, `field` giving each of its fields by name, rendered
    /// as text, or `None` where it has no such field.
    pub fn matches(&self, kind: &str, field: impl Fn(&str) -> Option<String>) -> bool {
        self.kind.0 == kind
            && self
                .fields
                .iter()
                .all(|(name, value)| field(&name.0).is_some_and(|text| text == value.0))
    }

    /// Whether one server can match both: the same type, and no field both list with different values.
    fn overlaps(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self
                .fields
                .iter()
                .all(|(name, value)| other.fields.get(name).is_none_or(|theirs| theirs == value))
    }
}

/// A required text key, trimmed.
fn written(key: &'static str, raw: &str) -> Result<String, InvalidEntropyCatalog> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(InvalidEntropyCatalog::Missing { key });
    }
    Ok(String::from(text))
}

fn absolute(key: &'static str, raw: &str) -> Result<PathBuf, InvalidEntropyCatalog> {
    let path = PathBuf::from(raw.trim());
    if path.is_absolute() {
        return Ok(path);
    }
    Err(InvalidEntropyCatalog::RelativePath {
        key,
        path: String::from(raw),
    })
}

#[cfg(test)]
mod tests;

//! `catalog.kind: entropy`: how sutura signs in to a data product marketplace, and which declared
//! source serves each server its data contracts name.
//!
//! The endpoint and `metric_property` are [`super::CatalogSettings`]' own fields, checked there the
//! way a `datahub` entry's are. The token endpoint and `scope` are held as written and parsed by the
//! crate that sends them - **so an unusable one loads here and is refused only by a build that links
//! the reader**, which no build does yet.

use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

use sutura_domain::model::SourceName;

use crate::raw::{RawCatalog, RawEntropyBinding, RawEntropyOidc};
use crate::sources::placement::SourcePlacement;
use crate::sources::{SourceKind, SourceRegistry};

/// A `kind: entropy` entry's sign-in and bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntropySettings {
    auth: EntropyAuth,
    bindings: Vec<EntropyBinding>,
}

/// How sutura signs in. A deployment declares exactly one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntropyAuth {
    /// `api_key_file`: a key sent as `x-api-key`. It reads whatever its owner - an organization, a
    /// team or a user - can see.
    ApiKey { file: PathBuf },
    /// `oidc`: the client credentials grant, against the marketplace's own token endpoint or an
    /// external issuer's. An agent client reads only the endpoints open to agents, data products and
    /// contracts among them. **An external issuer's `aud` is checked only when the marketplace
    /// registers that issuer with an audience**, which nothing here can see.
    Oidc(EntropyOidc),
}

/// The `oidc` block. The secret is read from `client_secret_file` at boot, never written here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntropyOidc {
    token_endpoint: String,
    client_id: String,
    client_secret_file: PathBuf,
    client_auth: EntropyClientAuth,
    scope: Option<String>,
}

/// How the client authenticates to the token endpoint (RFC 6749, section 2.3.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntropyClientAuth {
    /// HTTP Basic, which every authorization server must support.
    #[default]
    Basic,
    /// The client in the form body.
    Post,
}

/// One server a contract may name, and the declared source that serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntropyBinding {
    server: ServerKey,
    source: SourceName,
}

/// The fields that identify a contract's server, per server `type`. `cloudsql` has none: no source
/// kind dials it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerKey {
    BigQuery {
        project: String,
        dataset: String,
    },
    /// `type: postgres` or `type: postgresql`.
    Postgres {
        host: String,
        port: NonZeroU16,
        database: String,
        schema: String,
    },
    ClickHouse {
        host: String,
        port: NonZeroU16,
        database: String,
    },
    /// `service_name` is the contract's `serviceName`.
    Oracle {
        host: String,
        port: NonZeroU16,
        service_name: String,
    },
    Duckdb {
        database: String,
        schema: Option<String>,
    },
}

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
    /// Two bindings for one server, so which source serves it would depend on list order.
    #[error("`bindings[{first}]` and `bindings[{second}]` bind the same server - keep one")]
    DuplicateBinding { first: usize, second: usize },
}

/// Why one `bindings:` entry is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidBinding {
    #[error("`type: {found}` is not bindable - one of: bigquery, postgres, postgresql, clickhouse, oracle, duckdb")]
    UnsupportedType { found: String },
    #[error("a `type: {system}` server is identified by `{key}`, which is empty or absent")]
    MissingKey { system: String, key: &'static str },
    #[error("`{key}` does not identify a `type: {system}` server - remove it")]
    NotAKey { system: String, key: &'static str },
    #[error("`source: {source_alias}` is not declared under `sources:`")]
    UnknownSource { source_alias: String },
    #[error("`source: {source_alias}` is `kind: {declared}`, which does not serve a `type: {system}` server")]
    OtherKind {
        source_alias: SourceName,
        declared: &'static str,
        system: String,
    },
    /// A Postgres connection reaches one database, so a server in another is not served by it. The
    /// other kinds are not compared: a host or a service name has more than one spelling.
    #[error("`source: {source_alias}` dials database `{dialed}`, and the server is in `{written}`")]
    OtherDatabase {
        source_alias: SourceName,
        written: String,
        dialed: String,
    },
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
            if let Some(first) = bindings.iter().position(|seen| seen.server == binding.server) {
                return Err(InvalidEntropyCatalog::DuplicateBinding { first, second: position });
            }
            bindings.push(binding);
        }
        Ok(Self { auth, bindings })
    }

    pub const fn auth(&self) -> &EntropyAuth {
        &self.auth
    }

    /// Every binding, in declaration order; never empty.
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
            client_auth: raw.client_auth,
            scope: raw.scope.clone(),
        })
    }

    pub fn token_endpoint(&self) -> &str {
        &self.token_endpoint
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn client_secret_file(&self) -> &Path {
        &self.client_secret_file
    }

    pub const fn client_auth(&self) -> EntropyClientAuth {
        self.client_auth
    }

    /// The `scope` asked for, as written; `None` asks for the issuer's default.
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }
}

impl EntropyBinding {
    fn parse(raw: &RawEntropyBinding, sources: &SourceRegistry) -> Result<Self, InvalidBinding> {
        let system = raw.server_type.trim();
        let need = |key: &'static str, value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(String::from)
                .ok_or_else(|| InvalidBinding::MissingKey {
                    system: String::from(system),
                    key,
                })
        };
        let port = || {
            raw.port.ok_or_else(|| InvalidBinding::MissingKey {
                system: String::from(system),
                key: "port",
            })
        };
        let server = match system {
            "bigquery" => ServerKey::BigQuery {
                project: need("project", &raw.project)?,
                dataset: need("dataset", &raw.dataset)?,
            },
            "postgres" | "postgresql" => ServerKey::Postgres {
                host: need("host", &raw.host)?,
                port: port()?,
                database: need("database", &raw.database)?,
                schema: need("schema", &raw.schema)?,
            },
            "clickhouse" => ServerKey::ClickHouse {
                host: need("host", &raw.host)?,
                port: port()?,
                database: need("database", &raw.database)?,
            },
            "oracle" => ServerKey::Oracle {
                host: need("host", &raw.host)?,
                port: port()?,
                service_name: need("service_name", &raw.service_name)?,
            },
            "duckdb" => ServerKey::Duckdb {
                database: need("database", &raw.database)?,
                schema: raw.schema.clone(),
            },
            _ => {
                return Err(InvalidBinding::UnsupportedType {
                    found: String::from(system),
                });
            }
        };
        let (kind, keys) = server.shape();
        let written = [
            ("project", raw.project.is_some()),
            ("dataset", raw.dataset.is_some()),
            ("host", raw.host.is_some()),
            ("port", raw.port.is_some()),
            ("database", raw.database.is_some()),
            ("schema", raw.schema.is_some()),
            ("service_name", raw.service_name.is_some()),
        ];
        if let Some((key, _)) = written.into_iter().find(|&(key, on)| on && !keys.contains(&key)) {
            return Err(InvalidBinding::NotAKey {
                system: String::from(system),
                key,
            });
        }
        let declared = SourceName::parse(raw.source.trim())
            .ok()
            .and_then(|alias| sources.get(&alias).map(|configured| (alias, configured)));
        let Some((source, configured)) = declared else {
            return Err(InvalidBinding::UnknownSource {
                source_alias: raw.source.clone(),
            });
        };
        if configured.kind() != kind {
            return Err(InvalidBinding::OtherKind {
                source_alias: source,
                declared: configured.kind().as_str(),
                system: String::from(system),
            });
        }
        if let (ServerKey::Postgres { database, .. }, SourcePlacement::Postgres { database: dialed, .. }) =
            (&server, configured.placement())
            && database != dialed
        {
            return Err(InvalidBinding::OtherDatabase {
                source_alias: source,
                written: database.clone(),
                dialed: dialed.clone(),
            });
        }
        Ok(Self { server, source })
    }

    pub const fn server(&self) -> &ServerKey {
        &self.server
    }

    pub const fn source(&self) -> &SourceName {
        &self.source
    }
}

impl ServerKey {
    /// The source kind that serves this server, and every key its `type` reads.
    const fn shape(&self) -> (SourceKind, &'static [&'static str]) {
        match self {
            Self::BigQuery { .. } => (SourceKind::BigQuery, &["project", "dataset"]),
            Self::Postgres { .. } => (SourceKind::Postgres, &["host", "port", "database", "schema"]),
            Self::ClickHouse { .. } => (SourceKind::ClickHouse, &["host", "port", "database"]),
            Self::Oracle { .. } => (SourceKind::Oracle, &["host", "port", "service_name"]),
            Self::Duckdb { .. } => (SourceKind::Duckdb, &["database", "schema"]),
        }
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

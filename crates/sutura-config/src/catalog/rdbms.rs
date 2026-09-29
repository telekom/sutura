//! The `catalog.kind: rdbms` entry's own settings.
//!
//! A read-only Postgres connection, a closed-form live-row predicate, the environment key that
//! selects dictionary rows, the source the described objects are served from, and two read bounds.
//!
//! Authored as its own module, not a split. The parent re-exports every type, so
//! `crate::catalog::<Name>` and the crate root's `pub use` both still resolve.
//!
//! **Parsed here, read by a composition root that links the `live` reader.** This crate's own job
//! ends at a checked declaration; a build with `sutura-catalog-rdbms`'s `live` feature consumes
//! these values into a reader over a Postgres documentation schema, and a build without it refuses
//! `kind: rdbms` by name. Every value below is a checked declaration either way.
//!
//! The `connection:` block selects its dialect through a `dialect` key (default `postgres`):
//! `postgres` dials a Postgres documentation schema, `oracle` dials an Oracle dictionary over a
//! plaintext, loopback-confined EZCONNECT connection - the same constraint the `oracle` source
//! kind holds. An Oracle connection is parsed and refused at the composition root as not supported
//! by this build, so the declaration is checked but not yet opened.

use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

use sutura_domain::model::{ColumnName, InvalidIdentifier, SourceName};

use crate::raw::{RawCatalog, RawCatalogConnection, RawLiveRowPredicate};
use crate::sources::SourceRegistry;
use crate::sources::placement::{HostName, InvalidHostName, InvalidOracleServiceName, OracleServiceName, PostgresDial};
use crate::sources::transport::{InvalidTransport, SourceTransport, UnsafeChannel};

/// The `kind: rdbms`-only half of a catalog entry, all or nothing.
///
/// An rdbms entry carries every required field, and [`crate::catalog::CatalogSettings`] holds this
/// as one `Option`, so no state with half of it set is representable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RdbmsSettings {
    environment: CatalogEnvironment,
    live_row_predicate: Option<LiveRowPredicate>,
    source_alias: SourceName,
    dictionary_schema: Option<DocumentationSchema>,
    max_dictionary_rows: Option<NonZeroU64>,
    max_dictionary_bytes: Option<NonZeroU64>,
    dictionary_source: DictionarySource,
    connection: CatalogConnection,
}

impl RdbmsSettings {
    /// Reads the rdbms keys of one `catalogs:` entry. `name` is the entry's own, for the transport
    /// refusals; `sources` is the parsed registry, so a `source_alias` naming no declared source is
    /// refused here rather than at the composition root.
    pub(crate) fn parse(name: &SourceName, raw: &RawCatalog, sources: &SourceRegistry) -> Result<Self, InvalidRdbmsCatalog> {
        let environment = CatalogEnvironment::parse(
            raw.environment
                .as_deref()
                .ok_or(InvalidRdbmsCatalog::Missing { key: "environment" })?,
        )?;
        let live_row_predicate = raw.live_row_predicate.as_ref().map(LiveRowPredicate::parse).transpose()?;
        let alias = raw
            .source_alias
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or(InvalidRdbmsCatalog::Missing { key: "source_alias" })?;
        let source_alias = SourceName::parse(alias).map_err(|cause| InvalidRdbmsCatalog::SourceAlias { cause })?;
        if sources.get(&source_alias).is_none() {
            return Err(InvalidRdbmsCatalog::UnknownSourceAlias { alias: source_alias });
        }
        let connection = raw
            .connection
            .as_ref()
            .ok_or(InvalidRdbmsCatalog::Missing { key: "connection" })?;
        let connection = CatalogConnection::parse(name, connection).map_err(|cause| InvalidRdbmsCatalog::Connection { cause })?;
        let dictionary_schema = raw
            .dictionary_schema
            .as_deref()
            .map(DocumentationSchema::parse)
            .transpose()
            .map_err(|cause| InvalidRdbmsCatalog::DictionarySchema { cause })?;
        let dictionary_source = DictionarySource::parse(raw.dictionary_source.as_deref())?;
        Ok(Self {
            environment,
            live_row_predicate,
            source_alias,
            dictionary_schema,
            max_dictionary_rows: non_zero("max_dictionary_rows", raw.max_dictionary_rows)?,
            max_dictionary_bytes: non_zero("max_dictionary_bytes", raw.max_dictionary_bytes)?,
            dictionary_source,
            connection,
        })
    }

    #[inline]
    #[must_use]
    pub const fn environment(&self) -> &CatalogEnvironment {
        &self.environment
    }

    #[inline]
    #[must_use]
    pub const fn live_row_predicate(&self) -> Option<&LiveRowPredicate> {
        self.live_row_predicate.as_ref()
    }

    #[inline]
    #[must_use]
    pub const fn source_alias(&self) -> &SourceName {
        &self.source_alias
    }

    /// The schema holding the dictionary rows, or `None` for the reader's documented default.
    #[inline]
    #[must_use]
    pub const fn dictionary_schema(&self) -> Option<&DocumentationSchema> {
        self.dictionary_schema.as_ref()
    }

    /// The declared row cap, or `None` for the reader's own default.
    #[inline]
    #[must_use]
    pub const fn max_dictionary_rows(&self) -> Option<NonZeroU64> {
        self.max_dictionary_rows
    }

    /// The declared byte cap, or `None` for the reader's own default.
    #[inline]
    #[must_use]
    pub const fn max_dictionary_bytes(&self) -> Option<NonZeroU64> {
        self.max_dictionary_bytes
    }

    #[inline]
    #[must_use]
    pub const fn connection(&self) -> &CatalogConnection {
        &self.connection
    }

    /// Which dictionary the reader reads: the documentation schema (default) or the database's
    /// own dictionary views. Selected by the `dictionary_source` key on an `rdbms` catalog.
    #[inline]
    #[must_use]
    pub const fn dictionary_source(&self) -> DictionarySource {
        self.dictionary_source
    }
}

/// A written zero is refused rather than stored: it would refuse every read rather than bound one,
/// and absent is how "the reader's default" is written.
fn non_zero(key: &'static str, written: Option<u64>) -> Result<Option<NonZeroU64>, InvalidRdbmsCatalog> {
    written
        .map(|value| NonZeroU64::new(value).ok_or(InvalidRdbmsCatalog::ZeroBound { key }))
        .transpose()
}

/// The closed-form predicate a dictionary row must satisfy to be live.
///
/// A typed column, an operator from a closed set, and a value only `equals` reads - never a SQL
/// string. The column parses through [`ColumnName`], so it holds nothing that would need escaping;
/// the value is arbitrary text, and **a reader must bind it as a parameter, never interpolate it.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRowPredicate {
    column: ColumnName,
    operator: PredicateOperator,
    value: Option<String>,
}

impl LiveRowPredicate {
    fn parse(raw: &RawLiveRowPredicate) -> Result<Self, InvalidRdbmsCatalog> {
        let column = ColumnName::parse(&raw.column).map_err(|cause| InvalidRdbmsCatalog::PredicateColumn { cause })?;
        let operator = PredicateOperator::parse(&raw.operator)?;
        let value = match (operator, &raw.value) {
            (PredicateOperator::Equals, None) => return Err(InvalidRdbmsCatalog::PredicateValueMissing),
            (PredicateOperator::Equals, Some(value)) => Some(value.clone()),
            (PredicateOperator::IsNull | PredicateOperator::IsNotNull, Some(_)) => {
                return Err(InvalidRdbmsCatalog::PredicateValueUnexpected {
                    operator: operator.as_str(),
                });
            }
            (PredicateOperator::IsNull | PredicateOperator::IsNotNull, None) => None,
        };
        Ok(Self { column, operator, value })
    }

    #[inline]
    #[must_use]
    pub const fn column(&self) -> &ColumnName {
        &self.column
    }

    #[inline]
    #[must_use]
    pub const fn operator(&self) -> PredicateOperator {
        self.operator
    }

    /// The comparison value, `Some` exactly when the operator is [`PredicateOperator::Equals`].
    #[inline]
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
}

/// The operators a [`LiveRowPredicate`] may use. A closed set: a new one is a visible diff plus a
/// reader arm, which is the review it deserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateOperator {
    IsNull,
    IsNotNull,
    Equals,
}

impl PredicateOperator {
    /// Every operator, so the parser and the refusal's list cannot disagree.
    pub const ALL: [Self; 3] = [Self::IsNull, Self::IsNotNull, Self::Equals];

    fn parse(word: &str) -> Result<Self, InvalidRdbmsCatalog> {
        let word = word.trim();
        Self::ALL
            .into_iter()
            .find(|operator| operator.as_str() == word)
            .ok_or_else(|| InvalidRdbmsCatalog::UnknownPredicateOperator {
                found: String::from(word),
            })
    }

    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IsNull => "is_null",
            Self::IsNotNull => "is_not_null",
            Self::Equals => "equals",
        }
    }

    fn known() -> String {
        Self::ALL.map(Self::as_str).join(", ")
    }
}

/// A non-empty environment key. Empty would select no dictionary rows, silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEnvironment(String);

impl CatalogEnvironment {
    fn parse(written: &str) -> Result<Self, InvalidRdbmsCatalog> {
        let trimmed = written.trim();
        if trimmed.is_empty() {
            return Err(InvalidRdbmsCatalog::EmptyEnvironment);
        }
        Ok(Self(String::from(trimmed)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A typed PostgreSQL schema name holding the documentation rows.
///
/// Binding a schema name as a quoted, validated identifier - never interpolating it into a SQL
/// statement as raw text. The accepted set is `[A-Za-z0-9_]`, the same leaves every Postgres
/// identifier is built from, and case is preserved. The reader selects the view `columns` in this
/// schema; the schema of each described object is the row's own `schema_name` and `catalog_name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentationSchema(String);

impl DocumentationSchema {
    /// Reads a declared documentation schema.
    fn parse(written: &str) -> Result<Self, InvalidDocumentationSchema> {
        let trimmed = written.trim();
        if trimmed.is_empty() {
            return Err(InvalidDocumentationSchema::Empty);
        }
        if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(InvalidDocumentationSchema::NotIdentifier);
        }
        Ok(Self(String::from(trimmed)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a declared documentation schema is not usable.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDocumentationSchema {
    #[error("`dictionary_schema` is empty")]
    Empty,
    #[error("`dictionary_schema` is not a schema identifier - it must be letters, digits and underscores")]
    NotIdentifier,
}

/// Which dictionary an `rdbms` catalog reads.
///
/// A setting parsed but read by no composition root in this build. `documentation_schema` (the default) is the supported shape;
/// `native_dictionary` is parsed as an opt-in declaration but not opened. A closed set: a new one is a visible diff plus a
/// parse arm and a reader arm, which is the review it deserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictionarySource {
    /// A `columns` documentation-schema view installed in the database (the default).
    DocumentationSchema,
    /// The database's own dictionary views (`ALL_TAB_COLUMNS` etc.).
    NativeDictionary,
}

impl DictionarySource {
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const ALL: [Self; 2] = [Self::DocumentationSchema, Self::NativeDictionary];

    /// The default spelling, so [`crate::raw::RawCatalog`] can default the field without naming the
    /// variant in two places.
    pub const DEFAULT: &'static str = "documentation_schema";

    fn parse(written: Option<&str>) -> Result<Self, InvalidRdbmsCatalog> {
        let word = written.unwrap_or(Self::DEFAULT).trim();
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == word)
            .ok_or_else(|| InvalidRdbmsCatalog::DictionarySourceUnknown {
                found: String::from(word),
            })
    }

    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DocumentationSchema => "documentation_schema",
            Self::NativeDictionary => "native_dictionary",
        }
    }

    fn known() -> String {
        Self::ALL.map(Self::as_str).join(", ")
    }
}

/// An rdbms catalog's own read-only connection, in the dialect its `connection.dialect` key
/// selected.
///
/// **Default `postgres`**, so a deployment that wrote no `dialect` key reads exactly as it did
/// before the Oracle variant existed. The variant is the dispatch point: a Postgres connection
/// dials a documentation schema over a `PostgresDial`, and an Oracle connection dials a
/// dictionary over plaintext EZCONNECT, loopback-confined. The composition root matches on the
/// variant to build the reader; until the Oracle reader lands (PR 2 of
/// `github.com/telekom/sutura#972`), an `Oracle` variant is parsed and refused there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogConnection {
    /// A Postgres documentation-schema connection, the default dialect.
    Postgres(PostgresCatalogConnection),
    /// An Oracle dictionary connection: plaintext, loopback-confined EZCONNECT.
    Oracle(OracleCatalogConnection),
}

impl CatalogConnection {
    fn parse(name: &SourceName, raw: &RawCatalogConnection) -> Result<Self, InvalidConnection> {
        let dialect = Dialect::parse(raw.dialect.as_deref())?;
        match dialect {
            Dialect::Postgres => PostgresCatalogConnection::parse(name, raw).map(Self::Postgres),
            Dialect::Oracle => OracleCatalogConnection::parse(name, raw).map(Self::Oracle),
        }
    }
}

/// The dialect a `connection:` block names, defaulting to Postgres.
///
/// A closed set: a new one is a visible diff plus a parse arm, which is the review it deserves.
/// `postgres` is the default so an existing deployment that wrote no `dialect` key reads the same
/// connection it always did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// The Postgres documentation-schema reader - the default and the precedent.
    Postgres,
    /// An Oracle dictionary reader, plaintext EZCONNECT over loopback.
    Oracle,
}

impl Dialect {
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const ALL: [Self; 2] = [Self::Postgres, Self::Oracle];

    /// The default spelling, so [`RawCatalogConnection`] can default the field without naming the
    /// variant in two places.
    pub const DEFAULT: &'static str = "postgres";

    fn parse(written: Option<&str>) -> Result<Self, InvalidConnection> {
        let word = written.unwrap_or(Self::DEFAULT).trim();
        Self::ALL
            .into_iter()
            .find(|dialect| dialect.as_str() == word)
            .ok_or_else(|| InvalidConnection::UnknownDialect {
                found: String::from(word),
            })
    }

    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Oracle => "oracle",
        }
    }

    fn known() -> String {
        Self::ALL.map(Self::as_str).join(", ")
    }
}

/// A Postgres documentation-schema connection - the existing shape, now one variant.
///
/// The same parsed types a `sources:` Postgres entry holds ([`PostgresDial`], [`SourceTransport`])
/// and the same fail-closed channel rules, through the one function both call:
/// [`crate::sources::transport::refuse_unsafe_postgres_channel`]. **No password is held here:**
/// `password_file` is a path the composition root reads at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresCatalogConnection {
    dial: PostgresDial,
    database: String,
    user: String,
    password_file: PathBuf,
    transport: SourceTransport,
}

impl PostgresCatalogConnection {
    fn parse<'raw>(name: &SourceName, raw: &'raw RawCatalogConnection) -> Result<Self, InvalidConnection> {
        let required =
            |key: &'static str, value: &'raw Option<String>| written(value.as_deref()).ok_or(InvalidConnection::Missing { key });
        if written(raw.service_name.as_deref()).is_some() {
            return Err(InvalidConnection::ForeignKey {
                key: "service_name",
                dialect: "postgres",
            });
        }
        let port = raw.port.ok_or(InvalidConnection::Missing { key: "port" })?;
        let dial = match (written(raw.host.as_deref()), written(raw.unix_socket.as_deref())) {
            (Some(_), Some(_)) => return Err(InvalidConnection::HostAndUnixSocket),
            (None, None) => {
                return Err(InvalidConnection::Missing {
                    key: "host_or_unix_socket",
                });
            }
            (Some(host), None) => PostgresDial::Tcp {
                host: HostName::parse(host).map_err(|cause| InvalidConnection::Host { cause })?,
                port,
            },
            (None, Some(directory)) => PostgresDial::UnixSocket {
                directory: absolute("unix_socket", directory)?,
                port,
            },
        };
        let database = required("database", &raw.database)?.to_owned();
        let user = required("user", &raw.user)?.to_owned();
        let password_file = absolute("password_file", required("password_file", &raw.password_file)?)?;
        let transport = crate::sources::transport::parse(
            name,
            required("transport_mode", &raw.transport_mode)?,
            raw.transport_anchors.as_deref(),
            raw.client_certificate.as_deref(),
            raw.client_key.as_deref(),
        )
        .map_err(|cause| InvalidConnection::Transport { cause })?;
        crate::sources::transport::refuse_unsafe_postgres_channel(&dial, &transport)
            .map_err(|cause| InvalidConnection::Channel { cause })?;
        Ok(Self {
            dial,
            database,
            user,
            password_file,
            transport,
        })
    }

    #[inline]
    #[must_use]
    pub const fn dial(&self) -> &PostgresDial {
        &self.dial
    }

    #[inline]
    #[must_use]
    pub fn database(&self) -> &str {
        &self.database
    }

    #[inline]
    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }

    #[inline]
    #[must_use]
    pub fn password_file(&self) -> &Path {
        &self.password_file
    }

    #[inline]
    #[must_use]
    pub const fn transport(&self) -> &SourceTransport {
        &self.transport
    }
}

/// An Oracle dictionary connection: plaintext EZCONNECT, loopback-confined.
///
/// **The same stand the `oracle` source kind makes** (`crate::sources::oracle`): the pinned
/// driver builds its own TLS configuration and takes no caller-built one, so `verified` and
/// `mutual` transport modes are refused (not wired) and the declared `host` is confined to a
/// loopback literal through [`crate::sources::transport::refuse_remote_plaintext`]. The driver
/// names the database by `service_name` rather than `database`, so there is no `database` field.
/// **No password is held here:** `password_file` is a path the composition root reads at boot.
///
/// **Limit, next to the claim:** this confines the first dial, not the connection - the pinned
/// driver follows a listener's TNS redirect to whatever address it names, still plaintext, with
/// no option to refuse. The same limit the `oracle` source kind documents, held by the same cell
/// there (`a_listener_redirect_is_followed_to_an_address_nobody_declared`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleCatalogConnection {
    host: HostName,
    port: u16,
    service_name: OracleServiceName,
    user: String,
    password_file: PathBuf,
}

impl OracleCatalogConnection {
    fn parse<'raw>(name: &SourceName, raw: &'raw RawCatalogConnection) -> Result<Self, InvalidConnection> {
        let required =
            |key: &'static str, value: &'raw Option<String>| written(value.as_deref()).ok_or(InvalidConnection::Missing { key });
        if written(raw.database.as_deref()).is_some() {
            return Err(InvalidConnection::ForeignKey {
                key: "database",
                dialect: "oracle",
            });
        }
        if written(raw.unix_socket.as_deref()).is_some() {
            return Err(InvalidConnection::ForeignKey {
                key: "unix_socket",
                dialect: "oracle",
            });
        }
        let host = HostName::parse(required("host", &raw.host)?).map_err(|cause| InvalidConnection::Host { cause })?;
        let port = raw.port.ok_or(InvalidConnection::Missing { key: "port" })?;
        let service_name = OracleServiceName::parse(required("service_name", &raw.service_name)?)
            .map_err(|cause| InvalidConnection::OracleServiceName { cause })?;
        let user = required("user", &raw.user)?.to_owned();
        let password_file = absolute("password_file", required("password_file", &raw.password_file)?)?;
        let transport = crate::sources::transport::parse(
            name,
            required("transport_mode", &raw.transport_mode)?,
            raw.transport_anchors.as_deref(),
            raw.client_certificate.as_deref(),
            raw.client_key.as_deref(),
        )
        .map_err(|cause| InvalidConnection::Transport { cause })?;
        if transport != SourceTransport::Plaintext {
            return Err(InvalidConnection::TlsNotDeliverable {
                mode: transport.describe(),
            });
        }
        crate::sources::transport::refuse_remote_plaintext(&host, &transport)
            .map_err(|cause| InvalidConnection::Channel { cause })?;
        Ok(Self {
            host,
            port,
            service_name,
            user,
            password_file,
        })
    }

    #[inline]
    #[must_use]
    pub const fn host(&self) -> &HostName {
        &self.host
    }

    #[inline]
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    #[inline]
    #[must_use]
    pub const fn service_name(&self) -> &OracleServiceName {
        &self.service_name
    }

    #[inline]
    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }

    #[inline]
    #[must_use]
    pub fn password_file(&self) -> &Path {
        &self.password_file
    }
}

/// Written text, trimmed; empty reads as absent, as everywhere else in this crate.
fn written(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

fn absolute(key: &'static str, written: &str) -> Result<PathBuf, InvalidConnection> {
    let path = PathBuf::from(written);
    if path.is_relative() {
        return Err(InvalidConnection::RelativePath { key });
    }
    Ok(path)
}

/// Why an rdbms catalog's own keys are not usable.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRdbmsCatalog {
    #[error("`{key}` is required for `kind: rdbms` and was not written")]
    Missing { key: &'static str },
    #[error("`environment` is empty - write the key that selects this deployment's dictionary rows")]
    EmptyEnvironment,
    #[error("`source_alias` is not a source name")]
    SourceAlias {
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("`source_alias` is `{alias}`, which names no declared source")]
    UnknownSourceAlias { alias: SourceName },
    #[error("`dictionary_schema` is not usable: {cause}")]
    DictionarySchema {
        #[source]
        cause: InvalidDocumentationSchema,
    },
    #[error("`live_row_predicate.column` is not a column name")]
    PredicateColumn {
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("`live_row_predicate.operator` is `{found}`, not one of: {}", PredicateOperator::known())]
    UnknownPredicateOperator { found: String },
    #[error("`live_row_predicate.operator` is `equals` and no `value` was written")]
    PredicateValueMissing,
    #[error("`live_row_predicate.operator` is `{operator}`, which reads no `value` - remove it")]
    PredicateValueUnexpected { operator: &'static str },
    #[error("`{key}` is 0, which would refuse every read rather than bound one - remove it for the reader's default")]
    ZeroBound { key: &'static str },
    #[error("`connection` is not usable")]
    Connection {
        #[source]
        cause: InvalidConnection,
    },
    #[error("`dictionary_source` is `{found}`, not one of: {}", DictionarySource::known())]
    DictionarySourceUnknown { found: String },
}

/// Why an rdbms catalog's `connection:` block is not usable.
///
/// **Limit:** an [`InvalidConnection::Transport`] cause is the `sources:` transport reader's own, so
/// its message names the key under `sources.<catalog name>` rather than under this block.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidConnection {
    #[error("`connection.{key}` is required and was not written")]
    Missing { key: &'static str },
    #[error("`connection.dialect` is `{found}`, not one of: {}", Dialect::known())]
    UnknownDialect { found: String },
    #[error("`connection` declares both `host` and `unix_socket` - write one")]
    HostAndUnixSocket,
    #[error("`connection.{key}` is relative - write an absolute path")]
    RelativePath { key: &'static str },
    #[error("`connection.host` is not a host sutura can dial")]
    Host {
        #[source]
        cause: InvalidHostName,
    },
    #[error("`connection` declares a transport sutura cannot use")]
    Transport {
        #[source]
        cause: InvalidTransport,
    },
    #[error("`connection` declares a channel sutura refuses")]
    Channel {
        #[source]
        cause: UnsafeChannel,
    },
    #[error("`connection.service_name` is not a service name the driver would read as written")]
    OracleServiceName {
        #[source]
        cause: InvalidOracleServiceName,
    },
    #[error("`connection.{key}` is read only when `dialect` is `{dialect}` - remove it")]
    ForeignKey { key: &'static str, dialect: &'static str },
    #[error(
        "`connection` is `dialect: oracle` and declares transport mode `{mode}`, but the Oracle driver builds its own TLS - only `plaintext` is accepted"
    )]
    TlsNotDeliverable { mode: &'static str },
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::path::Path;

    use super::{CatalogConnection, InvalidConnection, InvalidDocumentationSchema, InvalidRdbmsCatalog, PredicateOperator};
    use crate::environment::Environment;
    use crate::raw::RawCatalogConnection;
    use crate::settings::{Settings, Sources};
    use sutura_domain::model::SourceName;

    #[test]
    fn a_remote_plaintext_catalog_connection_is_refused() {
        let raw = RawCatalogConnection {
            dialect: None,
            host: Some(String::from("192.0.2.1")),
            unix_socket: None,
            port: Some(5432),
            service_name: None,
            database: Some(String::from("dictionary")),
            user: Some(String::from("reader")),
            password_file: Some(String::from("/run/secrets/dictionary")),
            transport_mode: Some(String::from("plaintext")),
            transport_anchors: None,
            client_certificate: None,
            client_key: None,
        };
        let name = SourceName::parse("dictionary").expect("a catalog name parses");
        let error = CatalogConnection::parse(&name, &raw).expect_err("remote plaintext is unsafe");
        assert!(matches!(error, InvalidConnection::Channel { .. }), "{error:?}");
    }

    #[test]
    fn a_complete_rdbms_entry_reaches_the_catalog_settings() {
        let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
            catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
            environment: prod\n    source_alias: warehouse\n    max_dictionary_rows: 1000\n    \
            live_row_predicate:\n      column: status\n      operator: equals\n      value: live\n    \
            connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
            password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
        let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
            .expect("a complete rdbms catalog loads");
        let catalog = settings.catalogs().each().next().expect("one catalog");
        let rdbms = catalog.rdbms().expect("an rdbms entry carries its rdbms settings");
        assert_eq!(rdbms.environment().as_str(), "prod");
        assert_eq!(rdbms.source_alias().as_str(), "warehouse");
        assert_eq!(rdbms.max_dictionary_rows(), NonZeroU64::new(1000));
        assert_eq!(rdbms.max_dictionary_bytes(), None);
        let predicate = rdbms.live_row_predicate().expect("a declared predicate");
        assert_eq!(predicate.column().as_str(), "status");
        assert_eq!(predicate.operator(), PredicateOperator::Equals);
        assert_eq!(predicate.value(), Some("live"));
        let connection = match rdbms.connection() {
            CatalogConnection::Postgres(p) => p,
            CatalogConnection::Oracle(_) => panic!("the overlay declares no dialect"),
        };
        assert_eq!(connection.database(), "dictionary");
        assert_eq!(connection.user(), "reader");
        assert_eq!(connection.password_file(), Path::new("/run/secrets/dictionary"));
        assert_eq!(connection.transport().describe(), "plaintext");
    }

    /// An empty `dictionary_schema` is refused as `InvalidDocumentationSchema::Empty` through
    /// `Settings::load` - the public path a declaration takes.
    #[test]
    fn an_empty_dictionary_schema_is_refused() {
        let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
            catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
            environment: prod\n    source_alias: warehouse\n    dictionary_schema: \"\"\n    \
            connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
            password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
        let result = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay));
        assert!(result.is_err(), "an empty dictionary schema is refused");
        let error = result.unwrap_err();
        let mut source: Option<&dyn std::error::Error> = Some(error.reason());
        let mut found = false;
        while let Some(cause) = source {
            if let Some(schema) = cause.downcast_ref::<InvalidRdbmsCatalog>() {
                let InvalidRdbmsCatalog::DictionarySchema { cause } = schema else {
                    panic!("expected DictionarySchema, got {schema:?}");
                };
                assert!(matches!(cause, InvalidDocumentationSchema::Empty), "{cause:?}");
                found = true;
            }
            source = cause.source();
        }
        assert!(found, "the error chain never reached InvalidRdbmsCatalog::DictionarySchema");
    }

    /// A `dictionary_schema` that is not an identifier is refused as
    /// `InvalidDocumentationSchema::NotIdentifier` through `Settings::load`.
    #[test]
    fn a_non_identifier_dictionary_schema_is_refused() {
        let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
            catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
            environment: prod\n    source_alias: warehouse\n    dictionary_schema: bad-name\n    \
            connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
            password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
        let result = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay));
        assert!(result.is_err(), "a non-identifier dictionary schema is refused");
        let error = result.unwrap_err();
        let mut source: Option<&dyn std::error::Error> = Some(error.reason());
        let mut found = false;
        while let Some(cause) = source {
            if let Some(schema) = cause.downcast_ref::<InvalidRdbmsCatalog>() {
                let InvalidRdbmsCatalog::DictionarySchema { cause } = schema else {
                    panic!("expected DictionarySchema, got {schema:?}");
                };
                assert!(matches!(cause, InvalidDocumentationSchema::NotIdentifier), "{cause:?}");
                found = true;
            }
            source = cause.source();
        }
        assert!(found, "the error chain never reached InvalidRdbmsCatalog::DictionarySchema");
    }

    /// A valid `dictionary_schema` containing an underscore is accepted through `Settings::load`,
    /// pinning the accept side of the same `[A-Za-z0-9_]` rule the two refusal cells above pin the
    /// refusal side of.
    #[test]
    fn a_valid_dictionary_schema_with_an_underscore_is_accepted() {
        let overlay = "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
            catalogs:\n  - name: dict\n    kind: rdbms\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n    \
            environment: prod\n    source_alias: warehouse\n    dictionary_schema: docs_v1\n    \
            connection:\n      host: 127.0.0.1\n      port: 5432\n      database: dictionary\n      user: reader\n      \
            password_file: /run/secrets/dictionary\n      transport_mode: plaintext\n";
        let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
            .expect("a dictionary_schema with an underscore is accepted");
        let catalog = settings.catalogs().each().next().expect("one catalog");
        let rdbms = catalog.rdbms().expect("an rdbms entry carries its rdbms settings");
        let schema = rdbms.dictionary_schema().expect("dictionary_schema was declared");
        assert_eq!(schema.as_str(), "docs_v1");
    }
}

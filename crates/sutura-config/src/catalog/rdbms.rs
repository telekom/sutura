//! The `catalog.kind: rdbms` entry's own settings.
//!
//! A read-only Postgres connection, a closed-form live-row predicate, the environment key that
//! selects dictionary rows, the source the described objects are served from, and two read bounds.
//!
//! Split out of `catalog.rs` at the crate's 1000-line cap. The parent re-exports every type, so
//! `crate::catalog::<Name>` and the crate root's `pub use` both still resolve.
//!
//! **Parsed, not read:** nothing in this repository opens an rdbms catalog yet. The composition
//! root still refuses `kind: rdbms` by name, so every value here is a checked declaration no reader
//! has consumed.

use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

use sutura_domain::model::{ColumnName, InvalidIdentifier, SourceName};

use crate::raw::{RawCatalog, RawCatalogConnection, RawLiveRowPredicate};
use crate::sources::SourceRegistry;
use crate::sources::placement::{HostName, InvalidHostName, PostgresDial};
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
    max_dictionary_rows: Option<NonZeroU64>,
    max_dictionary_bytes: Option<NonZeroU64>,
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
        Ok(Self {
            environment,
            live_row_predicate,
            source_alias,
            max_dictionary_rows: non_zero("max_dictionary_rows", raw.max_dictionary_rows)?,
            max_dictionary_bytes: non_zero("max_dictionary_bytes", raw.max_dictionary_bytes)?,
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

/// An rdbms catalog's own read-only Postgres connection.
///
/// The same parsed types a `sources:` Postgres entry holds ([`PostgresDial`], [`SourceTransport`])
/// and the same fail-closed channel rules, through the one function both call:
/// [`crate::sources::transport::refuse_unsafe_postgres_channel`]. **No password is held here:**
/// `password_file` is a path the composition root reads at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogConnection {
    dial: PostgresDial,
    database: String,
    user: String,
    password_file: PathBuf,
    transport: SourceTransport,
}

impl CatalogConnection {
    fn parse<'raw>(name: &SourceName, raw: &'raw RawCatalogConnection) -> Result<Self, InvalidConnection> {
        let required =
            |key: &'static str, value: &'raw Option<String>| written(value.as_deref()).ok_or(InvalidConnection::Missing { key });
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
}

/// Why an rdbms catalog's `connection:` block is not usable.
///
/// **Limit:** an [`InvalidConnection::Transport`] cause is the `sources:` transport reader's own, so
/// its message names the key under `sources.<catalog name>` rather than under this block.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidConnection {
    #[error("`connection.{key}` is required and was not written")]
    Missing { key: &'static str },
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
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::path::Path;

    use super::PredicateOperator;
    use crate::environment::Environment;
    use crate::settings::{Settings, Sources};

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
        assert_eq!(rdbms.connection().database(), "dictionary");
        assert_eq!(rdbms.connection().user(), "reader");
        assert_eq!(rdbms.connection().password_file(), Path::new("/run/secrets/dictionary"));
        assert_eq!(rdbms.connection().transport().describe(), "plaintext");
    }
}

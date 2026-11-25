//! Which catalog the service serves, and where the files it reads live.
//!
//! Two directories rather than one, because they are owned by different people. The catalog is
//! authored and reviewed - it is the definitions somebody certified - and the data directory is
//! wherever the files the engine reads happen to be mounted. Conflating them would make a
//! deployment that moved its data look like a catalog change, which is the one thing a pinned
//! bundle exists to make visible.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sutura_domain::catalog::{DefinitionsByteBound, InvalidDefinitionsByteBound};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

pub mod foundry;
pub mod platform;
pub mod rdbms;
pub use crate::catalog::foundry::{FoundryAuth, FoundrySettings, InvalidFoundryCatalog};
pub use crate::catalog::platform::{DataHubPlatform, InvalidDataHubPlatform};
pub use crate::catalog::rdbms::{
    CatalogConnection, CatalogEnvironment, Dialect, DictionarySource, DocumentationSchema, InvalidConnection,
    InvalidDocumentationSchema, InvalidRdbmsCatalog, LiveRowPredicate, MissingEndpointPolicy, OracleCatalogConnection,
    PostgresCatalogConnection, PredicateOperator, RdbmsSettings,
};
pub use crate::client_credentials::ClientCredentialsSettings;
pub(crate) use rdbms::RawColumnRule;

/// Which adapter the catalog configuration names, and therefore which one opens it.
///
/// **A closed set of typed declarations, [`crate::sources::SourceKind`]'s shape on the metadata
/// side.** A DATA source's adapter is chosen by `SourceKind` and dispatched by the composition
/// root's exhaustive match with no wildcard arm; a METADATA source has exactly the same need, and
/// until this type existed the settings tree carried a directory and a version and no word an
/// operator could write to say *read the model from somewhere else* - so a second catalog kind
/// could merge complete and silently remain unreachable from any binary.
///
/// Eight variants. [`Self::Datahub`] says which and why, the way `SourceKind::BigQuery` does for
/// data systems: the vocabulary is the vocabulary of adapters this repository has, and an adapter
/// that exists in a record rather than in a linked crate is still a word an operator might write.
/// `#970` added the three declaring adapters that had a crate and no composition root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogKind {
    /// A directory of markdown documents with YAML frontmatter, read by `sutura-catalog-local`.
    ///
    /// The only kind either composition root can OPEN in this build: the markdown adapter is
    /// linked by `sutura serve` and, since issue #970, by `sutura mcp` over the same `catalogs:`
    /// declaration; `sutura query` still puts it behind its own directory argument.
    Markdown,
    /// A metadata service, read through the adapter `docs/adr/0016` specifies and #114 builds.
    ///
    /// **Openable behind `sutura-cli`'s default-off `datahub` feature; a build without it refuses
    /// this kind by name**, for exactly the reason `SourceKind::BigQuery` is refused: an operator
    /// who writes the word must be told the truth (the adapter is not linked) rather than sent
    /// looking for a typo.
    Datahub,
    /// A directory of OKF Frictionless Table Schema descriptors, read by `sutura-catalog-okf`.
    ///
    /// The narrowest of the three declaring adapters `#970` names, and the only one whose reader
    /// needs no service: one YAML document per table, on disk, like [`Self::Markdown`].
    /// `sutura-catalog-okf` is an unconditional dependency of `sutura serve`, so this kind is
    /// openable by every build of this binary.
    Okf,
    /// A directory of Open Data Contract Standard v3 contract documents, read by
    /// `sutura-catalog-datacontract`.
    ///
    /// The on-disk vocabulary used when interface catalogues are exported as data-contract YAML.
    /// `sutura-catalog-datacontract` is an unconditional dependency of `sutura serve`, so this kind
    /// is openable by every build of this binary.
    DataContract,
    /// An `OpenMetadata` deployment, decided by `sutura-catalog-openmetadata` over its own
    /// `SnapshotReader` port.
    ///
    /// **Openable behind `sutura-cli`'s default-off `openmetadata` feature; a build without it
    /// refuses this kind by name**, for exactly the reason `SourceKind::BigQuery` is refused: an
    /// operator who writes the word must be told the truth (the adapter is not linked) rather than
    /// sent looking for a typo. The HTTP reader (`#152`'s follow-up this arm wires) reads the
    /// deployment's `REST` API and maps it into the crate's recorded `Snapshot` shape.
    Openmetadata,
    /// An RDBMS dictionary, decided by `sutura-catalog-rdbms` over a `DictionaryReader` port.
    ///
    /// **A declarable kind that opens only on a build that links the `rdbms` feature** (which
    /// turns on `sutura-catalog-rdbms`'s `live` reader over a Postgres documentation schema); a
    /// build without it refuses the kind by name naming the feature, the same stand
    /// [`Self::Openmetadata`] makes. The composition root decides which, never the settings parse.
    Rdbms,
    /// A Cube metrics serving layer's metadata API, decided by `sutura-catalog-cube` over its
    /// `MetaReader` port.
    ///
    /// **Openable behind `sutura-cli`'s default-off `cube` feature; a build without it refuses this
    /// kind by name**, the stand [`Self::Openmetadata`] makes. It takes the same reader fields.
    Cube,
    /// **Experimental.** A Foundry ontology's metadata API, decided by `sutura-catalog-foundry` over
    /// its `OntologyReader` port, with the `foundry:` block's own keys ([`FoundrySettings`]).
    ///
    /// Openable behind `sutura-cli`'s default-off `foundry` feature, which no release turns on; a
    /// build without it refuses this kind by name.
    Foundry,
    /// EXPERIMENTAL: an HTTP metadata API that a pinned `OpenAPI` document and a mapping file in
    /// `dir` describe, decided by `sutura-catalog-openapi` over its `AnswerReader` port.
    ///
    /// **Openable behind `sutura-cli`'s default-off `openapi` feature; a build without it refuses
    /// this kind by name**, the stand [`Self::Openmetadata`] makes. It takes the same reader fields.
    OpenApi,
}
/// The configured word did not name a kind of catalog this build has.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` does not name a kind of catalog - one of: {}", CatalogKind::NAMES.join(", "))]
pub struct UnknownCatalogKind {
    found: String,
}

impl CatalogKind {
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &[
        "markdown",
        "datahub",
        "okf",
        "datacontract",
        "openmetadata",
        "rdbms",
        "cube",
        "foundry",
        "openapi",
    ];

    /// Reads the configured word.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownCatalogKind> {
        match raw.as_ref().trim() {
            "markdown" => Ok(Self::Markdown),
            "datahub" => Ok(Self::Datahub),
            "okf" => Ok(Self::Okf),
            "datacontract" => Ok(Self::DataContract),
            "openmetadata" => Ok(Self::Openmetadata),
            "rdbms" => Ok(Self::Rdbms),
            "cube" => Ok(Self::Cube),
            "foundry" => Ok(Self::Foundry),
            "openapi" => Ok(Self::OpenApi),
            other => Err(UnknownCatalogKind {
                found: String::from(other),
            }),
        }
    }

    /// The spelling, for the startup log.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Datahub => "datahub",
            Self::Okf => "okf",
            Self::DataContract => "datacontract",
            Self::Openmetadata => "openmetadata",
            Self::Rdbms => "rdbms",
            Self::Cube => "cube",
            Self::Foundry => "foundry",
            Self::OpenApi => "openapi",
        }
    }
}

/// Where the definitions and the data are, and what the resulting bundle is called.
///
/// Each catalog carries a declared NAME, the way a `sources:` entry carries an alias: the
/// contribution manifest keys on it, and a reviewer reads it in a settings file. It is named by
/// code and not by index so that reordering the list does not silently rename a contributor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSettings {
    name: SourceName,
    kind: CatalogKind,
    dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    version: DefinitionVersion,
    /// `catalog.kind: datahub` only - see [`Self::with_datahub_reader`]. `None` for every other
    /// kind, and for a `datahub` entry before that step runs.
    endpoint: Option<String>,
    /// `catalog.kind: datahub` only - the settings-declared file a composition root reads the
    /// personal access token from at boot. Never the token itself.
    token_file: Option<PathBuf>,
    /// `catalog.kind: datahub` only - the deployment-chosen structured property name.
    metric_property: Option<String>,
    /// `catalog.kind: datahub` only - each `DataHub` platform mapped to the declared source alias
    /// serving its datasets. Empty means the single `bigquery` platform, served by this catalog's
    /// own name.
    platforms: BTreeMap<DataHubPlatform, SourceName>,
    /// `catalog.kind: datahub` only - the read deadline in seconds, shared across every
    /// request one `read()` makes, pages included. `None` means the reader's own recommended
    /// default.
    deadline_seconds: Option<u64>,
    /// `catalog.kind: datahub` only - the response-size cap in bytes. `None` means the reader's
    /// own recommended default.
    max_response_bytes: Option<u64>,
    /// `github.com/telekom/sutura#975` - every kind, not `datahub` alone: how often a composition
    /// root that DRIVES a refresh re-reads this catalog and re-pins it. A settings file always
    /// yields `Some` - [`Self::DEFAULT_REFRESH_SECONDS`] when the key is absent; `None` means
    /// never and is reachable only by building the settings in code.
    refresh_seconds: Option<u64>,
    /// `github.com/telekom/sutura#2071` - every kind, not `rdbms` alone: the aggregate byte bound
    /// `Definitions::assemble_bounded` checks this catalog's authored content against. Always
    /// resolved - [`DefinitionsByteBound::default`] (1 MiB) when the key is absent, never a second
    /// `None`-means-default the way [`Self::refresh_seconds`] needs, because there is no "never"
    /// reading for this one.
    definitions_bound: DefinitionsByteBound,
    /// `catalog.kind: rdbms` only, all or nothing. `None` for every other kind.
    rdbms: Option<RdbmsSettings>,
    /// `catalog.kind: foundry` only, all or nothing. `None` for every other kind.
    foundry: Option<FoundrySettings>,
}

/// Why a catalog configuration is not usable.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidCatalogSettings {
    /// A required path was empty, which would resolve to the process working directory - a different
    /// directory on every host, and never the one the operator meant.
    #[error("{name} is empty - write the directory, not nothing")]
    EmptyPath { name: &'static str },
    /// No catalog was declared, so there is nothing to serve.
    #[error("no catalog is declared - a deployment serves at least one")]
    EmptyCatalog,
    /// Two catalogs share one declared name, so the contribution manifest could not tell them apart.
    #[error("{name} declares more than one catalog")]
    DuplicateName { name: SourceName },
    /// A `catalog.kind: datahub` entry did not declare a field only that kind needs.
    #[error("catalog.{field} is required when catalog.kind is datahub, and is empty or absent")]
    MissingForDatahub { field: &'static str },
    /// `catalogs[].endpoint` carries an `@`. A catalog endpoint is `scheme://host[:port]`, so an
    /// `@` is userinfo however the URL parser splits it - refused here, before the startup log
    /// prints the resolved settings, and never quoted back.
    #[error("catalogs.{name}.endpoint carries an `@` - credentials in the URL are refused, declare token_file")]
    CredentialsInEndpoint { name: SourceName },
    /// A `catalog.kind: openmetadata`, `cube` or `openapi` entry did not declare a field its reader
    /// needs.
    #[error("catalog.{field} is required when catalog.kind is {kind}, and is empty or absent")]
    MissingForReader { kind: &'static str, field: &'static str },
    /// `catalogs[].refresh_seconds: 0` - `github.com/telekom/sutura#975`. Zero re-reads on every
    /// tick of whatever drives it, which is not a refresh interval; absent is the default
    /// interval, not "never".
    #[error("catalogs.{name}.refresh_seconds is 0 - remove the key for the default, or write a positive interval")]
    ZeroRefresh { name: SourceName },
    /// `catalogs[].max_definitions_bytes` - `github.com/telekom/sutura#2071` - is `0` or above
    /// [`sutura_domain::catalog::DEFINITIONS_BYTES_CEILING`].
    #[error("catalogs.{name}.max_definitions_bytes is not usable: {cause}")]
    InvalidDefinitionsBound {
        name: SourceName,
        #[source]
        cause: InvalidDefinitionsByteBound,
    },
    /// A `catalog.kind: rdbms` entry's own keys are not usable.
    #[error("`catalogs.{name}` is `kind: rdbms` and is not usable")]
    Rdbms {
        name: SourceName,
        #[source]
        cause: InvalidRdbmsCatalog,
    },
    /// A catalog of another kind wrote an rdbms-only key - a key nothing would read, which is a
    /// configuration nobody can see, so it is refused the way `deny_unknown_fields` refuses one.
    #[error("`catalogs.{name}.{key}` is read only when `kind` is `rdbms` - remove it, or write `kind: rdbms`")]
    RdbmsKeyOnOtherKind { name: SourceName, key: &'static str },
    /// A `catalog.kind: datahub` entry maps a platform to a source nobody declared under `sources:`.
    #[error("`catalogs.{name}.platforms.{platform}` names source `{source_alias}`, which `sources:` does not declare")]
    UnknownPlatformSource {
        name: SourceName,
        platform: DataHubPlatform,
        source_alias: String,
    },
    /// A `platforms:` key that is no platform name.
    #[error("`catalogs.{name}.platforms` has the key `{platform}`, which is not a platform name: {cause}")]
    InvalidPlatform {
        name: SourceName,
        platform: String,
        #[source]
        cause: InvalidDataHubPlatform,
    },
    /// Two `platforms:` keys that read as one platform once trimmed, so the second would silently replace
    /// the first.
    #[error("`catalogs.{name}.platforms` writes the platform `{platform}` twice, as {first:?} and {second:?} - keep one")]
    DuplicatePlatform {
        name: SourceName,
        platform: DataHubPlatform,
        first: String,
        second: String,
    },
    /// `platforms` was written with no entry, which would read as the default and so declares nothing.
    #[error("`catalogs.{name}.platforms` is empty - remove the key for the default, or map a platform to a source")]
    EmptyPlatforms { name: SourceName },
    /// A platform was written with no source.
    #[error("`catalogs.{name}.platforms.{platform}` names no source - write a declared `sources:` alias")]
    PlatformWithoutSource { name: SourceName, platform: DataHubPlatform },
    /// A `catalog.kind: foundry` entry's own keys are not usable.
    #[error("`catalogs.{name}` is `kind: foundry` and is not usable")]
    Foundry {
        name: SourceName,
        #[source]
        cause: InvalidFoundryCatalog,
    },
    /// A catalog of another kind wrote the `foundry:` block, which nothing would read.
    #[error("`catalogs.{name}.foundry` is read only when `kind` is `foundry` - remove it, or write `kind: foundry`")]
    FoundryOnOtherKind { name: SourceName },
    /// A catalog of another kind wrote `platforms`, a key nothing would read.
    #[error("`catalogs.{name}.platforms` is read only when `kind` is `datahub` - remove it, or write `kind: datahub`")]
    PlatformsOnOtherKind { name: SourceName },
}

impl CatalogSettings {
    /// Reads the declared name, kind, directories and version label.
    ///
    /// Existence of the directories is checked when the catalog loads, not here: a directory can
    /// disappear after configuration parsing.
    pub fn parse(
        name: SourceName,
        kind: CatalogKind,
        dir: PathBuf,
        data_dir: PathBuf,
        version: DefinitionVersion,
    ) -> Result<Self, InvalidCatalogSettings> {
        Self::parse_optional(name, kind, Some(dir), Some(data_dir), version)
    }

    /// Allows the live catalog kind to omit directories while requiring them for other kinds.
    pub(crate) fn parse_optional(
        name: SourceName,
        kind: CatalogKind,
        dir: Option<PathBuf>,
        data_dir: Option<PathBuf>,
        version: DefinitionVersion,
    ) -> Result<Self, InvalidCatalogSettings> {
        if dir.as_ref().is_some_and(|path| path.as_os_str().is_empty()) {
            return Err(InvalidCatalogSettings::EmptyPath { name: "catalogs[].dir" });
        }
        if data_dir.as_ref().is_some_and(|path| path.as_os_str().is_empty()) {
            return Err(InvalidCatalogSettings::EmptyPath {
                name: "catalogs[].data_dir",
            });
        }
        if !matches!(kind, CatalogKind::Rdbms | CatalogKind::Foundry) {
            if dir.is_none() {
                return Err(InvalidCatalogSettings::EmptyPath { name: "catalogs[].dir" });
            }
            if data_dir.is_none() {
                return Err(InvalidCatalogSettings::EmptyPath {
                    name: "catalogs[].data_dir",
                });
            }
        }
        Ok(Self {
            name,
            kind,
            dir,
            data_dir,
            version,
            endpoint: None,
            token_file: None,
            metric_property: None,
            platforms: BTreeMap::new(),
            deadline_seconds: None,
            max_response_bytes: None,
            refresh_seconds: None,
            definitions_bound: DefinitionsByteBound::default(),
            rdbms: None,
            foundry: None,
        })
    }

    /// The interval a catalog declared without `refresh_seconds` is re-read at - fifteen minutes.
    /// `defaults.yaml`'s default entry spells the same number, and cells hold each against the
    /// literal 900.
    pub const DEFAULT_REFRESH_SECONDS: u64 = 900;

    /// Declares how often this catalog is re-read and re-pinned - `#975`. `None` (what
    /// [`Self::parse`] starts at) means never; `Some(0)` is refused rather than read as "never" or
    /// "as fast as possible", so an operator who wrote a literal `0` is told rather than silently
    /// ignored.
    pub fn with_refresh_seconds(mut self, refresh_seconds: Option<u64>) -> Result<Self, InvalidCatalogSettings> {
        if refresh_seconds == Some(0) {
            return Err(InvalidCatalogSettings::ZeroRefresh { name: self.name });
        }
        self.refresh_seconds = refresh_seconds;
        Ok(self)
    }

    /// The declared refresh interval, or `None` for never.
    #[inline]
    pub const fn refresh_seconds(&self) -> Option<u64> {
        self.refresh_seconds
    }

    /// Declares the aggregate byte bound this catalog's authored content is checked against -
    /// `github.com/telekom/sutura#2071`. `None` (what [`Self::parse`] starts at) keeps
    /// [`DefinitionsByteBound::default`]; `Some` is parsed and refused the same way a reader builds
    /// one from any other source.
    pub fn with_max_definitions_bytes(mut self, max_definitions_bytes: Option<u64>) -> Result<Self, InvalidCatalogSettings> {
        if let Some(bytes) = max_definitions_bytes {
            // A `u64` that does not fit `usize` (32-bit only) is already above the ceiling -
            // folding it to `usize::MAX` reports that refusal instead of a silent truncation.
            let bytes = usize::try_from(bytes).unwrap_or(usize::MAX);
            self.definitions_bound =
                DefinitionsByteBound::parse(bytes).map_err(|cause| InvalidCatalogSettings::InvalidDefinitionsBound {
                    name: self.name.clone(),
                    cause,
                })?;
        }
        Ok(self)
    }

    /// The resolved aggregate byte bound - always a value, never a second "absent" reading.
    #[inline]
    pub const fn max_definitions_bytes(&self) -> DefinitionsByteBound {
        self.definitions_bound
    }

    /// Adds the `catalog.kind: rdbms`-only settings. `parse_catalogs` calls this only when `kind`
    /// parsed as [`CatalogKind::Rdbms`].
    #[must_use]
    pub fn with_rdbms(mut self, rdbms: RdbmsSettings) -> Self {
        self.rdbms = Some(rdbms);
        self
    }

    /// The rdbms-only settings, `Some` exactly for a `kind: rdbms` entry read from settings.
    #[inline]
    pub const fn rdbms(&self) -> Option<&RdbmsSettings> {
        self.rdbms.as_ref()
    }

    /// Adds a `catalog.kind: foundry` entry's endpoint and its `foundry:` settings. The endpoint
    /// is checked as [`Self::with_http_reader`] checks it; the token comes from `foundry`.
    pub fn with_foundry(mut self, endpoint: String, foundry: FoundrySettings) -> Result<Self, InvalidCatalogSettings> {
        if endpoint.trim().is_empty() {
            return Err(InvalidCatalogSettings::MissingForReader {
                kind: self.kind.as_str(),
                field: "endpoint",
            });
        }
        if endpoint.contains('@') {
            return Err(InvalidCatalogSettings::CredentialsInEndpoint { name: self.name });
        }
        self.endpoint = Some(endpoint);
        self.foundry = Some(foundry);
        Ok(self)
    }

    /// The foundry-only settings, `Some` exactly for a `kind: foundry` entry read from settings.
    #[inline]
    pub const fn foundry(&self) -> Option<&FoundrySettings> {
        self.foundry.as_ref()
    }

    /// Adds the three `catalog.kind: datahub`-only fields to an already-parsed entry.
    ///
    /// A separate step rather than three more parameters on [`Self::parse`], so every existing
    /// caller - every markdown entry, every test that builds one - is unaffected by a kind no
    /// binary in this repository could open until issue #202's reader arrived. `parse_catalogs`
    /// calls this only when `kind` parsed as [`CatalogKind::Datahub`].
    pub fn with_datahub_reader(
        mut self,
        endpoint: String,
        token_file: PathBuf,
        metric_property: String,
    ) -> Result<Self, InvalidCatalogSettings> {
        if endpoint.trim().is_empty() {
            return Err(InvalidCatalogSettings::MissingForDatahub { field: "endpoint" });
        }
        if endpoint.contains('@') {
            return Err(InvalidCatalogSettings::CredentialsInEndpoint { name: self.name });
        }
        if token_file.as_os_str().is_empty() {
            return Err(InvalidCatalogSettings::MissingForDatahub { field: "token_file" });
        }
        if metric_property.trim().is_empty() {
            return Err(InvalidCatalogSettings::MissingForDatahub {
                field: "metric_property",
            });
        }
        self.endpoint = Some(endpoint);
        self.token_file = Some(token_file);
        self.metric_property = Some(metric_property);
        Ok(self)
    }

    /// Adds the `catalog.kind: datahub`-only platform-to-source map. `parse_catalogs` has already
    /// checked that every value names a declared source.
    #[must_use]
    pub fn with_datahub_platforms(mut self, platforms: BTreeMap<DataHubPlatform, SourceName>) -> Self {
        self.platforms = platforms;
        self
    }

    /// The declared platform-to-source map; empty selects the single `bigquery` platform.
    #[inline]
    pub const fn platforms(&self) -> &BTreeMap<DataHubPlatform, SourceName> {
        &self.platforms
    }

    /// Adds the two `catalog.kind: datahub`-only bounds, when the deployment declared either.
    ///
    /// **Infallible, unlike [`Self::with_datahub_reader`], because `None` is a valid value here
    /// rather than a missing required one** - it selects the reader's own recommended default
    /// (`sutura_catalog_datahub::http::{DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES}`),
    /// which this crate does not depend on that adapter crate to name. The composition root is
    /// where a declared zero is refused - `sutura_catalog_datahub::http::ReadBounds::parse` is the
    /// single owner of that range. This used to cite `BytesBilledCeiling::parse` as the same split
    /// for `BigQuery`'s ceiling; that type is deleted and its range is now owned by nobody, so the
    /// `DataHub` bounds are the only live example of the split.
    #[must_use]
    pub const fn with_datahub_bounds(mut self, deadline_seconds: Option<u64>, max_response_bytes: Option<u64>) -> Self {
        self.deadline_seconds = deadline_seconds;
        self.max_response_bytes = max_response_bytes;
        self
    }

    /// Adds the two reader fields of a `catalog.kind: openmetadata`, `cube` or `openapi` entry.
    ///
    /// A separate step rather than two more parameters on [`Self::parse`], for the same reason
    /// [`Self::with_datahub_reader`] is one: `parse_catalogs` calls it only when `kind` parsed as
    /// [`CatalogKind::Openmetadata`], [`CatalogKind::Cube`] or [`CatalogKind::OpenApi`], so every other kind is unaffected by fields only a reader over
    /// a real deployment needs.
    pub fn with_http_reader(mut self, endpoint: String, token_file: PathBuf) -> Result<Self, InvalidCatalogSettings> {
        if endpoint.trim().is_empty() {
            return Err(InvalidCatalogSettings::MissingForReader {
                kind: self.kind.as_str(),
                field: "endpoint",
            });
        }
        if endpoint.contains('@') {
            return Err(InvalidCatalogSettings::CredentialsInEndpoint { name: self.name });
        }
        if token_file.as_os_str().is_empty() {
            return Err(InvalidCatalogSettings::MissingForReader {
                kind: self.kind.as_str(),
                field: "token_file",
            });
        }
        self.endpoint = Some(endpoint);
        self.token_file = Some(token_file);
        Ok(self)
    }

    /// Adds the two read bounds of a `catalog.kind: openmetadata` or `cube` entry, when the deployment declared either.
    ///
    /// **Infallible, unlike [`Self::with_http_reader`], because `None` is a valid value
    /// here rather than a missing required one** - it selects the reader's own recommended default
    /// (`sutura_catalog_openmetadata::http::{DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES}`).
    #[must_use]
    pub const fn with_read_bounds(mut self, deadline_seconds: Option<u64>, max_response_bytes: Option<u64>) -> Self {
        self.deadline_seconds = deadline_seconds;
        self.max_response_bytes = max_response_bytes;
        self
    }

    /// The declared name, which the contribution manifest keys on.
    #[inline]
    pub const fn name(&self) -> &SourceName {
        &self.name
    }

    /// Which adapter opens this catalog.
    #[inline]
    pub const fn kind(&self) -> CatalogKind {
        self.kind
    }

    /// The declared catalog directory. Required for every kind except `rdbms` and `foundry`.
    #[inline]
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The declared data directory. Required for every kind except `rdbms` and `foundry`.
    #[inline]
    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }

    #[inline]
    pub const fn version(&self) -> &DefinitionVersion {
        &self.version
    }
    /// The declared `DataHub` endpoint, once [`Self::with_datahub_reader`] has run.
    #[inline]
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// The declared personal-access-token file, once [`Self::with_datahub_reader`] has run.
    #[inline]
    pub fn token_file(&self) -> Option<&Path> {
        self.token_file.as_deref()
    }

    /// The deployment-chosen structured property name, once [`Self::with_datahub_reader`] has run.
    #[inline]
    pub fn metric_property(&self) -> Option<&str> {
        self.metric_property.as_deref()
    }

    /// The declared read deadline in seconds, or `None` to use the reader's own recommended
    /// default.
    #[inline]
    pub const fn deadline_seconds(&self) -> Option<u64> {
        self.deadline_seconds
    }

    /// The declared response-size cap in bytes, or `None` to use the reader's own recommended
    /// default.
    #[inline]
    pub const fn max_response_bytes(&self) -> Option<u64> {
        self.max_response_bytes
    }
}

/// The catalogs a deployment declares, in declaration order.
///
/// **A non-empty, ordered collection, and the empty member is unrepresentable.** Composition -
/// the point of having N - is the metadata assembler in `sutura-app`; this type is the declared
/// configuration it is handed. Order is declaration order, which is content order: the contribution
/// manifest is a `BTreeMap` keyed on each entry's [`CatalogSettings::name`], so this ordering is
/// what a reviewer reads and manifest determinism does not depend on it surviving a rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogs {
    entries: Vec<CatalogSettings>,
}

impl Catalogs {
    /// Reads the declared catalogs, refusing an empty list and any duplicated name.
    pub fn parse(entries: Vec<CatalogSettings>) -> Result<Self, InvalidCatalogSettings> {
        if entries.is_empty() {
            return Err(InvalidCatalogSettings::EmptyCatalog);
        }
        let mut seen = std::collections::BTreeSet::new();
        for entry in &entries {
            if !seen.insert(&entry.name) {
                return Err(InvalidCatalogSettings::DuplicateName {
                    name: entry.name.clone(),
                });
            }
        }
        if let Some(first) = entries.first() {
            rdbms::descriptions::check_references(&entries).map_err(|cause| InvalidCatalogSettings::Rdbms {
                name: first.name().clone(),
                cause,
            })?;
        }
        Ok(Self { entries })
    }

    /// The serving contributors, excluding only names explicitly referenced for prose.
    /// References stay resolved against the original registry when readers are opened.
    pub fn structural_catalogs(&self) -> Result<Self, InvalidCatalogSettings> {
        let referenced: std::collections::BTreeSet<_> = self
            .entries
            .iter()
            .filter_map(CatalogSettings::rdbms)
            .flat_map(rdbms::RdbmsSettings::descriptions_from)
            .filter_map(|source| match source {
                rdbms::DescriptionSource::Catalog(name) => Some(name),
                rdbms::DescriptionSource::Table(_) => None,
            })
            .collect();
        let entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| !referenced.contains(entry.name()))
            .cloned()
            .collect();
        if entries.is_empty() {
            return Err(InvalidCatalogSettings::EmptyCatalog);
        }
        Ok(Self { entries })
    }

    /// Every catalog, in declaration order.
    pub fn each(&self) -> impl Iterator<Item = &CatalogSettings> {
        self.entries.iter()
    }

    /// How many catalogs are declared.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod description_relations_tests;

#[cfg(test)]
mod bound_tests;

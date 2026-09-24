//! Which catalog the service serves, and where the files it reads live.
//!
//! Two directories rather than one, because they are owned by different people. The catalog is
//! authored and reviewed - it is the definitions somebody certified - and the data directory is
//! wherever the files the engine reads happen to be mounted. Conflating them would make a
//! deployment that moved its data look like a catalog change, which is the one thing a pinned
//! bundle exists to make visible.

use std::path::{Path, PathBuf};

use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

/// Which adapter the catalog configuration names, and therefore which one opens it.
///
/// **A closed set of typed declarations, [`crate::sources::SourceKind`]'s shape on the metadata
/// side.** A DATA source's adapter is chosen by `SourceKind` and dispatched by the composition
/// root's exhaustive match with no wildcard arm; a METADATA source has exactly the same need, and
/// until this type existed the settings tree carried a directory and a version and no word an
/// operator could write to say *read the model from somewhere else* - so a second catalog kind
/// could merge complete and silently remain unreachable from any binary.
///
/// Five variants. [`Self::Datahub`] says which and why, the way `SourceKind::BigQuery` does for
/// data systems: the vocabulary is the vocabulary of adapters this repository has, and an adapter
/// that exists in a record rather than in a linked crate is still a word an operator might write.
/// `#970` added the three declaring adapters that had a crate and no composition root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogKind {
    /// A directory of markdown documents with YAML frontmatter, read by `sutura-catalog-local`.
    ///
    /// The only kind either composition root can OPEN in this build: the markdown adapter is
    /// linked by `sutura serve` and is what `sutura query`/`sutura mcp` put behind its directory
    /// argument.
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
    /// An `OpenMetadata` deployment, decided by `sutura-catalog-openmetadata` over its own
    /// `SnapshotReader` port.
    ///
    /// **A declarable kind no binary this repository ships can open yet, and that is deliberate.**
    /// The crate decides a whole metric against a fake reader; a reader over a real deployment is
    /// the follow-up `#152` names, so the composition root refuses this kind by name until one
    /// exists rather than opening the recorded fixture against a real deployment's name.
    Openmetadata,
    /// An RDBMS dictionary, decided by `sutura-catalog-rdbms` over a `DictionaryReader` port.
    ///
    /// **A declarable kind no binary this repository ships can open yet, for the identical reason
    /// [`Self::Openmetadata`] states.** The crate decides the conversion against a recorded
    /// dictionary; a reader over a real socket lands with `#972`.
    Rdbms,
}
/// The configured word did not name a kind of catalog this build has.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` does not name a kind of catalog - one of: {}", CatalogKind::NAMES.join(", "))]
pub struct UnknownCatalogKind {
    found: String,
}

impl CatalogKind {
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["markdown", "datahub", "okf", "openmetadata", "rdbms"];

    /// Reads the configured word.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownCatalogKind> {
        match raw.as_ref().trim() {
            "markdown" => Ok(Self::Markdown),
            "datahub" => Ok(Self::Datahub),
            "okf" => Ok(Self::Okf),
            "openmetadata" => Ok(Self::Openmetadata),
            "rdbms" => Ok(Self::Rdbms),
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
            Self::Openmetadata => "openmetadata",
            Self::Rdbms => "rdbms",
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
    dir: PathBuf,
    data_dir: PathBuf,
    version: DefinitionVersion,
    /// `catalog.kind: datahub` only - see [`Self::with_datahub_reader`]. `None` for every other
    /// kind, and for a `datahub` entry before that step runs.
    endpoint: Option<String>,
    /// `catalog.kind: datahub` only - the settings-declared file a composition root reads the
    /// personal access token from at boot. Never the token itself.
    token_file: Option<PathBuf>,
    /// `catalog.kind: datahub` only - the deployment-chosen structured property name.
    metric_property: Option<String>,
    /// `catalog.kind: datahub` only - the read deadline in seconds, shared across the (up to)
    /// three requests one `read()` makes. `None` means the reader's own recommended default.
    deadline_seconds: Option<u64>,
    /// `catalog.kind: datahub` only - the response-size cap in bytes. `None` means the reader's
    /// own recommended default.
    max_response_bytes: Option<u64>,
}

/// Why a catalog configuration is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidCatalogSettings {
    /// A path was empty, which resolves to the process working directory - a different directory
    /// on every host, and never the one the operator meant.
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
}

impl CatalogSettings {
    /// Reads the declared name, kind, the two directories and the version label.
    ///
    /// The version arrives already parsed, because what identifies a snapshot of a directory is
    /// a commit id or a build number and only the caller has it. Existence of the directories is
    /// deliberately *not* checked here: this type is the configuration, and a directory that
    /// disappears between reading the configuration and loading the catalog would make an
    /// existence check here a claim that goes stale immediately. The load is what fails.
    pub fn parse(
        name: SourceName,
        kind: CatalogKind,
        dir: PathBuf,
        data_dir: PathBuf,
        version: DefinitionVersion,
    ) -> Result<Self, InvalidCatalogSettings> {
        if dir.as_os_str().is_empty() {
            return Err(InvalidCatalogSettings::EmptyPath { name: "catalogs[].dir" });
        }
        if data_dir.as_os_str().is_empty() {
            return Err(InvalidCatalogSettings::EmptyPath {
                name: "catalogs[].data_dir",
            });
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
            deadline_seconds: None,
            max_response_bytes: None,
        })
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

    #[inline]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[inline]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
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
            if !seen.insert(entry.name.clone()) {
                return Err(InvalidCatalogSettings::DuplicateName {
                    name: entry.name.clone(),
                });
            }
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
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::DefinitionVersion;

    use super::{CatalogKind, CatalogSettings, Catalogs, InvalidCatalogSettings};

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test-1").expect("a test version is a version")
    }

    fn kind() -> CatalogKind {
        CatalogKind::Markdown
    }

    fn name(raw: &str) -> SourceName {
        SourceName::parse(raw).expect("a test catalog name is a name")
    }

    fn settings(name_raw: &str) -> CatalogSettings {
        CatalogSettings::parse(
            name(name_raw),
            kind(),
            PathBuf::from("/nowhere/catalog"),
            PathBuf::from("/nowhere/data"),
            version(),
        )
        .expect("a declared catalog is a catalog")
    }

    #[test]
    fn an_unknown_catalog_kind_is_refused_against_the_available_ones() {
        // `SourceKind`'s precedent, on the metadata side: the vocabulary is closed, and an unknown
        // word is a parse refusal listing what it could have been rather than a silent default.
        assert_eq!(
            CatalogKind::parse("markdown").expect("markdown is a kind"),
            CatalogKind::Markdown
        );
        assert_eq!(
            CatalogKind::parse("datahub").expect("datahub is a kind"),
            CatalogKind::Datahub
        );
        let error = CatalogKind::parse("atlas").expect_err("atlas is not a kind this build has");
        assert!(error.to_string().contains("atlas"), "{}", error);
        assert!(error.to_string().contains("markdown"), "{}", error);
        assert!(CatalogKind::NAMES.contains(&"markdown"));
        assert_eq!(
            CatalogKind::parse(CatalogKind::Markdown.as_str()).expect("a spelling is a kind"),
            CatalogKind::Markdown
        );
    }

    #[test]
    fn the_three_declaring_kinds_added_by_issue_970_parse_and_spell() {
        // `#970` added `okf`/`openmetadata`/`rdbms` to the closed vocabulary: adapters with a
        // crate and a recorded fixture and, at first, no composition root. Each parses and spells
        // back its own word, and all five appear in `NAMES` so a refusal lists the whole set.
        for (kind, word) in [
            (CatalogKind::Okf, "okf"),
            (CatalogKind::Openmetadata, "openmetadata"),
            (CatalogKind::Rdbms, "rdbms"),
        ] {
            assert_eq!(CatalogKind::parse(word).expect("a new kind is a kind"), kind);
            assert_eq!(kind.as_str(), word);
            assert!(CatalogKind::NAMES.contains(&word), "{word} is not in NAMES");
        }
    }

    #[test]
    fn an_empty_directory_is_refused_rather_than_resolving_to_the_working_directory() {
        // The bug this catches: an unset value deserializes to an empty string, an empty path
        // resolves to `.`, and the service then serves whatever catalog happens to be beside the
        // binary. That is a different bundle with no diff anywhere.
        let error = CatalogSettings::parse(name("catalog"), kind(), PathBuf::new(), PathBuf::from("data"), version())
            .expect_err("an empty catalog directory is not a directory");
        assert_eq!(error, InvalidCatalogSettings::EmptyPath { name: "catalogs[].dir" });

        let error = CatalogSettings::parse(name("catalog"), kind(), PathBuf::from("catalog"), PathBuf::new(), version())
            .expect_err("an empty data directory is not a directory");
        assert_eq!(
            error,
            InvalidCatalogSettings::EmptyPath {
                name: "catalogs[].data_dir"
            }
        );
    }

    #[test]
    fn a_directory_that_does_not_exist_yet_is_accepted() {
        // Deliberate. Checking existence here would be a claim that is already stale by the time
        // the catalog is loaded, and it would make configuration validation depend on the
        // filesystem - which is what makes a settings test need a temporary directory.
        let settings = settings("catalog");
        assert_eq!(settings.name(), &name("catalog"));
        assert_eq!(settings.kind(), CatalogKind::Markdown);
        assert_eq!(settings.dir(), PathBuf::from("/nowhere/catalog"));
        assert_eq!(settings.data_dir(), PathBuf::from("/nowhere/data"));
        assert_eq!(settings.version().as_str(), "test-1");
    }

    #[test]
    fn an_empty_catalog_list_is_refused_rather_than_serving_nothing() {
        // A deployment serves at least one metadata source; an empty `catalogs:` is a typo, not a
        // choice, and it is refused at the registry rather than discovering that no bundle loads.
        assert_eq!(
            Catalogs::parse(Vec::new()).expect_err("no catalog is not a deployment"),
            InvalidCatalogSettings::EmptyCatalog
        );
    }

    #[test]
    fn two_catalogs_may_not_share_one_declared_name() {
        // The contribution manifest keys on the declared name, so a duplicate name is two
        // contributors a record cannot tell apart - refused by name, like a duplicated source alias.
        let error = Catalogs::parse(vec![settings("model"), settings("model")])
            .expect_err("the same name twice is two contributors nobody can tell apart");
        assert_eq!(error, InvalidCatalogSettings::DuplicateName { name: name("model") });
        let catalogs = Catalogs::parse(vec![settings("structure"), settings("metrics")])
            .expect("two distinct names are two distinct contributors");
        assert_eq!(catalogs.count(), 2);
    }

    #[test]
    fn a_datahub_entrys_three_required_fields_and_two_optional_bounds_round_trip() {
        let base = settings("metrics");
        assert_eq!(base.endpoint(), None);
        assert_eq!(base.token_file(), None);
        assert_eq!(base.metric_property(), None);
        assert_eq!(base.deadline_seconds(), None);
        assert_eq!(base.max_response_bytes(), None);

        let complete = base
            .clone()
            .with_datahub_reader(
                String::from("https://datahub.example"),
                PathBuf::from("/nowhere/token"),
                String::from("deployment_metric_document"),
            )
            .expect("all three required fields are non-empty")
            .with_datahub_bounds(Some(45), Some(1 << 20));
        assert_eq!(complete.endpoint(), Some("https://datahub.example"));
        assert_eq!(complete.token_file(), Some(Path::new("/nowhere/token")));
        assert_eq!(complete.metric_property(), Some("deployment_metric_document"));
        assert_eq!(complete.deadline_seconds(), Some(45));
        assert_eq!(complete.max_response_bytes(), Some(1 << 20));

        // Declaring neither bound is not a refusal - `None` is what selects the reader's own
        // recommended default, resolved by the composition root rather than by this type.
        let defaulted = base
            .with_datahub_reader(
                String::from("https://datahub.example"),
                PathBuf::from("/nowhere/token"),
                String::from("deployment_metric_document"),
            )
            .expect("all three required fields are non-empty");
        assert_eq!(defaulted.deadline_seconds(), None);
        assert_eq!(defaulted.max_response_bytes(), None);
    }

    #[test]
    fn a_datahub_entry_missing_any_of_the_three_required_fields_is_refused_naming_it() {
        let base = settings("metrics");
        let missing_endpoint = base
            .clone()
            .with_datahub_reader(String::new(), PathBuf::from("/nowhere/token"), String::from("p"))
            .expect_err("an empty endpoint is refused");
        assert_eq!(
            missing_endpoint,
            InvalidCatalogSettings::MissingForDatahub { field: "endpoint" }
        );

        let missing_token_file = base
            .clone()
            .with_datahub_reader(String::from("https://datahub.example"), PathBuf::new(), String::from("p"))
            .expect_err("an empty token_file is refused");
        assert_eq!(
            missing_token_file,
            InvalidCatalogSettings::MissingForDatahub { field: "token_file" }
        );

        let missing_property = base
            .with_datahub_reader(
                String::from("https://datahub.example"),
                PathBuf::from("/nowhere/token"),
                String::new(),
            )
            .expect_err("an empty metric_property is refused");
        assert_eq!(
            missing_property,
            InvalidCatalogSettings::MissingForDatahub {
                field: "metric_property"
            }
        );
    }
}

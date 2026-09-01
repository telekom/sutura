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
/// Two variants today. [`Self::Datahub`] says which and why, the way `SourceKind::BigQuery` does for
/// data systems: the vocabulary is the vocabulary of adapters this repository has, and an adapter
/// that exists in a record rather than in a linked crate is still a word an operator might write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogKind {
    /// A directory of markdown documents with YAML frontmatter, read by `sutura-catalog-local`.
    ///
    /// The only kind either composition root can OPEN in this build: the markdown adapter is
    /// linked by `sutura-serve` and is what `sutura-cli` puts behind its directory argument.
    Markdown,
    /// A metadata service, read through the adapter `docs/adr/0016` specifies and #114 builds.
    ///
    /// **A declarable kind that no binary this repository ships can open yet, and that is
    /// deliberate.** The vocabulary of kinds is the vocabulary of adapters *this repository has*
    /// in its records, and the composition root refuses this kind by name for exactly the reason
    /// `SourceKind::BigQuery` is refused: an operator who writes the word must be told the truth
    /// (the adapter is not linked) rather than sent looking for a typo.
    Datahub,
}

/// The configured word did not name a kind of catalog this build has.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` does not name a kind of catalog - one of: {}", CatalogKind::NAMES.join(", "))]
pub struct UnknownCatalogKind {
    found: String,
}

impl CatalogKind {
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["markdown", "datahub"];

    /// Reads the configured word.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownCatalogKind> {
        match raw.as_ref().trim() {
            "markdown" => Ok(Self::Markdown),
            "datahub" => Ok(Self::Datahub),
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
            return Err(InvalidCatalogSettings::EmptyPath { name: "catalog.dir" });
        }
        if data_dir.as_os_str().is_empty() {
            return Err(InvalidCatalogSettings::EmptyPath {
                name: "catalog.data_dir",
            });
        }
        Ok(Self {
            name,
            kind,
            dir,
            data_dir,
            version,
        })
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
    use std::path::PathBuf;

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
    fn an_empty_directory_is_refused_rather_than_resolving_to_the_working_directory() {
        // The bug this catches: an unset value deserializes to an empty string, an empty path
        // resolves to `.`, and the service then serves whatever catalog happens to be beside the
        // binary. That is a different bundle with no diff anywhere.
        let error = CatalogSettings::parse(name("catalog"), kind(), PathBuf::new(), PathBuf::from("data"), version())
            .expect_err("an empty catalog directory is not a directory");
        assert_eq!(error, InvalidCatalogSettings::EmptyPath { name: "catalog.dir" });

        let error = CatalogSettings::parse(name("catalog"), kind(), PathBuf::from("catalog"), PathBuf::new(), version())
            .expect_err("an empty data directory is not a directory");
        assert_eq!(
            error,
            InvalidCatalogSettings::EmptyPath {
                name: "catalog.data_dir"
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
}

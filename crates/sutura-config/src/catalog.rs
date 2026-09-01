//! Which catalog the service serves, and where the files it reads live.
//!
//! Two directories rather than one, because they are owned by different people. The catalog is
//! authored and reviewed - it is the definitions somebody certified - and the data directory is
//! wherever the files the engine reads happen to be mounted. Conflating them would make a
//! deployment that moved its data look like a catalog change, which is the one thing a pinned
//! bundle exists to make visible.

use std::path::{Path, PathBuf};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSettings {
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
}

impl CatalogSettings {
    /// Reads the declared kind, the two directories and the version label.
    ///
    /// The version arrives already parsed, because what identifies a snapshot of a directory is
    /// a commit id or a build number and only the caller has it. Existence of the directories is
    /// deliberately *not* checked here: this type is the configuration, and a directory that
    /// disappears between reading the configuration and loading the catalog would make an
    /// existence check here a claim that goes stale immediately. The load is what fails.
    pub fn parse(
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
        Ok(Self { kind, dir, data_dir, version })
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sutura_domain::pinned::DefinitionVersion;

use super::{CatalogKind, CatalogSettings, InvalidCatalogSettings};

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("test-1").expect("a test version is a version")
}

fn kind() -> CatalogKind {
    CatalogKind::Markdown
}

#[test]
fn an_unknown_catalog_kind_is_refused_against_the_available_ones() {
    // `SourceKind`'s precedent, on the metadata side: the vocabulary is closed, and an unknown
    // word is a parse refusal listing what it could have been rather than a silent default.
    assert_eq!(CatalogKind::parse("markdown").expect("markdown is a kind"), CatalogKind::Markdown);
    assert_eq!(CatalogKind::parse("datahub").expect("datahub is a kind"), CatalogKind::Datahub);
    let error = CatalogKind::parse("atlas").expect_err("atlas is not a kind this build has");
    assert!(error.to_string().contains("atlas"), "{}", error);
    assert!(error.to_string().contains("markdown"), "{}", error);
    // And spelling and listing cannot disagree: `NAMES` is the one source for both, as it is for
    // `SourceKind`.
    assert!(CatalogKind::NAMES.contains(&"markdown"));
    assert_eq!(CatalogKind::parse(CatalogKind::Markdown.as_str()).expect("a spelling is a kind"), CatalogKind::Markdown);
}

#[test]
fn an_empty_directory_is_refused_rather_than_resolving_to_the_working_directory() {
    // The bug this catches: an unset value deserializes to an empty string, an empty path
    // resolves to `.`, and the service then serves whatever catalog happens to be beside the
    // binary. That is a different bundle with no diff anywhere.
    let error =
        CatalogSettings::parse(kind(), PathBuf::new(), PathBuf::from("data"), version())
            .expect_err("an empty catalog directory is not a directory");
    assert_eq!(error, InvalidCatalogSettings::EmptyPath { name: "catalog.dir" });

    let error =
        CatalogSettings::parse(kind(), PathBuf::from("catalog"), PathBuf::new(), version())
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
    let settings = CatalogSettings::parse(kind(), PathBuf::from("/nowhere/catalog"), PathBuf::from("/nowhere/data"), version())
        .expect("a path is a path whether or not it resolves");
    assert_eq!(settings.kind(), CatalogKind::Markdown);
    assert_eq!(settings.dir(), PathBuf::from("/nowhere/catalog"));
    assert_eq!(settings.data_dir(), PathBuf::from("/nowhere/data"));
    assert_eq!(settings.version().as_str(), "test-1");
}
}

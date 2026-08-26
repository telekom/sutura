//! Which catalog the service serves, and where the files it reads live.
//!
//! Two directories rather than one, because they are owned by different people. The catalog is
//! authored and reviewed - it is the definitions somebody certified - and the data directory is
//! wherever the files the engine reads happen to be mounted. Conflating them would make a
//! deployment that moved its data look like a catalog change, which is the one thing a pinned
//! bundle exists to make visible.

use std::path::{Path, PathBuf};

use sutura_domain::pinned::DefinitionVersion;

/// Where the definitions and the data are, and what the resulting bundle is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSettings {
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
    /// Reads the two directories and the version label.
    ///
    /// The version arrives already parsed, because what identifies a snapshot of a directory is
    /// a commit id or a build number and only the caller has it. Existence of the directories is
    /// deliberately *not* checked here: this type is the configuration, and a directory that
    /// disappears between reading the configuration and loading the catalog would make an
    /// existence check here a claim that goes stale immediately. The load is what fails.
    pub fn parse(dir: PathBuf, data_dir: PathBuf, version: DefinitionVersion) -> Result<Self, InvalidCatalogSettings> {
        if dir.as_os_str().is_empty() {
            return Err(InvalidCatalogSettings::EmptyPath { name: "catalog.dir" });
        }
        if data_dir.as_os_str().is_empty() {
            return Err(InvalidCatalogSettings::EmptyPath {
                name: "catalog.data_dir",
            });
        }
        Ok(Self { dir, data_dir, version })
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

    use super::{CatalogSettings, InvalidCatalogSettings};

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test-1").expect("a test version is a version")
    }

    #[test]
    fn an_empty_directory_is_refused_rather_than_resolving_to_the_working_directory() {
        // The bug this catches: an unset value deserializes to an empty string, an empty path
        // resolves to `.`, and the service then serves whatever catalog happens to be beside the
        // binary. That is a different bundle with no diff anywhere.
        let error = CatalogSettings::parse(PathBuf::new(), PathBuf::from("data"), version())
            .expect_err("an empty catalog directory is not a directory");
        assert_eq!(error, InvalidCatalogSettings::EmptyPath { name: "catalog.dir" });

        let error = CatalogSettings::parse(PathBuf::from("catalog"), PathBuf::new(), version())
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
        let settings = CatalogSettings::parse(PathBuf::from("/nowhere/catalog"), PathBuf::from("/nowhere/data"), version())
            .expect("a path is a path whether or not it resolves");
        assert_eq!(settings.dir(), PathBuf::from("/nowhere/catalog"));
        assert_eq!(settings.data_dir(), PathBuf::from("/nowhere/data"));
        assert_eq!(settings.version().as_str(), "test-1");
    }
}

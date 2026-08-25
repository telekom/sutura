//! A [`SemanticCatalog`] over a directory of markdown documents with YAML frontmatter.
//!
//! One document per model, relationship and metric: the frontmatter is the definition and the prose
//! is the description that travels with an answer. It is the catalog for the case where there is no
//! upstream semantic layer to take a rendered statement from, which is the case a person is in the
//! first time they try this. `docs/adr/0001-first-party-semantic-models.md` is the decision.
//!
//! Three properties of the load are worth stating, because each is a mechanism rather than an
//! intention:
//!
//! **[`LocalCatalog::load`] takes no request context**, because the trait does not have one to give
//! it. A catalog that could see the caller could return a different definition per caller, and the
//! digest that travels with an answer would then describe something other than what produced it.
//!
//! **The walk is sorted**, so the same directory produces the same bundle. `read_dir` returns
//! entries in whatever order the filesystem chose, and a digest that moves between two runs over
//! unchanged files is a digest nobody can act on.
//!
//! **An empty directory is an error.** A mistyped root that happens to exist would otherwise load a
//! catalog with no metrics and refuse every question, and the refusal would name the metric rather
//! than the path.

pub mod document;
pub mod frontmatter;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use sutura_domain::catalog::{Definitions, InconsistentDefinitions, Metric, Model, Relationship};
use sutura_domain::definitions::{DefinitionDigest, InvalidDigest};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};

use crate::document::{DocumentKind, InvalidMetricDocument, KindProbe, MetricDoc, ModelDoc, RelationshipDoc};
use crate::frontmatter::MalformedDocument;

/// The extension a catalog document has to have.
const DOCUMENT_EXTENSION: &str = "md";

/// Why a directory could not be read as a catalog.
///
/// Every variant carries the path, because a catalog is many files and "invalid type: integer" with
/// no file name is a message that sends the reader to read all of them.
#[derive(Debug, thiserror::Error)]
pub enum LocalCatalogError {
    #[error("the catalog root {path} is not a directory")]
    NotADirectory { path: PathBuf },
    #[error("could not read {path}")]
    Io {
        path: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    #[error("{path} is not a catalog document")]
    Malformed {
        path: PathBuf,
        #[source]
        cause: MalformedDocument,
    },
    #[error("could not read the frontmatter of {path} as a {kind}")]
    Frontmatter {
        path: PathBuf,
        kind: &'static str,
        #[source]
        cause: serde_norway::Error,
    },
    #[error("{path} declares no `kind`, so there is no way to tell what it defines")]
    UnknownKind {
        path: PathBuf,
        #[source]
        cause: serde_norway::Error,
    },
    #[error("{path} is not a usable metric")]
    Metric {
        path: PathBuf,
        #[source]
        cause: InvalidMetricDocument,
    },
    #[error("the catalog does not hold together")]
    Inconsistent {
        #[source]
        cause: InconsistentDefinitions,
    },
    #[error("the catalog at {path} holds no documents")]
    Empty { path: PathBuf },
    #[error("the definitions could not be put into canonical form to be hashed")]
    Canonicalize {
        #[source]
        cause: serde_json::Error,
    },
    #[error("the computed digest is not a digest, which means the hashing changed shape")]
    Digest {
        #[source]
        cause: InvalidDigest,
    },
}

/// The canonical byte form of a set of definitions: what the digest is taken over.
///
/// JSON rather than the YAML it was read from, and that is the whole point. Reformatting a document,
/// reordering two files or rewording a comment must not move the digest; changing what a metric means
/// must. Serializing the *parsed* definitions gives exactly that, because everything that survives
/// parsing is meaning and everything that does not is layout.
///
/// Deterministic for two reasons that both have to hold: [`Definitions`] uses `BTreeMap` throughout,
/// so collection order is content order rather than hash order, and `serde_json` writes struct
/// fields in declaration order.
///
/// It lives in this adapter because `sutura-domain` cannot hash - `sha2` is not on its allowlisted
/// dependency tree, deliberately. When a second real catalog adapter lands, this moves to something
/// both can depend on rather than being reimplemented; a second implementation of a canonical form
/// is two canonical forms.
pub fn canonical_form(definitions: &Definitions) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(definitions)
}

/// One hex digit.
///
/// Written out rather than reached through `format!`, because the formatting machinery returns a
/// `Result` that cannot fail here and the ways of discarding it all trip a lint: the alternatives
/// are an `expect` on a path a catalog file can reach, or a `let _` on a `#[must_use]` value.
fn nibble(value: u8) -> char {
    if value < 10 {
        char::from(b'0'.saturating_add(value))
    } else {
        char::from(b'a'.saturating_add(value.saturating_sub(10)))
    }
}

/// The digest of a set of definitions.
pub fn digest_of(definitions: &Definitions) -> Result<DefinitionDigest, LocalCatalogError> {
    let canonical = canonical_form(definitions).map_err(|cause| LocalCatalogError::Canonicalize { cause })?;
    let hash = Sha256::digest(&canonical);
    let hex: String = hash
        .iter()
        .flat_map(|byte| [nibble(byte >> 4_u8), nibble(byte & 0x0f_u8)])
        .collect();
    // Lower-case hex of 32 bytes is what `DefinitionDigest` parses. If that ever stops being true
    // the error says the hashing changed shape rather than blaming the catalog.
    DefinitionDigest::parse(hex).map_err(|cause| LocalCatalogError::Digest { cause })
}

/// A catalog read from a directory of documents.
#[derive(Debug, Clone)]
pub struct LocalCatalog {
    root: PathBuf,
    version: DefinitionVersion,
}

impl LocalCatalog {
    /// Points a catalog at a directory.
    ///
    /// The version is supplied rather than derived, because what identifies a snapshot of a
    /// directory is not something the directory knows: it is a commit id, a build number or a tag,
    /// and only the caller has it. Deriving it from the digest would make the two say the same thing
    /// twice and leave no way to tell two builds of identical content apart.
    pub const fn new(root: PathBuf, version: DefinitionVersion) -> Self {
        Self { root, version }
    }

    #[inline]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every document under the root, in sorted order.
    ///
    /// Depth-first with the entries of each directory sorted, so the traversal is a function of the
    /// tree rather than of the filesystem. Files that are not documents are skipped rather than
    /// refused, so a `README.md`-adjacent `.gitkeep` or a stray image does not break a load; a
    /// document with the right extension and the wrong content still fails loudly.
    ///
    /// **A symlink is one of the things that is not a document, and that is what bounds the walk.**
    /// Catalog content is untrusted, and a link pointing at an ancestor is a cycle: the walk descends
    /// into it, finds the link again one level down, descends again, and stops only where the kernel
    /// refuses to resolve any more links in one path - 40 of them on Linux, measured rather than
    /// assumed. So the failure is not a hang; it is the same document collected once per level, which
    /// is 41 copies of one metric handed to `Definitions::assemble` and a digest that depends on the
    /// link structure rather than on the definitions. Nothing about that reads as "this catalog has a
    /// loop in it". A loop built out of real directories - a bind mount of an ancestor - has no such
    /// kernel limit and would not terminate; only a visited set catches that one, and nothing in this
    /// repository mounts anything into a catalog.
    ///
    /// The decision is made from the directory entry's own type rather than from the path, because
    /// `Path::is_dir` follows the link and answers about the target.
    fn documents(&self) -> Result<Vec<PathBuf>, LocalCatalogError> {
        if !self.root.is_dir() {
            return Err(LocalCatalogError::NotADirectory { path: self.root.clone() });
        }
        let mut found = BTreeSet::new();
        let mut pending = vec![self.root.clone()];
        while let Some(directory) = pending.pop() {
            let entries = std::fs::read_dir(&directory).map_err(|cause| LocalCatalogError::Io {
                path: directory.clone(),
                cause,
            })?;
            for entry in entries {
                let entry = entry.map_err(|cause| LocalCatalogError::Io {
                    path: directory.clone(),
                    cause,
                })?;
                let path = entry.path();
                // The entry's OWN type, which says nothing about what a link points at. That is the
                // whole fix: `path.is_dir()` follows the link, so a link to an ancestor came back
                // as a directory and the walk descended into itself.
                let kind = entry.file_type().map_err(|cause| LocalCatalogError::Io {
                    path: path.clone(),
                    cause,
                })?;
                // `is_file` and not `!is_dir()`, which is what `filetype_is_file` asks for: the lint's
                // point is that `is_file` is false for a socket, a FIFO or a device node, and being
                // false for those is exactly what this wants. A catalog document is a regular file;
                // anything else carrying a `.md` name is one of the things this walk skips.
                #[expect(
                    clippy::filetype_is_file,
                    reason = "a document is a regular file - a link, a socket or a device node is not, and skipping those is the point"
                )]
                let is_document = kind.is_file() && path.extension().is_some_and(|ext| ext == DOCUMENT_EXTENSION);
                if kind.is_dir() {
                    pending.push(path);
                } else if is_document {
                    // A `BTreeSet` rather than a sort at the end: the ordering is the point, and
                    // making it a property of the collection means it cannot be forgotten.
                    found.insert(path);
                }
            }
        }
        if found.is_empty() {
            return Err(LocalCatalogError::Empty { path: self.root.clone() });
        }
        Ok(found.into_iter().collect())
    }

    /// Reads every document and turns it into domain types.
    fn read_all(&self) -> Result<Definitions, LocalCatalogError> {
        let mut models: Vec<Model> = Vec::new();
        let mut relationships: Vec<Relationship> = Vec::new();
        let mut metrics: Vec<Metric> = Vec::new();

        for path in self.documents()? {
            let text = std::fs::read_to_string(&path).map_err(|cause| LocalCatalogError::Io {
                path: path.clone(),
                cause,
            })?;
            let split = frontmatter::split(&text).map_err(|cause| LocalCatalogError::Malformed {
                path: path.clone(),
                cause,
            })?;
            let probe: KindProbe =
                serde_norway::from_str(split.frontmatter()).map_err(|cause| LocalCatalogError::UnknownKind {
                    path: path.clone(),
                    cause,
                })?;
            let description = String::from(split.body());
            match probe.kind() {
                DocumentKind::Model => {
                    let doc: ModelDoc = Self::parse(&path, split.frontmatter(), probe.kind())?;
                    models.push(doc.into_domain(description));
                }
                DocumentKind::Relationship => {
                    let doc: RelationshipDoc = Self::parse(&path, split.frontmatter(), probe.kind())?;
                    relationships.push(doc.into_domain());
                }
                DocumentKind::Metric => {
                    let doc: MetricDoc = Self::parse(&path, split.frontmatter(), probe.kind())?;
                    metrics.push(doc.into_domain(description).map_err(|cause| LocalCatalogError::Metric {
                        path: path.clone(),
                        cause,
                    })?);
                }
            }
        }

        Definitions::assemble(models, relationships, metrics).map_err(|cause| LocalCatalogError::Inconsistent { cause })
    }

    fn parse<T>(path: &Path, frontmatter: &str, kind: DocumentKind) -> Result<T, LocalCatalogError>
    where
        T: serde::de::DeserializeOwned,
    {
        serde_norway::from_str(frontmatter).map_err(|cause| LocalCatalogError::Frontmatter {
            path: PathBuf::from(path),
            kind: kind.as_str(),
            cause,
        })
    }
}

impl SemanticCatalog for LocalCatalog {
    type Error = LocalCatalogError;

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let definitions = self.read_all()?;
        let digest = digest_of(&definitions)?;
        Ok(PinnedDefinitions::new(self.version.clone(), digest, definitions))
    }
}

#[cfg(test)]
mod tests {
    /// Unix only, and that is a portability statement rather than a gap in the suite.
    ///
    /// The walk is what these tests exercise, and provoking the case that broke it needs a symlink.
    /// Creating one on Windows requires either developer mode or an elevated process, so a test that
    /// created one there would fail on a plain checkout for a reason that has nothing to do with this
    /// code. Everything that gates - the container and CI - is Linux, so the case is covered where the
    /// verdict is taken; the fix itself is `DirEntry::file_type`, which does not follow a link on either
    /// platform.
    #[cfg(unix)]
    mod symlinks {
        use crate::LocalCatalog;
        use std::path::PathBuf;
        use sutura_domain::pinned::DefinitionVersion;

        fn catalog(root: PathBuf) -> LocalCatalog {
            LocalCatalog::new(root, DefinitionVersion::parse("test-1").expect("a test version is a version"))
        }

        /// An empty directory of this test's own, cleared on the way IN.
        ///
        /// `tempfile` is not a dependency of this workspace and one test is not the argument for adding
        /// one; `xtask` builds its scratch directories the same way. Cleared before rather than after so
        /// a failing run leaves its evidence on disk and the next run still starts clean.
        fn scratch(name: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!("sutura-catalog-local-{name}-{}", std::process::id()));
            drop(std::fs::remove_dir_all(&dir));
            std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
            dir
        }

        #[test]
        fn a_symlink_pointing_at_an_ancestor_does_not_make_the_walk_descend_into_itself() {
            // The bug: `path.is_dir()` follows the link, so `root/loop` answered "directory" and the
            // walk pushed it, found `root/loop/loop` one level down, pushed that, and kept going. What
            // this asserted before the fix is worth recording, because it is not what it looks like:
            // the walk terminated - the kernel stops resolving after 40 links in one path - and
            // returned `revenue.md` 41 times, once per level. So the failure was not a hang, it was one
            // metric defined 41 times and a digest that depended on the link structure.
            let root = scratch("symlink-loop");
            std::fs::write(root.join("revenue.md"), "---\nkind: metric\n---\n").expect("a document is writable");
            std::os::unix::fs::symlink(&root, root.join("loop")).expect("a symlink to the root is creatable");

            let found = catalog(root.clone())
                .documents()
                .expect("the walk terminates and reports the one document");

            assert_eq!(found, vec![root.join("revenue.md")]);
            drop(std::fs::remove_dir_all(&root));
        }

        #[test]
        fn a_symlink_to_a_document_outside_the_root_is_skipped_rather_than_followed() {
            // The same rule seen from the other side, and the reason it is a rule rather than a
            // cycle-detector: a visited set would still have followed this one. A catalog is what is IN
            // the directory, so a link out of it is not a document - and the alternative is a catalog
            // whose digest depends on a file the tree does not contain.
            let root = scratch("symlink-out");
            let outside = scratch("symlink-out-target");
            let target = outside.join("elsewhere.md");
            std::fs::write(&target, "---\nkind: metric\n---\n").expect("a document is writable");
            std::fs::write(root.join("revenue.md"), "---\nkind: metric\n---\n").expect("a document is writable");
            std::os::unix::fs::symlink(&target, root.join("linked.md")).expect("a symlink to a file is creatable");

            let found = catalog(root.clone()).documents().expect("the walk reports the real document");

            assert_eq!(found, vec![root.join("revenue.md")]);
            drop(std::fs::remove_dir_all(&root));
            drop(std::fs::remove_dir_all(&outside));
        }
    }
}

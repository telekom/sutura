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

use sutura_domain::catalog::{Definitions, InconsistentDefinitions, Metric, Model, Relationship};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};

use crate::document::{DocumentKind, InvalidMetricDocument, KindProbe, MetricDoc, ModelDoc, RelationshipDoc};
use crate::frontmatter::MalformedDocument;

/// The extension a catalog document has to have.
const DOCUMENT_EXTENSION: &str = "md";

/// Why a directory could not be read as a catalog.
///
/// Every variant carries the path, because a catalog is many files and "invalid type: integer" with
/// no file name is a message that sends the reader to read all of them.
///
/// **The variant is the contract, so a variant that names the wrong failure is a false contract even
/// with a truthful `#[source]` beneath it.** This enum had one that did: every failure of the
/// kind-probe deserialization became `UnknownKind`, whose message was "declares no `kind`". A
/// document saying `kind: dashboard`, one saying `kind: 3`, and one whose YAML did not parse at all
/// were three different problems reported as the same missing key - and a caller matching on the
/// variant, which is the only thing a caller can match on, was told something untrue in two cases out
/// of three. Keeping the parse error reachable through the chain did not fix that; it only meant the
/// truth was available to whoever thought to look past the variant.
///
/// It is two variants now, and the line between them is a mechanism rather than a guess at an error
/// message: [`Self::MalformedFrontmatter`] is raised when the block does not parse as YAML at all,
/// and [`Self::IdentifyKind`] when it parses and still does not identify the document. They are two
/// rather than four - missing, unrecognised, wrong type - because nothing in this workspace matches
/// on any of them, so a split finer than the remedy is a branch nobody takes: "your frontmatter is
/// not YAML" and "your frontmatter does not say what this is" send a reader to different places, and
/// "the `kind` key is missing" versus "its value is not one of three" send them to the same one. A
/// finer split is a cheap change if a caller ever needs the branch.
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
    #[error("the frontmatter of {path} is not YAML")]
    MalformedFrontmatter {
        path: PathBuf,
        #[source]
        cause: serde_norway::Error,
    },
    #[error("the frontmatter of {path} does not say what kind of document it is")]
    IdentifyKind {
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
    /// The domain could not hash the definitions.
    ///
    /// One variant rather than the two this used to have. Those two - the canonical form failing to
    /// serialize, and the resulting hex failing to parse as a digest - are now both inside the
    /// domain's own hashing, and neither is a fact about reading a directory. The chain still says
    /// which one happened.
    #[error("the definitions could not be hashed")]
    Digest {
        #[source]
        cause: NotDigestible,
    },
}

// The canonical form and its hash used to live here, and a review showed why they could not: while
// `PinnedDefinitions::pin` took the hashing FUNCTION, safe public code could hand it one that
// ignored its argument and pair any digest with any set of definitions. The hash is now the domain's
// own, so this adapter reads documents and nothing else - and there is one canonical form rather
// than one per adapter, which is what a second catalog adapter would otherwise have had to
// reimplement or import from here.

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
            // Two passes, and the first one exists to tell two failures apart. Deserializing
            // `KindProbe` straight from the text collapses "this is not YAML" and "this YAML does not
            // identify itself" into one error, and the variant then has to claim one of them for
            // both. Parsing to a `Value` first answers the YAML question on its own terms, so each
            // failure gets the variant that is true of it.
            //
            // **The probe's own error cannot be trusted to make that distinction, and this was
            // measured rather than assumed.** `kind: [metric` followed by another key is an
            // unterminated flow sequence - not YAML at all - and the probe reports "invalid type:
            // sequence" for it, a message about a data shape in a document that has no shape. It can
            // do that because it stops caring once it has read one key, so it never reaches the point
            // where the document falls apart. Parsing to a `Value` has to read the whole block, which
            // is exactly why its verdict is the one worth having.
            //
            // The parsed value is not kept: `KindProbe` reads one key, and `from_value` takes a
            // `Value` by move, so threading it through would cost a clone of the whole block to save
            // a parse of it. What this pass produces is the verdict, not the data.
            if let Err(cause) = serde_norway::from_str::<serde_norway::Value>(split.frontmatter()) {
                return Err(LocalCatalogError::MalformedFrontmatter {
                    path: path.clone(),
                    cause,
                });
            }
            let probe: KindProbe =
                serde_norway::from_str(split.frontmatter()).map_err(|cause| LocalCatalogError::IdentifyKind {
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
        // `pin` hashes the definitions it is about to store, using the domain's own canonical form.
        // This adapter no longer supplies the hasher, and that is the point: while it did, safe
        // public code could pass a function that ignored its argument and pair any digest with any
        // definitions. There is nothing to pass now, so there is nothing to get wrong.
        PinnedDefinitions::pin(self.version.clone(), definitions).map_err(|cause| LocalCatalogError::Digest { cause })
    }
}

#[cfg(test)]
mod tests {
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

    /// What a document failed to be, given the whole document.
    ///
    /// A catalog of exactly one file, so the error is about that file and nothing else: an empty
    /// directory is its own error here, and a second document would let `Definitions::assemble` fail
    /// first for a reason these tests are not about.
    fn error_for(name: &str, document: &str) -> crate::LocalCatalogError {
        let root = scratch(name);
        std::fs::write(root.join("doc.md"), document).expect("a document is writable");
        let err = catalog(root.clone()).read_all().expect_err("this document cannot load");
        drop(std::fs::remove_dir_all(&root));
        err
    }

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
        use super::{catalog, scratch};

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

    /// The variant a document's failure lands in, which is the part a caller matches on.
    mod kinds {
        use super::error_for;
        use crate::LocalCatalogError;

        #[test]
        fn a_frontmatter_block_that_is_not_yaml_is_not_reported_as_a_missing_kind() {
            // The finding. Every failure of the kind probe used to become one variant whose message
            // read "declares no `kind`", so a syntax error at line 2 was reported as an absent key -
            // a false statement about the file, in the one field a caller can branch on. The source
            // chain carried the truth, which is not the same as the error stating it.
            let broken = error_for("kind-broken-yaml", "---\nkind: [metric\nname: revenue\n---\nProse.\n");
            assert!(
                matches!(broken, LocalCatalogError::MalformedFrontmatter { .. }),
                "a frontmatter block that is not YAML must say so: {broken:?}"
            );
            assert!(
                core::error::Error::source(&broken).is_some(),
                "the parse failure names the line, so it has to stay reachable: {broken:?}"
            );
        }

        #[test]
        fn a_document_that_parses_and_does_not_identify_itself_is_one_neutral_variant() {
            // Three different ways of not saying what a document is, and one variant for all three,
            // because nothing in this workspace branches on the difference and each one sends the
            // author to the same line of the same file. The variant is neutral for that reason: it
            // says the frontmatter does not identify the document rather than claiming which of the
            // three it was.
            for (name, document) in [
                ("kind-unknown", "---\nkind: dashboard\nname: revenue\n---\nProse.\n"),
                ("kind-wrong-type", "---\nkind: 3\nname: revenue\n---\nProse.\n"),
                ("kind-missing", "---\nname: revenue\n---\nProse.\n"),
            ] {
                let err = error_for(name, document);
                assert!(
                    matches!(err, LocalCatalogError::IdentifyKind { .. }),
                    "{name} must be an IdentifyKind failure: {err:?}"
                );
                assert!(
                    core::error::Error::source(&err).is_some(),
                    "{name} must keep the parse failure reachable as a source"
                );
            }
        }

        #[test]
        fn malformed_yaml_and_an_unrecognised_kind_are_different_variants() {
            // The assertion the finding actually asks for, made on the discriminant rather than on a
            // message: two failures that a caller has to be able to tell apart must not be one
            // variant, whatever their messages say.
            let broken = error_for("split-broken-yaml", "---\nkind: [metric\n---\nProse.\n");
            let unknown = error_for("split-unknown-kind", "---\nkind: dashboard\n---\nProse.\n");
            assert_ne!(
                core::mem::discriminant(&broken),
                core::mem::discriminant(&unknown),
                "these must not collapse into one variant: {broken:?} / {unknown:?}"
            );
        }
    }
}

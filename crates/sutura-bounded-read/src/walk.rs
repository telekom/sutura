//! The sorted directory walk over a catalog root, refused when empty.
//!
//! `read_dir` returns entries in whatever order the filesystem chose, and a digest that moves
//! between two runs over unchanged files is a digest nobody can act on - so the walk collects into
//! a `BTreeSet` and yields sorted, making the ordering a property of the collection rather than a
//! sort at the end that could be forgotten. Files that are not a document are skipped rather than
//! refused (so a stray `README.md` does not break a load); a regular file with the right extension
//! and the wrong content still fails loudly later. An empty or missing root is an error rather than
//! a silently-empty catalog: a mistyped path that happens to exist would otherwise load a catalog
//! with no documents and its declaration would then claim a digest nothing read.
//!
//! Which extensions count as a document is the adapter's own choice - the local catalog reads
//! `md`, the OKF and data-contract catalogs read `yaml`/`yml` - so it is a parameter.
//!
//! A symlink is one of the things that is not a document, and that is what bounds the walk. Catalog
//! content is untrusted, and a link pointing at an ancestor is a cycle: the walk descends into it,
//! finds the link again one level down, descends again, and stops only where the kernel refuses to
//! resolve any more links in one path - 40 of them on Linux. So the failure is not a hang; it is a
//! document collected once per level. A loop built out of real directories (a bind mount of an
//! ancestor) has no such kernel limit and would not terminate; only a visited set catches that one,
//! and nothing hosts a catalog that mounts something into it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The most documents a catalog root may hold.
///
/// A startup bound, checked on every `BTreeSet` insert - a directory built to be large is refused
/// as soon as it is large enough, rather than walked to its end first. **Documents, not entries**:
/// a directory holding this many document-shaped files plus an unbounded number of other entries
/// (subdirectories, non-document files, a skipped symlink) is unaffected - this bounds what becomes
/// a document, not the size of the tree it lives in.
pub const MAX_CATALOG_DOCUMENTS: usize = 1_000;

/// Why the walk could not produce the sorted document list.
#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    /// The root path does not name a directory.
    #[error("the catalog root {path} is not a directory")]
    NotADirectory { path: PathBuf },
    /// `read_dir` on a directory, or an entry or its type within it, failed.
    #[error("could not read {path}")]
    Io {
        path: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    /// The walk found more documents than [`walk`]'s `max_documents` parameter permits.
    ///
    /// `path` is the catalog root, `found` is how many document-shaped entries the walk had counted
    /// when it stopped - which may be less than the directory's true total, because the walk refuses
    /// as soon as it crosses `limit` rather than finishing the tree first.
    #[error("the catalog at {path} holds more than {limit} documents ({found} found before the walk stopped)")]
    TooManyDocuments { path: PathBuf, found: usize, limit: usize },
    /// The root is a directory that holds no documents.
    #[error("the catalog at {path} holds no documents")]
    Empty { path: PathBuf },
}

/// Every document under `root`, in sorted order, refused when empty or missing.
///
/// Depth-first with the entries of each directory sorted, so the traversal is a function of the
/// tree rather than of the filesystem. `extensions` is the set of file extensions treated as
/// documents; anything else is skipped. A document with the right extension and the wrong content
/// still fails loudly at deserialisation, which is the caller's job.
///
/// The decision is made from the directory entry's own type rather than from the path, because
/// `Path::is_dir` follows a link and answers about the target.
///
/// # Errors
///
/// [`WalkError::NotADirectory`] when `root` is not a directory; [`WalkError::Io`] for a failure to
/// read `root` or a directory within it; [`WalkError::TooManyDocuments`] once a directory crosses
/// `max_documents`; [`WalkError::Empty`] when `root` holds no documents.
pub fn walk(root: &Path, extensions: &[&str], max_documents: usize) -> Result<Vec<PathBuf>, WalkError> {
    if !root.is_dir() {
        return Err(WalkError::NotADirectory {
            path: root.to_path_buf(),
        });
    }
    let mut found = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory).map_err(|cause| WalkError::Io {
            path: directory.clone(),
            cause,
        })?;
        for entry in entries {
            let entry = entry.map_err(|cause| WalkError::Io {
                path: directory.clone(),
                cause,
            })?;
            let path = entry.path();
            // The entry's OWN type, which says nothing about what a link points at. That is the
            // whole fix: `path.is_dir()` follows the link, so a link to an ancestor came back as a
            // directory and the walk descended into itself.
            let kind = entry.file_type().map_err(|cause| WalkError::Io {
                path: path.clone(),
                cause,
            })?;
            // `is_file` and not `!is_dir()`, which is what `filetype_is_file` asks for: the lint's
            // point is that `is_file` is false for a socket, a FIFO or a device node, and being
            // false for those is exactly what this wants. A catalog document is a regular file;
            // anything else carrying a document name is one of the things this walk skips.
            #[expect(
                clippy::filetype_is_file,
                reason = "a document is a regular file - a link, a socket or a device node is not, and skipping those is the point"
            )]
            let is_document = kind.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| extensions.contains(&ext));
            if kind.is_dir() {
                pending.push(path);
            } else if is_document {
                // A `BTreeSet` rather than a sort at the end: the ordering is the point, and making
                // it a property of the collection means it cannot be forgotten.
                found.insert(path);
                // Checked on every insert, not once after the walk finishes: a directory built to be
                // large is refused as soon as it is large enough, rather than walked to its end
                // first. `pending`'s remaining entries are dropped with the early return, so a
                // directory with more documents past this one is never listed.
                if found.len() > max_documents {
                    return Err(WalkError::TooManyDocuments {
                        path: root.to_path_buf(),
                        found: found.len(),
                        limit: max_documents,
                    });
                }
            }
        }
    }
    if found.is_empty() {
        return Err(WalkError::Empty {
            path: root.to_path_buf(),
        });
    }
    Ok(found.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::WalkError;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-bounded-read-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    #[test]
    fn documents_come_back_sorted_regardless_of_write_order() {
        let root = scratch("sorted");
        // Write in reverse order; the walk must yield lexical order anyway.
        std::fs::write(root.join("z.yaml"), "z").expect("a document is writable");
        std::fs::write(root.join("a.yaml"), "a").expect("a document is writable");
        std::fs::write(root.join("m.yaml"), "m").expect("a document is writable");
        let docs = super::walk(&root, &["yaml"], super::MAX_CATALOG_DOCUMENTS).expect("the walk succeeds");
        let names: Vec<_> = docs.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, vec!["a.yaml", "m.yaml", "z.yaml"]);
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn an_non_document_extension_is_skipped_not_refused() {
        let root = scratch("skip-other");
        std::fs::write(root.join("a.yaml"), "a").expect("a document is writable");
        std::fs::write(root.join("README.md"), "readme").expect("a document is writable");
        let docs = super::walk(&root, &["yaml"], super::MAX_CATALOG_DOCUMENTS).expect("the walk succeeds");
        assert_eq!(docs.len(), 1, "a .yaml and a .md with a yaml walk yields one");
        assert_eq!(docs[0].file_name().unwrap().to_str().unwrap(), "a.yaml");
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn an_empty_directory_is_refused() {
        let root = scratch("empty");
        let err = super::walk(&root, &["yaml"], super::MAX_CATALOG_DOCUMENTS).expect_err("an empty root refuses");
        assert!(matches!(err, WalkError::Empty { .. }), "{err:?}");
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn a_missing_directory_is_refused_as_not_a_directory() {
        let err = super::walk(
            &PathBuf::from("/nonexistent/definitely"),
            &["yaml"],
            super::MAX_CATALOG_DOCUMENTS,
        )
        .expect_err("a missing root refuses");
        assert!(matches!(err, WalkError::NotADirectory { .. }), "{err:?}");
    }
}

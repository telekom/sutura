//! The scratch-tree implementation - the content `scratch_tree/mod.rs` re-exports. Kept one file
//! below the module root so every definition sits where `clippy::definition_in_module_root` wants
//! it, and so the `#[path = "scratch_tree/mod.rs"]` include stays one line in every including file.

use std::path::{Path, PathBuf};

/// One fixture file: a repo-relative path and its contents. Bytes rather than `&str` so a test can
/// plant binary data, which is the case #412 records reddening a correct tree.
pub(crate) type Fixture<'a> = (&'a str, &'a [u8]);

/// A scratch tree, removed when this value drops.
pub(crate) struct Tree {
    root: PathBuf,
    /// Paths [`Tree::seal`] made unreadable, so `Drop` can open them again before the sweep.
    sealed: Vec<PathBuf>,
}

impl Tree {
    /// Build a tree holding exactly `files`, as `(repo-relative path, contents)`.
    ///
    /// `tag` separates one test's tree from another's in the same process; the pid separates one
    /// process from another, which is what nextest's per-test process gives for free.
    pub(crate) fn of(tag: &str, files: &[Fixture<'_>]) -> Self {
        let root = std::env::temp_dir().join(format!("sutura-{tag}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(&root).expect("a scratch tree");
        let tree = Self {
            root,
            sealed: Vec::new(),
        };
        for (rel, body) in files {
            let at = tree.root.join(rel);
            if let Some(parent) = at.parent() {
                std::fs::create_dir_all(parent).expect("a parent directory");
            }
            std::fs::write(&at, body).expect("a fixture file");
        }
        tree
    }

    /// The tree's root, to hand a census door.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Make one file unreadable, and say whether the mode actually took effect.
    ///
    /// **The `false` return is not a formality.** Mode bits are ignored for uid 0, and a gate
    /// suite runs as root in some venues, so a test that asserts a refusal without checking this
    /// asserts nothing there. Callers return early rather than assert - `repo`'s own
    /// `an_unreadable_subtree_refuses_instead_of_shrinking_the_walk` is the derivation this copies.
    #[cfg(unix)]
    pub(crate) fn seal(&mut self, rel: &str) -> bool {
        use std::os::unix::fs::PermissionsExt as _;

        let at = self.root.join(rel);
        std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o000)).expect("chmod the fixture");
        self.sealed.push(at.clone());
        std::fs::read(&at).is_err()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        #[cfg(unix)]
        for at in &self.sealed {
            use std::os::unix::fs::PermissionsExt as _;
            drop(std::fs::set_permissions(at, std::fs::Permissions::from_mode(0o644)));
        }
        drop(std::fs::remove_dir_all(&self.root));
    }
}

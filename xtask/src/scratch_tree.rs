//! A throwaway tree for a gate test, swept when the binding drops.
//!
//! **One copy rather than one per gate test module.** The shape - `temp_dir().join(format!(..
//! process::id()))`, `create_dir_all`, write the fixture, `remove_dir_all` - is written in roughly
//! thirty test modules in this tree already, and the migration in
//! `github.com/telekom/sutura#619` needed three more of it *plus* a `chmod` and a restore. A copy
//! that has to restore a mode before `remove_dir_all` is a copy that leaks a mode-`000` directory
//! into the system temp directory whenever an assert fires first, so the restore belongs in a
//! `Drop` that runs either way.
//!
//! **The copy that keys the path on the process id AND creates it exclusively is the one that
//! fails, and it fails LATER than the run that wrote it** - `github.com/telekom/sutura#938`. A pid
//! is unique among live processes and not across runs, so a killed or panicking run leaves the
//! directory and the next run with that pid recycled panics in SETUP:
//! `new fixture directory: Os { code: 17, kind: AlreadyExists }`, before asserting anything. Worst
//! of all it surfaces under `just gates` and not `just validate`, because the leg that runs those
//! cells (`check-default-feature-tests`) is one of the two `just validate` does not run - so it is
//! invisible to CI and blames whoever gates locally. `Tree::of` sweeps before it creates, which
//! keeps the exclusivity that pattern wanted, and `Drop` sweeps on the way out so a killed run
//! poisons nothing. What holds it: `clippy::create_dir` is `restriction`, so `just lint` refuses a
//! bare `std::fs::create_dir` anywhere in the workspace - test code included - and after #938 no
//! `#[expect]` for it is left. THE LIMIT: an author can still write the allowance back, so the
//! lint refuses the silent copy rather than a deliberate one.
//!
//! **jscpd would not have reported the next copy**: its threshold is 30 lines / 250 tokens and
//! each copy is around fifteen. That is the signal's stated limit rather than a reason to lower it.
//! Nothing here weakens a gate - this module is `#[cfg(test)]` and ships in no binary.

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

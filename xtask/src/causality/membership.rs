//! What a crate this branch ADDED takes with it when the reconstruction reverts its sources.
//!
//! **THE DEFECT, measured on a branch adding `crates/sutura-exec-oracle`** -
//! `github.com/telekom/sutura#936`. `super::provenance::Reach` sorts every `Cargo.toml` into
//! [`super::provenance::Reach::BuildInput`] and holds it at HEAD, for a reason that is right about
//! a manifest whose package already existed: reverting one changes what cargo RESOLVES, and a test
//! file kept at HEAD needing a dependency this branch added would stop compiling. A manifest this
//! branch CREATED is the other case, and the rule read them the same way. The reconstruction
//! removed the new crate's `src/*.rs` and left its manifest declaring them, so cargo refused
//! before compiling anything:
//!
//! ```text
//! error: failed to parse manifest at `crates/sutura-exec-oracle/Cargo.toml`
//! no targets specified in the manifest
//! ```
//!
//! which the gate reports as `INCONCLUSIVE - cargo never reached the compiler` at exit 3. Correctly
//! not a pass - and it meant the repository's only mechanical refusal of a test that passes both
//! ways was unavailable for **every branch that adds a crate**, which is how every adapter arrives.
//!
//! WHAT THE WITHDRAWAL IS, and the scope is measured rather than reasoned. A workspace member is
//! its manifest AND its membership, and the membership is spread over more files than the crate's
//! own: the root manifest lists it in `members` and in `workspace.dependencies`, `Cargo.lock` holds
//! its package entry, and the manifest of whatever DEPENDS on it inherits that entry. Taken in
//! three steps on the oracle branch, each answer from `cargo metadata` over the reconstructed tree:
//!
//! | Base tree | cargo | What it said |
//! | --- | --- | --- |
//! | sources at base, every manifest at HEAD (the old rule) | 101 | `no targets specified in the manifest` |
//! | plus the new manifest gone and the root manifest and lock at base | 101 | ``dependency.sutura-exec-oracle` was not found in `workspace.dependencies`` - from `crates/sutura-app/Cargo.toml`, an EXISTING member's manifest held at HEAD |
//! | plus every changed member manifest at base | 0 | resolved |
//!
//! So [`withdrawn`] takes **every changed manifest and the lockfile**, not just the new crate's.
//! Narrower than that does not resolve, which is the second row. The same answer covers the second
//! half of #936's question: a gate that reads the member list is reverted with the list it reads,
//! because the list is the root manifest and the root manifest is in this set.
//!
//! WHAT IT REFUSES TO DO, because a withdrawal is only sound when the crate goes whole:
//!
//! * **A new crate with a file held at HEAD keeps its manifest.** A held file carries tests the
//!   proof measures; removing the manifest would take the package those tests compile into out of
//!   the workspace, and nextest would then match nothing - a loud failure about a tree this module
//!   broke rather than a verdict about the change. So one such file withdraws nothing, and the
//!   retry that puts everything at base is where the withdrawal happens.
//! * **All the added crates or none.** Withdrawing one while another stays leaves the root manifest
//!   at HEAD naming a member whose manifest is gone, which is the same refusal in a new place.
//!
//! **THE LIMIT, next to the claim - two of them.** Reverting every changed manifest also reverts
//! any OTHER dependency the branch added, so a test file kept at HEAD that needs one stops
//! compiling: the verdict is then `INCONCLUSIVE - the base tree does not build`, still not a pass,
//! still loud, and it names a cause an author can act on rather than one about a manifest nobody
//! asked to keep. And what was measured is that the reconstructed workspace RESOLVES - `cargo
//! metadata` at exit 0 - which is the boundary the old rule failed at. Whether it then COMPILES is
//! the retry's own arm and is not claimed here.

use std::path::Path;

use super::provenance::Commit;

/// What this branch ADDED to the workspace, asked of git once and answered per attempt.
///
/// A type rather than two free functions because the git half and the decision half are one
/// question - *is this manifest's package new here* - and the answer is needed twice, for two
/// attempts whose held-at-HEAD sets differ.
pub(super) struct Membership {
    /// Every changed build input the base commit does not have.
    added: Vec<String>,
}

impl Membership {
    /// Which of `inputs` this branch created.
    ///
    /// The question `super::provenance::Reach` cannot answer, because it decides from the path
    /// alone: whether the file exists at base. A manifest the base HAS is the case that rule is
    /// right about - hold it at HEAD, or a dependency this branch added goes missing from a test
    /// that needs it. One the base does not have may be a member whose sources the reconstruction
    /// is about to remove.
    pub(super) fn of(root: &Path, base: &Commit, inputs: &[String]) -> Self {
        Self {
            added: inputs
                .iter()
                .filter(|path| !super::worktree::base_has(root, base, path))
                .cloned()
                .collect(),
        }
    }

    /// One attempt's file list: what it reverts, plus the membership a withdrawn crate takes.
    ///
    /// Concatenated into one list rather than applied separately, because "revert to base" is two
    /// operations - check out the old content, or delete a file the base never had - and
    /// `super::worktree::base_state` is the one place that decides which. A withdrawal spans both:
    /// the new manifest is deleted, the root manifest and the lockfile are checked out.
    pub(super) fn reverting(&self, reverting: &[String], inputs: &[String], at_head: &[String]) -> Vec<String> {
        let mut out: Vec<String> = reverting.to_vec();
        for path in withdrawn(&self.added, inputs, at_head) {
            if !out.contains(&path) {
                out.push(path);
            }
        }
        out
    }
}

/// Is `path` a workspace member's own manifest, rather than the root one?
///
/// A new member is a directory with a `Cargo.toml` in it; the root manifest is the membership, and
/// a new one of those is a new workspace rather than a new crate.
fn member_manifest(path: &str) -> bool {
    path.ends_with("/Cargo.toml")
}

/// Does `path` decide what the workspace RESOLVES - a manifest, or the lockfile?
///
/// Named by what they decide rather than by extension: `rust-toolchain.toml` and `.cargo/config.toml`
/// are build inputs too and say nothing about membership, so they stay held at HEAD.
fn resolves_membership(path: &str) -> bool {
    path == "Cargo.lock" || path == "Cargo.toml" || member_manifest(path)
}

/// The paths an attempt must put at base so a withdrawn crate's absence is a resolvable workspace.
///
/// `added` is every changed build input this branch created; `inputs` every changed build input, so
/// the answer names only files the diff actually touched; `at_head` every compiled path this attempt
/// keeps at HEAD.
///
/// Empty when no crate was added, and empty when an added crate still has a file at HEAD - the
/// module header carries why both are refusals rather than omissions.
fn withdrawn(added: &[String], inputs: &[String], at_head: &[String]) -> Vec<String> {
    let mine: Vec<&String> = added.iter().filter(|path| member_manifest(path)).collect();
    if mine.is_empty() {
        return Vec::new();
    }
    let kept = |manifest: &str| {
        let dir = manifest.trim_end_matches("Cargo.toml");
        at_head.iter().any(|path| path.starts_with(dir))
    };
    if mine.iter().any(|manifest| kept(manifest)) {
        return Vec::new();
    }
    let mut out: Vec<String> = inputs.iter().filter(|path| resolves_membership(path)).cloned().collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::withdrawn;

    fn paths(of: &[&str]) -> Vec<String> {
        of.iter().map(|p| String::from(*p)).collect()
    }

    /// **THE VERDICT #936 COULD NOT REACH, over the branch it was measured on.** The retry puts
    /// every compiled file at base, so the new crate's sources are gone - and the whole membership
    /// goes with them: its own manifest, the root manifest that lists it, the lock entry pinning
    /// it, and the manifest of the member that DEPENDS on it. The dependent one is the row a
    /// narrower rule got wrong: with only the first three at base, `cargo metadata` still refused
    /// with ``dependency.sutura-exec-oracle` was not found in `workspace.dependencies``.
    #[test]
    fn a_new_crate_is_withdrawn_with_every_manifest_that_resolves_it() {
        let added = paths(&["crates/sutura-exec-oracle/Cargo.toml"]);
        let inputs = paths(&[
            "Cargo.lock",
            "Cargo.toml",
            "crates/sutura-app/Cargo.toml",
            "crates/sutura-exec-oracle/Cargo.toml",
        ]);

        assert_eq!(withdrawn(&added, &inputs, &[]), inputs);
    }

    /// A file of the new crate held at HEAD carries tests the proof measures, so the crate stays in
    /// the workspace: removing the manifest would take the package those tests compile into out of
    /// the build, and nextest failing an empty filterset is a statement about this module rather
    /// than about the change.
    #[test]
    fn a_crate_with_a_file_at_head_keeps_its_manifest() {
        let added = paths(&["crates/sutura-exec-oracle/Cargo.toml"]);
        let inputs = paths(&["Cargo.lock", "Cargo.toml", "crates/sutura-exec-oracle/Cargo.toml"]);
        let at_head = paths(&["crates/sutura-exec-oracle/src/lib.rs"]);

        assert_eq!(withdrawn(&added, &inputs, &at_head), Vec::<String>::new());
    }

    /// Two added crates, one of them still held: withdrawing the other would leave the root
    /// manifest at HEAD naming a member whose manifest is gone - the same refusal in a new place.
    #[test]
    fn one_held_crate_holds_every_added_crate() {
        let added = paths(&["crates/a/Cargo.toml", "crates/b/Cargo.toml"]);
        let inputs = paths(&["Cargo.lock", "Cargo.toml", "crates/a/Cargo.toml", "crates/b/Cargo.toml"]);

        assert_eq!(
            withdrawn(&added, &inputs, &paths(&["crates/b/src/lib.rs"])),
            Vec::<String>::new()
        );
        // Neither held: both go, with the one membership they share.
        assert_eq!(withdrawn(&added, &inputs, &[]).len(), 4);
    }

    /// A build input that decides no membership does not travel with the crate: reverting
    /// `rust-toolchain.toml` or `.cargo/config.toml` changes what every crate compiles WITH rather
    /// than what the workspace resolves, and `super::provenance` holds those at HEAD for its own
    /// reason. Widening to *every changed build input* would revert the toolchain under a tree this
    /// module only needs to make resolvable.
    #[test]
    fn a_toolchain_or_cargo_configuration_stays_at_head() {
        let added = paths(&["crates/sutura-exec-oracle/Cargo.toml"]);
        let inputs = paths(&[
            ".cargo/config.toml",
            "Cargo.lock",
            "Cargo.toml",
            "crates/sutura-exec-oracle/Cargo.toml",
            "rust-toolchain.toml",
        ]);

        assert_eq!(
            withdrawn(&added, &inputs, &[]),
            paths(&["Cargo.lock", "Cargo.toml", "crates/sutura-exec-oracle/Cargo.toml"])
        );
    }

    /// A branch that adds no crate withdraws nothing, whatever else its diff touched. This is the
    /// everyday case and the one a wrong widening here would break: the root manifest would go to
    /// base on every branch that edits it, taking a dependency a held-at-HEAD test needs.
    #[test]
    fn a_branch_that_adds_no_crate_withdraws_nothing() {
        let inputs = paths(&["Cargo.lock", "Cargo.toml", "crates/sutura-domain/Cargo.toml"]);
        assert_eq!(withdrawn(&[], &inputs, &[]), Vec::<String>::new());
    }
}

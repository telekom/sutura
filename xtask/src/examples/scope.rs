//! Which files a run in this venue could reach, and the numbers that witness the walk over them.
//!
//! Split from the rule itself because it answers a different question: [`super`] asks *what does
//! this line of test code name*, and this asks *is this file one a run here reaches at all*. Both
//! halves of the gate's own scope live here - the workspace membership and the self-exclusion -
//! together with [`Scan`], whose two counts are what stops a truncated walk reading as a clean
//! tree.

use std::collections::BTreeSet;
use std::path::Path;

/// The crate this gate lives in, whose files are excluded from its own scan.
///
/// Not tidiness - it is the difference between a gate and a mirror, and the mirror was live one
/// file over. This gate's own fixtures hold paths under `examples/`, and `changes.rs` holds
/// `examples/single-player/...` as a classification fixture; both are test code by every rule this
/// gate uses. Measured: repointing EVERY `crates/**/*.rs` mention of `examples/single-player`
/// left the verdict green on those two fixtures alone. No figure is written here - the count
/// moved from 23 to 40 between that measurement and this sentence, which is the rot
/// `super::report`'s own doc is about; `git grep -c "examples/single-player" -- "crates/**/*.rs"`
/// answers it. No gate's test runs a deployment example, so the crate is the honest scope rather
/// than one file.
///
/// DERIVED rather than written down, because a path constant is held by recall and fails OPEN when
/// it stops matching: moving this gate into a directory module still compiles, and it silently
/// re-admitted its own fixtures with the file count as the only tell. The manifest directory's own
/// last segment cannot disagree with where this file lives, and `super::run` fails closed if it
/// excludes nothing at all.
pub(super) fn gate_crate() -> Option<&'static str> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
}

/// Is this path Rust? Case-insensitive, for the reason `crate::docs::is_markdown` gives: half of
/// this repo is developed on a filesystem that does not distinguish `.RS` from `.rs`.
fn is_rust(rel: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Is this path inside `dir`, as a whole leading path segment?
pub(super) fn is_under(rel: &str, dir: &str) -> bool {
    rel.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// What `cargo metadata` says this workspace IS: where its members live, and which source files
/// cargo compiles as the root of a target.
///
/// TWO ANSWERS FROM ONE CALL because they are one question asked twice, and the second is the only
/// thing in this gate that does not come off the git listing. See [`Workspace::target_roots`].
pub(super) struct Workspace {
    /// Every member's directory, repo-relative.
    pub(super) dirs: BTreeSet<String>,
    /// Every member target's root source file, repo-relative.
    ///
    /// THE SECOND AUTHORITY, and it exists because of a measurement rather than a worry.
    /// `Scan::expected` and `Scan::read` are two loops over ONE listing, so a walk that stops
    /// inside `crate::repo::all_files` itself moves both together: measured on this tree with
    /// `.step_by(2)` over that listing, the verdict printed `127 of 127 in-scope file(s)` at
    /// **exit 0** having read half the tree. That is the shape a sibling gate shipped as
    /// `19 of 19`. Cargo does not read git, so a file cargo compiles that this scan never saw is
    /// a disagreement between two authorities rather than a number checked against itself.
    pub(super) target_roots: BTreeSet<String>,
}

/// What `cargo metadata` says, repo-relative.
///
/// THE VENUE IS THE WORKSPACE. `just test` is `cargo nextest run --workspace --all-features`, so a
/// `#[test]` in a package `[workspace] exclude` keeps out is one no run here reaches - and this
/// gate's whole claim is *a run here reaches*. Measured before this existed: one reach planted in
/// `vendor/mimalloc_rust/src/lib.rs` was accepted as the SOLE evidence for a variant, exit 0, and
/// the vendored allocator contributed 10 declarations and 2 files to the counts the verdict
/// printed.
///
/// DERIVED, for `crate::fmt`'s reason one question over - that module needs the member NAMES for
/// `cargo fmt -p`, this one needs their DIRECTORIES, and both fail open when written down: a
/// hardcoded list works today and silently stops covering a crate the day one is added. Both come
/// off the packages cargo reports rather than the `members` array in the root manifest, because
/// that array holds globs and cargo is the authority on what they expand to.
pub(super) fn workspace(root: &Path) -> Result<Workspace, String> {
    // CANONICALISED on both sides before the prefix is stripped: `crate::repo::root` comes from
    // git and cargo's paths come from cargo, and a symlinked checkout - `/tmp` on darwin is
    // `/private/tmp` - makes two spellings of one directory. The failure mode of a mismatch is a
    // hard red, so the cheap normalisation is worth more than the exactness of leaving it out.
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let metadata = crate::cargo_metadata(&["--no-deps"])?;
    let packages = metadata
        .get("packages")
        .and_then(|packages| packages.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;
    let mut found = Workspace {
        dirs: BTreeSet::new(),
        target_roots: BTreeSet::new(),
    };
    for package in packages {
        let manifest = package
            .get("manifest_path")
            .and_then(|path| path.as_str())
            .ok_or_else(|| String::from("a package in cargo metadata has no `manifest_path`"))?;
        let directory = Path::new(manifest)
            .parent()
            .ok_or_else(|| format!("`{manifest}` names no directory"))?
            .to_path_buf();
        found.dirs.insert(relative(&directory, &root)?);
        let targets = package
            .get("targets")
            .and_then(|targets| targets.as_array())
            .ok_or_else(|| format!("`{manifest}` reports no `targets` array"))?;
        for target in targets {
            let source = target
                .get("src_path")
                .and_then(|path| path.as_str())
                .ok_or_else(|| format!("a target of `{manifest}` has no `src_path`"))?;
            found.target_roots.insert(relative(Path::new(source), &root)?);
        }
    }
    if found.dirs.is_empty() {
        return Err(String::from("cargo metadata reported no workspace packages"));
    }
    if found.target_roots.is_empty() {
        // FAIL CLOSED on the second authority itself: an empty target list satisfies the
        // comparison below vacuously, which is the floor-with-no-denominator shape one level up.
        return Err(String::from("cargo metadata reported no workspace targets"));
    }
    Ok(found)
}

/// `path` under `root`, as a repo-relative `/`-separated string.
fn relative(path: &Path, root: &Path) -> Result<String, String> {
    let owned = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let rel = owned
        .strip_prefix(root)
        .map_err(|error| format!("`{}` is outside `{}`: {error}", owned.display(), root.display()))?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// Is this path inside one of the workspace's member directories?
///
/// An empty entry is a package AT the repo root, under which every path qualifies - not this
/// workspace, whose root manifest is virtual, but a layout this must not silently drop.
pub(super) fn under_any(rel: &str, dirs: &BTreeSet<String>) -> bool {
    dirs.iter().any(|dir| dir.is_empty() || is_under(rel, dir))
}

/// Is this file Rust the walk should visit, before the self-exclusion is applied?
///
/// [`in_scope`] minus the exclusion, because the walk has to SEE an excluded file to count it -
/// an exclusion that removed nothing is the broken scan `super::problems` refuses on.
pub(super) fn is_candidate(rel: &str, members: &BTreeSet<String>) -> bool {
    is_rust(rel) && under_any(rel, members)
}

/// Is this file one whose test code a run in this venue could reach?
///
/// The predicate the DENOMINATOR is counted with, written in terms of the one the walk filters on
/// so the two cannot drift: a second spelling of the same rule is exactly the thing that would
/// make `Scan::expected` and `Scan::read` disagree over a healthy tree.
pub(super) fn in_scope(rel: &str, members: &BTreeSet<String>, crate_dir: &str) -> bool {
    is_candidate(rel, members) && !is_under(rel, crate_dir)
}

/// What the read left behind, beside the evidence itself. Named for the scan rather than
/// `Read`, which is a trait everybody already knows.
pub(super) struct Scan {
    /// The gate's own crate directory, left out of the scan.
    pub(super) crate_dir: &'static str,
    /// How many files that removed. Zero is a broken exclusion, not a clean tree.
    pub(super) excluded: usize,
    /// In-scope Rust files, counted off the LISTING rather than off the walk.
    ///
    /// The denominator for the walk, and it has to come from the other side or comparing it proves
    /// nothing: [`in_scope`] filters the same listing `super::run` iterates, so a walk that stops
    /// early moves [`Self::read`] and leaves this where it was. Without it the verdict was an
    /// at-least-one floor over hundreds of files with nothing to compare against.
    ///
    /// WHAT IT DOES NOT REACH, and it was measured rather than reasoned about: this and
    /// [`Self::read`] are two loops over ONE listing, so a walk that stops inside
    /// `crate::repo::all_files` moves both together - `.step_by(2)` over that listing printed
    /// `127 of 127 in-scope file(s)` at exit 0 having read half the tree. [`Self::unseen`] is the
    /// arm for that, and it is the only number here that does not come from git.
    pub(super) expected: usize,
    /// In-scope files the walk actually reached, read or not.
    pub(super) read: usize,
    /// In-scope target roots `cargo metadata` names that the walk never reached.
    ///
    /// THE PAIR FROM TWO DIFFERENT PLACES. Every other count in this gate is git's listing
    /// checked against itself; this is cargo's answer to *what does this workspace compile*
    /// checked against what the scan actually read. A truncated listing moves every git-derived
    /// number in step and leaves this one alone.
    pub(super) unseen: Vec<String>,
    /// In-scope target roots compared, so an EMPTY comparison cannot pass vacuously.
    ///
    /// The floor on the floor: [`workspace`] already refuses an empty target list, and this is
    /// what a caller states beside the arm above rather than trusting that.
    pub(super) targets: usize,
    /// Files the scan could not read, each with the error.
    pub(super) unreadable: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{in_scope, is_under, workspace};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn the_gate_excludes_its_own_crate_whichever_file_of_it_the_gate_lives_in() {
        assert!(is_under("xtask/src/examples/mod.rs", "xtask"));
        assert!(
            is_under("xtask/src/examples/scope.rs", "xtask"),
            "a move inside the crate stays excluded"
        );
        assert!(
            is_under("xtask/src/changes.rs", "xtask"),
            "the classification fixture next door too"
        );
        assert!(!is_under("xtaskish/src/lib.rs", "xtask"), "a whole segment, not a prefix");
        assert!(!is_under("crates/x/tests/t.rs", "xtask"));
    }

    #[test]
    fn only_the_workspace_members_own_files_are_in_scope() {
        // Measured before this existed: one reach planted in `vendor/mimalloc_rust/src/lib.rs`
        // was accepted as a variant's SOLE evidence, exit 0. `[workspace] exclude` keeps that
        // package out, and `just test` is `--workspace`, so nothing in it runs here.
        let members = set(&["crates/x", "xtask"]);
        assert!(in_scope("crates/x/tests/t.rs", &members, "xtask"));
        assert!(
            !in_scope("vendor/mimalloc_rust/src/lib.rs", &members, "xtask"),
            "a package the workspace excludes is not a run here"
        );
        assert!(
            !in_scope("xtask/src/examples/mod.rs", &members, "xtask"),
            "the gate's own crate"
        );
        assert!(!in_scope("crates/x/README.md", &members, "xtask"), "not Rust");
        assert!(
            !in_scope("crates/y/src/lib.rs", &members, "xtask"),
            "a directory under crates/ that is not a member either"
        );
        // A package AT the repo root puts every path in scope rather than none, which is the
        // direction an empty prefix must fail in.
        assert!(in_scope("anywhere/t.rs", &set(&[""]), "xtask"));
    }

    #[test]
    fn the_derived_member_list_holds_this_crate_and_not_the_vendored_ones() {
        // The property `crate::fmt` proves for the same derivation one question over, and the
        // reason it is a test rather than a sentence: if `[workspace] exclude` is removed, this
        // reddens - which is the point, because the vendored allocator's ten `#[test]`s would
        // otherwise become evidence that a run here reaches something.
        let root = crate::repo::root().expect("this workspace has a repo root");
        let found = workspace(&root).expect("cargo metadata reads this workspace");
        assert!(found.dirs.contains("xtask"), "{:?}", found.dirs);
        assert!(
            found.dirs.iter().all(|dir| !dir.starts_with("vendor/")),
            "a vendored package is not a workspace member: {:?}",
            found.dirs
        );
    }

    #[test]
    fn the_target_roots_are_cargos_answer_and_not_a_second_reading_of_git() {
        // THE SECOND AUTHORITY. Every other number in this gate is git's listing compared against
        // itself, and a walk that stops inside `repo::all_files` moves all of them together -
        // measured, `.step_by(2)` over that listing printed `127 of 127 in-scope file(s)` at
        // exit 0. These come from `cargo metadata`, which does not read git.
        let root = crate::repo::root().expect("this workspace has a repo root");
        let found = workspace(&root).expect("cargo metadata reads this workspace");
        assert!(
            !found.target_roots.is_empty(),
            "an empty target list would satisfy the comparison vacuously"
        );
        assert!(
            found.target_roots.iter().all(|rel| rel.ends_with(".rs")),
            "a target root is a Rust file: {:?}",
            found.target_roots
        );
        assert!(
            found.target_roots.iter().all(|rel| !rel.starts_with("vendor/")),
            "a vendored package's targets are not this workspace's: {:?}",
            found.target_roots
        );
        // Every one of them is a path this repository actually has, which is what makes a MISSING
        // one evidence about the scan rather than about cargo.
        for rel in &found.target_roots {
            assert!(root.join(rel).is_file(), "cargo names `{rel}`, which is not a file here");
        }
    }
}

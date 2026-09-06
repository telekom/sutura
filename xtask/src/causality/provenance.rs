//! What the gate believes about the diff it was handed, for the three facts a diff cannot state.
//!
//! Everything else in this gate reads the diff and the post-image: which lines are test code, what
//! they name, where those tests land. This module reads the things that are true of the diff and
//! are **not in it** - the commit it is measured against, what the reconstruction may do with each
//! changed path, and whether a test the added lines name is new here at all. Three defects, one
//! shape: the gate acted on a belief nothing had established.
//!
//! **1. THE REF IS NOT THE COMMIT.** `git diff <ref>` compares the tree at that ref against the
//! working tree, so a base ref that has MOVED puts other people's commits in the diff - the gate
//! reverts them, measures a tree nobody proposed, and its verdict is a function of when
//! `origin/main` was last fetched. Both venues that invoke this gate already resolve
//! `git merge-base`; the recipe a person runs by hand passed the ref straight through, which is the
//! worst place for it. [`Commit`] is what every consumer takes now, and the only way to obtain one
//! is to resolve a ref, so the moving-tip shape does not typecheck.
//!
//! **2. THE CLASSIFICATION READ `.rs` ONLY.** A test whose subject is a markdown page, a justfile
//! recipe or a nix file had NOTHING the gate could revert, so it reported *tests changed but no
//! implementation did* - a pass, over a suite whose implementation is prose. Reverting a file cargo
//! never reads cannot break a build and it is exactly what makes such a test red on base, so
//! [`Reach`] sorts a changed path into what the reconstruction may do with it rather than dropping
//! everything that is not Rust. The class it must NOT revert is a **build input**: reverting a
//! manifest or a lockfile changes what cargo RESOLVES rather than what the tests measure, and a
//! test file held at HEAD that needs a new dependency would stop compiling. Those are named in the
//! output instead - which is also the honest statement of `github.com/telekom/sutura#343`, where a
//! manifest-only diff can enable a whole test module and nothing here reads a feature table.
//!
//! **3. A DIFF CANNOT TELL A MOVED TEST FROM AN ADDED ONE.** The gate's premise is *a test the diff
//! ADDED must be red against the base behaviour*. Move a test into a new file - the refactor this
//! repository's own guidance asks for when a file hits the line cap - and the added lines name a
//! test whose subject never changed: green on base, and the gate printed
//! *FAILED - green against base behaviour* about a defect that does not exist. [`Moved`] asks the
//! base tree instead, and the question is deliberately narrow: **did a `.rs` file THIS DIFF ALSO
//! TOUCHED already have a function of that name at the base commit?** A move always touches the
//! file the test came from - it lost those lines - so the true case is caught, while the 23
//! duplicated test names in this tree do not make a new test read as moved unless the colliding
//! file is in the same diff.
//!
//! WHICH DIRECTION EACH ONE FAILS IN, because that is what makes them safe to act on:
//!
//! | Answer | Wrong how | What that costs |
//! | --- | --- | --- |
//! | [`Commit`] | git cannot resolve a merge base | the gate refuses; it does not guess a ref |
//! | [`Reach::Revertible`] | a reverted file is `include_str!`d and NEW at HEAD | the base tree does not build: INCONCLUSIVE, loud, never a false green |
//! | [`Moved`] | a name search misses a move | today's answer - the false failure this module exists to remove |
//! | [`Moved`] | a name search invents one | **materially a green step.** A *green against base behaviour* FAILURE - the vacuous test this gate exists to catch - becomes exit 3, and BOTH declared venues continue over it: `ci.yml` warns and exits 0, `ship-check` prints a line and carries on. #344's default-closed argument is closed for a consumer that has not been taught the code, and these two have. It takes EVERY test in scope matching, and the remedy if it is ever seen is narrower matching - a base-side hit under a `#[test]`-bearing region, or a pathspec of only the `.rs` files that LOST lines |

use std::collections::BTreeSet;

/// A commit the diff may be measured against.
///
/// **A ref is not one, and that is the whole type.** `origin/main` names whatever was last fetched;
/// this names one tree. Obtained only by resolving a merge base (`super::worktree::merge_base`), so
/// every consumer below - the diff, the `cat-file` existence check, the checkout, the base-tree
/// search - is reading the commit HEAD actually diverged from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Commit(String);

impl Commit {
    /// The commit `text` names, if it is one.
    ///
    /// Parsed rather than trusted: `git merge-base` prints one full object name and nothing else,
    /// so anything with whitespace inside it, a non-hex character or a length no object name has is
    /// a git that answered something other than the question - an error on stdout, an empty
    /// success, a `--all` listing - and the gate refuses instead of passing it to `git checkout`.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        let hex = (7..=64).contains(&trimmed.len()) && trimmed.chars().all(|c| c.is_ascii_hexdigit());
        hex.then(|| Self(String::from(trimmed)))
    }

    /// The object name, for a git command line.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The first twelve characters, for a line a person reads.
    pub(crate) fn short(&self) -> &str {
        self.0.get(..12).unwrap_or(&self.0)
    }
}

/// What the reconstruction may do with a changed file, decided by its path alone.
///
/// The base tree is built by reverting files. Which files it may revert is a question about what
/// cargo reads, not about what the file is called: a page, a recipe or a nix file is data to every
/// test that reads it and invisible to every test that does not, while a manifest decides what
/// exists to compile at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    /// Rust this workspace compiles. The added lines decide whether it holds tests.
    Compiled,
    /// Not compiled, and safe to revert: it can change what a test READS and cannot change what
    /// cargo resolves. This is the class that was dropped, and dropping it is why a
    /// documentation-driven suite was unprovable.
    Revertible,
    /// A manifest, a lockfile or a cargo/toolchain configuration. Reverting one changes the
    /// dependency graph rather than the behaviour under test - and a test file kept at HEAD that
    /// needs a dependency this branch added would stop compiling - so it is held at HEAD and NAMED.
    BuildInput,
    /// Outside every workspace member, so nothing cargo builds reads it. `crate::changes` owns that
    /// rule; `super::plan`'s vendored-test case is the measurement behind it.
    Outside,
}

impl Reach {
    /// What this gate may do with `path`.
    ///
    /// Order is load-bearing: a vendored manifest is [`Self::Outside`] rather than a build input,
    /// because nothing in the workspace resolves it, and the extension question comes last so a
    /// path named below is never read as source.
    pub(crate) fn of(path: &str) -> Self {
        if crate::changes::is_non_member(path) {
            return Self::Outside;
        }
        if is_build_input(path) {
            return Self::BuildInput;
        }
        if std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("rs"))
        {
            return Self::Compiled;
        }
        Self::Revertible
    }
}

/// The files whose content decides what cargo RESOLVES rather than what a test measures.
///
/// A short, declared list rather than "every `.toml`": the point is not the extension, it is that
/// reverting one of these in a tree whose test files are held at HEAD produces a tree that cannot
/// build - a dependency this branch added would be gone while the test needing it stays. Anything
/// else non-Rust is data, and reverting data is the fix in this module's header.
fn is_build_input(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(name, "Cargo.toml" | "Cargo.lock" | "rust-toolchain.toml")
        || path.starts_with(".cargo/")
        || path.contains("/.cargo/")
}

/// Which of the tests the filterset names the base tree ALREADY had.
///
/// The gate's premise is that a test the diff added must be red against the base behaviour. That
/// premise is about tests the diff added **behaviourally**, and a diff cannot tell those from a
/// test that MOVED - which is why the failing direction was reachable from a refactor this
/// repository asks for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Moved {
    /// None of them. Every test in scope is new here, so *green on base* is exactly the defect
    /// this gate exists to name.
    Nothing,
    /// Some of them, named. The rest are new, so *green on base* is still that defect - the
    /// verdict just says which tests it is not about.
    Partly(Vec<String>),
    /// All of them. *Green on base* is then not a defect at all: nothing in the scope was added
    /// behaviourally, so there is no old behaviour any of them could have been red against.
    Wholly(Vec<String>),
}

impl Moved {
    /// What `at_base` says about the tests in `scope`.
    ///
    /// `at_base` is the search output over the base commit, restricted to the `.rs` files this diff
    /// also touched - `super::worktree::search` produces it and this reads names out of it, so the
    /// classification is testable without a repository. An empty scope is [`Self::Nothing`]:
    /// [`Self::Wholly`] over nothing would turn the loud arm into a pass, and a scope is non-empty
    /// by `super::scoped::Scoped`'s construction anyway.
    pub(crate) fn of(scope: &[&str], at_base: &str) -> Self {
        let found: Vec<String> = scope
            .iter()
            .filter(|name| at_base.contains(&format!("fn {name}(")))
            .map(|name| String::from(*name))
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        let distinct: BTreeSet<&str> = scope.iter().copied().collect();
        if found.is_empty() {
            Self::Nothing
        } else if found.len() == distinct.len() {
            Self::Wholly(found)
        } else {
            Self::Partly(found)
        }
    }

    /// The names the base tree already had.
    pub(crate) fn names(&self) -> &[String] {
        match *self {
            Self::Nothing => &[],
            Self::Partly(ref names) | Self::Wholly(ref names) => names,
        }
    }
}

/// The search patterns that find the tests in `scope` in another tree.
///
/// `fn <name>(` and not the bare name: a name on its own matches the call in the moved test's own
/// body, a `use`, and this gate's own prose about it. rustfmt writes a signature's name against its
/// `(` for `fn`, `pub fn`, `async fn` and a generic alike - `super::scoped::function_name` carries
/// that measurement - so one fixed string per name is the whole search. A spelling it misses reads
/// as *not moved*, which is the answer the gate gave before this module existed.
pub(crate) fn needles(scope: &[&str]) -> Vec<String> {
    let distinct: BTreeSet<&str> = scope.iter().copied().collect();
    distinct.into_iter().map(|name| format!("fn {name}(")).collect()
}

#[cfg(test)]
mod tests {
    use super::{Commit, Moved, Reach, needles};

    #[test]
    fn a_ref_is_not_a_commit_and_only_one_of_them_parses() {
        // THE DEFECT, at the level the type removes. `git diff origin/main` measures whatever was
        // last fetched, so the gate's verdict was a function of the fetch rather than of the
        // change. Only a resolved object name gets past this, so a consumer cannot be handed a ref.
        assert_eq!(
            Commit::parse("7a65f1e1a1b2c3d4e5f60718293a4b5c6d7e8f90\n")
                .as_ref()
                .map(Commit::as_str),
            Some("7a65f1e1a1b2c3d4e5f60718293a4b5c6d7e8f90")
        );
        // Short-but-unambiguous is what a person types, and git prints it back the same way.
        assert_eq!(Commit::parse("7a65f1e").as_ref().map(Commit::short), Some("7a65f1e"));
        // And every shape that is NOT one commit: a ref, an empty success, a listing, an error
        // message on stdout. Each of these used to be spliced into `git checkout` verbatim.
        for not_one in [
            "origin/main",
            "",
            "\n",
            "HEAD~2",
            "7a65f1e1 7a65f1e2",
            "fatal: Not a valid object name",
        ] {
            assert_eq!(Commit::parse(not_one), None, "not one commit: {not_one:?}");
        }
    }

    #[test]
    fn a_page_is_revertible_and_a_manifest_is_not() {
        // THE MARKDOWN BLINDNESS, as the classification now answers it. A page is data to the test
        // that reads it, so the reconstruction can revert it and the test is red on base; a
        // manifest decides what compiles, so reverting it would break a tree whose test files are
        // held at HEAD. The two used to be one answer - dropped - and a suite whose implementation
        // is prose therefore passed while proving nothing.
        for revertible in [
            "docs/getting-started.md",
            "examples/single-player/README.md",
            "justfile",
            "flake.nix",
            "nix/with-tier.sh",
            "crates/sutura-config/src/defaults.yaml",
            ".github/workflows/ci.yml",
        ] {
            assert_eq!(Reach::of(revertible), Reach::Revertible, "{revertible}");
        }
        for input in [
            "Cargo.toml",
            "Cargo.lock",
            "crates/sutura-domain/Cargo.toml",
            "rust-toolchain.toml",
            ".cargo/config.toml",
        ] {
            assert_eq!(Reach::of(input), Reach::BuildInput, "{input}");
        }
        assert_eq!(Reach::of("crates/sutura-domain/src/model.rs"), Reach::Compiled);
        // Nothing cargo builds reads a vendored path, manifest included: `plan`'s own
        // `a_vendored_test_is_not_a_changed_test_this_gate_can_measure` is the measurement.
        assert_eq!(Reach::of("vendor/mimalloc_rust/src/lib.rs"), Reach::Outside);
        assert_eq!(Reach::of("vendor/mimalloc_rust/Cargo.toml"), Reach::Outside);
    }

    #[test]
    fn a_test_the_base_tree_already_had_is_not_one_this_diff_added() {
        // THE MOVE. `a_moved_assertion` arrives in a new file and the file it came from is in the
        // same diff, so the base tree has it under that other path - it was not added
        // behaviourally, and requiring it to be red against a behaviour nobody changed is how this
        // gate named a defect that does not exist.
        let at_base = concat!(
            "7a65f1e:crates/x/tests/blocking_span.rs:fn a_moved_assertion() {\n",
            "7a65f1e:crates/x/tests/blocking_span.rs:    let _ = a_moved_assertion;\n",
        );
        assert_eq!(
            Moved::of(&["a_moved_assertion"], at_base),
            Moved::Wholly(vec![String::from("a_moved_assertion")])
        );
        // A MIX still fails, and that is the half a blanket rule would have lost: the new test
        // passed on base too, which is the defect, so the verdict stands and names the other one.
        assert_eq!(
            Moved::of(&["a_moved_assertion", "genuinely_new"], at_base),
            Moved::Partly(vec![String::from("a_moved_assertion")])
        );
        // Nothing found is the premise holding: every test in scope is new here.
        assert_eq!(Moved::of(&["genuinely_new"], at_base), Moved::Nothing);
        // An empty scope may not read as *everything moved*, which would turn the loud arm into a
        // pass over nothing.
        assert_eq!(Moved::of(&[], at_base), Moved::Nothing);
    }

    #[test]
    fn a_longer_name_containing_a_scoped_one_is_not_that_test() {
        // The search is `fn <name>(`, so a base tree holding `fn not_a_moved_assertion(` does not
        // make `a_moved_assertion` read as moved. A name-only search would, and this gate has
        // already paid once for treating a bare test name as a key - `super::place` carries that
        // measurement.
        let at_base = "7a65f1e:crates/x/tests/other.rs:fn not_a_moved_assertion() {\n";
        assert_eq!(Moved::of(&["a_moved_assertion"], at_base), Moved::Nothing);
        assert_eq!(needles(&["a_moved_assertion"]), vec![String::from("fn a_moved_assertion(")]);
        // One needle per DISTINCT name: two added tests sharing a name are one search.
        assert_eq!(needles(&["dupe", "dupe"]).len(), 1);
    }
}

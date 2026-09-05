//! How the two runs are invoked, and the one target directory they share.
//!
//! `super` decides WHICH tree to ask; this decides how, and the two things worth knowing are both
//! about the sharing: it is what keeps the gate affordable, and it is a hole the verdict cannot
//! see. In its own file because the measurement below is longer than the code it qualifies.

use std::path::Path;
use std::process::Command;

/// Run the test suite in `dir` with nextest, building into `target`.
///
/// nextest, not `cargo test`, because this gate compares two runs and every other test
/// invocation in the repo uses nextest: measuring "green on head" with a different runner than
/// CI trusts would make the comparison meaningless. It also gives per-test process isolation,
/// so one panicking test cannot take others down and skew the comparison.
///
/// ONE TARGET DIRECTORY FOR BOTH RUNS, and that is what keeps this gate affordable. The base
/// run happens in a worktree, so by default it gets its own `target/` and compiles the whole
/// dependency closure a second time - `DataFusion`, Arrow and the rest, none of which the base
/// commit changed. On a 14 GB runner the second copy is what exhausted the disk. Sharing the
/// directory leaves the dependencies built once and rebuilds only our own crates, which is
/// exactly the difference between the two trees.
///
/// Safe to share because both runs use the same toolchain and the same profile. Alternating
/// COMPILERS in one directory invalidates every artifact in it; alternating our own sources
/// does not, because cargo fingerprints them and the dependency graph below them is identical.
///
/// WHAT SHARING IT DOES NOT KEEP APART, measured on 2026-09-04 and NOT fixed here. The two trees
/// are one unit as far as cargo is concerned - same package names, same relative paths, so the
/// same artifact - and freshness is decided by mtime, so a build in either tree overwrites the
/// other's binaries and the next run reuses them without noticing. Reproduced from an empty
/// directory: the HEAD run built and passed, the base run in the worktree rebuilt the same test
/// binary, and the same command back at the root then rebuilt NOTHING and failed - on a file that
/// was present at the root, because the binary it executed was the worktree's.
///
/// So a run can be measuring the OTHER tree's code, and it reaches this repository twice: the
/// postgres harness resolves its endpoint by walking up from `env!("CARGO_MANIFEST_DIR")`, which
/// is baked into whichever tree compiled the binary; and the reverted source is baked in the same
/// way. It is a defect in this optimisation rather than in the classification below, and it is one
/// reason two runs of one tree can disagree - the other, and the larger one, is that only
/// `just causality` provisions a tier at all (see [`nextest`]).
///
/// The direction it fails in is what makes it survivable now: a stale binary makes a scoped test
/// pass on base ("green against base behaviour") or vanish from HEAD, both of which are loud. It
/// can only manufacture a false GREEN by executing some third tree's binary in which the same
/// test failed, which is a far narrower window than *any failure anywhere counts*. Removing it
/// needs a second target directory, whose cost is the paragraph above, or a first-party rebuild
/// forced on every run - a trade-off with its own measurement, not a detail to slip in here.
///
/// `--cargo-profile` and not `--profile`: nextest reserves `--profile` for its own profiles,
/// and passing `ci` there would select a nextest profile that does not exist rather than a
/// cargo one that does.
pub(super) fn cargo_test(dir: &Path, target: &Path, only: &str, tree: Tree) -> (bool, String) {
    match nextest(dir, target, only, tree).output() {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            (o.status.success(), text)
        }
        Err(e) => (false, format!("could not run cargo: {e}")),
    }
}

/// Which tree a run happens in - which is a question about SERVICES, not about sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tree {
    /// The repo root: the tree whose service tier something provisioned before invoking the gate.
    Provisioned,
    /// The base worktree under `target/`, which no provisioner has ever seen.
    Reconstructed,
}

/// The nextest invocation for one run.
///
/// THE REQUIREMENT DOES NOT CROSS INTO A TREE ITS ENDPOINT CANNOT REACH. `nix/with-tier.sh`
/// exports `SUTURA_DEV_REQUIRE_TIER` into the recipe's whole process tree, so this gate inherits
/// it - while the endpoint that requirement belongs to is published to
/// `<root>/.sutura-dev/endpoints.json` and resolved by walking up from the harness. The base run
/// happens in a git-derived worktree whose root is a different directory, and that file is
/// gitignored, so it is not there and cannot be: the requirement crossed and the endpoint did
/// not, and every tier-backed cell failed CLOSED in a tree where nothing was provisioned.
///
/// Measured, and it is why this parameter exists rather than a comment: two such cells were the
/// whole of a base run's red, 86 tests into 1810, and the gate reported it as proof about a
/// change that touched neither. `sutura_dev::requirement`'s own rule is that only the thing which
/// provisioned a tier may declare one - so a run in a tree nothing provisioned gets the
/// developer-machine direction, which skips loudly and names what did not run.
///
/// AND IT IS WHY ONE COMMIT ANSWERED DIFFERENTLY IN TWO VENUES. `just causality` sources
/// `nix/with-tier.sh` and so provisions a tier; `just ship-check` and `nix run .#causality` do
/// not, so there those cells skipped and the verdict was about the change. The false green was
/// reachable from the one venue a person runs by hand and cites, which is the worst place for it
/// to live and the reason it went unnoticed.
///
/// WHAT THAT LEAVES: a tier-backed cell cannot be proven causal by this gate. It skips in the
/// reconstructed tree instead of running, so the base run is green and the verdict is *green
/// against base behaviour* - a false alarm rather than a false green, which is the direction to
/// be wrong in. Closing it means publishing an endpoint into a second root, and a second root for
/// the discovery file is the cross-worktree defect `dev/src/scope.rs` exists to prevent.
///
/// `--no-fail-fast` IS AFFORDABLE ONLY BECAUSE THE RUN IS SCOPED, and it is what makes the verdict
/// the same twice. Over the whole suite it would be a long bill for output nobody reads; over the
/// tests one diff added it costs almost nothing, and it removes the last ordering dependence -
/// with fail-fast, WHICH failures a verdict names is a function of nextest's scheduling, and a
/// gate whose answer moves between two runs of one tree is the property this gate exists to
/// supply.
pub(super) fn nextest(dir: &Path, target: &Path, only: &str, tree: Tree) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .args(["nextest", "run", "--workspace", "--all-features", "--cargo-profile", "ci"])
        .args(["--no-fail-fast", "-E", only]);
    if tree == Tree::Reconstructed {
        command.env_remove(sutura_dev::requirement::FORCE);
    }
    command
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::{Tree, nextest};
    use crate::causality::fixtures::{changed, manifest, tree};
    use crate::causality::scoped::{Scan, Scoped};

    /// The tests one added file declares, for the two wiring assertions below.
    fn one_added_test() -> Scoped {
        let files = vec![changed("crates/x/tests/t.rs", 1, &["#[test]", "fn the_added_one() {}"])];
        let read = tree(&[
            ("crates/x/tests/t.rs", "#[test]\nfn the_added_one() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match Scan::of(&files, &[String::from("crates/x/tests/t.rs")], &read) {
            Scan::Runnable(scoped) => scoped,
            other => panic!("expected one added test, got {other:?}"),
        }
    }

    #[test]
    fn both_runs_are_filtered_to_the_tests_the_diff_added() {
        // The wiring, which no assertion about the filter expression alone would catch: a
        // filterset that never reaches the command line leaves the whole-suite run in place, and
        // that run's verdict is a property of the suite rather than of the change.
        let command = nextest(
            Path::new("/tmp/root"),
            Path::new("/tmp/target"),
            &one_added_test().filterset(),
            Tree::Provisioned,
        );
        let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(args.iter().any(|arg| arg == "-E"), "{args:?}");
        // Qualified, and that is the half the wiring has to carry: an unqualified name is not a
        // key in this tree, so a filterset built from one runs tests in packages the diff never
        // touched - measured at six matches in three packages on nextest 0.9.143.
        assert!(
            args.iter()
                .any(|arg| arg == "(binary_id(=x::t) & test(/^(?:.*::)?the_added_one(?:::|$)/))"),
            "{args:?}"
        );
        // And the run completes, so which failures the verdict names is not a function of
        // nextest's scheduling. This gate was reported as answering differently for one tree in
        // two venues, and fail-fast over an unfiltered run is how that happens.
        assert!(args.iter().any(|arg| arg == "--no-fail-fast"), "{args:?}");
    }

    #[test]
    fn a_tier_backed_cell_is_not_required_in_a_worktree_the_endpoint_does_not_reach() {
        // `nix/with-tier.sh` exports the requirement into this gate's whole process tree, and the
        // endpoint it belongs to is published under the ROOT - so the base run, which happens in
        // a worktree under `target/`, inherited a requirement whose discovery file is gitignored
        // and therefore absent there. Every tier-backed cell then failed CLOSED in a tree nothing
        // had provisioned, and the gate reported that as red-on-base.
        let requirement = OsStr::new(sutura_dev::requirement::FORCE);
        let removed = |tree: Tree| {
            nextest(Path::new("/tmp/dir"), Path::new("/tmp/target"), "test(=t)", tree)
                .get_envs()
                .any(|(name, value)| name == requirement && value.is_none())
        };
        assert!(
            removed(Tree::Reconstructed),
            "a tree nothing provisioned may not inherit the requirement"
        );
        assert!(
            !removed(Tree::Provisioned),
            "the root keeps it: that is the tree whose endpoint was published"
        );
    }
}

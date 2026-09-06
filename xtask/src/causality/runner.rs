//! How the two runs are invoked, and the one target directory they share.
//!
//! `super` decides WHICH tree to ask; this decides how, and the thing worth knowing is the sharing:
//! it is what keeps the gate affordable, and it USED to be a hole the verdict could not see - a run
//! reusing the other tree's binaries and even its saved diagnostics. `super::isolation` carries
//! that measurement and the witness that closes it. In its own file because the measurements below
//! are longer than the code they qualify.

use std::path::Path;
use std::process::Command;

use super::isolation::Isolated;

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
/// WHAT SHARING IT DID NOT KEEP APART, and the removal that now precedes every run. The two trees
/// are one unit as far as cargo is concerned - same package names, same relative paths, so the same
/// artifact - and freshness is decided by mtime, so a build in either tree satisfied the other.
/// Reproduced in the gate's own sequence: the base run in the worktree printed
/// `Finished in 0.02s`, compiled nothing, and answered about the HEAD tree; and a warning present
/// only in the worktree was re-emitted by the next run at the root, quoting a source line the root
/// tree does not have. So a run could measure the OTHER tree's code, and a verdict could be
/// manufactured out of the previous run's saved output.
///
/// [`Isolated`] is what closed it - `super::isolation` carries both reproductions, the measurement
/// that `cargo clean --workspace` leaves the dependency closure and the warm-start stamp alone, and
/// why the removal is a witness rather than a line somebody remembers. **The bill is our own crates
/// compiled once per run**, which is what the sharing paragraph above already claimed the gate paid.
///
/// The reason two runs of one tree can still disagree is now only that one venue provisions a
/// service tier and the others do not (see [`nextest`]).
///
/// `--cargo-profile` and not `--profile`: nextest reserves `--profile` for its own profiles,
/// and passing `ci` there would select a nextest profile that does not exist rather than a
/// cargo one that does.
pub(super) fn cargo_test(dir: &Path, target: &Path, only: &str, tree: Tree) -> (bool, String) {
    // The removal comes FIRST and its failure is the run's failure: the run that would follow a
    // failed clean is exactly the one whose verdict cannot be trusted.
    let isolated = match Isolated::of(dir, target) {
        Ok(witness) => witness,
        Err(why) => return (false, why),
    };
    match nextest(dir, target, only, tree, &isolated).output() {
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
///
/// **IT TAKES THE ISOLATION WITNESS AND READS NOTHING OUT OF IT.** That is the point: the invariant
/// is *no run may reuse an artifact built from another tree*, and it is held by this parameter
/// rather than by a caller remembering to clean - `super::isolation` is the only place that can
/// produce one, and producing one is performing the removal.
pub(super) fn nextest(dir: &Path, target: &Path, only: &str, tree: Tree, _isolated: &Isolated) -> Command {
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

    use super::{Isolated, Tree, nextest};
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
            &Isolated::for_a_wiring_test(),
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
            nextest(
                Path::new("/tmp/dir"),
                Path::new("/tmp/target"),
                "test(=t)",
                tree,
                &Isolated::for_a_wiring_test(),
            )
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

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
/// WHAT SHARING IT DID NOT KEEP APART, and the removal that now precedes every run in a tree other
/// than the one the previous removal cleaned. The two trees are one unit as far as cargo is
/// concerned - same package names, same relative paths, so the same artifact - and freshness is
/// decided by mtime, so a build in either tree satisfied the other.
/// Reproduced in the gate's own sequence, with the root at `f() -> 1` and the worktree at
/// `f() -> 999`: the base run printed `Finished in 0.01s`, compiled nothing, and answered `ok` over
/// source that says 999; and a warning present only in the worktree was re-emitted by the next run
/// at the root, quoting a source line the root tree does not have. So a run could measure the OTHER
/// tree's code, and a verdict could be manufactured out of the previous run's saved output.
///
/// [`Isolated`] is what closed it, and **the profile is the whole mechanism**: a `cargo clean` with
/// a package selection and no `--profile` cleans `dev`, which is the one profile this gate never
/// builds, so the first version of the removal took out nothing either run would reuse and the
/// sequence above reproduced straight through it. The witness carries the directory, the tree and
/// the profile it cleaned, and this function reads all three OUT of it - so a clean of one profile
/// cannot license a run at another. `super::isolation` carries both reproductions, the measurement
/// that the corrected removal leaves the dependency closure and the warm-start stamp alone, and why
/// it is a witness rather than a line somebody remembers.
///
/// The reason two runs of one tree can still disagree is now only that one venue provisions a
/// service tier and the others do not (see [`nextest`]).
///
/// `--cargo-profile` and not `--profile`: nextest reserves `--profile` for its own profiles,
/// and passing the cargo profile there would select a nextest profile that does not exist.
pub(super) fn cargo_test(dir: &Path, target: &Path, only: &str, tree: Tree<'_>) -> (bool, String) {
    // The removal comes FIRST and its failure is the run's failure: the run that would follow a
    // failed clean is exactly the one whose verdict cannot be trusted.
    let isolated = match Isolated::of(dir, target) {
        Ok(witness) => witness,
        Err(why) => return (false, why),
    };
    // WHAT THE REMOVAL SAID IT DID, because an exit status says only that it ran - and the first
    // version of this ran fine while removing nothing at all. A zero here on a directory the gate
    // has built in before is the tell that the removal has stopped reaching what the run reuses.
    println!("  {isolated}");
    match nextest(&isolated, only, tree).output() {
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
pub(super) enum Tree<'a> {
    /// The repo root: the tree whose service tier something provisioned before invoking the gate.
    Provisioned,
    /// The base worktree under `target/`, carrying a copy of the root's discovery file.
    Reconstructed,
    /// The claim arm's kill worktree, provisioned its OWN Postgres tier keyed on that worktree's
    /// path (`claim::tier`), whose credential this carries - empty when it did not start. A skip there would pass as a mutation
    /// the cell survived, so the requirement is FORCED and an absent tier fails loudly.
    Mutated(&'a [(String, String)]),
}

/// The nextest invocation for one run.
///
/// **THE BASE RUN SHARES THE HEAD RUN'S TIER.** `nix/with-tier.sh` exports `SUTURA_DEV_REQUIRE_TIER`
/// into the recipe's whole process tree, and the endpoint it belongs to is published to the gitignored
/// `<root>/.sutura-dev/endpoints.json`, which a worktree under `target/` does not have. `add_base_worktree`
/// copies that file in, so both runs reach one tier and the requirement holds in both. Dropping the
/// requirement there instead made every tier-backed cell skip on base - green on base, so a causal tier
/// cell could never be proven.
///
/// A cell whose tier is absent on base still panics in the lookup, and `base::never_reached_its_tier`
/// reads that test's OWN panic line as INCONCLUSIVE for the whole run rather than as red on base: two
/// such cells were once the whole of a base run's red, 86 tests into 1810, reported as proof about a
/// change that touched neither.
///
/// **The limit:** HEAD runs first and base second against one tier, so a cell that is not idempotent
/// against tier state can be green on HEAD and red on base for a state reason. Nothing holds that pair.
///
/// `just causality` and `nix run .#causality` both source `nix/with-tier.sh`, and `just ship-check`
/// calls the recipe, so every venue gives the base run the same requirement.
///
/// THE CLAIM ARM'S KILL RUN, `Tree::Mutated`, does not share it: its kill worktree starts its OWN
/// Postgres tier and passes its credential here, and the requirement is forced, because a skip there
/// reads as a mutation the cell survived. Where that tier did not start - or the cell needs another
/// tier - `claim::kill::attest` reads the tier failure as INCONCLUSIVE.
///
/// `--no-fail-fast` IS AFFORDABLE ONLY BECAUSE THE RUN IS SCOPED, and it is what makes the verdict
/// the same twice. Over the whole suite it would be a long bill for output nobody reads; over the
/// tests one diff added it costs almost nothing, and it removes the last ordering dependence -
/// with fail-fast, WHICH failures a verdict names is a function of nextest's scheduling, and a
/// gate whose answer moves between two runs of one tree is the property this gate exists to
/// supply.
///
/// **THE RUN'S DIRECTORY, TARGET AND PROFILE ARE THE WITNESS'S**, and that is the whole point. The
/// invariant is *no run may reuse an artifact built from another tree*, and it was held by a
/// parameter this function read nothing out of - so review found the route that leaves open:
/// `Isolated::of(Path::new("/"), Path::new("/nowhere"))` and then a run somewhere else compiled and
/// passed every gate, because nothing tied the pair together. There is no second value to disagree
/// with now: what was cleaned is what runs, at the profile it was cleaned at.
///
/// **AND THE CALLER'S NEXTEST PROFILE IS NOT A SECOND VALUE EITHER**, which the sentence above
/// asserted one layer too low to be true. The CARGO profile came from the witness while the NEXTEST
/// profile came from whatever shell invoked the gate, because `NEXTEST_PROFILE` was inherited - so
/// there was a second value, and it decided `slow-timeout` and `fail-fast` for the very runs the
/// verdict compares. Measured on `github.com/telekom/sutura#879`: this repository's own documented
/// local invocation - `.config/nextest.toml` names `NEXTEST_PROFILE=nofailfast` for a complete
/// table - reached the nested run inside `super::claim`'s fixture, whose throwaway crate carries no
/// nextest configuration at all, and the child exited 96 with nextest's own `profile ... not found`
/// having compiled and run nothing. `super::base::could_not_attest` does not recognise a CONFIG
/// error, so the claim arm reported *the mutation does not kill it* about a run that never reached
/// the cell - red under the repository's own documented command and green in every venue that does
/// not set the variable. REMOVED rather than pinned to a name: this gate wants nextest's defaults
/// plus the flags on this line, and a profile it did not choose changes the run whatever the name
/// resolves to.
///
/// **WHAT THAT DOES NOT COVER**: every other `NEXTEST_*` variable is still inherited.
/// `NEXTEST_RETRIES` and `NEXTEST_TEST_THREADS` perturb a run rather than killing it, so nothing
/// measured here settles whether this should be an `env_clear` - which would also take
/// `CARGO_TARGET_DIR` and the toolchain's own variables with it.
pub(super) fn nextest(isolated: &Isolated, only: &str, tree: Tree<'_>) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(isolated.dir())
        .env("CARGO_TARGET_DIR", isolated.target())
        .env_remove("NEXTEST_PROFILE")
        .args(["nextest", "run", "--workspace", "--all-features", "--cargo-profile"])
        .arg(isolated.profile())
        .args(["--no-fail-fast", "--status-level", "all", "-E", only]);
    match tree {
        Tree::Provisioned | Tree::Reconstructed => {}
        Tree::Mutated(tier) => {
            command
                .env(sutura_dev::requirement::FORCE, "1")
                .envs(tier.iter().map(|(k, v)| (k, v)));
        }
    }
    command
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::{Isolated, Tree, nextest};
    use crate::causality::fixtures::{changed, manifest, tree};
    use crate::causality::isolation::cleaning;
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
        let isolated = Isolated::for_a_wiring_test(Path::new("/tmp/root"), Path::new("/tmp/target"));
        let command = nextest(&isolated, &one_added_test().filterset(), Tree::Provisioned);
        let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(args.iter().any(|arg| arg == "-E"), "{args:?}");
        // Qualified, and that is the half the wiring has to carry: an unqualified name is not a
        // key in this tree, so a filterset built from one runs tests in packages the diff never
        // touched - measured at six matches in three packages on the pinned nextest.
        assert!(
            args.iter()
                .any(|arg| arg == "(binary_id(=x::t) & test(/^(?:.*::)?the_added_one(?:::|$)/))"),
            "{args:?}"
        );
        // And the run completes, so which failures the verdict names is not a function of
        // nextest's scheduling. This gate was reported as answering differently for one tree in
        // two venues, and fail-fast over an unfiltered run is how that happens.
        assert!(args.iter().any(|arg| arg == "--no-fail-fast"), "{args:?}");
        // THE PAIR THAT CANNOT DISAGREE. Both the run's directory and its target come from the
        // witness, so the tree that was cleaned is the tree that runs - review found that a unit
        // witness let a clean of `/` into `/nowhere` license a run anywhere.
        assert_eq!(command.get_current_dir(), Some(Path::new("/tmp/root")));
        assert!(
            command
                .get_envs()
                .any(|(name, value)| name == OsStr::new("CARGO_TARGET_DIR") && value == Some(OsStr::new("/tmp/target"))),
            "the run builds into the directory that was cleaned"
        );
        // AND NO PROFILE THE CALLER CHOSE. Inherited, `NEXTEST_PROFILE` was the second value the
        // doc above says cannot exist: `#879`'s red was the documented `nofailfast` reaching a
        // fixture crate that defines no profiles, where the child dies at configuration load.
        assert!(
            command
                .get_envs()
                .any(|(name, value)| name == OsStr::new("NEXTEST_PROFILE") && value.is_none()),
            "the caller's nextest profile does not reach the child"
        );
    }

    #[test]
    fn the_run_builds_at_the_profile_the_removal_cleaned() {
        // THE DEFECT REVIEW FOUND, at the level that would have caught it. `cargo clean` with a
        // package selection and no `--profile` cleans `dev`; this gate builds `--cargo-profile ci`.
        // So the removal took out artifacts no run here reads and left every one it does - measured
        // as `Summary 0 files` against a tree built only at `ci` - and #281 reproduced end to end
        // straight through the fix.
        //
        // Reading the two commands SIDE BY SIDE is the assertion that closes it: whatever the clean
        // names after `--profile`, the run names after `--cargo-profile`.
        //
        // **What that does and does not catch**, because an overstated test is worth less than a
        // narrow one: it reddens whenever the two DISAGREE - a missing flag, or a literal that is
        // not the witness's profile - and a literal that happens to equal it today passes here and
        // reddens only when `warm_start::WARM_PROFILE` moves. Both values come from that constant,
        // so there is nothing to drift; the assertion is what notices if that stops being true.
        let isolated = Isolated::for_a_wiring_test(Path::new("/tmp/root"), Path::new("/tmp/target"));
        let run: Vec<String> = nextest(&isolated, "test(=t)", Tree::Provisioned)
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let cleaned: Vec<String> = cleaning(&isolated)
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let after = |args: &[String], flag: &str| {
            args.iter()
                .position(|arg| arg == flag)
                .and_then(|at| args.get(at + 1))
                .cloned()
        };
        let built_at = after(&run, "--cargo-profile");
        assert_eq!(
            built_at,
            after(&cleaned, "--profile"),
            "the run's profile is the cleaned profile: run {run:?}, clean {cleaned:?}"
        );
        assert_eq!(built_at.as_deref(), Some(isolated.profile()), "and it is the witness's");
        // nextest reserves `--profile` for its OWN profiles, so the run may not spell it that way -
        // passing a cargo profile there selects a nextest profile that does not exist.
        assert!(!run.iter().any(|arg| arg == "--profile"), "{run:?}");
    }

    #[test]
    fn the_base_run_keeps_the_requirement_the_head_run_has() {
        // The base worktree gets the root's discovery file, so a tier-backed cell reaches the tier
        // there too. Dropping the requirement made it skip instead: green on base, a false alarm.
        let requirement = OsStr::new(sutura_dev::requirement::FORCE);
        let isolated = Isolated::for_a_wiring_test(Path::new("/tmp/dir"), Path::new("/tmp/target"));
        let envs = |tree: Tree| {
            nextest(&isolated, "test(=t)", tree)
                .get_envs()
                .map(|(name, value)| (name.to_owned(), value.map(OsStr::to_owned)))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            envs(Tree::Reconstructed),
            envs(Tree::Provisioned),
            "the base run inherits the head run's requirement"
        );
        assert!(
            nextest(&isolated, "test(=t)", Tree::Mutated(&[]))
                .get_envs()
                .any(|(name, value)| name == requirement && value == Some(OsStr::new("1"))),
            "the kill worktree forces it, so a tier-gated cell cannot skip into a survived mutation"
        );
    }
}

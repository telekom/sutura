//! Why a run in one tree could measure the other tree's code, and the witness that stops it.
//!
//! THE DEFECT, reproduced twice on cargo 1.100.0-nightly over a two-crate synthetic workspace, one
//! dependency, one integration test, one git worktree - the gate's own arrangement. Both runs share
//! `CARGO_TARGET_DIR`, deliberately: a second dependency closure exhausted the disk on a 14 GB
//! runner, and the trees differ only in our own crates. What sharing does not survive is that cargo
//! sees ONE unit - same package names, same relative paths, and the metadata hash carries neither
//! tree's path - so freshness is decided by mtime and a build in either tree satisfies the other.
//!
//! **Run the gate's sequence and the base run measures HEAD.** Root `f() -> 1`, worktree
//! `f() -> 999`, one test asserting `1`, so the worktree's own test must fail there: the base run
//! printed `Finished in 0.01s`, compiled nothing, and returned `ok. 1 passed` over source that says
//! 999. In the gate that is `BaseOutcome::Green` and *FAILED - green against base behaviour* about a
//! correct change; the other order gives the HEAD run the reverted tree's binaries, which is the
//! false-green direction.
//!
//! **And a diagnostic outlives the tree that produced it.** With a warning-producing line present
//! only in the worktree, the next run AT THE ROOT re-emitted it - `unused variable`, quoting a source
//! line that is not in the root tree, `Finished in 0.00s`. Cargo replays a fresh unit's saved
//! diagnostics, so a verdict about HEAD can be manufactured out of the previous run's output; under
//! `-D warnings` a replayed warning IS that run's failure. `github.com/telekom/sutura#281`.
//!
//! **THE PROFILE IS THE WHOLE MECHANISM, and the first version of this module got it wrong.**
//! `cargo clean` with a package selection and no `--profile` cleans the REQUESTED profile, which
//! defaults to `dev` - and this gate builds `--cargo-profile ci`, so the removal took out artifacts
//! no run here ever reads and left every one it does. Measured on the same synthetic workspace,
//! built only at `ci`:
//!
//! ```text
//! cargo clean --workspace --dry-run              ->  Summary 0 files
//! cargo clean --workspace --dry-run --profile ci ->  Summary 57 files, 2.2MiB total
//! ```
//!
//! The sequence above reproduces END TO END through the profile-less clean - the base run still
//! compiled nothing and still answered `ok` over `f() -> 999`. **So the profile is not a detail of
//! the removal; it is the removal**, and it is why [`Isolated`] carries it: the run's profile IS the
//! cleaned profile, read out of the witness, and one constant feeds both.
//!
//! THE FIX IS CARGO'S OWN VOCABULARY, and the cost is the one the sharing comment already claimed to
//! pay. `cargo clean --workspace --profile <p>` removes the artifacts of the workspace MEMBERS at
//! that profile and nothing else: measured on the same workspace, `Removed 44 files, 1.2MiB`, the
//! registry dependency was NOT recompiled afterwards (only our crate was), and the warm-start stamp
//! `nix/cargo-env.nix` writes into the directory survived - so the dependency closure is still built
//! once and `crate::warm_start::profile_for` still answers. Re-run the sequence with it and every
//! step compiles its own tree: the base run reported `FAILED` about `f() -> 999`, and the HEAD run
//! after it reported `ok` again with no diagnostic crossing between them.
//!
//! **WHAT IT COSTS ON THIS WORKSPACE, measured rather than argued**, because a correctness fix that
//! doubles a gate is a trade a reader has to be able to see. Against a warm
//! `target/causality-target` at `ci`, timing the gate's own build step (`--no-run`, so the tests
//! themselves are the same work either way):
//!
//! | Step | Wall clock |
//! | --- | --- |
//! | a warm run BEFORE this change (nothing to rebuild) | 9.9 s |
//! | the removal itself - `Removed 445 files, 2.0GiB total` | 0.3 s |
//! | the run after it, our own crates rebuilt, closure untouched | 1 m 18 s |
//!
//! So **about 68 s per run**, and the gate makes two runs - three when the retry fires - which is
//! **+2 to +3.5 minutes** per `just causality`, per `ship-check` and per CI causality step, against
//! a step that already measured 12 minutes in CI. The dependency closure is what the sharing exists
//! to protect and it is untouched: nothing outside our own crates recompiled. If that ever needs
//! buying back, the cheapest shape is the marker this module argues against above, and the argument
//! against it is unchanged - a stale marker is a silent false green.
//!
//! WHY IT IS A TYPE AND NOT A LINE IN `cargo_test`. A cleaning step is a thing a second invocation
//! site forgets, and the run it forgets it in looks exactly like a fast one. [`Isolated`] is evidence
//! that ONE named directory, for ONE named tree, at ONE named profile, holds no first-party artifact
//! from another tree - and it carries all three, because a witness that names none of them cannot
//! support that sentence. `super::runner::nextest` takes its directory, its target and its profile
//! FROM the witness rather than beside it, so a clean of one tree cannot license a run in another and
//! a clean of one profile cannot license a run at another. The fields are private to this module and
//! [`Isolated::of`] - which performs the removal - is the only thing that returns one. The pattern is
//! `super::base::PerTestResults`, for the same reason: an invariant a caller has to remember is one
//! the compiler is not holding.
//!
//! WHAT IT DOES NOT REACH. Cleaning is UNCONDITIONAL, on purpose: a marker recording which tree owns
//! the directory would save one removal per retry and would make the invariant depend on a file this
//! gate writes - and a stale marker is a silent false green, which is the class of defect this module
//! is about. It is bounded to what cargo calls a workspace MEMBER at that one profile, so anything
//! else in the directory - a build script's output for a third-party crate, another profile someone
//! else built there - is outside it, which is why `super::remedies` still offers removing the
//! directory as the last resort. Two runs of one tree still share artifacts, which is cargo's
//! ordinary mtime path and is what the whole optimisation is for.
//!
//! AND AN `Ok` FROM A SUBPROCESS IS NOT EVIDENCE THAT ANYTHING WENT - `sutura/gates` states that
//! rule for `file`, `echo` and `grep -c`, and a removal that silently removed nothing is the same
//! shape one directory over, because it is exactly what the profile-less version did. So
//! [`Isolated`] carries the COUNT cargo reported and `super::runner::cargo_test` prints it. A zero
//! is not a failure - a directory nothing has built in yet has nothing to remove, which is every
//! first run - so it is STATED rather than refused: `isolated: removed 0 …` on a run that took
//! minutes is the tell, and it is visible instead of inferred. That is why `--quiet` is gone: the
//! summary line is the only place the count exists, and this reads it back from the very command
//! that did the removal rather than asking a second one.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::warm_start::WARM_PROFILE;

/// Evidence that one target directory holds no first-party artifact built from another tree.
///
/// **A sealed witness whose fields ARE the run's arguments.** The first version was a unit struct,
/// and review found the hole that leaves: `Isolated::of(Path::new("/"), Path::new("/nowhere"))`
/// followed by a run in a different directory compiled and passed every gate, because nothing tied
/// the two together. A witness that names no directory, no tree and no profile cannot support a
/// sentence about any of them - so it names all three and `super::runner::nextest` reads them out of
/// it. `Isolated::of` is the only thing that returns one, and it returns one only after cargo has
/// reported what it removed.
#[derive(Debug)]
pub(super) struct Isolated {
    /// The tree the removal resolved the workspace from, and the tree the run happens in.
    dir: PathBuf,
    /// The directory the artifacts were removed from, and the directory the run builds into.
    target: PathBuf,
    /// The profile they were removed at, and the profile the run builds at.
    profile: &'static str,
    /// How many files cargo said it removed.
    ///
    /// Read back from the removal's own summary line, because `status.success()` says the command
    /// ran and says nothing about whether it did anything - and the first version of this module
    /// removed nothing while exiting 0. A zero is legitimate on a directory nothing has built in,
    /// so it is printed rather than refused.
    removed: usize,
}

impl Isolated {
    /// Remove every workspace member's artifacts at [`WARM_PROFILE`] from `target`, resolved from
    /// the workspace in `dir`.
    ///
    /// `--workspace` rather than a list of packages: cargo answers *which packages are ours*, and a
    /// list here would be a second copy of `Cargo.toml`'s `members` that goes stale silently - the
    /// failure being a run that measures the other tree again.
    ///
    /// **THE PROFILE COMES FROM `crate::warm_start`**, which owns that literal because two other
    /// gates derive cargo's profile from it and *"a third Rust spelling of `ci` next to a gate that
    /// already enforces it is a copy that can go stale while the gate stays green"*. A fourth
    /// spelling here is exactly how the removal came to name a profile the gate never builds.
    ///
    /// A failure to clean is an ERROR rather than a warning, because the run that follows it is
    /// exactly the run whose verdict cannot be trusted. `super::runner::cargo_test` turns it into
    /// the run's own failure text, which is where every other subprocess failure in this gate goes.
    pub(super) fn of(dir: &Path, target: &Path) -> Result<Self, String> {
        let removal = clean(dir, target)
            .output()
            .map_err(|e| format!("could not run cargo clean in {}: {e}", dir.display()))?;
        let mut said = String::from_utf8_lossy(&removal.stderr).into_owned();
        said.push_str(&String::from_utf8_lossy(&removal.stdout));
        if !removal.status.success() {
            return Err(format!(
                "could not remove the first-party {WARM_PROFILE} artifacts in {}: {}",
                target.display(),
                said.trim()
            ));
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            target: target.to_path_buf(),
            profile: WARM_PROFILE,
            removed: counted(&said).unwrap_or_default(),
        })
    }

    /// The tree the run happens in.
    pub(super) fn dir(&self) -> &Path {
        &self.dir
    }

    /// The directory the run builds into.
    pub(super) fn target(&self) -> &Path {
        &self.target
    }

    /// The profile the run builds at.
    pub(super) const fn profile(&self) -> &'static str {
        self.profile
    }

    /// What the removal said it removed, for the line the gate prints.
    ///
    /// A COUNT AND NOT A GUARANTEE: it is cargo's own number for the files it took out of this
    /// directory at this profile. Zero on a cold directory is right; zero on a warm one is the tell
    /// that this removal is not reaching what the run reuses, which is the defect that shipped.
    pub(super) const fn removed(&self) -> usize {
        self.removed
    }

    /// A witness for a test that reads a COMMAND rather than running one.
    ///
    /// `super::runner`'s wiring assertions build the nextest invocation and inspect its arguments;
    /// running a real `cargo clean` for that would make a unit test depend on a workspace and a
    /// target directory. It takes the same pair `of` does, so what those tests read is still the
    /// witness's own values rather than a second set. Test-only, so nothing shipped can reach it -
    /// the same escape `super::base`'s own tests have on `PerTestResults`.
    #[cfg(test)]
    pub(super) fn for_a_wiring_test(dir: &Path, target: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            target: target.to_path_buf(),
            profile: WARM_PROFILE,
            removed: 0,
        }
    }
}

/// The removal, as a command, so a test can read what it names.
///
/// **THE PROFILE IS AN ARGUMENT AND MUST BE**, which is the correction this module exists around. A
/// `cargo clean` with a package selection and no `--profile` cleans exactly ONE profile - `dev` - and
/// that is the one profile this gate never builds. The doc that used to sit here argued the opposite
/// (*"every profile in it is the gate's own too, and naming one would leave the others to be reused
/// across trees"*), and it was inverted: naming none leaves the only one that matters.
///
/// **NO `--quiet`**, and that is not cosmetic: the summary line - `Removed 44 files, 1.2MiB total`,
/// or `Removed 0 files` - is the only place cargo reports what went, and [`counted`] reads it back
/// so the witness carries a fact rather than an exit status.
fn clean(dir: &Path, target: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .args(["clean", "--workspace", "--profile", WARM_PROFILE]);
    command
}

/// The removal a witness performed, as a command.
///
/// Test-only, and it exists for ONE assertion: `super::runner`'s
/// `the_run_builds_at_the_profile_the_removal_cleaned` reads this command and the nextest command
/// side by side, because the defect review found was that those two named different profiles. It
/// rebuilds rather than records, which is sound only because [`clean`] is a pure function of the
/// witness's own fields plus [`WARM_PROFILE`] - the same three values [`Isolated::of`] used.
#[cfg(test)]
pub(super) fn cleaning(isolated: &Isolated) -> Command {
    clean(isolated.dir(), isolated.target())
}

/// The count out of `cargo clean`'s summary line.
///
/// Measured wordings on cargo 1.100.0-nightly: `     Removed 44 files, 1.2MiB total` when something
/// went, `     Removed 0 files` when nothing did. `--dry-run` says `Summary <n> files` instead, which
/// is why both spellings are read - [`removed_by_a_dry_run`] uses the second.
fn counted(said: &str) -> Option<usize> {
    said.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed.strip_prefix("Removed ").or_else(|| trimmed.strip_prefix("Summary "))
        })
        .find_map(|rest| rest.split_whitespace().next().and_then(|count| count.parse().ok()))
}

/// How many files a removal of the same selection WOULD take out of `target`.
///
/// The question that distinguishes a removal from a no-op BEFORE one happens, which is what the test
/// that removes something for real needs on both sides of the removal. The gate itself does not pay
/// for a second cargo invocation per run: it reads [`counted`] off the removal it already ran.
#[cfg(test)]
fn removed_by_a_dry_run(dir: &Path, target: &Path) -> Option<usize> {
    let out = Command::new("cargo")
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .args(["clean", "--workspace", "--profile", WARM_PROFILE, "--dry-run"])
        .output()
        .ok()?;
    let mut said = String::from_utf8_lossy(&out.stderr).into_owned();
    said.push_str(&String::from_utf8_lossy(&out.stdout));
    counted(&said)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Isolated, WARM_PROFILE, clean, counted, removed_by_a_dry_run};

    #[test]
    fn the_removal_names_the_profile_the_gate_builds_at() {
        // THE DEFECT, and the reason the first version of this test was worse than none: it asserted
        // the argument list WITHOUT a profile, which is the list that removes nothing at the profile
        // the two runs build at - so the one mechanism guarding the removal was a guard against
        // correcting it. Measured: `cargo clean --workspace --dry-run` over a tree built only at
        // `ci` reports `Summary 0 files`; with `--profile ci` it reports 57.
        //
        // The profile is read from `crate::warm_start`, which owns that literal, so a rename on the
        // nix side reddens here too rather than leaving a fourth spelling of `ci` behind.
        let command = clean(Path::new("/tmp/tree"), Path::new("/tmp/root/target/causality-target"));
        let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(args, vec!["clean", "--workspace", "--profile", WARM_PROFILE]);
        assert_eq!(command.get_current_dir(), Some(Path::new("/tmp/tree")));
        let target = command
            .get_envs()
            .find(|(name, _)| *name == "CARGO_TARGET_DIR")
            .and_then(|(_, value)| value);
        assert_eq!(target, Some(Path::new("/tmp/root/target/causality-target").as_os_str()));
        // And no `-p`: a package list here would be a copy of the workspace members that rots.
        assert!(!args.iter().any(|arg| arg == "-p" || arg == "--package"), "{args:?}");
    }

    #[test]
    fn the_count_is_read_out_of_what_cargo_said() {
        // AN `Ok` IS NOT EVIDENCE THAT ANYTHING WENT, which is the shape the profile-less removal
        // had: exit 0, nothing removed, a witness minted. Both of cargo 1.100.0-nightly's wordings
        // are read - the removal's own summary and `--dry-run`'s - so the number the gate prints
        // comes from the command that did the work.
        assert_eq!(counted("     Removed 44 files, 1.2MiB total\n"), Some(44));
        assert_eq!(counted("     Removed 0 files\n"), Some(0));
        assert_eq!(counted("     Summary 57 files, 2.2MiB total\n"), Some(57));
        // And nothing invented from output that carries no count: `unwrap_or_default` then prints a
        // zero, which reads as *it removed nothing* rather than as a number nobody produced.
        assert_eq!(counted("error: no such profile\n"), None);
        assert_eq!(counted(""), None);
    }

    #[test]
    fn the_witness_carries_the_directory_and_the_profile_it_cleaned() {
        // The third route the seal did not close: the witness was a unit struct, so nothing tied
        // `of`'s arguments to the run's. A clean of `/` into `/nowhere` licensed a run anywhere.
        // `super::runner::nextest` reads all three out of this value now, which is what makes the
        // pair impossible to disagree - and this asserts the values are the ones handed in.
        let one = Isolated::for_a_wiring_test(Path::new("/tmp/tree"), Path::new("/tmp/target"));
        assert_eq!(one.dir(), Path::new("/tmp/tree"));
        assert_eq!(one.target(), Path::new("/tmp/target"));
        assert_eq!(one.profile(), WARM_PROFILE);
    }

    #[test]
    fn the_removal_takes_the_gate_s_own_profile_out_and_leaves_the_rest() {
        // A TEST THAT REMOVES SOMETHING, because a test reading a command line cannot tell a removal
        // from a no-op - which is the distinction this module exists to hold, and the one the first
        // version of it got wrong. A scratch workspace, built at the gate's profile, then this
        // module's own `Isolated::of`: the artifacts are there before and gone after.
        //
        // `dev` is asserted UNTOUCHED in the same breath: that is the profile the profile-less clean
        // took out, and a removal that reaches it would be paying for artifacts no run here reads.
        let root = std::env::temp_dir().join(format!("sutura-isolation-{}", std::process::id()));
        let member = root.join("m");
        let _swept = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(member.join("src")).expect("a scratch workspace");
        std::fs::write(
            root.join("Cargo.toml"),
            format!("[workspace]\nresolver = \"3\"\nmembers = [\"m\"]\n\n[profile.{WARM_PROFILE}]\ninherits = \"dev\"\n"),
        )
        .expect("a workspace manifest");
        std::fs::write(
            member.join("Cargo.toml"),
            "[package]\nname = \"m\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("a member manifest");
        std::fs::write(member.join("src/lib.rs"), "pub fn f() -> u8 {\n    1\n}\n").expect("a source file");
        let target = root.join("t");

        let built = std::process::Command::new("cargo")
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &target)
            .args(["build", "--workspace", "--profile", WARM_PROFILE, "--quiet"])
            .status();
        let dev = std::process::Command::new("cargo")
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &target)
            .args(["build", "--workspace", "--quiet"])
            .status();
        // A host with no usable cargo is not this assertion's subject: `nix/run-gate.sh` states the
        // same rule one level up, and the whole gate needs cargo anyway.
        if !built.is_ok_and(|s| s.success()) || !dev.is_ok_and(|s| s.success()) {
            let _swept = std::fs::remove_dir_all(&root);
            return;
        }

        let before = removed_by_a_dry_run(&root, &target);
        let witness = Isolated::of(&root, &target);
        let after = removed_by_a_dry_run(&root, &target);
        let dev_survived = target.join("debug").join("libm.rlib").is_file();
        let _swept = std::fs::remove_dir_all(&root);

        assert!(witness.is_ok(), "the removal ran: {witness:?}");
        assert!(
            before.is_some_and(|n| n > 0),
            "the profile the gate builds at had artifacts to remove: {before:?}"
        );
        assert_eq!(after, Some(0), "and they are gone");
        assert!(dev_survived, "the removal does not reach a profile no run here builds");
    }
}

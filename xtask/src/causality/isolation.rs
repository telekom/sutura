//! Why a run in one tree could measure the other tree's code, and the witness that stops it.
//!
//! THE DEFECT, reproduced twice on cargo 1.100.0-nightly (2026-09-06) over a two-crate synthetic
//! workspace, one dependency, one integration test, one git worktree - the gate's own arrangement.
//! Both runs share `CARGO_TARGET_DIR`, deliberately: a second dependency closure exhausted the
//! disk on a 14 GB runner, and the trees differ only in our own crates. What sharing does not
//! survive is that cargo sees ONE unit - same package names, same relative paths, and the metadata
//! hash carries neither tree's path - so freshness is decided by mtime and a build in either tree
//! satisfies the other.
//!
//! **Run the gate's sequence and the base run measures HEAD.** From an empty directory, HEAD ran at
//! the root (compiled, green), then the base run in the worktree printed `Finished in 0.02s`,
//! compiled nothing at all, and reported the HEAD tree's answer for a source file whose content it
//! never saw. The reverted implementation was in the tree and out of the measurement.
//!
//! **And a diagnostic outlives the tree that produced it.** With a warning-producing line present
//! only in the worktree, the next run AT THE ROOT re-emitted it - `unused variable`, quoting a
//! source line that is not in the root tree, `Finished in 0.00s`. Cargo replays a fresh unit's
//! saved diagnostics, so a verdict about HEAD can be manufactured out of the previous run's output;
//! under `-D warnings` a replayed warning is that run's failure. `github.com/telekom/sutura#281`.
//!
//! THE FIX IS CARGO'S OWN VOCABULARY, and the cost is the one the sharing comment already claimed
//! to pay. `cargo clean --workspace` removes the artifacts of the workspace MEMBERS and nothing
//! else: measured on the same workspace, the registry dependency was not recompiled afterwards
//! (only our crate was), and the warm-start stamp `nix/cargo-env.nix` writes into the directory
//! survives it - so the dependency closure is still built once and
//! `crate::warm_start::profile_for` still answers. Re-run with a clean before each run and the same
//! sequence answers about the tree it names: the base run compiled its own tree and failed there,
//! the next HEAD run compiled its own and passed, and no diagnostic crossed between them.
//!
//! WHY IT IS A TYPE AND NOT A LINE IN `cargo_test`. A cleaning step is a thing a second invocation
//! site forgets, and the run it forgets it in looks exactly like a fast one. [`Isolated`] is
//! evidence that the shared directory holds no first-party artifact from another tree, its field is
//! private to this module, and [`Isolated::of`] - which performs the removal - is the only thing
//! that can produce one. `super::runner::nextest` requires it, so a run built without the removal
//! does not compile. The pattern is `super::base::PerTestResults`, for the same reason: an
//! invariant a caller has to remember is one the compiler is not holding.
//!
//! WHAT IT DOES NOT REACH. Cleaning is UNCONDITIONAL, on purpose: a marker recording which tree
//! owns the directory would save one removal per retry and would make the invariant depend on a
//! file this gate writes - and a stale marker is a silent false green, which is the class of defect
//! this module is about. Two runs of one tree still share artifacts, which is cargo's ordinary
//! mtime path and is what the whole optimisation is for. And nothing here isolates a run from a
//! **third** tree: another checkout pointed at the same directory would collide the same way, and
//! only the two this gate creates go through here.

use std::path::Path;
use std::process::Command;

/// Evidence that the shared target directory holds no first-party artifact built from another tree.
///
/// **A sealed witness: the field is private to this module.** So the only way a caller obtains one
/// is [`Isolated::of`], the only way that returns is by having run the removal, and a `nextest`
/// invocation built without one is `error[E0061]`. That is the mechanism; the paragraph in
/// `super::runner` that used to say a run *can be measuring the other tree's code* was the whole
/// control before it.
#[derive(Debug)]
pub(super) struct Isolated(());

impl Isolated {
    /// Remove every workspace member's artifacts from `target`, resolved from the workspace in `dir`.
    ///
    /// `--workspace` rather than a list of packages: cargo answers *which packages are ours*, and a
    /// list here would be a second copy of `Cargo.toml`'s `members` that goes stale silently - the
    /// failure being a run that measures the other tree again.
    ///
    /// A failure to clean is an ERROR rather than a warning, because the run that follows it is
    /// exactly the run whose verdict cannot be trusted. `super::runner::cargo_test` turns it into
    /// the run's own failure text, which is where every other subprocess failure in this gate goes.
    pub(super) fn of(dir: &Path, target: &Path) -> Result<Self, String> {
        let removal = clean(dir, target)
            .output()
            .map_err(|e| format!("could not run cargo clean in {}: {e}", dir.display()))?;
        if removal.status.success() {
            Ok(Self(()))
        } else {
            Err(format!(
                "could not remove the first-party artifacts in {}: {}",
                target.display(),
                String::from_utf8_lossy(&removal.stderr).trim()
            ))
        }
    }

    /// A witness for a test that reads a COMMAND rather than running one.
    ///
    /// `super::runner`'s two wiring assertions build the nextest invocation and inspect its
    /// arguments; running a real `cargo clean` for that would make a unit test depend on a
    /// workspace and a target directory. Test-only, so nothing shipped can reach it - the same
    /// escape `super::base`'s own tests have on `PerTestResults`.
    #[cfg(test)]
    pub(super) const fn for_a_wiring_test() -> Self {
        Self(())
    }
}

/// The removal, as a command, so a test can read what it names.
///
/// No profile argument: this directory is the gate's own, so every profile in it is the gate's own
/// too, and naming one would leave the others to be reused across trees. `--workspace` is the whole
/// selection.
fn clean(dir: &Path, target: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .args(["clean", "--workspace", "--quiet"]);
    command
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::clean;

    #[test]
    fn the_removal_is_scoped_to_the_workspace_and_to_the_gate_s_own_directory() {
        // Both halves are load-bearing and neither is visible in a verdict. `--workspace` is what
        // keeps the dependency closure - the reason the two runs share a directory at all - built
        // once; the target directory is what makes the removal apply to the artifacts the next run
        // would otherwise reuse. A removal in the wrong directory is a slow gate that still
        // measures the other tree.
        let command = clean(Path::new("/tmp/tree"), Path::new("/tmp/root/target/causality-target"));
        let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(args, vec!["clean", "--workspace", "--quiet"]);
        assert_eq!(command.get_current_dir(), Some(Path::new("/tmp/tree")));
        let target = command
            .get_envs()
            .find(|(name, _)| *name == "CARGO_TARGET_DIR")
            .and_then(|(_, value)| value);
        assert_eq!(target, Some(Path::new("/tmp/root/target/causality-target").as_os_str()));
        // And no `-p`: a package list here would be a copy of the workspace members that rots.
        assert!(!args.iter().any(|arg| arg == "-p" || arg == "--package"), "{args:?}");
    }
}

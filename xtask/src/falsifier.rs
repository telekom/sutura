//! The tree every registered hygiene gate has to refuse, and the argument for its shape.
//!
//! `github.com/telekom/sutura#371`. The defect that issue collects is a gate whose HELPERS are
//! tested while `run` and the exit code it produces are driven by nothing, so a mutation on the
//! verdict path leaves the suite green. Measured on this tree before the falsifier existed: 2 of
//! 31 hygiene gates had a test driving the registered entry point's verdict, and 7 of 31 answered
//! `ok` over a tree with none of their subjects. The test that uses this lives in this module's
//! own `tests`, and reads `crate::tasks()` - re-exported from the crate root, so it is blind to
//! which area module declares a row (`task_table/` since `#610`).
//!
//! **Both root markers are load-bearing.** [`crate::repo::root`] identifies a root by `flake.nix`
//! AND `Cargo.toml`; without them its walk falls through to `CARGO_MANIFEST_DIR`'s parent and
//! every gate reads this repository, which is the one tree that makes the assertion vacuous.
//!
//! **The seeded files are for the gates whose subject is a KIND OF FILE**, where absence is a
//! legitimate pass: a tree with no text file genuinely has no over-long file and no CRLF, so only a
//! violation falsifies them. Measured - without the seed, `max-lines`, `line-endings` and
//! `text-hygiene` all answer `ok` here, and every other gate is falsified by the bare root alone.
//! The shared tree has no `.rs` file. Gates with a Rust subject seed their own: for example,
//! `check-expect-thresholds` supplies its required `xtask/src/main.rs` anchor with a forbidden
//! attribute. Without that seed, its absent-anchor refusal (`Refusal::NotJudged`) would say
//! nothing about the threshold rule. `check-worktree-state` uses `nix/shared-scratch.sh` because
//! its subject is a shared-path write in a shell script.
//!
//! **A seed is what makes a refusal come from a gate's OWN RULE rather than from a missing input.**
//! The shared files falsify some rules directly; task-specific seeds cover others. The test below
//! asserts a `Fail` verdict and that each declared subject exists, but it does not classify the
//! refusal's reason. A gate can still refuse on a different arm after its real finding decays.
//! The review mutation for each new seed must suppress that finding and observe the sweep fail;
//! that is the limit `telekom/sutura#371` names.
//!
//! `telekom/sutura#405` asks its own gate to be one of them, so the shell script below carries a
//! real violation - an unkeyed path under the machine's temporary root - and
//! `check-worktree-state` refuses it by name, with a line number, having found and adjudicated one
//! taking. The file is a `.sh` under `nix/` because that is a scope no other gate in the sweep
//! reads for content, so it falsifies exactly one gate.
//!
//! `check-skills` compares a parsed router with the skill tree only after checking three agent
//! links. The shared tree gives it valid links; otherwise a missing-link refusal would hide the
//! seeded missing route. Other gates skip these symlinks in the file census.
//!
//! **Why this module exists at all, rather than sitting in `main.rs`:** that file hit the
//! 1000-line cap on the merge of two branches that both grew it, and `sutura/gates` says to move
//! the HARNESS and keep every `#[test]` where it is. It carries a test of its own for the reason
//! that same page gives: a file adding no `#[test]` is revertible, so the base tree would delete
//! it while `main.rs` - held for its tests - still declared the module, and `E0583` would turn a
//! real causal verdict into `INCONCLUSIVE`. It stays `#[cfg(test)]` because its constructor is
//! full of scratch-tree `expect`s that `-D expect_used` allows in tests but would not in shipped
//! code - these functions exist for the sweep in `crate::tests`.
//!
//! What the mechanism does NOT hold is the `sutura/invariants` row's third column; the short form
//! is that a refusal says nothing about WHY.
//!
//! [`crate::registry::Paired`] covers a gate no synthetic tree can isolate: its real inputs must
//! pass (the control), then one own-rule edit must fail. Limit: nothing here holds that the edit
//! trips ONLY the own rule - the hand mutation table in the commit that added it showed that.

use std::path::{Path, PathBuf};

/// A repository root that is not this repository, seeded to be adversarial to every gate.
///
/// Keyed on the process id and REMOVED FIRST, because a pid is reusable: a directory left behind
/// by an earlier run would otherwise seed files this tree never declared into a scan whose whole
/// point is that the tree is known. That is not hypothetical - `telekom/sutura#328` is a sibling
/// test failing exactly that way on inherited state.
pub(crate) fn falsifier_tree() -> PathBuf {
    let root = std::env::temp_dir().join(format!("sutura-falsifier-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    std::fs::create_dir_all(&root).expect("a scratch root");
    std::fs::write(root.join("flake.nix"), "{ }\n").expect("the first root marker");
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").expect("the second root marker");
    std::fs::create_dir_all(root.join(".agents/skills")).expect("the routed skills tree");
    for link in crate::repo::INDEX_SYMLINKS {
        let path = root.join(link);
        std::fs::create_dir_all(path.parent().expect("a skill link has a parent")).expect("an agent directory");
        #[cfg(unix)]
        std::os::unix::fs::symlink("../.agents/skills", path).expect("an agent skill link");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir("../.agents/skills", path).expect("an agent skill link");
    }
    // Over the 1000-line cap and clean in every other way, so `max-lines` is the only gate this
    // file is about.
    let mut over_long = String::new();
    for _ in 0..1001_u16 {
        over_long.push_str("a line\n");
    }
    std::fs::write(root.join("over-long.txt"), over_long).expect("the over-long file");
    // CRLF for `line-endings`; the trailing space and the missing final newline for
    // `text-hygiene`, whose rules are neither of the other two.
    let malformed = "a line with a trailing space \r\nand no final newline";
    std::fs::write(root.join("carriage-return.txt"), malformed).expect("the malformed text file");
    // A gate script writing to a path every checkout on the machine reaches, which is
    // `check-worktree-state`'s subject and `telekom/sutura#405`'s instance 2 in shape. Clean in
    // every other way - LF, a final newline, no trailing space, well under the line cap - so this
    // file falsifies one gate and tells the other three nothing.
    std::fs::create_dir_all(root.join("nix")).expect("the shell scope");
    // A platform predicate read off the deprecated `stdenv` alias, which is `check-nix-platform`'s
    // own rule. Seeded rather than left to the bare root FOR THE REASON THIS MODULE'S HEADER
    // GIVES: `flake.nix` is a `.nix` file, so that gate's scan is neither empty nor missing its
    // anchor here, and without a violation its refusal would have to come from an input it could
    // not read - which is the weaker of the two and the residue `telekom/sutura#371` names. Clean
    // in every other way - LF, a final newline, no trailing space, four lines - so it falsifies
    // exactly one gate and tells the others nothing.
    std::fs::write(
        root.join("nix/platform.nix"),
        "{ pkgs }:\n{\n  buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];\n}\n",
    )
    .expect("the deprecated platform predicate");
    std::fs::write(
        root.join("nix/shared-scratch.sh"),
        "#!/usr/bin/env bash\nlog=\"/tmp/sutura-gate-state.log\"\necho hi >\"$log\"\n",
    )
    .expect("the unkeyed shared path");
    root
}

/// Write `falsifier`'s own-rule seeds into `root`, creating parent directories as needed.
///
/// The per-gate half of the #371 sweep (see [`crate::registry::Falsifier`]): the shared tree is
/// adversarial to every gate, and this overlays the one real violation that makes a SINGLE gate
/// fire its own substantive rule. Empty seeds change nothing, which is the seed-programme marker's
/// contract - those gates are still falsified by the shared tree alone, just not on their own rule.
pub(crate) fn apply_seeds(root: &std::path::Path, falsifier: &crate::registry::Falsifier) {
    for (rel, contents) in falsifier.seeds {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a seed's parent directory");
        }
        std::fs::write(&path, contents).expect("a seeded violation");
    }
}

/// A fresh scratch tree holding the real repository's published files under `paired.inputs`, plus
/// both root markers. Keyed on the pid and removed first, like [`falsifier_tree`].
pub(crate) fn paired_tree(paired: &crate::registry::Paired) -> PathBuf {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits under the repo root");
    let root = std::env::temp_dir().join(format!("sutura-paired-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    for rel in published(repo_root) {
        let wanted = rel == "flake.nix"
            || rel == "Cargo.toml"
            || paired
                .inputs
                .iter()
                .any(|input| *input == "." || rel == *input || rel.strip_prefix(input).is_some_and(|rest| rest.starts_with('/')));
        if wanted {
            copy(&repo_root.join(&rel), &root.join(&rel));
        }
    }
    root
}

/// The set `repo::all_files` judges: git's tracked-plus-unignored listing on a checkout, the walk
/// where there is no `.git` (the nix sandbox).
fn published(repo_root: &Path) -> Vec<String> {
    let mut git = std::process::Command::new("git");
    crate::repo::strip_git_env(&mut git);
    match git
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .current_dir(repo_root)
        .output()
    {
        Ok(out) if out.status.success() => out
            .stdout
            .split(|byte| *byte == 0)
            .filter(|raw| !raw.is_empty())
            .map(|raw| String::from_utf8_lossy(raw).into_owned())
            .filter(|rel| repo_root.join(rel).symlink_metadata().is_ok())
            .collect(),
        _ => {
            let mut found = Vec::new();
            walk_published(repo_root, repo_root, &mut found);
            found
        }
    }
}

/// Every file and symlink under `dir`, repo-relative, never descending into a `repo::SKIP_DIRS` name.
fn walk_published(repo_root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("a directory to walk") {
        let path = entry.expect("a directory entry").path();
        let rel = crate::repo::relative(repo_root, &path).expect("a walked path under the root");
        if crate::repo::skip_dir_crossed(&rel).is_some() {
            continue;
        }
        if path.symlink_metadata().is_ok_and(|meta| meta.is_dir()) {
            walk_published(repo_root, &path, found);
        } else {
            found.push(rel);
        }
    }
}

/// Copy one file, recreating a symlink as a symlink rather than following it.
fn copy(source: &Path, dest: &Path) {
    std::fs::create_dir_all(dest.parent().expect("a copied path has a parent")).expect("a paired copy's parent");
    if source.symlink_metadata().is_ok_and(|meta| meta.file_type().is_symlink()) {
        let target = std::fs::read_link(source).expect("a paired symlink's target");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, dest).expect("a paired symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, dest).expect("a paired symlink");
        return;
    }
    std::fs::copy(source, dest).expect("a paired input copy");
}

/// Apply a paired gate's own-rule violation. Each `find` must occur exactly once, so an edit that
/// cannot land panics rather than handing the gate an unchanged tree.
pub(crate) fn apply_violation(root: &Path, paired: &crate::registry::Paired) {
    for edit in paired.violation {
        let path = root.join(edit.path);
        let text = std::fs::read_to_string(&path).expect("a violation's file to read");
        let count = text.matches(edit.find).count();
        assert_eq!(
            count, 1,
            "violation {:?} matched {count} time(s) in {}, expected once",
            edit.find, edit.path
        );
        std::fs::write(&path, text.replace(edit.find, edit.replace)).expect("a violation to write");
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_seeds, apply_violation, falsifier_tree, paired_tree};

    #[test]
    fn a_stale_tree_from_an_earlier_run_is_not_inherited() {
        // THE PROPERTY THE CONSTRUCTOR'S FIRST LINE EXISTS FOR, and the one a reader would not
        // think to check: the directory name carries a process id, process ids are reused, and a
        // run that panicked before its cleanup leaves the tree behind. Inheriting it would put a
        // file no gate's verdict is about into a tree whose whole argument is that its contents
        // are known - which is how `telekom/sutura#328`'s sibling flake behaves, measured.
        let root = falsifier_tree();
        std::fs::write(root.join("left-behind.md"), "from a previous run\n").expect("a stray file");

        let again = falsifier_tree();
        let stray = again.join("left-behind.md").exists();
        let seeded = again.join("over-long.txt").exists();
        drop(std::fs::remove_dir_all(&again));

        assert!(
            !stray,
            "the constructor inherited a file an earlier run left in {}",
            again.display()
        );
        assert!(seeded, "the constructor removed the tree and did not seed it again");
    }

    #[test]
    fn every_registered_hygiene_gate_refuses_a_tree_it_cannot_attest() {
        const SCANNED_CLONE_TASK: &str = "check-jscpd";
        // EVERY GATE IN THE TABLE, EXECUTED AGAINST A TREE IT MUST REFUSE, AND REFUSED ON ITS
        // OWN RULE - `github.com/telekom/sutura#371`. This is the guard that drove `falsifier_tree`
        // out of `main.rs`; see that file's header for the 1000-line cap it pays. It drives the
        // gates through the FN POINTER out of `TASKS` and, for each, (1) applies the gate's
        // own-rule seeds, (2) asserts the seeded subject is in scope, and (3) asserts
        // `run() == Verdict::Fail` - the own-rule refusal, not a floor. Membership is the table.
        //
        // WHY THE THREE STEPS, IN THE ORDER THEY ARE IN. The old sweep asserted only the exit
        // code, so a gate that flipped its real Finding `Fail -> Pass` stayed green by refusing
        // on a DIFFERENT arm - an absent input or an empty scan - which #371 can see (proven live
        // on newtype_leaks: a real `impl Deref` in the tree, flipped arm, still green). Seeding
        // the gate's OWN violation removes the missing-input hide; proving the subject is in scope
        // removes the empty-scan hide; asserting `Verdict::Fail` over a tree that visibly carries
        // a real violation then leaves the own-rule arm as the only possible cause.
        //
        // HELD, NOT WISHED - the first version got that wrong in the change whose subject it is.
        // `set_current_dir` is process-global: measured, `cargo test -p xtask --bin xtask` went
        // from `917 passed` to `907 passed; 11 failed`, the eleven every real-tree anchor
        // resolving through `repo::root`, whose walk reads the current directory first.
        // `AGENTS.md` bans a bare `cargo clippy` and `cargo nextest`, NOT `cargo test`, so
        // "correct under nextest" was a sentence. nextest gives each test its own process and sets
        // `NEXTEST`; refusing without it fails before the directory moves.
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "this test moves the process's current directory, so it must have the process to \
             itself: run it under `just test`, which is cargo-nextest and one process per test. \
             Under `cargo test`'s threads it breaks every sibling resolving a path through \
             `repo::root` - 11 of them, measured."
        );

        let original = std::env::current_dir().expect("a current directory");

        let mut executed: Vec<&str> = Vec::new();
        let mut attested: Vec<&str> = Vec::new();
        for task in crate::tasks() {
            if !matches!(task.kind, crate::registry::Kind::Hygiene(_)) {
                continue;
            }
            // ONE FRESH TREE PER GATE, so a refusal is attributable to THIS gate's own seeds - a
            // verdict borrowed from a sibling's seed is not this gate's rule firing.
            let tree = falsifier_tree();
            apply_seeds(&tree, &task.falsifier);

            // STEP 2 - THE SUBJECTS-IN-SCOPE PROVISO. A refusal on a tree that does not visibly
            // carry the gate's own subject is the absent-input / empty-scan floor #371 names, not
            // the own-rule refusal this sweep is for. Checked HERE, on the seeded tree, so hiding
            // by emptying a subject table (the proven newtype_leaks empty floor) reddens whether
            // or not `run()` manages to refuse.
            if let Some(subject) = task.falsifier.in_scope {
                assert!(
                    tree.join(subject).exists(),
                    "`{}` declares falsifier subject `{subject}`, which the sweep never seeded - \
                     the gate's own-rule Fail arm is unheld (`github.com/telekom/sutura#371`)",
                    task.name
                );
            }
            if task.name == SCANNED_CLONE_TASK {
                crate::jscpd::assert_seeded_clone(&tree);
            }

            std::env::set_current_dir(&tree).expect("point the process at the falsifier tree");
            // STEP 3 - THE OWN-RULE FAIL ASSERTION, at the `Verdict` level rather than its exit
            // code: what must be seen is `Fail`, not merely "an exit code that is not 0". With
            // the gate's subject seeded and in scope, `Pass` here can only mean the gate's real
            // Finding arm decayed to a pass undetected.
            let verdict = (task.run)(&[]);
            std::env::set_current_dir(&original).expect("restore the current directory");
            drop(std::fs::remove_dir_all(&tree));

            executed.push(task.name);
            if verdict != crate::Verdict::Fail {
                attested.push(task.name);
            }
        }

        // THE FLOOR IS A SET OF NAMES AND ITS OTHER SIDE IS `hygiene_gates`. Two counts off two
        // spellings of one expression are two enforcers of one key: measured, `.take(18)` on BOTH
        // left the previous version green with 13 gates unexecuted. `hygiene_gates` is the
        // registry's other reader - `check-gate-classification` reconciles it against the
        // implementation plan's two tables, both directions - so narrowing it to hide a narrowed
        // loop reddens that gate instead. The hand-written anchor list this replaces was #371's
        // own defect 8: red when a name joins the list, green when one is left out of it.
        let registered: Vec<&str> = crate::hygiene_gates().map(|(name, _)| name).collect();
        assert!(
            executed.contains(&SCANNED_CLONE_TASK),
            "the scanner probe is keyed to check-jscpd; a renamed task must update that probe"
        );
        assert!(
            !registered.is_empty(),
            "the sweep registers no gate - this test judged nothing"
        );
        assert_eq!(
            executed, registered,
            "the gates this test executed are not the gates the sweep registers - one it skipped \
             is one it says nothing about"
        );

        assert_eq!(
            attested,
            Vec::<&str>::new(),
            "{} of {} gate(s) did not FAIL over a tree that is not this repository. A gate that \
             cannot be made to fail is a gate whose green says nothing - `github.com/telekom/\
             sutura#371`. WHICH REMEDY IS RIGHT DEPENDS ON THE GATE'S SUBJECT. If that subject is \
             every text file in the tree, absence is a legitimate pass and nothing is wrong with \
             the gate: seed a violation into `falsifier_tree`, whose doc carries the argument. \
             Otherwise the gate needs a floor over what it actually read, or a refusal on the input \
             whose absence makes its other rules vacuous.",
            attested.len(),
            registered.len()
        );
    }

    /// The control and the violated verdicts of the one paired gate `name`, each run in its own
    /// scratch tree. Moves the cwd, hence the NEXTEST guard; the assertions stay in the cells.
    fn paired_verdicts(name: &str) -> (crate::Verdict, crate::Verdict) {
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "this test moves the process's current directory: run it under `just test`"
        );
        let original = std::env::current_dir().expect("a current directory");
        let run_in = |tree: &std::path::Path, task: &crate::registry::Task| {
            std::env::set_current_dir(tree).expect("point the process at the paired tree");
            let verdict = (task.run)(&[]);
            std::env::set_current_dir(&original).expect("restore the cwd");
            verdict
        };
        let task = crate::tasks().find(|task| task.name == name).expect("a registered task");
        let paired = task.falsifier.paired.expect("a gate registered as paired");
        let tree = paired_tree(paired);
        let control = run_in(&tree, task);
        apply_violation(&tree, paired);
        let violated = run_in(&tree, task);
        drop(std::fs::remove_dir_all(&tree));
        (control, violated)
    }

    const PAIRED_MESSAGE: &str = "must pass its real inputs and fail one own-rule edit of them";

    #[test]
    fn check_boundaries_passes_its_real_inputs_and_refuses_one_violation() {
        assert_eq!(
            paired_verdicts("check-boundaries"),
            (crate::Verdict::Pass, crate::Verdict::Fail),
            "check-boundaries {PAIRED_MESSAGE}"
        );
    }

    #[test]
    fn check_guidance_passes_its_real_inputs_and_refuses_one_violation() {
        assert_eq!(
            paired_verdicts("check-guidance"),
            (crate::Verdict::Pass, crate::Verdict::Fail),
            "check-guidance {PAIRED_MESSAGE}"
        );
    }

    #[test]
    fn check_venues_passes_its_real_inputs_and_refuses_one_violation() {
        assert_eq!(
            paired_verdicts("check-venues"),
            (crate::Verdict::Pass, crate::Verdict::Fail),
            "check-venues {PAIRED_MESSAGE}"
        );
    }

    #[test]
    fn check_warm_start_passes_its_real_inputs_and_refuses_one_violation() {
        assert_eq!(
            paired_verdicts("check-warm-start"),
            (crate::Verdict::Pass, crate::Verdict::Fail),
            "check-warm-start {PAIRED_MESSAGE}"
        );
    }

    #[test]
    fn the_paired_set_is_the_four_gates_with_a_cell_each() {
        let mut paired_gates: Vec<&str> = crate::tasks()
            .filter(|task| task.falsifier.paired.is_some())
            .map(|task| task.name)
            .collect();
        paired_gates.sort_unstable();
        assert_eq!(
            paired_gates,
            ["check-boundaries", "check-guidance", "check-venues", "check-warm-start"],
            "the paired set changed: a gate gained or lost `Falsifier::paired`; add or drop its cell"
        );
    }
}

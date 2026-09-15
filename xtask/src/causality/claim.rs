//! A commit may DECLARE a claim cell, and this module is the check that holds the claim rather
//! than the permission that replaces it.
//!
//! WHY A CLAIM CELL EXISTS AT ALL. This gate answers *these tests are green on HEAD and red on
//! base*. Some tests can never be red against what the base tree has: a test that PINNS behaviour
//! the base tree already provides (the agent surface offering the same verified subject token the
//! HTTP surface does is one) passes a base run that reverted nothing because there is nothing to
//! revert - so the gate answers *FAILED - green against base behaviour* over a correct change,
//! and the only honest proof left is a mutation: BREAK the behaviour the test claims and require
//! the test to redden. That proof existed but this gate could not consume it.
//!
//! WHAT THE TRAILER IS AND IS NOT. `Claim-Cell: <test-fn-name>` in a commit message says *this
//! added test pins behaviour that already exists, and the killing mutation for it lives at
//! `devco/claim-mutations/<test-fn-name>.patch`*. It is **not** the permission: a trailer
//! nothing checks is the gate that has quietly stopped gating, because anyone can bypass
//! red-before-green by typing a word. So the trailer NARROWS what may pass and never disables the
//! check - [`Claim`] collects what is declared, [`decide`] holds every part of it, and any
//! declaration that does not hold is a refusal rather than a pass. An undeclared claim cell (a
//! test pinning existing behaviour, no trailer) still reaches the normal proof and its
//! green-against-base refusal unchanged.
//!
//! THE CLAIM IS RANGE-WIDE, mirroring `super::relocation::Claim::of`: [`Claim::of`] reads every
//! message in `base..HEAD`, because the range is what merges, so the range is the unit. And the
//! declaration must be a BIJECTION with the diff's added tests - a declared test the diff did not
//! add, or an added test not declared, is refused - for the same reason every claim here is
//! checked rather than believed: a trailer that names something other than what the diff added is
//! a trailer over a different diff.
//!
//! THE MUTATION IS ONE COMMITTED PATCH PER CELL, under `devco/claim-mutations/`. A patch file is
//! reviewable in the diff and byte-reproducible; it is read at RUNTIME from the checkout (so it
//! lives in the same range the trailer does) and it must be a `git apply`-able unified diff over
//! PRODUCTION code that TOUCHES NO TEST FILE - because the whole point is to break the behaviour
//! the cell claims, and a mutation that edits the test to fail proves nothing. Each cell is then
//! run in the ISOLATED causality target with the patch applied, and the cell is required to
//! FAIL by assertion - the mutation kills it. A patch that does not apply, touches a test file,
//! or leaves the cell green is refused by name.
//!
//! **THE COST**: each mutated run pays the same ~68 s isolated rebuild every base/head run pays
//! (the patch forces this workspace's own crates to recompile), so a declared diff costs `+N`
//! runs per gate invocation. Stated, not hidden: this is the price of the evidence the arm is
//! here to consume.
//!
//! **WHAT AN ACCEPTED ARM DOES AND DOES NOT PROVE.** It proves each declared cell DIES under its
//! compiled mutation - the assertion the cell carries really does fail when its subject is broken.
//! It does not prove red-on-base (the behaviour pre-exists, which is the whole point), and it
//! does not prove HEAD is green - that stays `just test` and the nix nextest check. The witness
//! discipline is `super::base::PerTestResults`'s: [`classify_mutation`] is pure and under test,
//! and the killed verdict is minted only from a classified output that NAMES the cell, never from
//! a subprocess's `Ok`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Verdict;
use crate::causality::base;
use crate::causality::isolation::Isolated;
use crate::causality::place::AddedTest;
use crate::causality::runner::{Tree, cargo_test};
use crate::causality::scoped::Scoped;
use crate::causality::worktree;

/// The repository-relative directory every committed mutation patch lives in.
pub(super) const MUTATIONS_DIR: &str = "devco/claim-mutations";

/// The commit trailer that declares a claim cell.
const TRAILER: &str = "Claim-Cell:";

#[cfg(test)]
mod probe;

/// What the commit messages in the measured range DECLARED.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Claim {
    /// The test-fn names the trailers named, deduplicated and sorted.
    cells: Vec<String>,
}

impl Claim {
    /// The claim `log` carries, if it carries one.
    ///
    /// A TRAILER WITH NO NAME IS NO CLAIM, which is the fail-closed direction: it leaves the run
    /// exactly as it was, so a malformed declaration buys nothing. Requiring a name is what stops
    /// the trailer being copied forward onto an unrelated commit without being obviously wrong to
    /// a reviewer - [`Cause::NotAdded`] refuses a name the diff did not add.
    pub(super) fn of(log: &str) -> Option<Self> {
        let cells: Vec<String> = log
            .lines()
            .filter_map(|line| line.trim().strip_prefix(TRAILER))
            .map(|rest| String::from(rest.trim()))
            .filter(|name| !name.is_empty())
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        (!cells.is_empty()).then_some(Self { cells })
    }

    /// The declared test-fn names.
    pub(super) fn cells(&self) -> &[String] {
        &self.cells
    }
}

/// One reason a declared claim cell is not one.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Cause {
    /// The trailer names a test the diff did not ADD.
    NotAdded(String),
    /// A test the diff added was not declared. The bijection failing this direction.
    Undeclared(String),
    /// No mutation patch lives at `devco/claim-mutations/<cell>.patch`.
    MissingPatch(String),
    /// The patch touches a file this diff's scan classified as test-bearing. A mutation edits
    /// PRODUCTION code only.
    TouchesTests { cell: String, path: String },
    /// The patch does not `git apply` cleanly in the worktree at HEAD.
    DoesNotApply { cell: String, why: String },
    /// Applied and run, and the run did not REPORT that cell failing - the mutation does not kill.
    NotKilled { cell: String },
}

/// Did the run REPORT `cell` failing by assertion?
///
/// Pure, and the verdict's whole mechanism: a mutation "kills" a cell exactly when a run of that
/// cell, with the mutation applied, fails and names that cell. The naming goes through the SAME
/// key the base run uses ([`super::base::failures`] + [`super::base::is_scoped`]+
/// [`AddedTest::claims`]), so a mutation that reddened a DIFFERENT test reads as *does not kill*
/// rather than as evidence. A run that compiled and passed names no failure; a run that failed to
/// compile names none either - neither kills.
pub(super) fn classify_mutation(text: &str, cell: &AddedTest) -> bool {
    base::failures(text)
        .iter()
        .any(|failure| base::is_scoped(failure, std::slice::from_ref(cell)))
}

/// The patch file a declared cell's mutation lives at.
fn mutation_path(root: &Path, cell: &str) -> PathBuf {
    root.join(MUTATIONS_DIR).join(format!("{cell}.patch"))
}

/// The paths a `git apply`-able unified diff touches, out of its `diff --git` headers.
///
/// THE `+++` LINE WOULD DO, but a patch can carry hunks for several files and a rename or a pure
/// deletion changes what `a/` vs `b/` means - the headers are where git itself names the files it
/// would rewrite, so that is what the test-file check reads.
fn patch_paths(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| line.strip_prefix("diff --git "))
        .filter_map(|rest| rest.split_whitespace().nth(1))
        .map(|b| b.strip_prefix("b/").unwrap_or(b).to_owned())
        .collect()
}

/// A `git apply` over `patch`, optionally as a dry check, run in `wt`.
///
/// The patch is written to a temporary file rather than stdin: the patch is already a committed
/// file this branch carries, so handing git its path keeps the invocation identical between the
/// check and the run and lets the `--check` answer the run will rely on.
fn apply_git(wt: &Path, patch: &Path, check: bool) -> Result<(), String> {
    let mut command = Command::new("git");
    crate::repo::strip_git_env(&mut command);
    command.current_dir(wt).args(["apply"]);
    if check {
        command.arg("--check");
    }
    command.arg(patch);
    let out = command.output().map_err(|e| format!("could not run git apply: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let mut why = String::from_utf8_lossy(&out.stderr).into_owned();
        why.push_str(&String::from_utf8_lossy(&out.stdout));
        Err(why)
    }
}

/// The bijection and patch validations, collected WITHOUT short-circuiting.
///
/// Every cause is collected rather than returned at the first break, mirroring
/// `super::remedies`' rule about printed causes: an author told one of five breakages fixes it
/// and comes back to meet the next one. The git-backed checks ([`apply_git`]) are the third
/// layer and run only for a cell whose earlier checks passed, because a missing patch or a
/// test-file touch makes a dry apply either impossible or pointless.
fn validate(root: &Path, wt: &Path, claim: &Claim, added: &[&str], test_files: &[String]) -> Vec<Cause> {
    let mut causes: Vec<Cause> = Vec::new();
    let added_set: BTreeSet<&str> = added.iter().copied().collect();
    let declared_set: BTreeSet<&str> = claim.cells().iter().map(String::as_str).collect();
    // Bijection, both directions: the declaration must name exactly what the diff added.
    for cell in claim.cells() {
        if !added_set.contains(cell.as_str()) {
            causes.push(Cause::NotAdded(cell.clone()));
        }
    }
    for name in added {
        if !declared_set.contains(name) {
            causes.push(Cause::Undeclared((*name).to_owned()));
        }
    }
    for cell in claim.cells() {
        let patch = mutation_path(root, cell);
        let Ok(content) = std::fs::read_to_string(&patch) else {
            causes.push(Cause::MissingPatch(cell.clone()));
            continue;
        };
        let touched = patch_paths(&content);
        if let Some(touched_path) = touched.iter().find(|path| test_files.contains(path)) {
            causes.push(Cause::TouchesTests {
                cell: cell.clone(),
                path: touched_path.clone(),
            });
            continue;
        }
        if let Err(why) = apply_git(wt, &patch, true) {
            causes.push(Cause::DoesNotApply { cell: cell.clone(), why });
        }
    }
    causes
}

/// Apply one cell's mutation in the isolated target, run the cell, restore, and require it dead.
///
/// RESTORE IS ON EVERY PATH. The mutation is a working-tree change in a worktree git created at
/// HEAD, so `git checkout HEAD -- <touched>` puts the production files back between cells - a
/// leftover mutation would leak into the next cell's run or into a later base run sharing the
/// target. The isolation (`super::isolation::Isolated`) is what stops the mutated build reusing
/// the unmutated one's artifacts, which is the same witness the base/head runs use.
fn kill_cell(root: &Path, wt: &Path, target: &Path, scoped: &Scoped, cell: &str) -> Result<(), Cause> {
    let Some(added) = scoped.tests().iter().find(|one| one.name() == cell) else {
        return Err(Cause::NotAdded(cell.to_owned()));
    };
    let patch = mutation_path(root, cell);
    let Ok(content) = std::fs::read_to_string(&patch) else {
        return Err(Cause::MissingPatch(cell.to_owned()));
    };
    let touched = patch_paths(&content);

    if let Err(why) = Isolated::of(wt, target) {
        return Err(Cause::DoesNotApply {
            cell: cell.to_owned(),
            why,
        });
    }
    if let Err(why) = apply_git(wt, &patch, false) {
        return Err(Cause::DoesNotApply {
            cell: cell.to_owned(),
            why,
        });
    }
    let term = added.term();
    let (_ok, text) = cargo_test(wt, target, &term, Tree::Reconstructed);
    restore(wt, &touched);
    if classify_mutation(&text, added) {
        Ok(())
    } else {
        Err(Cause::NotKilled { cell: cell.to_owned() })
    }
}

/// Restore the files a mutation patch touched back to HEAD.
///
/// Best-effort, like the base worktree teardown: a tree left mutated is a correctness problem for
/// the NEXT cell (whose isolation recompiles our crates from whatever source is on disk), so a
/// failure here is the run's own failure rather than a warning.
fn restore(wt: &Path, paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    let mut command = Command::new("git");
    crate::repo::strip_git_env(&mut command);
    command.current_dir(wt).args(["checkout", "HEAD", "--"]);
    for path in paths {
        command.arg(path);
    }
    let _outcome = command.output();
}

/// Run the whole claim arm: create the worktree, validate, kill every cell, verdict.
pub(super) fn run(root: &Path, scoped: &Scoped, test_files: &[String], claim: &Claim) -> Verdict {
    let target = root.join("target").join("causality-target");
    let wt = root.join("target").join("causality-claim-worktree");
    worktree::remove_worktree(root, &wt);
    if let Err(e) = worktree::add_worktree(root, &wt) {
        eprintln!("xtask test-causality: could not create a claim worktree: {e}");
        return Verdict::Fail;
    }

    let added: Vec<&str> = scoped.tests().iter().map(AddedTest::name).collect();
    let causes = validate(root, &wt, claim, &added, test_files);
    if !causes.is_empty() {
        worktree::remove_worktree(root, &wt);
        return report_refused(&causes);
    }

    let declared = claim.cells().len();
    let mut killed = 0_usize;
    for cell in claim.cells() {
        match kill_cell(root, &wt, &target, scoped, cell) {
            Ok(()) => killed += 1,
            Err(cause) => {
                worktree::remove_worktree(root, &wt);
                return report_refused(&[cause]);
            }
        }
    }
    worktree::remove_worktree(root, &wt);
    report_accepted(declared, killed)
}

/// The arm refused: print every reason, then what the trailer does and does not do.
pub(super) fn report_refused(causes: &[Cause]) -> Verdict {
    for line in refused_lines(causes) {
        eprintln!("{line}");
    }
    Verdict::Fail
}

/// Every line the arm above prints, in order.
///
/// PURE for the reason `super::remedies` learned twice: a printed sentence is prose that nothing
/// derives, and the one thing that keeps a verdict honest is a test that reads its wording.
fn refused_lines(causes: &[Cause]) -> Vec<String> {
    let mut lines = vec![format!(
        "xtask test-causality: FAILED - a `{TRAILER}` declaration did not hold"
    )];
    lines.extend(causes.iter().map(cause_line));
    lines.extend([
        String::new(),
        String::from("The trailer is a CLAIM and this is the check that holds it. A claim cell is an"),
        String::from("added test pinning behaviour the base tree already provides, proved only by a"),
        String::from("committed mutation at `devco/claim-mutations/<test-fn-name>.patch` that makes the"),
        String::from("test FAIL. Fix or drop the declaration - the undeclared path is unchanged and still"),
        String::from("refuses a test that is green against the base behaviour."),
    ]);
    lines
}

/// The one sentence a reason gets.
fn cause_line(cause: &Cause) -> String {
    match cause {
        Cause::NotAdded(cell) => {
            format!("  not added:   {cell}  (the trailer names it; the diff added no such test)")
        }
        Cause::Undeclared(cell) => {
            format!("  not declared: {cell}  (the diff added it and no {TRAILER} names it)")
        }
        Cause::MissingPatch(cell) => {
            format!("  no patch:    {MUTATIONS_DIR}/{cell}.patch  (a claim cell needs a killing mutation)")
        }
        Cause::TouchesTests { cell, path } => {
            format!("  touches tests: {path}  (the `{cell}` mutation must break PRODUCTION code, not a test)")
        }
        Cause::DoesNotApply { cell, why } => {
            format!("  does not apply: {cell}: {why}")
        }
        Cause::NotKilled { cell } => {
            format!("  not killed:  {cell}  (applied, run, and the cell did not fail - the mutation does not kill it)")
        }
    }
}

/// The arm accepted: every declared cell was killed by its mutation.
pub(super) fn report_accepted(declared: usize, killed: usize) -> Verdict {
    for line in accepted_lines(declared, killed) {
        println!("{line}");
    }
    Verdict::Pass
}

/// Every line the arm above prints, in order.
fn accepted_lines(declared: usize, killed: usize) -> Vec<String> {
    vec![format!("ok - claim cells: {declared} declared, {killed} killed")]
}

#[cfg(test)]
mod tests {
    use super::probe::{KILLED, UNRELATED, cell};
    use super::{Cause, Claim, classify_mutation, patch_paths, report_accepted, report_refused};
    use crate::Verdict;

    // A claim over one committed commit message, deduped and sorted, and no claim from a range
    // that carries none - the mirror of the trailer-required rule over on `relocation`.
    #[test]
    fn the_claim_is_deduped_sorted_and_absent_without_a_trailer() {
        let log = "feat(x): subject\n\nClaim-Cell: z_third\nClaim-Cell: a_first\nClaim-Cell: z_third\n";
        assert_eq!(
            Claim::of(log),
            Some(Claim {
                cells: vec![String::from("a_first"), String::from("z_third")],
            })
        );
        // A trailer with no name is no claim: it leaves the run exactly as it was.
        assert_eq!(Claim::of("chore: split\n\nClaim-Cell:\n"), None);
        assert_eq!(Claim::of("chore: no trailer at all\n"), None);
    }

    // RED for the arm, half 1: a run that reports a DIFFERENT test failing does not kill the
    // cell - a mutation that reddens someone else is not evidence about this one.
    #[test]
    fn a_run_that_names_a_different_test_is_not_a_kill() {
        assert!(!classify_mutation(UNRELATED, &cell()));
    }

    // RED for the arm, half 2: a run that compiled and ran green names no failure - a mutation
    // that leaves the cell green does not kill it.
    #[test]
    fn a_run_that_names_no_failure_is_not_a_kill() {
        assert!(!classify_mutation("    Summary [   0.1s] 1 test run: 1 passed\n", &cell()));
        // Compile errors carry no failure either: they measure nothing about the assertion.
        assert!(!classify_mutation("error[E0061]: this function takes 1 argument\n", &cell()));
    }

    // GREEN for the arm: a run that reports this exact cell failing IS the kill, under the same
    // key the base run uses.
    #[test]
    fn a_run_that_names_the_cell_is_a_kill() {
        assert!(classify_mutation(KILLED, &cell()));
    }

    // The accepted verdict and its line, asserted on the compiled verdict rather than prose.
    #[test]
    fn the_accepted_arm_prints_and_passes() {
        assert_eq!(report_accepted(1, 1), Verdict::Pass);
        assert_eq!(report_accepted(2, 2), Verdict::Pass);
    }

    // The refused verdict, with a non-killing mutation refused by name. This is the shape an
    // author is told about, so a test pins the wording.
    #[test]
    fn a_declared_but_unkilled_cell_refuses_the_whole_arm() {
        assert_eq!(
            report_refused(&[Cause::NotKilled {
                cell: String::from("the_cell"),
            }]),
            Verdict::Fail
        );
    }

    // RED: a declared test the diff did not add, and an added test not declared, are both
    // refusals - the bijection is checked both ways.
    #[test]
    fn the_bijection_is_checked_both_ways() {
        let claim = Claim {
            cells: vec![String::from("declared_not_added")],
        };
        let causes = super::validate(
            std::path::Path::new("/nowhere"),
            std::path::Path::new("/nowhere"),
            &claim,
            &["added_not_declared"],
            &[],
        );
        assert!(
            causes.contains(&Cause::NotAdded(String::from("declared_not_added"))),
            "{causes:?}"
        );
        assert!(
            causes.contains(&Cause::Undeclared(String::from("added_not_declared"))),
            "{causes:?}"
        );
    }

    // RED: a declared cell whose mutation is missing is refused, and one whose patch touches a
    // test-bearing file of the diff is refused - the mutation edits PRODUCTION code only.
    #[test]
    fn a_cell_without_a_patch_or_with_a_test_touch_is_refused() {
        let root = std::env::temp_dir().join(format!("sutura-claim-{}", std::process::id()));
        let _swept = std::fs::remove_dir_all(&root);
        let claim = Claim {
            cells: vec![String::from("missing_patch")],
        };
        // No patch file: the git checks never run, so this is testable without one.
        let test_files = [String::from("crates/x/tests/t.rs")];
        let causes = super::validate(&root, &root, &claim, &["missing_patch"], &test_files);
        assert!(
            causes.contains(&Cause::MissingPatch(String::from("missing_patch"))),
            "{causes:?}"
        );
        let _swept = std::fs::remove_dir_all(&root);
    }

    // The patch reader names the files a diff touches, from its `diff --git` headers - including
    // several hunks in one patch and a rename's `b/` side.
    #[test]
    fn the_patch_paths_reader_names_what_git_would_rewrite() {
        let content = concat!(
            "diff --git a/crates/x/src/lib.rs b/crates/x/src/lib.rs\n",
            "index 1111111..2222222 100644\n",
            "--- a/crates/x/src/lib.rs\n",
            "+++ b/crates/x/src/lib.rs\n",
            "@@ -1 +1 @@\n",
            "-pub fn f() -> u8 { 1 }\n",
            "+pub fn f() -> u8 { 2 }\n",
            "diff --git a/crates/x/tests/next.rs b/crates/x/tests/next.rs\n",
            "--- a/crates/x/tests/next.rs\n",
            "+++ b/crates/x/tests/next.rs\n",
        );
        assert_eq!(
            patch_paths(content),
            vec![String::from("crates/x/src/lib.rs"), String::from("crates/x/tests/next.rs")]
        );
    }
}

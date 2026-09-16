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
//! check - [`Claim`] collects what is declared, [`validate`] holds every part of it, and [`run`]
//! acts on nothing that does not hold, and any
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
//! PRODUCTION code that TOUCHES NO TEST LINE - because the whole point is to break the behaviour
//! the cell claims, and a mutation that edits the test to fail proves nothing. The test-line rule
//! is LINE-PRECISE: a mutation may touch a MIXED file (production + an inline `#[cfg(test)] mod
//! tests`) as long as no hunk lands inside a test region, which is what lets #761's inline cells
//! touch their own file's production lines. Each cell is then run in the ISOLATED causality target
//! with the patch applied, and the cell is required to FAIL by its OWN ASSERTION - a `panicked at`
//! site inside its own test region - the mutation kills it. A patch that does not apply, touches
//! a test line, or leaves the cell green is refused by name.
//!
//! **THE COST**: each mutated run pays the same ~68 s isolated rebuild every base/head run pays
//! (the patch forces this workspace's own crates to recompile), so a declared diff costs `+N`
//! runs per gate invocation. Stated, not hidden: this is the price of the evidence the arm is
//! here to consume.
//!
//! **WHAT AN ACCEPTED ARM DOES AND DOES NOT PROVE.** It proves each declared cell DIES under its
//! compiled mutation - the assertion the cell carries really does fail when its subject is broken.
//! It does not prove red-on-base (the behaviour pre-exists, which is the whole point), and it
//! does not prove HEAD is green - that stays `just test` and the nix nextest check. And the
//! mutation a declaration carries is the AUTHOR'S choice: whether the specific broken behaviour it
//! encodes is the behaviour the cell claims - a `const` flip, a body replaced by
//! `Default::default()` - is held by review, not by the gate; the gate holds only that the chosen
//! mutation kills by the cell's own assertion, which is what keeps an accepted arm from looking
//! like coverage a reviewer did not read. The witness discipline is `super::base::PerTestResults`'s:
//! [`classify_mutation`] is pure and under test, and the killed verdict is minted only from a
//! classified output that NAMES the cell, never from a subprocess's `Ok`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Verdict;
use crate::causality::base;
use crate::causality::place::AddedTest;
use crate::causality::regions::{PostImage, TestScope, scope as test_scope};
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
    /// The patch touches a file the repo classifies as test-bearing at HEAD (or one this diff
    /// itself added as a test file). A mutation edits PRODUCTION code only.
    TouchesTests { cell: String, path: String },
    /// The patch does not `git apply` cleanly in the worktree at HEAD.
    DoesNotApply { cell: String, why: String },
    /// Applied and run, and the run reports the cell failing but no `panicked at` site lands
    /// inside the cell's own test region - a kill by `process::exit`/`abort`/signal, a panic in a
    /// production file (a downstream `.expect()`), a `#[track_caller]` relocation, or a FAIL with
    /// no site at all. The assertion the cell carries never discriminated.
    NotByAssertion { cell: String, site: String },
    /// Applied and run, but the tree could not be restored to HEAD, so this cell's mutation leaked
    /// into the next cell's run.
    RestoreFailed { cell: String, why: String },
    /// Applied and run, and the run did not REPORT that cell failing - the mutation does not kill.
    NotKilled { cell: String },
}

/// Why a mutated run did or did not kill its cell.
///
/// The verdict's whole mechanism, reads the PANIC SITE, not the patched set. A patch whose only
/// change is `panic!` / `unwrap()` on `None` at the top of a reached function kills ANY cell that
/// reaches it - it proves reachability, not that the cell's assertion discriminates, which is the
/// "looks like coverage" test AGENTS.md refuses. So a run is a KILL only when nextest's
/// `panicked at <path>:<line>` lands INSIDE the cell's own test region ([`test_scope`] +
/// [`TestScope::covers`]): a genuine assertion-fail panics at the assert's own line in the cell's
/// test code, and every other death - `process::exit`/`abort`/signal (no site), a panic in a
/// production file (a downstream `.expect()` in ordinary careless mutation), a `#[track_caller]`
/// relocation that stops in production - carries no site inside any test region and is refused.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum MutationKill {
    /// The run reported `cell` failing, and a `panicked at` site landed inside the cell's own test
    /// region - the assertion the cell carries discriminated.
    Killed,
    /// The run did not REPORT `cell` failing - a different test, a green run, or a failed compile.
    NotAsserted,
    /// The run reports `cell` failing but no `panicked at` site lands inside a test region - the
    /// cell died some other way. `site` is the first `panicked at <path>:<line>` the run carried,
    /// empty when there was none.
    NotByAssertion { site: String },
}

/// Did the run REPORT `cell` failing by its own assertion?
///
/// Pure, and the verdict's whole mechanism: a mutation "kills" a cell exactly when a run of that
/// cell, with the mutation applied, fails, names that cell, AND carries a `panicked at <path>:<line>`
/// inside the cell's own test region - only an assertion fail panics there. A panic inside
/// production (patched or not) proves reachability and not the assertion; an exit/abort/signal
/// death or a FAIL with no site proves nothing about the assertion either. The naming goes through
/// the SAME key the base run uses ([`super::base::failures`] + [`super::base::is_scoped`] +
/// [`AddedTest::claims`]), so a mutation that reddened a DIFFERENT test reads as *does not kill*
/// rather than as evidence. A run that compiled and passed names no failure; a run that failed to
/// compile names none either - neither kills.
///
/// `read` supplies each panic site's file so [`test_scope`] can say whether the line is test
/// code - a parameter rather than a filesystem call, so the classifier is testable without a
/// checkout, exactly as the region reader itself is.
pub(super) fn classify_mutation(text: &str, cell: &AddedTest, read: &PostImage<'_>) -> MutationKill {
    let named = base::failures(text)
        .iter()
        .any(|failure| base::is_scoped(failure, std::slice::from_ref(cell)));
    if !named {
        return MutationKill::NotAsserted;
    }
    let sites = panic_sites(text);
    if sites.iter().any(|(path, line)| test_scope(path, read).covers(*line)) {
        MutationKill::Killed
    } else {
        MutationKill::NotByAssertion {
            site: sites.first().map(|(path, line)| format!("{path}:{line}")).unwrap_or_default(),
        }
    }
}

/// The `(path, line)` of every `panicked at <path>:<line>:` site nextest printed, for panics inside
/// a test's stack.
fn panic_sites(text: &str) -> Vec<(String, usize)> {
    text.lines()
        .filter_map(|line| {
            let rest = line.split_once(" panicked at ")?.1;
            let mut parts = rest.split(':');
            let path = parts.next()?.to_owned();
            let line: usize = parts.next()?.trim().parse().ok()?;
            Some((path, line))
        })
        .collect()
}

/// The patch file a declared cell's mutation lives at, under `dir` (the HEAD worktree the arm
/// reads it from).
fn mutation_path(dir: &Path, cell: &str) -> PathBuf {
    dir.join(MUTATIONS_DIR).join(format!("{cell}.patch"))
}

/// The paths a `git apply`-able unified diff touches, out of `git apply --numstat` output: one
/// `<added>\t<deleted>\t<path>` row per file git would rewrite, the `b/` side of a rename, a pure
/// deletion still named.
fn numstat_paths(numstat: &str) -> Vec<String> {
    numstat
        .lines()
        .filter_map(|line| line.split('\t').nth(2))
        .map(String::from)
        .collect()
}

/// The paths `git apply --numstat` reports for `patch`, run in `wt` - git's own account of what it
/// would rewrite, which is the forgiveness the hand parser lacked.
///
/// `git apply` ACCEPTS a header-less unified diff (`--- a/x` / `+++ b/x`, with no `diff --git`
/// line), and parsing `diff --git` headers alone would see no paths in one - the test-file rule
/// would compare nothing and `restore` would early-return, leaving the mutation in the tree for
/// the NEXT cell's run. `--numstat` is git resolving exactly the set it would rewrite, so a
/// header-less patch names its paths too. Git refuses a patch that names no file (it errors
/// without `--allow-empty`), so a successful call names at least one; a malformed patch reaches
/// the caller as `Err`.
fn touched_paths(wt: &Path, patch: &Path) -> Result<Vec<String>, String> {
    let mut command = Command::new("git");
    crate::repo::strip_git_env(&mut command);
    command.current_dir(wt).args(["apply", "--numstat"]).arg(patch);
    let out = command
        .output()
        .map_err(|e| format!("could not run git apply --numstat: {e}"))?;
    if !out.status.success() {
        let mut why = String::from_utf8_lossy(&out.stderr).into_owned();
        why.push_str(&String::from_utf8_lossy(&out.stdout));
        return Err(why);
    }
    Ok(numstat_paths(&String::from_utf8_lossy(&out.stdout)))
}

/// One touched file, and the post-image line numbers the patch ADDS there.
type AddedLines = Vec<(String, Vec<usize>)>;

/// The post-image line numbers `patch` ADDS, per file it touches: for each `@@ -a,b +c,d @@` hunk,
/// the `+` body lines, each at its post-image line number (context advances the counter, removed
/// lines do not).
///
/// This is the "hunk line ranges" half of the mixed-file rule - `git apply --numstat` supplies the
/// touched PATHS, this supplies the LINES, and [`test_scope`] supplies which lines are test
/// code. A header-less patch is handled because the current file tracks the `+++ b/` side, which
/// such a patch carries.
fn patch_added_lines(patch: &str) -> AddedLines {
    let mut out: AddedLines = Vec::new();
    let mut current: Option<usize> = None;
    let mut new_line = 0_usize;
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            let file = rest.trim().strip_prefix("b/").unwrap_or_else(|| rest.trim()).to_owned();
            current = Some(out.iter().position(|(one, _)| *one == file).unwrap_or_else(|| {
                out.push((file, Vec::new()));
                out.len() - 1
            }));
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            // `@@ -a,b +c,d @@` (optionally a label after): the `+c,d` opens the post-image range.
            new_line = new_hunk_start(rest);
            continue;
        }
        if let Some(index) = current {
            match line.as_bytes().first() {
                Some(b'+') => {
                    if let Some(entry) = out.get_mut(index) {
                        entry.1.push(new_line);
                    }
                    new_line += 1;
                }
                Some(b' ') => new_line += 1,
                _ => {}
            }
        }
    }
    out
}

/// The post-image start line a unified-diff hunk header `@@ -a,b +c,d @@` names for the NEW side.
fn new_hunk_start(header: &str) -> usize {
    // `header` is everything after `@@ `, e.g. `-1,5 +1,7 @@` (a label may follow the second `@@`).
    let body = header.split(" @@").next().unwrap_or(header);
    let new_token = body.split_whitespace().find(|token| token.starts_with('+')).unwrap_or("+1");
    let number = new_token.trim_start_matches('+');
    let start = number.split(',').next().unwrap_or(number);
    start.trim().parse().unwrap_or(1)
}

/// Does `patch` add any line inside a test region of any file it touches?
///
/// The mixed-file rule, LINE-PRECISE: a file the repo classifies as all-test ([`TestScope::WholeFile`])
/// is refused on any touch; a MIXED file (production + a `#[cfg(test)]` region) is refused only
/// when the patch ADDS a line inside one of its test regions; and a NON-test file reaches here and
/// is allowed. A diff-declared test file (`test_files`) whose test region this reader cannot find
/// at HEAD stays conservatively whole: if the repo classifier cannot show WHERE its tests sit, a
/// production-looking hunk is still a test-file edit from the diff's own account, and the direction
/// that asks rather than guesses keeps refusing.
fn touches_test_line(patch: &str, touched: &[String], test_files: &[String], read: &PostImage<'_>) -> Option<String> {
    let added = patch_added_lines(patch);
    for path in touched {
        match test_scope(path, read) {
            TestScope::WholeFile => return Some(path.clone()),
            TestScope::Regions(regions) if regions.is_empty() && test_files.contains(path) => {
                return Some(path.clone());
            }
            TestScope::Regions(regions) => {
                let added_here = added.iter().find(|(one, _)| one == path);
                if added_here
                    .is_some_and(|(_, lines)| lines.iter().any(|line| regions.iter().any(|region| region.contains(line))))
                {
                    return Some(path.clone());
                }
            }
        }
    }
    None
}

/// A post-image reader over the worktree the arm created at HEAD.
///
/// The test-file rule reads the state the mutation actually runs against, and the patch is read
/// from the SAME committed tree the trailer declared it in - an uncommitted patch in the caller's
/// working tree is not the range under review, so it is not evidence.
fn head_reader(wt: &Path) -> impl Fn(&str) -> Option<String> + '_ {
    move |path| std::fs::read_to_string(wt.join(path)).ok()
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
fn validate(wt: &Path, claim: &Claim, added: &[&str], test_files: &[String]) -> Vec<Cause> {
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
    let read = head_reader(wt);
    for cell in claim.cells() {
        // The patch is read from the HEAD worktree, not the caller's working tree - an UNCOMMITTED
        // patch is not the range the trailer declared, so it is not under review.
        let patch = mutation_path(wt, cell);
        let Ok(content) = std::fs::read_to_string(&patch) else {
            causes.push(Cause::MissingPatch(cell.clone()));
            continue;
        };
        let touched = match touched_paths(wt, &patch) {
            Ok(paths) => paths,
            Err(why) => {
                causes.push(Cause::DoesNotApply { cell: cell.clone(), why });
                continue;
            }
        };
        // The test-file rule is LINE-PRECISE and region-aware: a mutation may touch a MIXED file
        // (production + inline `#[cfg(test)] mod tests`) as long as no hunk lands inside a test
        // region - that is what makes #761's inline cells declarable at all, since a patch to that
        // file's production lines would otherwise be refused for touching the very file they assert
        // in. `git apply --numstat` supplies the touched path set; the patch's own `+c,d` hunks
        // supply the post-image line numbers; `regions::scope` supplies which lines are test code.
        if let Some(touched_path) = touches_test_line(&content, &touched, test_files, &read) {
            causes.push(Cause::TouchesTests {
                cell: cell.clone(),
                path: touched_path,
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
fn kill_cell(wt: &Path, target: &Path, scoped: &Scoped, cell: &str) -> Result<(), Cause> {
    let Some(added) = scoped.tests().iter().find(|one| one.name() == cell) else {
        return Err(Cause::NotAdded(cell.to_owned()));
    };
    let patch = mutation_path(wt, cell);
    let Ok(_content) = std::fs::read_to_string(&patch) else {
        return Err(Cause::MissingPatch(cell.to_owned()));
    };
    let touched = match touched_paths(wt, &patch) {
        Ok(paths) => paths,
        Err(why) => {
            return Err(Cause::DoesNotApply {
                cell: cell.to_owned(),
                why,
            });
        }
    };
    if let Err(why) = apply_git(wt, &patch, false) {
        return Err(Cause::DoesNotApply {
            cell: cell.to_owned(),
            why,
        });
    }
    // The isolation clean runs INSIDE `cargo_test` on the same witness (dir, target, profile);
    // there is no second call here whose failure would read as a misleading "does not apply".
    let term = added.term();
    let (_ok, text) = cargo_test(wt, target, &term, Tree::Reconstructed);
    // CLASSIFY BEFORE RESTORE: the region the panic site is judged against must be the cell's
    // post-mutation file, because a mixed-file patch shifts its own test region's line numbers -
    // reading the HEAD image would compare a mutated `panicked at <line>` against the wrong region.
    let verdict = classify_mutation(&text, added, &head_reader(wt));
    // RESTORE IS ON EVERY PATH and a failed restore is the run's own failure: a tree left mutated
    // leaks this cell's mutation into the NEXT cell's run, whose isolation recompiles whatever is
    // on disk.
    if let Err(why) = restore(wt, &touched) {
        return Err(Cause::RestoreFailed {
            cell: cell.to_owned(),
            why,
        });
    }
    match verdict {
        MutationKill::Killed => Ok(()),
        MutationKill::NotAsserted => Err(Cause::NotKilled { cell: cell.to_owned() }),
        MutationKill::NotByAssertion { site } => Err(Cause::NotByAssertion {
            cell: cell.to_owned(),
            site,
        }),
    }
}

/// Restore the files a mutation patch touched back to HEAD.
///
/// A restore failure is the run's own failure rather than a warning (returned as the cause, not
/// swallowed): a tree left mutated is a correctness problem for the NEXT cell, and staying silent
/// about it would let one cell's patch leak into another's run.
fn restore(wt: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut command = Command::new("git");
    crate::repo::strip_git_env(&mut command);
    command.current_dir(wt).args(["checkout", "HEAD", "--"]);
    for path in paths {
        command.arg(path);
    }
    let out = command.output().map_err(|e| format!("could not run git checkout: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let mut why = String::from_utf8_lossy(&out.stderr).into_owned();
        why.push_str(&String::from_utf8_lossy(&out.stdout));
        Err(why)
    }
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
    let causes = validate(&wt, claim, &added, test_files);
    if !causes.is_empty() {
        worktree::remove_worktree(root, &wt);
        return report_refused(&causes);
    }

    let declared = claim.cells().len();
    let mut killed = 0_usize;
    for cell in claim.cells() {
        match kill_cell(&wt, &target, scoped, cell) {
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
        Cause::NotByAssertion { cell, site } => {
            format!(
                "  not by the cell's own assertion: {site}  (the `{cell}` mutation killed it without failing the assertion inside the cell's own test region - an exit/signal, a production panic, or a FAIL with no site)"
            )
        }
        Cause::RestoreFailed { cell, why } => {
            format!("  restore failed: {cell}: {why}  (a failed restore leaks the mutation into the next cell)")
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
mod tests;

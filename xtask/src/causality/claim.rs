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
//! THE BIJECTION IS PER COMMIT, and is a bijection in one direction only. [`Claim::of`] reads
//! `base..HEAD`'s messages NUL-delimited PER COMMIT, so a declaration is tied to the commit that
//! carried it and answers for the tests THAT commit added ([`validate`]/[`Cause::NotAdded`]).
//! The old range-wide unity was the bug `github.com/telekom/sutura#954`: one legitimate declaring
//! commit made every OTHER added test in the range read as undeclared (measured `×135` on #929).
//! The reverse direction - an added test no declaration names - is NOT a refusal here: a real
//! branch commonly lands ordinary red-on-base tests beside a claim cell (measured on `main`:
//! `f14e8a3a` declared one of three added tests), and those are proven by the ordinary base/head
//! proof after the claim arm (the composite in `super::run`), not refused. `super::rot`'s
//! re-proof builds its claim through [`Claim::synthetic`], which declares no commit and so has no
//! per-commit check to skip - never by a log shape `Claim::of` would have to guess at.
//!
//! THE MUTATION IS ONE COMMITTED PATCH PER CELL, under `devco/claim-mutations/`. A patch file is
//! reviewable in the diff and byte-reproducible; it is read at RUNTIME from the checkout (so it
//! lives in the same range the trailer does) and it must be a `git apply`-able unified diff over
//! PRODUCTION code that TOUCHES NO TEST LINE and CREATES NO FILE - because the whole point is to
//! break the behaviour the cell claims, and a mutation that edits the test to fail (or a new file
//! whose own test region no image at HEAD exists to compare against) proves nothing. A CREATED
//! file is read from the patch's own bytes (a `--- /dev/null` file section, or a git rename/copy
//! header's `to` path), never from whether a reader happens to find the path at HEAD - a mutation
//! may only edit code HEAD already carries.
//! The test-line rule is a PATH half and a TEXT half. The PATH half refuses a touch of a file that
//! is all test at HEAD. The TEXT half decides a MIXED file (production + an inline
//! `#[cfg(test)] mod tests`, the shape #761's inline cells need to patch their own file's
//! production lines): it APPLIES the patch, then requires every `#[cfg(test)]` region of the
//! post-image to be byte-identical to HEAD's by BYTES, re-locating the regions by content on each
//! image - so a deletion-only hunk above the region (which shifts line numbers and used to let a
//! second hunk smuggle an edit into the cell's own assertion past the old LINE rule) still refuses
//! as *patch rewrites the cell*. Each cell is then run in the ISOLATED causality target with the
//! patch applied, and the cell is required to FAIL by its OWN ASSERTION - a `panicked at` site in
//! the cell's OWN file, inside its own test fn (located by the fn's name on the post-image - `fn`,
//! `async fn`, any visibility in front, never by a line) - the mutation kills it. A patch that
//! does not apply, touches a test line or test region, creates a file, or leaves the cell green is
//! refused by name.
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
//! [`kill::classify_mutation`] is pure and under test, and the killed verdict is minted only from a
//! classified output that NAMES the cell, never from a subprocess's `Ok`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Verdict;
use crate::causality::diff::{self, ChangedFile};
use crate::causality::names::Ident;
use crate::causality::place::AddedTest;
use crate::causality::regions::{PostImage, TestScope, cfg_test_regions as test_regions, scope as test_scope};
use crate::causality::runner::{Tree, cargo_test};
use crate::causality::scoped::Scoped;
use crate::causality::worktree;

mod kill;
use kill::attest;
#[cfg(test)]
use kill::{MutationKill, classify_mutation};

/// The repository-relative directory every committed mutation patch lives in.
pub(super) const MUTATIONS_DIR: &str = "devco/claim-mutations";

/// The commit trailer that declares a claim cell.
const TRAILER: &str = "Claim-Cell:";

/// A declaring commit object name paired with the cells its own message named.
///
/// Named so the per-commit scope (`github.com/telekom/sutura#954`) is one readable unit instead
/// of a bare tuple type repeated across `Claim`'s field, its accessor and `Claim::of`'s builder.
pub(super) type CommitCells = Vec<(String, Vec<String>)>;

#[cfg(test)]
mod probe;
/// What the commit messages in the measured range DECLARED, scoped per commit.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Claim {
    /// The test-fn names the trailers named, deduplicated and sorted.
    cells: Vec<String>,
    /// The cells each commit in the range declared, keyed by commit object name.
    ///
    /// THE PER-COMMIT HALF OF THE SCOPE. `validate` needs to know which tests a declaration
    /// ANSWERS for - the ones the declaring COMMIT added - rather than every test the whole range
    /// added. A declaration on one commit must not be held against a sibling commit's own added
    /// tests (`github.com/telekom/sutura#954`), so this map is what ties a cell back to the
    /// commit whose message carried it. The flat `cells` list stays for the kill loop and for
    /// `super::rot`'s synthetic claim; this map is what the bijection is checked through.
    by_commit: CommitCells,
}

/// The distinct, identifier-shaped names among `names`, sorted.
fn named<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    names
        .map(str::trim)
        .filter(|name| Ident::parse(name).is_some())
        .map(String::from)
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

impl Claim {
    /// The claim `log` carries, if it carries one.
    ///
    /// `log` is `worktree::messages`' NUL-delimited per-commit stream: for each commit, the
    /// declaration is read from THAT commit's own message, so a declaration answers for the
    /// commit that carries it and not for its neighbours (`github.com/telekom/sutura#954`).
    ///
    /// A stream with NO NUL at all carries NO claim. It is not a `-z` log (which always ends each
    /// commit's body with one), so there is no commit to tie a declaration to, and reading it
    /// under a made-up key would let a declaration answer for nothing - fail-open on the one
    /// input the old `--format=%B` produced. [`Claim::synthetic`] is `super::rot`'s constructor.
    ///
    /// A TRAILER WITH NO NAME IS NO CLAIM, which is the fail-closed direction: it leaves the run
    /// exactly as it was, so a malformed declaration buys nothing. Requiring a name is what stops
    /// the trailer being copied forward onto an unrelated commit without being obviously wrong to
    /// a reviewer - [`Cause::NotAdded`] refuses a name the diff did not add.
    ///
    /// TIGHTENED: a declared name must be a valid Rust identifier ([`Ident::parse`]). The old
    /// scan matched ANY line whose trimmed form started with `Claim-Cell:`, so a commit merely
    /// DISCUSSING claim cells in prose declared one. Every real cell this tree commits is a bare
    /// identifier, so the tightening drops a prose mention and keeps every declaration.
    pub(super) fn of(log: &str) -> Option<Self> {
        let per_commit: CommitCells = worktree::commit_logs(log)
            .into_iter()
            .filter_map(|(commit, message)| {
                let cells = named(message.lines().filter_map(|line| line.trim().strip_prefix(TRAILER)));
                (!cells.is_empty()).then_some((String::from(commit), cells))
            })
            .collect();
        let cells: Vec<String> = per_commit
            .iter()
            .flat_map(|(_, cells)| cells.iter().cloned())
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        (!cells.is_empty()).then_some(Self {
            cells,
            by_commit: per_commit,
        })
    }

    /// A claim over `cells` that NO commit declared: `super::rot` re-proving every committed
    /// mutation, with no diff for a declaration to be held against - so no per-commit check.
    pub(super) fn synthetic<'a>(cells: impl Iterator<Item = &'a str>) -> Option<Self> {
        let cells = named(cells);
        (!cells.is_empty()).then_some(Self {
            cells,
            by_commit: Vec::new(),
        })
    }

    /// The declared test-fn names.
    pub(super) fn cells(&self) -> &[String] {
        &self.cells
    }

    /// Each declaring commit and the cells its own message named.
    pub(super) const fn by_commit(&self) -> &CommitCells {
        &self.by_commit
    }
}

/// One reason a declared claim cell is not one.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Cause {
    /// The trailer names a test the diff did not ADD.
    NotAdded(String),
    /// No mutation patch lives at `devco/claim-mutations/<cell>.patch`.
    MissingPatch(String),

    /// The patch touches a file the repo classifies as test-bearing at HEAD (or one this diff
    /// itself added as a test file). A mutation edits PRODUCTION code only.
    TouchesTests { cell: String, path: String },
    /// The patch's own bytes declare a `--- /dev/null` file section, or a git rename/copy header's
    /// `to` path - the mutation would CREATE a
    /// file. `head_region_texts` only snapshots paths that exist at HEAD, so a new file's own
    /// `#[cfg(test)]` region is never text-compared; a mutation may only edit code HEAD already
    /// carries.
    CreatesFile { cell: String, path: String },
    /// The patch's POST-image is not byte-identical to HEAD inside the touched file's test
    /// regions: it rewrites the cell's own test code (a deletion-only hunk above `#[cfg(test)]`
    /// that shifts a later hunk into the region included) rather than breaking production. The
    /// comparison is of region TEXT on each image, never of a line number, so a line-shift cannot
    /// smuggle an edit past it.
    PatchRewritesCell { cell: String, path: String },
    /// The patch does not `git apply` cleanly in the worktree at HEAD.
    DoesNotApply { cell: String, why: String },
    /// Applied and run, and the run reports the cell failing but no `panicked at` site lands
    /// inside the cell's OWN test fn, in the cell's OWN file - a kill by `process::exit`/`abort`/
    /// signal, a panic in a production file (a downstream `.expect()`), a panic inside another
    /// file's test region (a shared `tests/common` helper), or a FAIL with no site at all. The
    /// assertion the cell carries never discriminated.
    NotByAssertion { cell: String, site: String },
    /// Applied and run, but the tree could not be restored to HEAD, so this cell's mutation leaked
    /// into the next cell's run.
    RestoreFailed { cell: String, why: String },
    /// Applied and run, and the run did not REPORT that cell failing - the mutation does not kill.
    NotKilled { cell: String },
    /// The nested build under the mutation never reached a test at all - cargo could not resolve
    /// the manifest, or the tree did not compile. Distinct from `NotKilled`: that is a build that
    /// ran and the cell survived it, this is a build the gate never got a chance to run the cell
    /// against, so it is not a verdict about the declaration at all.
    BuildFailed { cell: String, why: String },
}

impl Cause {
    /// Is this cause the gate FAILING TO MEASURE, rather than measuring and finding the
    /// declaration wanting?
    ///
    /// The distinction [`report_refused`] exits on: every other cause is an author-actionable
    /// defect in THIS diff's own declaration (a missing patch, a bijection mismatch, a rewritten
    /// cell) and stays `Verdict::Fail`; this one is the gate's own precondition not holding, which
    /// is the same non-verdict shape as `Verdict::Inconclusive` elsewhere in this gate - a subject
    /// it did not reach, not a subject it read and rejected.
    const fn unmeasured(&self) -> bool {
        matches!(self, Self::BuildFailed { .. })
    }
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

/// The numstat PATH half of the test-file rule: refuse any touched file the repo classifies as
/// all-test at HEAD ([`TestScope::WholeFile`]), or a diff-declared test file (`test_files`) whose
/// test regions this reader cannot find at HEAD.
///
/// A file is also a legitimate MIXED target in its production lines - the shape an inline claim
/// cell needs to patch its own file - so this half refuses only what is test code WITHOUT a
/// recoverable region to preserve; whether a mixed-file patch edits test code is the TEXT half's
/// question ([[`rewrites_cell_test_region`]]), not this one's.
fn touches_test(touched: &[String], test_files: &[String], read: &PostImage<'_>) -> Option<String> {
    for path in touched {
        match test_scope(path, read) {
            TestScope::WholeFile => return Some(path.clone()),
            TestScope::Regions(regions) if regions.is_empty() && test_files.contains(path) => {
                return Some(path.clone());
            }
            TestScope::Regions(_) => {}
        }
    }
    None
}

/// The path of every file `patch_text` CREATES, read from the patch's own bytes - a `--- /dev/null`
/// line (the header-less shape too, matching how [`touched_paths`] already treats a header-less
/// patch as evidence) followed by `+++ b/<path>`, OR a git rename/copy header (`rename to <path>` /
/// `copy to <path>`, with no `---`/`+++` lines at ~100% similarity) whose `to` path is a path HEAD
/// does not carry.
///
/// A TEXTUAL signal on the patch itself, not "does the reader find this path at HEAD": a patch
/// touching a path a fixture's reader simply never populated (a malformed or context-mismatched
/// patch) is a [`Cause::DoesNotApply`], not a file-creating mutation, and conflating the two would
/// misreport the former. `head_region_texts` only snapshots a touched path's test-region text when
/// `read` can find that path at HEAD, so a NEW file's own `#[cfg(test)]` region is never
/// text-compared - the TEXT rule has nothing to compare it against. Before this check the hole was
/// closed only by an accident: `restore`'s `git checkout HEAD -- <new>` fails for a path HEAD never
/// had, so the run answered `RestoreFailed`, a cause whose wording ("leaks the mutation into the
/// next cell") misdescribes what actually happened. Refusing here names the real limit and runs
/// before any patch is applied, so a file-creating mutation never reaches `apply_git` at all - the
/// rename/copy half is the same accident: `git apply --numstat` ACCEPTS a rename/copy header, the
/// patch applies in the isolated worktree, and `git checkout HEAD -- <renamed>` fails exactly like
/// the `--- /dev/null` case, so the run answered `RestoreFailed` there too.
fn created_paths(patch_text: &str) -> Vec<String> {
    let lines: Vec<&str> = patch_text.lines().collect();
    let mut out: Vec<String> = Vec::new();
    // A `--- /dev/null` file section names its created path via the `+++ b/<path>` that follows.
    for (index, line) in lines.iter().enumerate() {
        if *line == "--- /dev/null"
            && let Some(path) = lines.get(index + 1).and_then(|next| next.strip_prefix("+++ b/"))
        {
            out.push(String::from(path));
        }
    }
    // A git rename/copy header names its created path on the `rename to`/`copy to` line (bare, no
    // `b/` prefix): both tell git to WRITE a path HEAD does not carry, so both are creations.
    for line in &lines {
        if let Some(path) = line.strip_prefix("rename to ") {
            out.push(String::from(path));
        } else if let Some(path) = line.strip_prefix("copy to ") {
            out.push(String::from(path));
        }
    }
    out
}

/// The lines of every `#[cfg(test)]` region in `text`, concatenated in file order.
///
/// CONTENT-DERIVED: the regions are re-found on whatever image is handed here, so a line-shift
/// above them (a deletion-only production hunk) does not change the answer, while a byte change
/// inside one does. `""` for a file with no `#[cfg(test)]` region. This is the TEXT rule's
/// comparison key: `claim` compares it for a touched file at HEAD and after the patch and refuses
/// on any difference, which is how a deletion-only hunk above `#[cfg(test)]` stops being an
/// exploit - the shifted post-image region is still found and compared by its bytes, never by a
/// line number from the other image.
fn test_region_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    // `test_regions` yields 1-BASED, half-open ranges (`#[cfg(test)]` lines as a person numbers
    // them); `lines` is 0-based, so each side is shifted down by one for the slice.
    for region in test_regions(text) {
        if let Some(slice) = lines.get(region.start.saturating_sub(1)..region.end.saturating_sub(1)) {
            for &line in slice {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// The test-region text of `path` as `read` sees it, or `None` when `read` has no such file.
///
/// The regions are re-found by CONTENT on the image handed in ([`test_region_text`]), never by
/// a line number carried from another image - that is what makes the TEXT rule shift-proof.
fn region_text_of(path: &str, read: &PostImage<'_>) -> Option<String> {
    read(path).map(|text| test_region_text(&text))
}

/// HEAD's test-region text for every touched path that exists at HEAD, as the TEXT rule's
/// `before` image. Snapshot taken BEFORE the patch applies, because the same reader afterwards
/// reads the post-image.
fn head_region_texts(touched: &[String], read: &PostImage<'_>) -> Vec<(String, String)> {
    touched
        .iter()
        .filter_map(|path| region_text_of(path, read).map(|text| (path.clone(), text)))
        .collect()
}

/// The TEXT half of the test-file rule, decided AFTER `git apply`: is any touched file's test
/// region no longer byte-identical to HEAD's?
///
/// The exploit this closes was LINE-BASED: the old rule compared the patch's post-image `+c,d`
/// line numbers against the file's regions at HEAD, so ONE deletion-only hunk above `#[cfg(test)]`
/// shifted every later post-image number, and a SECOND hunk then rewrote the cell's own assertion
/// inside the region while the rule judged it against the un-shifted HEAD range. Comparing region
/// TEXT ([[`test_region_text`]], re-found by content on each image) is immune: a line-shift
/// changes line numbers but not bytes, so an edit inside the region still differs and a
/// production-only shift still matches. `before` is the snapshot taken pre-apply; `after` reads
/// the worktree post-apply.
fn rewrites_cell_test_region(before: &[(String, String)], after: &PostImage<'_>) -> Option<String> {
    before.iter().find_map(|(path, head_text)| {
        region_text_of(path, after)
            .filter(|post_text| post_text != head_text)
            .map(|_| path.clone())
    })
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
///
/// `pub(super)` for `super::rot`: the apply-only hygiene half of `github.com/telekom/sutura#950`
/// re-checks every COMMITTED patch rather than only the one a diff's own trailer just declared,
/// and it is the same `git apply --check` this module already runs - reusing it is what keeps the
/// two from drifting into two spellings of one rule.
pub(super) fn apply_git(wt: &Path, patch: &Path, check: bool) -> Result<(), String> {
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
///
/// THE BIJECTION IS PER COMMIT (`github.com/telekom/sutura#954`), with a deliberate asymmetry.
/// Each DECLARING commit must name tests IT added - a cell whose declaring commit added no such
/// test is [`Cause::NotAdded`]. But the reverse direction, an added test the range never
/// declared, is NOT a refusal here: a real branch commonly adds ordinary red-on-base tests
/// beside a claim cell (measured on `main`: `f14e8a3a` declared one of three added tests), and
/// those tests are proven by the ordinary base/head proof after the claim arm, not refused.
/// `claim.rs`'s own header always said an undeclared claim cell "still reaches the normal proof
/// and its green-against-base refusal unchanged" - this is that sentence made real.
fn validate(wt: &Path, claim: &Claim, test_files: &[String]) -> Vec<Cause> {
    let mut causes: Vec<Cause> = Vec::new();
    // Per commit: the range's added set is not the unit - a declaration on one commit must not be held against a
    // sibling commit's tests, which is exactly the bug #954 measured (`×135` undeclared on #929).
    // `super::rot`'s synthetic claim declares no commit, so this loop has nothing to check for it.
    let read = head_reader(wt);
    for (commit, declared) in claim.by_commit() {
        let added: Vec<String> = commit_added_names(wt, commit).unwrap_or_default();
        let added_set: BTreeSet<&str> = added.iter().map(String::as_str).collect();
        for cell in declared {
            if !added_set.contains(cell.as_str()) {
                causes.push(Cause::NotAdded(cell.clone()));
            }
        }
    }
    for cell in claim.cells() {
        // The patch is read from the HEAD worktree, not the caller's working tree - an UNCOMMITTED
        // patch is not the range the trailer declared, so it is not under review.
        let patch = mutation_path(wt, cell);
        let Ok(patch_text) = std::fs::read_to_string(&patch) else {
            causes.push(Cause::MissingPatch(cell.clone()));
            continue;
        };
        // A mutation may only edit code HEAD already carries: a `--- /dev/null` file section (or a
        // git rename/copy header's `to` path) would CREATE a file, and its own `#[cfg(test)]`
        // region could never be text-compared below -
        // checked from the patch's own bytes before any git subprocess runs, so a file-creating
        // patch never reaches `apply_git`.
        if let Some(created) = created_paths(&patch_text).into_iter().next() {
            causes.push(Cause::CreatesFile {
                cell: cell.clone(),
                path: created,
            });
            continue;
        }
        let touched = match touched_paths(wt, &patch) {
            Ok(paths) => paths,
            Err(why) => {
                causes.push(Cause::DoesNotApply { cell: cell.clone(), why });
                continue;
            }
        };
        // The test-file rule is a PATH half and a TEXT half. The PATH half (`--numstat`) keeps
        // refusing a touch of a file that is all test at HEAD, or a diff-declared test file whose
        // test regions this reader cannot find. The TEXT half decides a MIXED file (production +
        // inline `#[cfg(test)] mod tests`, the shape #761's inline cells need to patch): it
        // APPLIES the patch for real, then requires every `#[cfg(test)]` region of the post-image
        // to be byte-identical to HEAD's, so an edit smuggled past a line-shift still refuses.
        // Applying for real also makes a bad patch answer `DoesNotApply` the way the run will.
        if let Some(touched_path) = touches_test(&touched, test_files, &read) {
            causes.push(Cause::TouchesTests {
                cell: cell.clone(),
                path: touched_path,
            });
            continue;
        }
        // Snapshot the HEAD test-region text BEFORE the apply mutates the worktree; the same
        // reader afterwards reads the post-image.
        let before = head_region_texts(&touched, &read);
        if let Err(why) = apply_git(wt, &patch, false) {
            causes.push(Cause::DoesNotApply { cell: cell.clone(), why });
            continue;
        }
        let rewritten = rewrites_cell_test_region(&before, &read);
        // RESTORE IS ON EVERY PATH and a failed restore is a cause: an apply this method leaves in
        // the tree would leak into the next cell's validate or kill run.
        let restored = restore(wt, &touched).err();
        if let Some(path) = rewritten {
            causes.push(Cause::PatchRewritesCell {
                cell: cell.clone(),
                path,
            });
        } else if let Some(why) = restored {
            causes.push(Cause::RestoreFailed { cell: cell.clone(), why });
        }
    }
    causes
}

/// The tests ONE commit's own diff added, by name.
///
/// The single-commit diff numbers its `AddedLine`s in THAT commit's post-image, so they resolve
/// against the commit's own tree, never HEAD's: a later commit in the range that shifts or deletes
/// lines above the test would otherwise read it as absent (`github.com/telekom/sutura#1054`). The
/// kill step still reads HEAD, because the mutation runs there. `OK(empty)` for a
/// commit whose diff added no named test, which a declaration over it must then answer as
/// [`Cause::NotAdded`]; `None` when the commit's diff cannot be read at all (an unnameable hash),
/// fail-closed in the direction that refuses.
///
/// INSEPARABLE IS NOT EMPTY. A declaring commit whose added test shares a file with its own
/// implementation change (`github.com/telekom/sutura#837`'s shape) is still a commit that ADDED
/// the test - `causality.rs`'s own `NotSeparable` arm `Scan::of`s the inseparable file list and
/// builds the `Scoped` the claim arm runs from. Empty would falsely `NotAdded` every cell of an
/// inseparable declaring commit, which is exactly the shape `f61bcf28` (and this gate's own
/// inseparable fixture) declares in.
fn commit_added_names(wt: &Path, commit: &str) -> Option<Vec<String>> {
    use crate::causality::plan::Plan;
    use crate::causality::scoped::Scan;
    let at = crate::causality::provenance::Commit::parse(commit)?;
    let read = |path: &str| worktree::at_base(wt, &at, path);
    let files: Vec<ChangedFile> = diff::commit_additions(wt, commit)?;
    let (test_files, scannable) = match crate::causality::plan::plan(&files, &read) {
        Plan::Separable(separable) => (separable.test_files, files),
        Plan::NotSeparable { files: inseparable, .. } => (inseparable, files),
        Plan::NotRequired => return Some(Vec::new()),
    };
    match Scan::of(&scannable, &test_files, &read) {
        Scan::Runnable(scoped) => Some(scoped.tests().iter().map(AddedTest::name).map(String::from).collect()),
        _ => Some(Vec::new()),
    }
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
    let (ok, text) = cargo_test(wt, target, &term, Tree::Reconstructed);
    // ATTEST BEFORE RESTORE: the region the panic site is judged against must be the cell's
    // post-mutation file, because a mixed-file patch shifts its own test region's line numbers -
    // reading the HEAD image would compare a mutated `panicked at <line>` against the wrong region.
    let verdict = attest(ok, &text, cell, added, &head_reader(wt));
    // RESTORE IS ON EVERY PATH and a failed restore is the run's own failure: a tree left mutated
    // leaks this cell's mutation into the NEXT cell's run, whose isolation recompiles whatever is
    // on disk.
    if let Err(why) = restore(wt, &touched) {
        return Err(Cause::RestoreFailed {
            cell: cell.to_owned(),
            why,
        });
    }
    verdict
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

/// Which task is asking, and what an unheld declaration should be told to do about it.
///
/// The remedy differs by caller: "fix or drop the declaration" is only true where the trailer
/// itself lives in the diff a reader has open. `causality::rot::run` re-checks a declaration
/// accepted commits ago - nothing in the current diff to fix - so the actionable step there is
/// re-anchoring or dropping the committed patch, not the declaration. Reusing [`run`] wholesale
/// without this would print the WRONG task name and send that reader to a trailer that is not
/// theirs to change.
#[derive(Clone, Copy)]
pub(super) struct Caller {
    /// The task name the verdict line names - the one a reader actually ran.
    pub(super) task: &'static str,
    /// The closing remedy, one line per element, printed under a `Verdict::Fail`.
    pub(super) remedy: &'static [&'static str],
}

impl Caller {
    /// `causality::run`'s own dispatch - the gate this module was built to hold.
    pub(super) const TEST_CAUSALITY: Self = Self {
        task: "test-causality",
        remedy: &[
            "The trailer is a CLAIM and this is the check that holds it. A claim cell is an",
            "added test pinning behaviour the base tree already provides, proved only by a",
            "committed mutation at `devco/claim-mutations/<test-fn-name>.patch` that makes the",
            "test FAIL. Fix or drop the declaration - the undeclared path is unchanged: an added",
            "test no declaration names still reaches the ordinary proof and its green-against-base",
            "refusal.",
        ],
    };
}

/// Run the whole claim arm: create the worktree, validate, kill every cell, verdict.
pub(super) fn run(root: &Path, scoped: &Scoped, test_files: &[String], claim: &Claim, caller: Caller) -> Verdict {
    let target = root.join("target").join("causality-target");
    let wt = root.join("target").join("causality-claim-worktree");
    worktree::remove_worktree(root, &wt);
    if let Err(e) = worktree::add_worktree(root, &wt) {
        eprintln!("xtask {}: could not create a claim worktree: {e}", caller.task);
        return Verdict::Fail;
    }
    let causes = validate(&wt, claim, test_files);
    if !causes.is_empty() {
        worktree::remove_worktree(root, &wt);
        return report_refused(&causes, caller);
    }

    let declared = claim.cells().len();
    let mut killed = 0_usize;
    for cell in claim.cells() {
        match kill_cell(&wt, &target, scoped, cell) {
            Ok(()) => killed += 1,
            Err(cause) => {
                worktree::remove_worktree(root, &wt);
                return report_refused(&[cause], caller);
            }
        }
    }
    worktree::remove_worktree(root, &wt);
    report_accepted(declared, killed)
}

/// The arm refused: print every reason, then what the trailer does and does not do.
///
/// **THE VERDICT ITSELF SPLITS ON [`Cause::unmeasured`].** Every cause but `BuildFailed` is an
/// author-actionable defect in what THIS diff declared, and stays `Verdict::Fail`. A run whose
/// every cause is `BuildFailed` never measured anything - the gate's own precondition (a build
/// that reaches the cell) did not hold - so it is `Verdict::Inconclusive`, the same non-verdict
/// exit code `ci.yml` already tolerates elsewhere in this gate, printed to stdout the way every
/// other `Inconclusive` arm in this gate is (`base::report::report_base`'s own convention: `Fail`
/// prints to stderr, `Inconclusive` to stdout).
pub(super) fn report_refused(causes: &[Cause], caller: Caller) -> Verdict {
    let unmeasured = !causes.is_empty() && causes.iter().all(Cause::unmeasured);
    for line in refused_lines(causes, unmeasured, caller) {
        if unmeasured {
            println!("{line}");
        } else {
            eprintln!("{line}");
        }
    }
    if unmeasured { Verdict::Inconclusive } else { Verdict::Fail }
}

/// Every line the arm above prints, in order.
///
/// PURE for the reason `super::remedies` learned twice: a printed sentence is prose that nothing
/// derives, and the one thing that keeps a verdict honest is a test that reads its wording.
fn refused_lines(causes: &[Cause], unmeasured: bool, caller: Caller) -> Vec<String> {
    let mut lines = vec![if unmeasured {
        format!(
            "xtask {}: INCONCLUSIVE - a claim cell's mutation build never reached it",
            caller.task
        )
    } else {
        format!("xtask {}: FAILED - a `{TRAILER}` declaration did not hold", caller.task)
    }];
    lines.extend(causes.iter().map(cause_line));
    if unmeasured {
        lines.extend([
            String::new(),
            String::from("The gate could not attest either way: the nested build under the mutation never"),
            String::from("reached the cell, so this is NOT a verdict about the `Claim-Cell:` declaration -"),
            String::from("re-run it, and if it recurs, read the build output above it in the log."),
        ]);
    } else {
        lines.push(String::new());
        lines.extend(caller.remedy.iter().map(|line| String::from(*line)));
    }
    lines
}

/// The one sentence a reason gets.
fn cause_line(cause: &Cause) -> String {
    match cause {
        Cause::NotAdded(cell) => {
            format!("  not added:   {cell}  (the trailer names it; the diff added no such test)")
        }
        Cause::MissingPatch(cell) => {
            format!("  no patch:    {MUTATIONS_DIR}/{cell}.patch  (a claim cell needs a killing mutation)")
        }
        Cause::TouchesTests { cell, path } => {
            format!("  touches tests: {path}  (the `{cell}` mutation must break PRODUCTION code, not a test)")
        }
        Cause::CreatesFile { cell, path } => {
            format!("  creates a file: {path}  (the `{cell}` mutation must edit code that exists at HEAD, not create {path})")
        }
        Cause::PatchRewritesCell { cell, path } => {
            format!(
                "  patch rewrites the cell: {path}  (the `{cell}` mutation must leave its own test region byte-identical at HEAD)"
            )
        }
        Cause::DoesNotApply { cell, why } => {
            format!("  does not apply: {cell}: {why}")
        }
        Cause::NotByAssertion { cell, site } => {
            format!(
                "  not by the cell's own assertion: {site}  (the `{cell}` mutation killed it without failing the assertion inside the cell's own test fn - an exit/signal, a production panic, or a FAIL with no site)"
            )
        }
        Cause::RestoreFailed { cell, why } => {
            format!("  restore failed: {cell}: {why}  (a failed restore leaks the mutation into the next cell)")
        }
        Cause::NotKilled { cell } => {
            format!("  not killed:  {cell}  (applied, run, and the cell did not fail - the mutation does not kill it)")
        }
        Cause::BuildFailed { cell, why } => {
            format!(
                "  could not attest: {cell}: {why}  (the mutated build never reached the cell - not a verdict about the declaration)"
            )
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

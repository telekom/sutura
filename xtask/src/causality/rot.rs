//! Whether every COMMITTED claim-mutation patch still applies against the work tree and still
//! kills the cell it names - `github.com/telekom/sutura#950`.
//!
//! **"AGAINST THE WORK TREE", NOT "AT HEAD"**: [`check_apply`] reads `repo::root()` - whatever is
//! checked out, uncommitted edits included - while [`run`]'s kill half applies the same patch
//! inside a detached-HEAD worktree (`claim::validate`'s own comment: *"an UNCOMMITTED patch is not
//! the range the trailer declared"*). The two agree on a clean tree, and the `hygiene` derivation
//! that gates every commit builds from the commit's own tree with nothing uncommitted in it - so
//! the venue that actually gates never sees the gap. A dirty checkout can, in both directions: it
//! can report rot that is not there at HEAD, or miss rot that is. The same split gives `run`'s own
//! `patches()` a call one step earlier that reads the WORKING tree while `validate` reads the HEAD
//! worktree - an uncommitted patch refuses as `Cause::MissingPatch` (fail-closed, if confusingly
//! worded for a file sitting right there), and a patch deleted from the working tree while still
//! committed is not checked at all.
//!
//! `claim.rs`'s own arm only re-proves a patch a COMMIT'S OWN trailer just declared, in the range
//! that commit is measured against. A patch accepted on an earlier commit and never touched again
//! is never asked again by that arm, so drift since acceptance goes unnoticed until someone reads
//! it by hand (`#941`, a re-anchor after trailing context drifted) or a merge conflict surfaces it
//! (`#929`, a patch deleted because the test it killed was gone). This module asks the same two
//! questions `claim.rs` asks a newly declared cell, of every cell `devco/claim-mutations/` already
//! holds.
//!
//! TWO HALVES IN TWO VENUES, split by cost exactly as the issue states it. [`check_apply`] is
//! `claim::apply_git`'s `--check`, which reads a WORK TREE and nothing else - no compiler, no
//! test run - so it is cheap enough for the `hygiene` sweep that runs on every commit.
//! [`run`] is the kill half: it recompiles this workspace once per patch, the same ~68s
//! `claim.rs`'s own header prices for a newly declared cell, so re-proving every COMMITTED patch
//! on every commit would multiply that by however many `devco/claim-mutations/` holds (12 today,
//! and growing). It is registered `Standalone` - an on-demand task, for the release path or for a
//! person re-verifying the set after touching something nearby.
//!
//! **THE RESIDUAL, STATED RATHER THAN LEFT IMPLICIT**: [`check_apply`] alone proves a patch is not
//! STALE. It does NOT prove the patch still DISCRIMINATES - only [`run`], which recompiles and
//! executes the named cell under the mutation, proves that. A tree where every patch applies but
//! one no longer kills its cell (the assertion it once broke was refactored around) is still
//! silently unproven between two `run` invocations; nothing here claims otherwise. And BOTH halves
//! are keyed on the patches [`patches`] finds on disk: a `Claim-Cell:` accepted in history whose
//! PATCH was later deleted while its test survives has nothing here to check it against - `#929`'s
//! own instance deleted the test too, which [`Located::Gone`] now catches, but the reverse (patch
//! gone, test present) is the same silently-unproven state and this module does not reach it.
//!
//! **A CELL NAME TWO FILES DECLARE** - an HTTP/MCP parity pair shares one on purpose - is not rot
//! for [`check_apply`]: the name still exists, and only [`Located::Gone`] is rot. Which file the
//! patch means is [`run`]'s question, and the patch answers it on its own FIRST line,
//! `Claim-Cell-File: <repo-relative path>`; `git apply` skips text ahead of its first diff header
//! (measured on the pinned git, `--check` and the apply, with and without `diff --git`), so the
//! line costs the patch nothing. The line is trusted as written: it names a FILE, not a module path
//! (the scan places a file, so two modules of one file stay one cell), and nothing checks that the
//! mutation touches anything that file's cell reads. `claim.rs`'s own arm needs no line - it takes
//! the file from the diff that added the test.
//!
//! [`run`] REUSES `claim::run` WHOLESALE rather than re-deriving its git/panic-site machinery a
//! second time: the mutation being re-checked here is the SAME shape that gate already validates
//! and kills for a cell a commit's own trailer just declared. The only real difference is which
//! cells are asked about - every committed one, not only the ones a diff just added - so a
//! synthetic [`Claim`] and [`Scoped`] naming the whole committed set is the whole difference;
//! [`locate`] is what building the `Scoped` half needs that a diff would otherwise hand over for
//! free (a diff's own added lines say which file and which module a test landed in; a committed
//! patch's file name says only the cell's NAME).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::Verdict;
use crate::causality::claim::{self, Claim};
use crate::causality::place::{self, AddedTest};
use crate::causality::scoped::{Code, Scoped};
use crate::repo;

/// Every `.patch` file committed at `dir`, sorted by name.
///
/// An ABSENT directory is a legitimate "nothing committed yet" and answers `Ok(empty)` - the same
/// distinction `repo::census`'s own walk makes between absent and unreachable. A directory this
/// call CANNOT READ (an unreadable parent, a regular file or a broken symlink standing in for it)
/// is refused instead: treating the two alike is exactly the fail-OPEN direction `#950` is about,
/// so a real read error costs a `Fail` rather than reading as "nothing to check".
///
/// `!dir.is_dir()` alone cannot tell those apart - `is_dir` answers `false` for absent, for an
/// unreadable parent, for a regular file AND for a broken symlink alike, so it collapsed three of
/// those four into "nothing committed yet". `symlink_metadata` is stat, not lstat-then-follow: it
/// reports the link itself rather than resolving through it, so a broken symlink's metadata call
/// still succeeds and reads as "not a directory" instead of vanishing into `NotFound`.
fn patches(dir: &Path) -> Result<Vec<PathBuf>, String> {
    match std::fs::symlink_metadata(dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("could not stat {}: {error}", dir.display())),
        Ok(meta) if !meta.is_dir() => return Err(format!("{} is not a directory", dir.display())),
        Ok(_) => {}
    }
    let entries = std::fs::read_dir(dir).map_err(|error| format!("could not read {}: {error}", dir.display()))?;
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read an entry of {}: {error}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("patch") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// The cell name a committed patch's own file name declares - `claim::mutation_path`'s inverse,
/// read off the filesystem rather than re-derived, so the two conventions cannot drift apart.
/// `None` for a file name that is not valid UTF-8, which this refuses rather than guesses at.
fn cell_name(patch: &Path) -> Option<String> {
    patch.file_stem().and_then(|stem| stem.to_str()).map(String::from)
}

/// Is this a Rust file the repo-wide scan reads? Excludes `vendor/`, the same exclusion
/// `newtype_leaks::in_scope` makes for the same reason: a vendored fn that happened to share a
/// cell's exact, sentence-length name would be a coincidence this gate should not resolve either
/// way without a person looking.
fn outside_vendor(rel: &str) -> bool {
    !rel.starts_with("vendor/")
}

/// THE APPLY HALF: every committed mutation patch still `git apply --check`s against the work
/// tree - see this module's header for why that is not the same claim as "at HEAD".
///
/// See this module's header for the residual: this proves a patch is not stale, not that it
/// still discriminates.
pub(crate) fn check_apply(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-claim-mutations: could not locate the repo root");
        return Verdict::Fail;
    };
    check_apply_at(&root)
}

/// [`check_apply`]'s testable core, over an arbitrary work tree - `git apply --check` needs a
/// work tree and nothing else, which is exactly what makes this hermetic and lets a test hand it
/// a scratch one instead of this repository.
fn check_apply_at(root: &Path) -> Verdict {
    let dir = root.join(claim::MUTATIONS_DIR);
    let list = match patches(&dir) {
        Ok(list) => list,
        Err(why) => {
            eprintln!("xtask check-claim-mutations: {why}");
            return Verdict::Fail;
        }
    };
    if list.is_empty() {
        println!("xtask check-claim-mutations: ok - no {} committed yet", claim::MUTATIONS_DIR);
        return Verdict::Pass;
    }

    let mut rotted: Vec<String> = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    for patch in &list {
        let name = cell_name(patch).unwrap_or_else(|| patch.display().to_string());
        if let Err(why) = claim::apply_git(root, patch, true) {
            rotted.push(format!("{name}: {why}"));
        }
        cells.push(name);
    }
    // A patch that applies but names no test is the same silent rot, one step earlier: the kill
    // half is on demand, so this is the only per-commit gate that sees a renamed cell.
    match locate(root, &cells) {
        Ok(located) => rotted.extend(gone(&cells, &located)),
        Err(why) => rotted.push(why),
    }

    if rotted.is_empty() {
        println!(
            "xtask check-claim-mutations: ok - {} patch(es) apply against the work tree",
            list.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-claim-mutations: FAILED - a committed claim mutation no longer applies or names no test\n");
    for line in &rotted {
        eprintln!("  {line}");
    }
    eprintln!();
    eprintln!(
        "A patch under {}/ is read at RUNTIME by the `Claim-Cell:` gate, so one that no longer",
        claim::MUTATIONS_DIR
    );
    eprintln!("applies leaves the claim cell it names silently unproven. Re-anchor the patch against");
    eprintln!("HEAD, or drop the claim and its test if the behaviour it pinned is gone. `just");
    eprintln!("check-claim-mutation-kills` re-verifies that every patch still kills its cell too.");
    Verdict::Fail
}

/// Where a repo-wide scan found - or did not find - the file declaring `fn <cell>` that its patch means.
#[derive(Debug)]
enum Located {
    /// The one file the cell means: the only file that declares it, or the one its patch's first
    /// line names among several. Placed on the module tree a nextest filter can reach.
    One(AddedTest),
    /// No file's CODE declares a fn by this name any more - `#929`'s own shape: the cell's test
    /// was deleted and nothing but a merge conflict noticed. A comment, a doc comment or a string
    /// naming it does not count. A non-test `fn` of the same name still does, because the scan
    /// reads the word after `fn` on each code line and not test attributes.
    Gone,
    /// More than one file declares a test fn by this name and the patch does not choose. Not rot -
    /// the name exists, so [`check_apply`] accepts it - but [`run`] refuses rather than guess, since
    /// a wrong guess would mutate and kill the WRONG file's assertion. The remedy is the patch's
    /// first line, `Claim-Cell-File: <repo-relative path>`, read as written: it names a file, not a
    /// module path, and nothing checks that the mutation touches what that file's cell reads.
    Ambiguous(Vec<String>),
    /// The patch's first line names a file that does not declare the cell. Refused for one declaring
    /// file as for several: a line that is wrong is not a line to fall back from.
    Misnamed { file: String, declared: Vec<String> },
}

/// Where a repo-wide scan placed every one of a committed patch set's cells.
type Placement = BTreeMap<String, Located>;

/// Find every one of `cells`'s own declaring file, by scanning `root` once.
///
/// PLACE IS DERIVED FROM THE PATH ALONE ([`place::place`]), never from a diff - there is no diff
/// here, only a name a past commit's trailer already proved once. `must_judge` is empty by a
/// REVIEWABLE choice: which file declares a given cell is exactly what this scan is FOR, so there
/// is no anchor to declare ahead of running it (`repo::census::Census::inspect`'s own doc on an
/// empty set).
fn locate(root: &Path, cells: &[String]) -> Result<Placement, String> {
    let census = repo::collect_files(root, root, &["rs"]);
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    let mut found: BTreeMap<String, Vec<AddedTest>> = cells.iter().map(|cell| (cell.clone(), Vec::new())).collect();

    let scope: repo::Scope = outside_vendor;
    census
        .inspect(&[], scope, |rel, bytes| {
            // CODE only: a comment still naming a renamed cell's old `fn` must not keep its patch alive.
            for ident in Code::of(&String::from_utf8_lossy(bytes)).functions() {
                let Some(bucket) = found.get_mut(ident.as_str()) else {
                    continue;
                };
                let Some(at) = place::place(rel, &read) else { continue };
                bucket.push(AddedTest::at(rel, &at, ident));
            }
        })
        .map_err(|why| why.describe())?;

    Ok(found
        .into_iter()
        .map(|(cell, mut matches)| {
            let files = |tests: &[AddedTest]| tests.iter().map(|test| String::from(test.file())).collect();
            let located = if matches.is_empty() {
                Located::Gone
            } else if let Some(file) = named_file(root, &cell) {
                match matches.iter().position(|test| test.file() == file) {
                    Some(at) => Located::One(matches.swap_remove(at)),
                    None => Located::Misnamed {
                        file,
                        declared: files(&matches),
                    },
                }
            } else if matches.len() == 1 {
                Located::One(matches.remove(0))
            } else {
                Located::Ambiguous(files(&matches))
            };
            (cell, located)
        })
        .collect())
}

/// The first line of a committed patch, naming the file whose cell it means.
const FILE_LINE: &str = "Claim-Cell-File:";

/// The file `cell`'s committed patch names on its first line, if it does. An unreadable patch names
/// none: `claim::run` reads the same file next and refuses it there.
fn named_file(root: &Path, cell: &str) -> Option<String> {
    let patch = std::fs::read_to_string(claim::mutation_path(root, cell)).ok()?;
    let line = patch.lines().next()?;
    line.strip_prefix(FILE_LINE).map(|file| file.trim().to_owned())
}

/// The cells [`locate`] placed on exactly one file, and a refusal line for each one it could not.
type Split = (Vec<AddedTest>, Vec<String>);

/// Split `cells` into what [`Split`] holds.
fn unlocated(cells: &[String], located: &Placement) -> Split {
    let mut tests: Vec<AddedTest> = Vec::new();
    let mut causes: Vec<String> = Vec::new();
    for cell in cells {
        match located.get(cell) {
            Some(Located::One(test)) => tests.push(test.clone()),
            Some(Located::Gone) => causes.push(gone_line(cell)),
            Some(Located::Ambiguous(files)) => causes.push(format!(
                "{cell}: declared in more than one file ({}) - refusing rather than guessing which one \
                 the patch means; start the patch with `{FILE_LINE} <one of them>`",
                files.join(", ")
            )),
            Some(Located::Misnamed { file, declared }) => causes.push(format!(
                "{cell}: the patch's first line names {file}, which does not declare it (declared in {})",
                declared.join(", ")
            )),
            None => causes.push(lost_line(cell)),
        }
    }
    (tests, causes)
}

/// The refusal line for each of `cells` the apply half cannot let stand: none of them is placed, so
/// no test by the name exists. A cell with several declaring files is not among them.
fn gone(cells: &[String], located: &Placement) -> Vec<String> {
    cells
        .iter()
        .filter_map(|cell| match located.get(cell) {
            Some(Located::Gone) => Some(gone_line(cell)),
            None => Some(lost_line(cell)),
            Some(_) => None,
        })
        .collect()
}

fn gone_line(cell: &str) -> String {
    format!("{cell}: no test fn by this name exists in the tree any more - the cell it names is gone")
}

fn lost_line(cell: &str) -> String {
    format!("{cell}: not scanned - this gate's own bookkeeping lost it")
}

/// THE KILL HALF: does every committed mutation patch still kill the cell it names?
///
/// ON-DEMAND, not `hygiene` - see this module's header for the cost and the residual.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-claim-mutation-kills: could not locate the repo root");
        return Verdict::Fail;
    };
    run_at(&root)
}

/// [`run`]'s testable core, over an arbitrary root - up to the point the mutation actually needs
/// compiling. The refusal paths below (a cell that is `Gone`, `Ambiguous` or `Misnamed`) are exercised
/// without ever reaching [`claim::run`], which is the expensive call this module's header prices;
/// `claim::run` itself is `claim.rs`'s own tested surface, not re-tested here.
fn run_at(root: &Path) -> Verdict {
    let dir = root.join(claim::MUTATIONS_DIR);
    let list = match patches(&dir) {
        Ok(list) => list,
        Err(why) => {
            eprintln!("xtask check-claim-mutation-kills: {why}");
            return Verdict::Fail;
        }
    };
    if list.is_empty() {
        println!(
            "xtask check-claim-mutation-kills: ok - no {} committed yet",
            claim::MUTATIONS_DIR
        );
        return Verdict::Pass;
    }

    let mut cells: Vec<String> = Vec::new();
    for patch in &list {
        let Some(name) = cell_name(patch) else {
            eprintln!(
                "xtask check-claim-mutation-kills: FAILED - {} is not valid UTF-8, refusing rather \
                 than guessing its cell",
                patch.display()
            );
            return Verdict::Fail;
        };
        cells.push(name);
    }

    let located = match locate(root, &cells) {
        Ok(map) => map,
        Err(why) => {
            eprintln!("xtask check-claim-mutation-kills: {why}");
            return Verdict::Fail;
        }
    };

    let (tests, causes) = unlocated(&cells, &located);

    if !causes.is_empty() {
        eprintln!("xtask check-claim-mutation-kills: FAILED - could not locate every committed cell\n");
        for cause in &causes {
            eprintln!("  {cause}");
        }
        eprintln!();
        eprintln!("A cell this cannot locate is exactly the rot `#950` exists to catch: its patch is");
        eprintln!("still committed at devco/claim-mutations/ and nothing verifies it any more. Drop the");
        eprintln!("patch and its `Claim-Cell:` history, or restore the test it was proving.");
        return Verdict::Fail;
    }

    // Not a commit log, so not `Claim::of`: no commit declared these, and a claim keyed to none
    // is what `Claim::synthetic` builds.
    let Some(declared) = Claim::synthetic(cells.iter().map(String::as_str)) else {
        eprintln!(
            "xtask check-claim-mutation-kills: could not build a claim from {} committed patch(es)",
            cells.len()
        );
        return Verdict::Fail;
    };

    claim::run(root, &Scoped::of_named(tests), &[], &declared, KILL_HALF)
}

/// [`run`]'s own identity, threaded into [`claim::run`] so a refusal names the task actually run
/// and the remedy actually reachable - re-anchoring or dropping the committed patch, not "fix or
/// drop the declaration" `claim::Caller::TEST_CAUSALITY` prints for a trailer sitting in a diff.
/// No declaration lives in the diff here; the trailer was accepted commits ago.
const KILL_HALF: claim::Caller = claim::Caller {
    task: "check-claim-mutation-kills",
    remedy: &[
        "A patch under devco/claim-mutations/ that no longer kills its cell leaves the claim it",
        "names silently unproven, the same way one that no longer applies does. Re-anchor the",
        "patch against HEAD, or drop it and the test it pinned if the behaviour it once broke is",
        "gone. `cargo xtask check-claim-mutations` re-verifies that every patch still applies too.",
    ],
};

#[cfg(test)]
mod tests {
    use super::{Located, cell_name, check_apply_at, locate, patches, run_at, unlocated};
    use crate::Verdict;
    use std::path::PathBuf;

    /// One unique scratch directory per test, under the machine's temp root, so no two tests
    /// racing under nextest collide on the same path.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-claim-rot-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch root");
        dir
    }

    fn write_patch(dir: &std::path::Path, name: &str, contents: &str) {
        let patches_dir = dir.join(super::claim::MUTATIONS_DIR);
        std::fs::create_dir_all(&patches_dir).expect("the patches directory");
        std::fs::write(patches_dir.join(format!("{name}.patch")), contents).expect("a patch file");
    }

    /// A `git` invocation in `dir`, with this process's own git environment stripped - the same
    /// requirement `worktree.rs`'s own [`super::claim`] machinery has, for the same reason: a
    /// pre-commit hook running these tests under nextest has `GIT_DIR`/`GIT_INDEX_FILE` set, and
    /// an unstripped subprocess would operate on the OUTER repository instead of the fixture.
    fn git_cmd(dir: &std::path::Path, args: &[&str]) {
        let mut command = std::process::Command::new("git");
        crate::repo::strip_git_env(&mut command);
        let out = command.current_dir(dir).args(args).output().expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A throwaway ONE-CRATE real git repo with `content` as `src/lib.rs`, committed - exactly
    /// the shape `causality::tests::inseparable_claim_case` already builds. Fast, not the ~68s
    /// full-workspace rebuild `just causality` itself pays: `run_at` recompiles only this crate.
    fn kill_half_repo(name: &str, content: &str) -> PathBuf {
        let root = scratch(name);
        git_cmd(&root, &["init", "-q", "-b", "main"]);
        git_cmd(&root, &["config", "user.email", "test@example.com"]);
        git_cmd(&root, &["config", "user.name", "test"]);
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[profile.ci]\ninherits = \"dev\"\n",
        )
        .expect("a manifest");
        std::fs::create_dir_all(root.join("src")).expect("src/");
        std::fs::write(root.join("src/lib.rs"), content).expect("the crate root");
        root
    }

    /// The `diff --no-index` patch turning `before` into `after`, rewritten onto `src/lib.rs` -
    /// same technique `causality::tests` uses to keep the mutation's own text out of a diff.
    fn diff_onto_lib_rs(root: &std::path::Path, before: &str, after: &str) -> String {
        std::fs::write(root.join(".old.rs"), before).expect("before");
        std::fs::write(root.join(".new.rs"), after).expect("after");
        let out = std::process::Command::new("git")
            .current_dir(root)
            .args(["diff", "--no-index", "--", ".old.rs", ".new.rs"])
            .output()
            .expect("git diff --no-index runs");
        std::fs::remove_file(root.join(".old.rs")).expect("cleanup");
        std::fs::remove_file(root.join(".new.rs")).expect("cleanup");
        String::from_utf8_lossy(&out.stdout)
            .replace(".old.rs", "src/lib.rs")
            .replace(".new.rs", "src/lib.rs")
    }

    /// `src/lib.rs` for every kill-half fixture below: a production line and its OWN inline test.
    const KILL_HALF_LIB_RS: &str = "pub fn f() -> u8 { 2 }\n\n#[cfg(test)]\nmod tests {\n    use super::f;\n\n    #[test]\n    fn the_wired_one() {\n        assert_eq!(f(), 2);\n    }\n}\n";

    /// THE ADOPTED TECHNIQUE, four ways - the review on #951 built these live and asked for them
    /// as cells rather than a claim resting on having watched it work once. Each is a real
    /// end-to-end run of [`run_at`] over a throwaway one-crate repo, so the kill half is KNOWN to
    /// work rather than known to pass.
    #[test]
    fn the_kill_half_accepts_a_mutation_that_kills_its_cell() {
        let root = kill_half_repo("kill-half-kills", KILL_HALF_LIB_RS);
        let mutated = KILL_HALF_LIB_RS.replacen("{ 2 }", "{ 9 }", 1);
        let patch = diff_onto_lib_rs(&root, KILL_HALF_LIB_RS, &mutated);
        write_patch(&root, "the_wired_one", &patch);
        git_cmd(&root, &["add", "-A"]);
        git_cmd(&root, &["commit", "-q", "-m", "init"]);
        let verdict = run_at(&root);
        drop(std::fs::remove_dir_all(&root));
        assert_eq!(verdict, Verdict::Pass, "a mutation that kills the cell must be accepted");
    }

    #[test]
    fn the_kill_half_refuses_a_mutation_that_does_not_kill_its_cell() {
        let root = kill_half_repo("kill-half-does-not-kill", KILL_HALF_LIB_RS);
        let mutated = KILL_HALF_LIB_RS.replacen("pub fn f() -> u8 { 2 }", "pub fn f() -> u8 { 2 } // same", 1);
        let patch = diff_onto_lib_rs(&root, KILL_HALF_LIB_RS, &mutated);
        write_patch(&root, "the_wired_one", &patch);
        git_cmd(&root, &["add", "-A"]);
        git_cmd(&root, &["commit", "-q", "-m", "init"]);
        let verdict = run_at(&root);
        drop(std::fs::remove_dir_all(&root));
        assert_eq!(
            verdict,
            Verdict::Fail,
            "a mutation that leaves the cell green must be refused"
        );
    }

    #[test]
    fn the_kill_half_refuses_a_cell_gone_from_the_tree() {
        let root = kill_half_repo("kill-half-gone", "pub fn f() -> u8 { 2 }\n");
        write_patch(
            &root,
            "the_wired_one",
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-pub fn f() -> u8 { 2 }\n+pub fn f() -> u8 { 9 }\n",
        );
        git_cmd(&root, &["add", "-A"]);
        git_cmd(&root, &["commit", "-q", "-m", "init"]);
        let verdict = run_at(&root);
        drop(std::fs::remove_dir_all(&root));
        assert_eq!(
            verdict,
            Verdict::Fail,
            "a cell no file declares any more must refuse before any build"
        );
    }

    #[test]
    fn the_kill_half_is_inconclusive_over_a_mutation_that_does_not_compile() {
        let root = kill_half_repo("kill-half-build-fails", KILL_HALF_LIB_RS);
        let mutated = KILL_HALF_LIB_RS.replacen("pub fn f() -> u8 { 2 }", "pub fn f() -> ThisTypeDoesNotExist { 2 }", 1);
        let patch = diff_onto_lib_rs(&root, KILL_HALF_LIB_RS, &mutated);
        write_patch(&root, "the_wired_one", &patch);
        git_cmd(&root, &["add", "-A"]);
        git_cmd(&root, &["commit", "-q", "-m", "init"]);
        let verdict = run_at(&root);
        drop(std::fs::remove_dir_all(&root));
        assert_eq!(
            verdict,
            Verdict::Inconclusive,
            "a mutation the tree never compiles under is a non-verdict, not a kill or a refusal"
        );
    }

    #[test]
    fn patches_reports_ok_over_an_absent_directory() {
        let root = scratch("absent-dir");
        let found = patches(&root.join("devco/claim-mutations")).expect("an absent dir is not an error");
        assert!(found.is_empty(), "nothing is committed yet - that is a legitimate empty set");
    }

    #[test]
    fn patches_collects_only_dot_patch_files_and_sorts_them() {
        let root = scratch("mixed-extensions");
        let dir = root.join("devco/claim-mutations");
        std::fs::create_dir_all(&dir).expect("the dir");
        std::fs::write(dir.join("z_cell.patch"), "").expect("a patch");
        std::fs::write(dir.join("a_cell.patch"), "").expect("a patch");
        std::fs::write(dir.join("README.md"), "not a patch").expect("a non-patch file");
        let found = patches(&dir).expect("a readable dir");
        let names: Vec<String> = found.iter().filter_map(|p| cell_name(p)).collect();
        assert_eq!(names, vec![String::from("a_cell"), String::from("z_cell")]);
    }

    #[test]
    fn no_committed_mutation_is_a_legitimate_apply_pass() {
        let root = scratch("apply-no-patches");
        assert_eq!(check_apply_at(&root), Verdict::Pass);
    }

    #[test]
    fn a_patch_that_still_applies_is_an_apply_pass() {
        let root = scratch("apply-healthy");
        std::fs::write(root.join("a.txt"), "hello\n").expect("the target file");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("a manifest");
        std::fs::create_dir_all(root.join("src")).expect("src/");
        std::fs::write(root.join("src/lib.rs"), "#[test]\nfn healthy_cell() {}\n").expect("the cell's file");
        write_patch(
            &root,
            "healthy_cell",
            "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-hello\n+world\n",
        );
        assert_eq!(check_apply_at(&root), Verdict::Pass);
    }

    #[test]
    fn a_patch_that_applies_but_names_no_test_fails_the_apply_half() {
        let root = scratch("apply-renamed-cell");
        std::fs::write(root.join("a.txt"), "hello\n").expect("the target file");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("a manifest");
        std::fs::create_dir_all(root.join("src")).expect("src/");
        std::fs::write(root.join("src/lib.rs"), "#[test]\nfn the_cell_after_its_rename() {}\n").expect("the renamed cell");
        write_patch(
            &root,
            "the_cell_before_its_rename",
            "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-hello\n+world\n",
        );
        assert_eq!(check_apply_at(&root), Verdict::Fail);
    }

    /// A scratch tree whose one committed patch, for `cell`, applies to `a.txt`, with `files`
    /// beside it on a crate `place` can place.
    fn applying_tree(name: &str, cell: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = scratch(name);
        std::fs::write(root.join("a.txt"), "hello\n").expect("the target file");
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("the parent directory");
            std::fs::write(path, text).expect("a fixture file");
        }
        write_patch(&root, cell, "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-hello\n+world\n");
        root
    }

    const MANIFEST: (&str, &str) = ("Cargo.toml", "[package]\nname = \"demo\"\n");

    /// An HTTP/MCP parity pair shares a test name on purpose. The name still exists, so there is
    /// no rot to report; which file the patch means is the kill half's question.
    #[test]
    fn a_cell_two_files_declare_passes_the_apply_half() {
        let declared = "#[test]\nfn a_shared_cell_name() {}\n";
        let root = applying_tree(
            "apply-ambiguous-cell",
            "a_shared_cell_name",
            &[
                MANIFEST,
                ("src/lib.rs", ""),
                ("src/one.rs", declared),
                ("src/two.rs", declared),
            ],
        );
        assert_eq!(check_apply_at(&root), Verdict::Pass);
    }

    #[test]
    fn a_cell_scan_that_refuses_fails_the_apply_half() {
        // No `.rs` file at all: the census refuses an empty discovery, and that refusal is the verdict.
        let root = applying_tree("apply-unscannable", "a_cell_the_scan_never_reached", &[]);
        assert_eq!(check_apply_at(&root), Verdict::Fail);
    }

    #[test]
    fn a_comment_naming_a_renamed_cell_does_not_keep_its_patch_alive() {
        let root = applying_tree(
            "apply-comment-only-cell",
            "the_cell_before_its_rename",
            &[
                MANIFEST,
                (
                    "src/lib.rs",
                    "// fn the_cell_before_its_rename was renamed\n#[test]\nfn the_cell_after_its_rename() {}\n",
                ),
            ],
        );
        assert_eq!(check_apply_at(&root), Verdict::Fail);
    }

    #[test]
    fn a_patch_whose_context_no_longer_matches_fails_closed() {
        // THE ROT ITSELF: the target file no longer carries the line the patch's context names,
        // exactly as `#941`'s trailing-context drift did.
        let root = scratch("apply-rotted");
        std::fs::write(root.join("a.txt"), "goodbye\n").expect("a file the patch's context does not match");
        write_patch(
            &root,
            "rotted_cell",
            "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-hello\n+world\n",
        );
        assert_eq!(check_apply_at(&root), Verdict::Fail);
    }

    #[test]
    fn an_empty_patch_fails_closed_rather_than_reading_as_nothing_to_apply() {
        // THE FAIL-CLOSED DIRECTION THE ISSUE NAMES EXPLICITLY: an empty committed file is not
        // "no mutation to check", it is a mutation this cannot read - `git apply --check` itself
        // refuses it ("no valid patches"), and this must surface that as a `Fail`, not swallow it.
        let root = scratch("apply-empty");
        std::fs::write(root.join("a.txt"), "hello\n").expect("a target file");
        write_patch(&root, "empty_cell", "");
        assert_eq!(check_apply_at(&root), Verdict::Fail);
    }

    #[test]
    fn a_directory_shaped_patch_file_fails_closed_rather_than_being_skipped() {
        // A DIRECTORY standing in for the PATCH FILE (not for `devco/claim-mutations` itself): as
        // "unreadable as a diff" as a permission failure, and it needs no chmod bit a sandboxed
        // runner might not honour. This is `claim::apply_git`'s refusal, not `patches`'s own - see
        // the cells below for the directory-level fail-open direction.
        let root = scratch("apply-unreadable");
        let dir = root.join(super::claim::MUTATIONS_DIR);
        std::fs::create_dir_all(dir.join("unreadable_cell.patch")).expect("a directory, not a file");
        assert_eq!(check_apply_at(&root), Verdict::Fail);
    }

    #[test]
    fn patches_refuses_a_regular_file_standing_in_for_the_directory() {
        let root = scratch("patches-file-not-dir");
        let dir = root.join(super::claim::MUTATIONS_DIR);
        std::fs::create_dir_all(dir.parent().expect("devco/")).expect("devco/");
        std::fs::write(&dir, "not a directory\n").expect("a file where a directory is expected");
        assert!(
            patches(&dir).is_err(),
            "a regular file must refuse, not read as nothing committed"
        );
    }

    #[test]
    fn patches_refuses_a_broken_symlink_standing_in_for_the_directory() {
        let root = scratch("patches-broken-symlink");
        let dir = root.join(super::claim::MUTATIONS_DIR);
        std::fs::create_dir_all(dir.parent().expect("devco/")).expect("devco/");
        std::os::unix::fs::symlink(root.join("nowhere"), &dir).expect("a broken symlink");
        assert!(
            patches(&dir).is_err(),
            "a broken symlink must refuse, not read as nothing committed"
        );
    }

    #[test]
    fn patches_refuses_an_unreadable_parent_directory() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = scratch("patches-unreadable-parent");
        let parent = root.join("devco");
        std::fs::create_dir_all(&parent).expect("devco/");
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).expect("chmod 000 on devco/");
        let result = patches(&parent.join("claim-mutations"));
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).expect("restore for cleanup");
        assert!(
            result.is_err(),
            "an unreadable parent must refuse, not read as nothing committed"
        );
    }

    /// `locate()` itself, not `run_at`'s Fail/Pass: both `Gone` and `Ambiguous` end in
    /// `Verdict::Fail` through `run_at`, but so does a `One` whose worktree creation fails in a
    /// scratch root with no `.git` - so a mutation that turned `Ambiguous`/`Gone` into `One` would
    /// still read `Fail` at that level and the cell would stay green over a broken rule. Asserting
    /// on the ENUM `locate` returns is what a mutation of its own match arms actually moves.
    #[test]
    fn locate_reports_gone_for_a_cell_no_file_declares() {
        // `#929`'s own shape: the patch is still committed and still applies, but nothing in the
        // tree declares a test by this name any more.
        let root = scratch("locate-gone");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("a manifest");
        std::fs::create_dir_all(root.join("src")).expect("src/");
        std::fs::write(root.join("src/lib.rs"), "").expect("an empty crate root");
        let cells = vec![String::from("a_test_fn_nothing_declares_any_more")];
        let found = locate(&root, &cells).expect("a readable tree");
        match found.get("a_test_fn_nothing_declares_any_more") {
            Some(Located::Gone) => {}
            other => panic!("expected Gone, got {other:?}"),
        }
    }

    #[test]
    fn locate_reports_ambiguous_for_a_cell_two_files_declare() {
        let root = scratch("locate-ambiguous");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("a manifest");
        std::fs::create_dir_all(root.join("src")).expect("src/");
        std::fs::write(root.join("src/lib.rs"), "").expect("an empty crate root");
        std::fs::write(root.join("src/one.rs"), "#[test]\nfn a_shared_cell_name() {}\n").expect("the first file");
        std::fs::write(root.join("src/two.rs"), "#[test]\nfn a_shared_cell_name() {}\n").expect("the second file");
        let cells = vec![String::from("a_shared_cell_name")];
        let found = locate(&root, &cells).expect("a readable tree");
        match found.get("a_shared_cell_name") {
            Some(Located::Ambiguous(files)) => {
                assert_eq!(files.len(), 2, "both declaring files must be named, not just counted");
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn a_gone_or_ambiguous_cell_fails_run_at_before_any_worktree_is_built() {
        // THE WIRING, once `locate`'s own classification is proven above: `run_at` must turn
        // EITHER non-`One` outcome into a `Fail` rather than reaching `claim::run` at all - a
        // scratch root with no `.git` makes that reachable half fail too, so this only proves the
        // wiring surfaces a refusal, not which cause fired.
        let root = scratch("kill-gone-wiring");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("a manifest");
        std::fs::create_dir_all(root.join("src")).expect("src/");
        std::fs::write(root.join("src/lib.rs"), "").expect("an empty crate root");
        std::fs::write(root.join("a.txt"), "hello\n").expect("a target file for the patch");
        write_patch(
            &root,
            "a_test_fn_nothing_declares_any_more",
            "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-hello\n+world\n",
        );
        assert_eq!(run_at(&root), Verdict::Fail);
    }

    #[test]
    fn locate_finds_the_one_file_that_declares_a_cell() {
        let root = scratch("locate-one");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("a manifest");
        std::fs::create_dir_all(root.join("src")).expect("src/");
        std::fs::write(root.join("src/lib.rs"), "").expect("an empty crate root");
        std::fs::write(
            root.join("src/feature.rs"),
            "fn helper() {}\n#[test]\nfn my_own_cell() {\n    assert!(true);\n}\n",
        )
        .expect("the declaring file");
        let cells = vec![String::from("my_own_cell")];
        let found = locate(&root, &cells).expect("a readable tree");
        match found.get("my_own_cell") {
            Some(Located::One(test)) => assert_eq!(test.file(), "src/feature.rs"),
            other => panic!("expected exactly one match, got {other:?}"),
        }
    }

    const SHARED: &str = "a_shared_cell_name";

    /// A tree whose one committed patch, for [`SHARED`], starts with `head` and whose `declaring`
    /// files each declare that cell.
    fn shared_cell_tree(name: &str, head: &str, declaring: &[&str]) -> PathBuf {
        let declared = "#[test]\nfn a_shared_cell_name() {}\n";
        let mut files = vec![MANIFEST, ("src/lib.rs", "")];
        files.extend(declaring.iter().map(|file| (*file, declared)));
        let root = applying_tree(name, SHARED, &files);
        write_patch(
            &root,
            SHARED,
            &format!("{head}--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-hello\n+world\n"),
        );
        root
    }

    #[test]
    fn a_patch_naming_a_file_selects_that_files_cell_among_two() {
        for file in ["src/one.rs", "src/two.rs"] {
            let root = shared_cell_tree(
                &format!("select-{}", file.replace('/', "-")),
                &format!("Claim-Cell-File: {file}\n"),
                &["src/one.rs", "src/two.rs"],
            );
            let found = locate(&root, &[String::from(SHARED)]).expect("a readable tree");
            match found.get(SHARED) {
                Some(Located::One(test)) => assert_eq!(test.file(), file),
                other => panic!("expected the file the patch names, {file}, got {other:?}"),
            }
        }
    }

    #[test]
    fn an_ambiguous_cell_with_no_file_line_is_refused_naming_the_line() {
        let root = shared_cell_tree("ambiguous-no-line", "", &["src/one.rs", "src/two.rs"]);
        let cells = [String::from(SHARED)];
        let (tests, causes) = unlocated(&cells, &locate(&root, &cells).expect("a readable tree"));
        assert!(tests.is_empty(), "no file was chosen, so nothing may be run");
        assert!(
            causes.iter().any(|cause| cause.contains("Claim-Cell-File:")),
            "the refusal must name the line that resolves it: {causes:?}"
        );
    }

    #[test]
    fn a_file_line_naming_a_file_that_does_not_declare_the_cell_is_refused() {
        for declaring in [&["src/one.rs", "src/two.rs"][..], &["src/one.rs"][..]] {
            let root = shared_cell_tree(
                &format!("misnamed-{}", declaring.len()),
                "Claim-Cell-File: src/three.rs\n",
                declaring,
            );
            let cells = [String::from(SHARED)];
            let (tests, causes) = unlocated(&cells, &locate(&root, &cells).expect("a readable tree"));
            assert!(
                tests.is_empty(),
                "{declaring:?}: a file that does not declare the cell may not be run"
            );
            assert!(
                causes.iter().any(|cause| cause.contains("src/three.rs")),
                "{declaring:?}: the refusal must name the file the patch got wrong: {causes:?}"
            );
        }
    }

    /// The HTTP/MCP parity shape end to end: one cell name in two files, and only the one the
    /// patch's first line names reads the line the mutation breaks.
    #[test]
    fn the_kill_half_runs_the_file_a_patch_names_when_two_declare_its_cell() {
        let lib = "pub fn f() -> u8 { 2 }\n\n#[cfg(test)]\nmod one;\n#[cfg(test)]\nmod two;\n";
        let root = kill_half_repo("kill-half-named-file", lib);
        std::fs::write(
            root.join("src/one.rs"),
            "use crate::f;\n\n#[test]\nfn a_shared_cell_name() {\n    assert_eq!(f(), 2);\n}\n",
        )
        .expect("the file the patch names");
        std::fs::write(
            root.join("src/two.rs"),
            "#[test]\nfn a_shared_cell_name() {\n    assert_eq!(2 + 2, 4);\n}\n",
        )
        .expect("the sibling that never reads f");
        let patch = diff_onto_lib_rs(&root, lib, &lib.replacen("{ 2 }", "{ 9 }", 1));
        write_patch(&root, SHARED, &format!("Claim-Cell-File: src/one.rs\n{patch}"));
        git_cmd(&root, &["add", "-A"]);
        git_cmd(&root, &["commit", "-q", "-m", "init"]);
        let verdict = run_at(&root);
        drop(std::fs::remove_dir_all(&root));
        assert_eq!(
            verdict,
            Verdict::Pass,
            "the mutation kills the named file's cell, so the run must be accepted"
        );
    }
}

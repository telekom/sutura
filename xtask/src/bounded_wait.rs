//! One place in the compose tier waits on a child process, and it is the docker module.
//!
//! The property this gate holds is *locality*, and locality is what the bound rests on. A gate
//! that hangs prints nothing, so it reads as a slow machine rather than as a failure, and this
//! repository has paid for that twice: `38 min`, `50 min` and an `864s` SIGTERM the first time, a
//! second unbounded wait the next. The fix both times was to route the wait through one function
//! that carries a deadline - and nothing made the NEXT call come through it. A `.output()` or a
//! `.status()` written into a sibling of that function compiles, passes review, passes every test
//! in the tree, and restores the exact defect. `AGENTS.md` is unambiguous about a rule in that
//! position: an invariant is held by a type, a lint, a hook or a gate, never by recall.
//!
//! # Why the obvious mechanism does not fit
//!
//! `clippy.toml`'s `disallowed-methods` is this repository's way of saying *"this call may only
//! appear where an `#[expect]` is visible"*, and it cannot express this one. An entry there is
//! workspace-wide, and `Command::output` / `Command::status` have dozens of legitimate call sites
//! in `xtask` driving `git`, `cargo` and `nix` - waits with no bound **on purpose**, because
//! killing a build half way through is worse than waiting for it. A workspace-wide ban would mean
//! dozens of expectations that say nothing, and a lint that says nothing gets disabled.
//!
//! So this is a path-scoped text gate, the shape `check-newtype-leaks` and `check-boot-order`
//! already have here: a rule the code cannot state about itself, read as text, starting from a tree
//! that already obeys it.
//!
//! # The rule, and why it is derived rather than listed
//!
//! Over the Rust matched by [`TIER`], every file containing one of [`BLOCKING`] must be either
//!
//! 1. **the docker module** ([`RUNTIME`]), of which **exactly one file** may wait - that file is
//!    the tier's waiter, whatever it is called; or
//! 2. **a declared allowance** - one entry today, in [`ALLOWED`], each naming the ONE needle it
//!    excuses in that file, what matches it, and why that is not an unbounded wait on a child.
//!
//! The first half is a discovery rather than a name, and that is deliberate: an open change moves
//! the waiter out of the module root into a file of its own, and a gate naming the file it lives in
//! today would go red for that move rather than for a defect. What the gate asks is *how many
//! places in this tier can be blocked by a child*, and the answer it accepts is one.
//!
//! **It starts green.** Measured when it was written: in this tier the docker module is the only
//! file that can be blocked by a child process, beside the one allowance below. So the gate's
//! whole job is to keep it that way, and the cheapest gate is the one most likely to earn its keep
//! years from now.
//!
//! # What counts, and why it is not a list of wait methods
//!
//! The subject is **a thread of execution that can be blocked indefinitely by a child process**,
//! which is broader than any set of method names. Reviewed against this gate's first draft, which
//! keyed on five: a `docker compose logs --follow` spawned with `Stdio::piped()` and drained with
//! `read_to_string` compiles under `-D warnings --all-features`, restores the exact hang, and was
//! reported `ok` by that draft. Not laundered, not split across lines, not inside the waiter -
//! outside every limit it declared.
//!
//! So [`BLOCKING`] carries two kinds of needle, and [`Blocks`] is which:
//!
//! * **The wait itself** - the five calls on a `Command` or a `Child` that return only when the
//!   child does.
//! * **The pipe** - `Stdio::piped()` and `io::pipe()`. Neither waits, and both are the
//!   *precondition* for a drain that can. `read_to_string`, `read_to_end`, `read_line`, `lines`,
//!   `io::copy` and `bytes` all block forever on a pipe that never reaches EOF, and none of them
//!   is enumerable: gating the readers is incomplete, and here it is also **wrong**. Measured
//!   2026-09-05 with `grep -c std::fs::read_to_string` over each file in scope: **8 lines across
//!   three of the seven files**, every one reading a file rather than a child. Gating the
//!   precondition instead covers the class with two needles, because those two are the only ways
//!   in `std` to make a pipe a child can be handed, beside the two `Command` calls that pipe
//!   internally - and those are needles already.
//!
//!   **`io::pipe()` was missed on the first pass at this fix, so the finding's own class recurred
//!   inside the fix for it.** `Stdio::piped()` alone reads as complete and is not: `std::io::pipe`
//!   has been stable since 1.87, and `Stdio::from` over its write end restores the identical hang
//!   with no `piped` anywhere. Verified on the pinned compiler rather than assumed - `rustc
//!   1.98.0`, `--edition 2024`, compiled and ran. **The needle is a pipe's CREATION, not its
//!   handoff:** `Stdio::from` is also how the waiter hands its child the capture FILES, so gating
//!   that would fire on the fix.
//!
//! **The pipe half is a rule this tier already stated about itself, in prose, with no mechanism.**
//! `docker/bounded.rs` documents its capture as *"Files, because a pipe puts the hang back"* -
//! polling for an exit while the child writes into a pipe deadlocks when the buffer fills, and
//! `docker` spawns CLI plugins that inherit the write end and can hold it open past the kill. That
//! argument is exactly this gate's subject, and until now nothing held it.
//!
//! **`.spawn()` is the more principled needle, and what blocks it is this rule's SHAPE rather
//! than the tree.** It is the one call that makes a child exist at all. Two files in the docker
//! module spawn today - the presence probe and the bounded runner - so keying on it reports two
//! waiters over a tree that obeys the rule. But that is only true because ONE table answers two
//! different questions: *does anything outside the docker module touch a child* (locality) and
//! *does exactly one file inside it block* (the waiter count). Splitting [`BLOCKING`] by role -
//! `.spawn()` counting for locality only - admits it, and would close a hole nothing else covers:
//! `command.stdout(Stdio::from(file)).spawn()` in a sibling, then polling that file for a
//! sentinel, blocks forever with no needle firing. **That is a design change and not a needle**,
//! so it is written down here rather than half-done: the claim above is about this gate, not
//! about `docker`.
//!
//! # Fails closed, in five directions
//!
//! Each of these is a failure rather than a pass over silence, because a scan that finds nothing is
//! how the property went unheld in the first place:
//!
//! * **No file in scope.** The tier renamed or moved out from under the gate.
//! * **A file in scope it cannot read.** Not skipped: a file this gate did not read is a file it
//!   did not judge, and skipping one is how a scan comes out clean over the violation.
//! * **No waiter in the docker module.** Either the wait left the tier - in which case the gate
//!   follows it or is deleted - or the scan stopped seeing it, which is the same thing to a reader.
//! * **More than one waiter in the docker module.** The property IS one, and a second one is the
//!   defect this exists for.
//! * **A declared allowance whose needle no longer matches there.** An entry that has stopped
//!   being true widens what is permitted while reading as a considered decision.
//!
//! # The limits, next to the claim
//!
//! **It holds locality, not boundedness.** What it says is *no second wait was written in this
//! tier*, not *every child is bounded*. Whether the one waiter carries a deadline is argued in that
//! file's own documentation and by its tests; this gate makes sure there is one file to argue about.
//!
//! **It is per-FILE, not per-function.** A `.output()` added beside the bounded loop **inside** the
//! waiter is invisible here, and on a tree where that file holds a documented unbounded call next
//! to the bounded one there is nothing to separate them by. Per-function would be the stronger
//! rule and is not available while both live in one file.
//!
//! **THE RECEIVER IS NOT KNOWABLE, so three of the seven needles fire on anything.** `.output()`,
//! `.status()` and `.wait()` are ordinary method names, and a line scan sees the call, never the
//! type it is called on. Measured, not hypothesised: a plain `Spared::status()` accessor over a
//! `&'static str` in `compose/teardown.rs`, with no child process anywhere near it, was reported
//! as a wait. **This is the likelier of the two false positives, not the rarer** - this tier's
//! domain is container *status* and *health*, and `docker.rs` already reads a row's state and
//! health, so a `status()` of its own is a name waiting to be written. [`Blocks::OnAnyReceiver`]
//! marks the three, the failure line says so at the site rather than asserting a child is
//! involved, and [`ALLOWED`] is the escape: an entry excuses **one needle in one file**, so
//! declaring an accessor does not blanket-exempt the file that holds it. The alternative was
//! weighed and rejected - resolving the receiver by SPELLING (`command`, `child`, `cmd`) trades a
//! false positive for false negatives, and a missed wait is the defect this gate exists for.
//!
//! **A wait laundered through a helper elsewhere is invisible.** The scan reads text in this tier;
//! a function in another module that waits, called from here, is not found. So is a wait added to a
//! directory nobody put in scope - which is the price of scoping to two narrow patterns rather than
//! sweeping `xtask/`, where the unbounded `git` and `cargo` calls legitimately live. Renaming on
//! import is the same blind spot spelled differently: `use std::process::Stdio as S` makes
//! `S::piped()` invisible to the pipe needles - as does `use std::io::pipe` and then a bare
//! `pipe()`, since the needle is the qualified spelling. A bare `pipe()` needle was weighed and
//! rejected: it would put a THIRD ordinary name in the table immediately after the ambiguity of
//! the first three was declared a known weakness. A pipe made outside `std` is invisible for the
//! same reason - a `libc::pipe` handed over as an `OwnedFd`, or the `os_pipe` crate - and this
//! tier depends on neither today.
//!
//! **Comments and multi-line string interiors are blanked first**, through the shared lexer
//! [`code_lines`](crate::serde_parse::scan::code_lines), and that is load-bearing rather than tidy:
//! the prose in this tier writes `.output()` repeatedly while explaining the defect, so a raw scan
//! would report the documentation that argues the rule. **A SINGLE-LINE string is NOT blanked** -
//! the lexer keeps a one-line literal's content on purpose - so an `eprintln!(".output()")` in this
//! tier is a live anchor to this gate and would read as a violation. There is no such literal in
//! scope today, checked over every needle.
//!
//! **A call split across two lines evades the line scan.** `command.output(` on one line and `)` on
//! the next is one wait and reads as none. Whitespace inside a line is removed before matching, so
//! `command . output ()` is found; the newline case is left to the mechanism that already owns that
//! shape - `just fmt` joins a zero-argument method call back onto one line, and the `fmt` check
//! fails a tree it would have reformatted.
//!
//! **Nothing asserts `RUNTIME` is inside `TIER` at the pattern level**, and a `RUNTIME` edited
//! outside `TIER` would go permanently red on *no waiter* rather than naming its cause. A test
//! holds the two constants as they stand; the general containment is not decidable over globs.

use crate::Verdict;
use crate::repo;
use crate::serde_parse::scan::code_lines;

/// The compose tier: its module root, and the Rust beside it.
///
/// Deliberately not the whole of `xtask/`, where waiting on `git`, `cargo` and `nix` without a
/// bound is correct. Read by [`repo::matches`], the matcher every path-scoped check here shares.
const TIER: &[&str] = &["xtask/src/compose.rs", "xtask/src/compose/**/*.rs"];

/// The docker module inside the tier: the one place where waiting on a child is the contract.
///
/// A pattern pair rather than a file name, so the waiter splitting out of the module root is a
/// move and not a red gate.
const RUNTIME: &[&str] = &["xtask/src/compose/docker.rs", "xtask/src/compose/docker/**/*.rs"];

/// What a needle's match means. Three cases: a name only a child carries, an ordinary name
/// anything can carry, and a pipe that is not a wait at all. One sentence for all seven would be
/// false about most of them, and it was - that is review finding two.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Blocks {
    /// A name `Command` and `Child` alone carry, so a match is the call it looks like.
    OnAChild,
    /// An ordinary method name. It is a wait when the receiver is a command or a child, and a line
    /// scan cannot see a receiver's type - so the site is reported as ambiguous rather than as a
    /// child, and [`ALLOWED`] is where a proven-innocent one gets declared.
    OnAnyReceiver,
    /// Not a wait: it hands a child a pipe, which is the precondition for every drain that never
    /// reaches EOF. The header argues why the precondition is the needle and the readers are not.
    PipeToAChild,
}

impl Blocks {
    /// The sentence that follows the path, the line and the needle in a failure.
    const fn says(self) -> &'static str {
        match self {
            Self::OnAChild => "waits on a child process",
            Self::OnAnyReceiver => {
                "waits on a child process - or is a method of that name on something that is not \
                 one, which a line scan cannot tell apart"
            }
            Self::PipeToAChild => {
                "gives a child a pipe, and a read on a pipe that never reaches EOF blocks this \
                 thread forever"
            }
        }
    }
}

/// A call that can leave this thread at a child process's mercy, as it is written.
struct Needle {
    /// The text matched, whitespace removed - see [`wait_sites`].
    text: &'static str,
    /// Which of the three things a match here is.
    blocks: Blocks,
}

impl Needle {
    /// One row of [`BLOCKING`]. A constructor rather than a struct literal so each row is one
    /// line and the needle set is legible as a set - which is the point of a table whose whole
    /// value is that adding to it is a visible architecture diff.
    const fn new(text: &'static str, blocks: Blocks) -> Self {
        Self { text, blocks }
    }
}

/// Everything that can block this tier on a child process.
///
/// `.output()` and `.status()` are the two that produced the hangs; `.wait_with_output()`,
/// `.wait()` and `.try_wait()` are the rest of the surface a hand-rolled loop reaches for, and
/// including them is what stops the fix from being *"write the loop yourself"*. `Stdio::piped()`
/// and `io::pipe()` are the two that are not waits at all - see the header for why a pipe's
/// creation is the needle and the reads on it are not.
///
/// **Adding or removing one is an architecture decision**, the sentence `LEAKY` in
/// `xtask/src/newtype_leaks.rs` carries for the same reason: the diff is where the argument happens.
const BLOCKING: &[Needle] = &[
    Needle::new(".output()", Blocks::OnAnyReceiver),
    Needle::new(".status()", Blocks::OnAnyReceiver),
    Needle::new(".wait_with_output()", Blocks::OnAChild),
    Needle::new(".try_wait()", Blocks::OnAChild),
    Needle::new(".wait()", Blocks::OnAnyReceiver),
    Needle::new("Stdio::piped()", Blocks::PipeToAChild),
    Needle::new("io::pipe()", Blocks::PipeToAChild),
];

/// One needle, in one file of the tier, that is not a wait on a container-runtime child.
struct Allowance {
    /// Repo-relative path, matched exactly.
    path: &'static str,
    /// The ONE needle this entry excuses there. Per needle rather than per file, because a file
    /// exempted wholesale stops being watched: `lock.rs` may run `lsof` and still must not
    /// acquire a pipe to a container.
    needle: &'static str,
    /// What matches it. Printed, because a reader hitting the stale-allowance failure needs to
    /// know what the entry was about.
    what: &'static str,
    /// Why that is not an unbounded wait on a child. Printed, because an allowance whose reason is
    /// unstated is indistinguishable from an oversight.
    why: &'static str,
}

/// Every needle in the tier, outside the docker module, that is declared not to be the defect.
///
/// One entry. It is declared rather than excluded from the scan, because an exclusion hides a file
/// and a declaration classifies it - and [`stale`] then makes the classification answer for itself.
const ALLOWED: &[Allowance] = &[Allowance {
    path: "xtask/src/compose/lock.rs",
    needle: ".output()",
    what: "`lsof`, to resolve the working directory of the process holding the worktree lock",
    why: "it is not a container-runtime child, so no daemon sits between the call and its answer, \
          and the result decides only which of three descriptions a refusal carries - a host with \
          neither `/proc` nor `lsof` already reports an unidentified holder and behaves \
          identically. A bound there would be a bound on a local process listing",
}];

impl Allowance {
    /// Does this entry excuse `needle` in `rel`? One definition, because [`is_allowed`] asks it
    /// forwards and [`stale`] asks it backwards, and two spellings of one predicate can disagree.
    fn covers(&self, rel: &str, needle: &Needle) -> bool {
        self.path == rel && self.needle == needle.text
    }
}

/// A file in the tier, and every blocking site in it.
struct Waiting {
    /// Repo-relative path.
    path: String,
    /// 1-based line, and the needle found there.
    sites: Vec<(usize, &'static Needle)>,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let (root, files) = match repo::all_files().and_then(|census| census.into_listing(repo::Unmigrated::BoundedWait)) {
        Ok(listing) => listing,
        Err(why) => {
            eprintln!("xtask check-bounded-wait: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };

    let mut scanned = 0_usize;
    let mut waiting: Vec<Waiting> = Vec::new();
    for rel in &files {
        if !in_tier(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            // Not skipped. A file in scope this gate cannot read is a file it did not judge.
            eprintln!("xtask check-bounded-wait: could not read {rel}, which is in scope");
            return Verdict::Fail;
        };
        scanned = scanned.saturating_add(1);
        let sites = wait_sites(&code_lines(&text));
        if !sites.is_empty() {
            waiting.push(Waiting {
                path: rel.clone(),
                sites,
            });
        }
    }

    let problems = judged(scanned, &waiting);
    if problems.is_empty() {
        // Exactly one, or `judged` would have said which direction was wrong.
        let (named, sites) = waiting
            .iter()
            .find(|found| in_runtime(&found.path))
            .map_or(("", 0), |found| (found.path.as_str(), found.sites.len()));
        println!(
            "xtask check-bounded-wait: ok - {scanned} file(s) in the compose tier, one waiter \
             ({named}, {sites} site(s)), {} declared allowance(s) beside it",
            ALLOWED.len()
        );
        // Named on GREEN, the convention `check-boundaries` states for the same reason: an
        // allowlist a green run never mentions is one nobody re-reads.
        for allowance in ALLOWED {
            println!("  allowed  {} - `{}`: {}", allowance.path, allowance.needle, allowance.what);
        }
        return Verdict::Pass;
    }

    eprintln!("xtask check-bounded-wait: FAILED - the compose tier does not wait in one place:");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    explain(&problems);
    Verdict::Fail
}

/// Every way the tier can disagree with the rule, as one message each.
///
/// `scanned` is here rather than checked by the caller so that the dead-gate direction is decided
/// in the same function as the others, and can be tested the same way.
fn judged(scanned: usize, waiting: &[Waiting]) -> Vec<String> {
    let mut problems = Vec::new();

    if scanned == 0 {
        // A gate that silently checked nothing is the failure mode a gate exists to prevent, and
        // it is the only one that returns before the rest: with no files read, every other
        // question below is answered by an absence rather than by the tree.
        problems.push(format!(
            "no Rust in scope, so this gate checked nothing - it expected {}, so either the tier \
             moved or the listing it reads is empty",
            TIER.join(", ")
        ));
        return problems;
    }

    let runtime: Vec<&str> = waiting
        .iter()
        .map(|found| found.path.as_str())
        .filter(|path| in_runtime(path))
        .collect();
    match runtime.len() {
        0 => problems.push(format!(
            "nothing matching {} can be blocked by a child process - the wait this gate is about \
             has left the module, or the scan stopped finding it",
            RUNTIME.join(", ")
        )),
        1 => {}
        // Not "wait": a second file matching only `Stdio::piped()` is this failure too, and
        // saying it waits would send a reader looking for a `.output()` that is not there.
        found => problems.push(format!(
            "{found} files in the docker module can be blocked by a child process, and the \
             property is that ONE is: {}",
            runtime.join(", ")
        )),
    }

    for allowance in stale(waiting) {
        problems.push(format!(
            "{} is declared here as `{}` being deliberate ({}) and no longer matches it - delete \
             the entry rather than leaving it to widen what is permitted",
            allowance.path, allowance.needle, allowance.what
        ));
    }

    for found in waiting {
        if in_runtime(&found.path) {
            continue;
        }
        for &(line, needle) in &found.sites {
            if is_allowed(&found.path, needle) {
                continue;
            }
            problems.push(format!("{}:{line}: `{}` {}", found.path, needle.text, needle.blocks.says()));
        }
    }

    problems
}

/// The allowances that no longer match the needle they excuse.
fn stale(waiting: &[Waiting]) -> impl Iterator<Item = &'static Allowance> {
    ALLOWED.iter().filter(move |allowance| {
        !waiting
            .iter()
            .any(|found| found.sites.iter().any(|&(_, needle)| allowance.covers(&found.path, needle)))
    })
}

/// Is this needle, in this file, declared?
fn is_allowed(rel: &str, needle: &Needle) -> bool {
    ALLOWED.iter().any(|allowance| allowance.covers(rel, needle))
}

/// Printed on failure. A gate that only says no gets worked around.
///
/// `problems` is read for one thing: whether any site reported was an ordinary method name, which
/// decides whether the false-positive paragraph is relevant here. Keyed on
/// [`Blocks::OnAnyReceiver::says`](Blocks::says) itself, so the two cannot drift apart.
fn explain(problems: &[String]) {
    eprintln!("A gate that HANGS prints nothing, so it reads as a slow machine rather than as a");
    eprintln!("failure - measured here at 38 min, 50 min and an 864s SIGTERM before the cause was");
    eprintln!("understood. The cause is a subprocess wait with no timeout, and the fix routes every");
    eprintln!("such wait through one function carrying a deadline. This gate holds the half a wait");
    eprintln!("loop cannot hold about itself: that there is only ONE of it in this tier.");
    eprintln!();
    eprintln!("So the fix is not a second loop, and it is not a pipe either - the waiter captures to");
    eprintln!("FILES because polling for an exit while a child fills a pipe buffer puts the hang");
    eprintln!("back. Call the docker module's waiter, giving it a budget for the subcommand run.");
    eprintln!();
    if problems.iter().any(|problem| problem.contains(Blocks::OnAnyReceiver.says())) {
        eprintln!("KNOWN FALSE POSITIVE, and a site above is one of the three it can reach. This is a");
        eprintln!("line scan: it sees the call, never the type it is called on. If that site is a method");
        eprintln!("of one of this tier's own types and no child process is involved, it is this limit");
        eprintln!("rather than a defect - and an allowance is how it gets said out loud.");
        eprintln!();
    }
    eprintln!("Either way the entry in xtask/src/bounded_wait.rs is what has to change, and that is");
    eprintln!("an architecture decision: it should be a visible diff with the argument in it. An");
    eprintln!("entry excuses ONE needle in ONE file, so the rest of that file stays watched. Today:");
    for allowance in ALLOWED {
        eprintln!("  {} - `{}`: {}", allowance.path, allowance.needle, allowance.what);
        eprintln!("    Why: {}", allowance.why);
    }
}

/// Is this file part of the compose tier?
fn in_tier(rel: &str) -> bool {
    repo::matches_any(TIER, rel)
}

/// Is this file part of the docker module?
fn in_runtime(rel: &str) -> bool {
    repo::matches_any(RUNTIME, rel)
}

/// Every blocking site in `code`, as a 1-based line number and the needle found.
///
/// Whitespace inside a line is removed before matching, so `command . output ()` is one site. A
/// call broken across two lines is not - see the limit in this module's header, and the mechanism
/// that owns that shape.
///
/// **A line with no `(` is skipped, and that is a property of [`BLOCKING`] rather than a guess:**
/// removing whitespace only DELETES characters, so a line without `(` cannot produce a dense
/// string containing a needle that has one. `every_needle_carries_the_parenthesis_the_scan_skips`
/// is what stops the next needle from silently invalidating it. Measured over this tier: 3792 code
/// lines, 1236 with a `(`, and the scan's own cost 3.40 ms to 1.14 ms.
fn wait_sites(code: &[String]) -> Vec<(usize, &'static Needle)> {
    let mut found = Vec::new();
    let mut dense = String::new();
    for (index, line) in code.iter().enumerate() {
        if !line.contains('(') {
            continue;
        }
        dense.clear();
        dense.extend(line.chars().filter(|character| !character.is_whitespace()));
        for needle in BLOCKING {
            if dense.contains(needle.text) {
                found.push((index.saturating_add(1), needle));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{ALLOWED, BLOCKING, Blocks, Needle, Waiting, in_runtime, in_tier, judged, wait_sites};
    use crate::serde_parse::scan::code_lines;

    /// The tier's module root as a PATH - what the first entry of `TIER` matches, spelled out so
    /// these tests assert about files rather than about the patterns that select them.
    const TIER_ROOT: &str = "xtask/src/compose.rs";

    /// The docker module's root, for the same reason.
    const RUNTIME_ROOT: &str = "xtask/src/compose/docker.rs";

    /// A file in the tier that is neither the docker module nor an allowance.
    const SIBLING: &str = "xtask/src/compose/health.rs";

    /// How many files were in scope. Only zero versus non-zero is behaviour - the dead-gate
    /// direction - so every other case passes this rather than a number that looks significant.
    const SCANNED: usize = 7;

    /// The declared needle with this text, so a fixture names a call rather than an index.
    fn needle(text: &str) -> &'static Needle {
        BLOCKING
            .iter()
            .find(|found| found.text == text)
            .expect("a needle BLOCKING declares")
    }

    /// A file whose only site is `text`, as the scan would have reported it.
    fn site(path: &str, text: &str) -> Waiting {
        Waiting {
            path: String::from(path),
            sites: vec![(1, needle(text))],
        }
    }

    /// A file with one `.output()` site - the call that produced the first hang.
    fn waits(path: &str) -> Waiting {
        site(path, ".output()")
    }

    /// The declared allowance, as the scan would report it. Present in most trees below so the
    /// stale-allowance direction stays quiet and each test fails for its own reason - and derived
    /// from `ALLOWED` rather than spelled out, because a literal gives no hint it is load-bearing.
    fn declared() -> Waiting {
        site(ALLOWED[0].path, ALLOWED[0].needle)
    }

    /// The needles found in `source`, read the way the gate reads a file.
    fn found(source: &str) -> Vec<&'static str> {
        wait_sites(&code_lines(source))
            .into_iter()
            .map(|(_, needle)| needle.text)
            .collect()
    }

    #[test]
    fn one_waiter_in_the_docker_module_is_the_property() {
        let tree = [waits(RUNTIME_ROOT), declared()];
        let problems = judged(SCANNED, &tree);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn the_waiter_may_move_into_the_docker_directory() {
        // The reason the docker half is a discovery and not a name: the waiter has already moved
        // once inside that module, and a gate naming the file would have gone red for the move.
        let tree = [waits("xtask/src/compose/docker/bounded.rs"), declared()];
        let problems = judged(SCANNED, &tree);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_second_wait_beside_the_waiter_is_the_defect_this_exists_for() {
        // `.output()` written into a sibling: it compiles, it reviews clean, and it restores the
        // unbounded wait that hangs a gate with no output.
        let tree = [waits(RUNTIME_ROOT), declared(), waits(SIBLING)];
        let problems = judged(SCANNED, &tree);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("xtask/src/compose/health.rs:1:"), "{problems:?}");
    }

    #[test]
    fn a_piped_child_is_the_shape_a_list_of_method_names_misses() {
        // The blocking finding on this gate's first draft: `docker compose logs --follow` spawned
        // with a pipe and drained to EOF. No wait METHOD anywhere, the exact hang restored, and
        // the draft printed `ok`.
        let source = "fn follow(command: &mut Command) -> Option<String> {\n\
                      let mut child = command.stdout(Stdio::piped()).spawn().ok()?;\n\
                      let mut sink = String::new();\n\
                      child.stdout.take()?.read_to_string(&mut sink).ok()?;\n\
                      Some(sink)\n}\n";
        assert_eq!(found(source), vec!["Stdio::piped()"]);
    }

    #[test]
    fn an_anonymous_pipe_is_the_same_shape_without_the_word_piped() {
        // Missed on the first pass at the blocking finding, and measured on the pinned compiler:
        // `std::io::pipe` is stable, `Stdio::from` over its write end is the same hang, and
        // `piped` appears nowhere in it.
        let source = "let (mut reader, writer) = std::io::pipe().ok()?;\n\
                      let mut child = command.stdout(Stdio::from(writer)).spawn().ok()?;\n\
                      reader.read_to_string(&mut sink).ok()?;\n";
        assert_eq!(found(source), vec!["io::pipe()"]);
    }

    #[test]
    fn handing_a_child_a_file_is_not_handing_it_a_pipe() {
        // `Stdio::from` is how the waiter gives its child the capture files, which is the FIX for
        // the pipe class - so the needle is a pipe's creation and never its handoff.
        let source = "Ok((Stdio::from(fresh(&self.stdout)?), Stdio::from(fresh(&self.stderr)?)))\n";
        let hits = found(source);
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn a_null_stdio_is_not_a_pipe() {
        // The waiter hands its child three of these, so reading one as a pipe would make the gate
        // fire on the code it protects.
        let source = "let spawned = command.stdin(Stdio::null()).stdout(Stdio::null()).spawn();\n";
        let hits = found(source);
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn a_reported_site_gets_its_own_kind_of_sentence_and_not_a_shared_one() {
        // Both review findings were one message asserting a child for everything it matched: a
        // pipe does not wait, and three of the names belong to anything. Asserted against
        // `says()` rather than against copied fragments, so a reworded sentence cannot leave a
        // stale expectation behind in one of three places.
        for text in [".output()", ".try_wait()", "Stdio::piped()"] {
            let needle = needle(text);
            let tree = [waits(RUNTIME_ROOT), declared(), site(SIBLING, text)];
            let problems = judged(SCANNED, &tree);
            assert_eq!(problems.len(), 1, "{text}: {problems:?}");
            let want = format!("{SIBLING}:1: `{text}` {}", needle.blocks.says());
            assert_eq!(problems[0], want, "{text}");
        }
    }

    #[test]
    fn an_allowance_excuses_one_needle_and_not_the_file_holding_it() {
        // A file exempted wholesale stops being watched: the `lsof` allowance is not permission to
        // acquire a pipe to a container in the same file.
        let tree = [
            waits(RUNTIME_ROOT),
            Waiting {
                path: String::from(ALLOWED[0].path),
                sites: vec![(1, needle(ALLOWED[0].needle)), (2, needle("Stdio::piped()"))],
            },
        ];
        let problems = judged(SCANNED, &tree);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with(&format!("{}:2:", ALLOWED[0].path)), "{problems:?}");
    }

    #[test]
    fn the_split_this_module_states_is_five_waits_and_two_pipes() {
        // What this holds and what it does NOT: the header says *the five calls on a `Command` or
        // a `Child`*, and that split is what this pins. The TOTAL is held elsewhere and properly -
        // `COUNTS` in xtask/src/guidance/claims/counts.rs derives it by counting this table's
        // constructor calls and fails unless a page states it, so the row and the table cannot
        // drift. That entry's own literal is deliberately not spelled here: it counts raw text,
        // so a comment naming it would contribute to the number it checks - which it did, and
        // the gate said `the tree has 8` on the first run.
        // A test comparing this array to a literal beside it could never have held that row, and
        // said it did until review measured otherwise.
        let pipes = BLOCKING.iter().filter(|found| found.blocks == Blocks::PipeToAChild).count();
        assert_eq!(pipes, 2, "{:?}", BLOCKING.iter().map(|found| found.text).collect::<Vec<_>>());
        assert_eq!(BLOCKING.len() - pipes, 5);
    }

    #[test]
    fn the_three_kinds_do_not_share_a_sentence() {
        // One message for all seven would be false about five of them, which is what made the
        // second review finding possible in the first place.
        assert_ne!(Blocks::OnAChild.says(), Blocks::OnAnyReceiver.says());
        assert!(
            !Blocks::PipeToAChild.says().contains("waits"),
            "{}",
            Blocks::PipeToAChild.says()
        );
        assert!(
            Blocks::OnAnyReceiver.says().contains("cannot tell apart"),
            "{}",
            Blocks::OnAnyReceiver.says()
        );
    }

    #[test]
    fn a_wait_in_the_tier_root_is_reported_too() {
        let tree = [waits(RUNTIME_ROOT), declared(), waits(TIER_ROOT)];
        let problems = judged(SCANNED, &tree);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains(TIER_ROOT), "{problems:?}");
    }

    #[test]
    fn a_second_file_inside_the_docker_module_is_reported_as_two_waiters() {
        let tree = [waits(RUNTIME_ROOT), waits("xtask/src/compose/docker/bounded.rs"), declared()];
        let problems = judged(SCANNED, &tree);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("2 files in the docker module"), "{problems:?}");
    }

    #[test]
    fn no_waiter_at_all_fails_closed() {
        // Not a pass. Either the wait left the tier - in which case this gate follows it or is
        // deleted - or the scan stopped seeing it, which is the same thing to a reader.
        let problems = judged(SCANNED, &[declared()]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("can be blocked by a child process"), "{problems:?}");
        // The patterns, so a reader learns WHERE the gate looked and stopped finding one.
        assert!(problems[0].contains(RUNTIME_ROOT), "{problems:?}");
    }

    #[test]
    fn an_empty_scan_fails_closed() {
        // The dead-gate shape: nothing read, nothing found, and a green verdict over silence.
        let problems = judged(0, &[]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("no Rust in scope"), "{problems:?}");
    }

    #[test]
    fn an_allowance_that_stopped_waiting_is_a_failure_in_the_other_direction() {
        // An entry that has stopped being true widens what is permitted while reading as a
        // considered decision - so it is red until somebody deletes it.
        let problems = judged(SCANNED, &[waits(RUNTIME_ROOT)]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("no longer matches it"), "{problems:?}");
        assert!(problems[0].contains(ALLOWED[0].path), "{problems:?}");
    }

    #[test]
    fn prose_explaining_the_defect_is_not_a_wait() {
        // Load-bearing rather than tidy: this tier writes `.output()` repeatedly while explaining
        // the hang, so a raw scan would report the documentation that argues the rule.
        let source = "/// | [`compose`] (`command.output()`) | unbounded on purpose |\n\
                      // let out = command.output()?;\n\
                      /* command.status() */\n\
                      pub fn compose() {}\n";
        let hits = found(source);
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn a_wait_inside_a_multi_line_string_is_not_a_wait() {
        // Which is what keeps this module's own fixtures out of the scan it defines.
        let source = "fn fixture() -> &'static str {\n    r#\"\nlet out = command.output()?;\n\"#\n}\n";
        let hits = found(source);
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn every_needle_carries_the_parenthesis_the_scan_skips() {
        // `wait_sites` skips a line with no `(` to avoid building a dense string per line. That is
        // sound only while every needle contains one, so the assumption is held here rather than
        // by the comment that states it.
        for needle in BLOCKING {
            assert!(needle.text.contains('('), "{} would be skipped by the scan", needle.text);
        }
    }

    #[test]
    fn every_needle_is_found_where_it_is_written() {
        // The fixture is text this scan reads, not Rust a compiler reads, so one shape serves all
        // seven - what is under test is that no declared needle is unreachable.
        for needle in BLOCKING {
            let source = format!("fn f() {{\n    let _ = subject{};\n}}\n", needle.text);
            assert_eq!(found(&source), vec![needle.text], "{} was not found", needle.text);
        }
    }

    #[test]
    fn whitespace_inside_the_call_does_not_hide_it() {
        assert_eq!(found("let out = command . output ();\n"), vec![".output()"]);
    }

    #[test]
    fn a_status_field_is_not_a_status_call() {
        // `out.status.success()` is how every caller in this tier reads an exit code, so reading it
        // as a wait would make the gate fire on the code it is protecting.
        assert!(found("let ok = out.status.success();\n").is_empty());
    }

    #[test]
    fn wait_with_output_is_reported_as_itself() {
        // Not as `.output()`, and not as `.wait()`: a needle that swallowed its neighbours would
        // name the wrong call in the failure message.
        assert_eq!(found("let out = child.wait_with_output()?;\n"), vec![".wait_with_output()"]);
        assert_eq!(found("match child.try_wait() {\n"), vec![".try_wait()"]);
    }

    #[test]
    fn the_scope_is_the_compose_tier_and_nothing_else_in_xtask() {
        assert!(in_tier(TIER_ROOT));
        assert!(in_tier(RUNTIME_ROOT));
        assert!(in_tier("xtask/src/compose/docker/bounded.rs"));
        // `git` and `cargo` are waited on without a bound on purpose, one directory up.
        assert!(!in_tier("xtask/src/repo.rs"));
        assert!(!in_tier("xtask/src/causality.rs"));
        // A name that only starts the same way is not inside the tier.
        assert!(!in_tier("xtask/src/compose_notes.md"));
        assert!(!in_tier("xtask/src/compose/README.md"));
    }

    #[test]
    fn every_docker_module_pattern_selects_files_the_tier_also_selects() {
        // Otherwise the gate is permanently red on `no waiter` and the message names the symptom
        // rather than the cause. Over the patterns as they stand - general containment is not
        // decidable over globs, which is why the header declares it rather than claiming it.
        for path in [RUNTIME_ROOT, "xtask/src/compose/docker/bounded.rs"] {
            assert!(in_runtime(path), "{path} is not in the docker module");
            assert!(in_tier(path), "{path} is in the docker module but outside the tier");
        }
    }

    #[test]
    fn the_docker_module_is_its_root_and_its_directory() {
        assert!(in_runtime(RUNTIME_ROOT));
        assert!(in_runtime("xtask/src/compose/docker/bounded.rs"));
        assert!(!in_runtime(TIER_ROOT));
        assert!(!in_runtime("xtask/src/compose/lock.rs"));
    }

    #[test]
    fn every_allowance_is_in_the_tier_outside_the_docker_module_and_says_why() {
        for allowance in ALLOWED {
            assert!(in_tier(allowance.path), "{} is not in scope", allowance.path);
            assert!(!in_runtime(allowance.path), "{} needs no allowance", allowance.path);
            // A needle no table declares excuses nothing, and would reach a reader as the
            // stale-allowance failure instead of as the typo it is.
            assert!(
                BLOCKING.iter().any(|needle| needle.text == allowance.needle),
                "{} excuses `{}`, which is not a declared needle",
                allowance.path,
                allowance.needle
            );
            assert!(
                !allowance.what.is_empty(),
                "{} says nothing about what matches it",
                allowance.path
            );
            assert!(!allowance.why.is_empty(), "{} says nothing about why", allowance.path);
        }
    }
}

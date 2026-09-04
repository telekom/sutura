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
//! Over [`TIER_ROOT`] and everything under [`TIER_DIR`], every file containing one of [`WAITS`]
//! must be either
//!
//! 1. **the docker module** ([`RUNTIME_ROOT`] or under [`RUNTIME_DIR`]), of which **exactly one
//!    file** may wait - that file is the tier's waiter, whatever it is called; or
//! 2. **a declared allowance** - one entry today, in [`ALLOWED`], each carrying what it runs and
//!    why a bound would be wrong there.
//!
//! The first half is a discovery rather than a name, and that is deliberate: the waiter has already
//! moved once inside that module, and a gate naming the file it lived in would have gone red for
//! the move rather than for a defect. What the gate asks is *how many places in this tier wait*,
//! and the answer it accepts is one.
//!
//! **It starts green.** Measured when it was written: in this tier the docker module is the only
//! file that waits on a child process, beside the one allowance below. So the gate's whole job is
//! to keep it that way, and the cheapest gate is the one most likely to earn its keep years from
//! now.
//!
//! # Fails closed, in four directions
//!
//! Each of these is a failure rather than a pass over silence, because a scan that finds nothing is
//! how the property went unheld in the first place:
//!
//! * **No file in scope.** The tier renamed or moved out from under the gate.
//! * **No waiter in the docker module.** Either the wait left the tier - in which case the gate
//!   follows it or is deleted - or the scan stopped seeing it, which is the same thing to a reader.
//! * **More than one waiter in the docker module.** The property IS one, and a second one is the
//!   defect this exists for.
//! * **A declared allowance that no longer waits.** An entry that has stopped being true widens
//!   what is permitted while reading as a considered decision.
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
//! **A wait laundered through a helper elsewhere is invisible.** The scan reads text in this tier;
//! a function in another module that waits, called from here, is not found. So is a wait added to a
//! directory nobody put in scope - which is the reason the scope is two constants and not a glob
//! over `xtask/`, where the unbounded `git` and `cargo` calls legitimately live.
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

use crate::Verdict;
use crate::repo;
use crate::serde_parse::scan::code_lines;

/// The compose tier's module root. This file and everything under [`TIER_DIR`] is what the gate
/// judges - deliberately not the whole of `xtask/`, where waiting on `git`, `cargo` and `nix`
/// without a bound is correct.
const TIER_ROOT: &str = "xtask/src/compose.rs";

/// The directory beside [`TIER_ROOT`].
const TIER_DIR: &str = "xtask/src/compose/";

/// The docker module's root: the one place in the tier where waiting on a child is the contract.
const RUNTIME_ROOT: &str = "xtask/src/compose/docker.rs";

/// The directory beside [`RUNTIME_ROOT`]. Named as a prefix rather than as a file, so the waiter
/// splitting out of the module root is a move and not a red gate.
const RUNTIME_DIR: &str = "xtask/src/compose/docker/";

/// The calls that wait on a child process, as they are written.
///
/// `.output()` and `.status()` are the two that produced the hangs; `.wait_with_output()`,
/// `.wait()` and `.try_wait()` are the rest of the surface a hand-rolled loop reaches for, and
/// including them is what stops the fix from being *"write the loop yourself"*.
///
/// **Adding or removing one is an architecture decision**, the sentence `LEAKY` in
/// `xtask/src/newtype_leaks.rs` carries for the same reason: the diff is where the argument happens.
const WAITS: &[&str] = &[".output()", ".status()", ".wait_with_output()", ".try_wait()", ".wait()"];

/// A file in the tier that waits deliberately and is not the docker module.
struct Allowance {
    /// Repo-relative path, matched exactly.
    path: &'static str,
    /// What it runs. Printed, because a reader hitting the stale-allowance failure needs to know
    /// what the entry was about.
    what: &'static str,
    /// Why a bound would be wrong there. Printed, because an allowance whose reason is unstated is
    /// indistinguishable from an oversight.
    why: &'static str,
}

/// Every deliberate wait in the tier outside the docker module.
///
/// One entry. It is declared rather than excluded from the scan, because an exclusion hides a file
/// and a declaration classifies it - and [`stale`] then makes the classification answer for itself.
const ALLOWED: &[Allowance] = &[Allowance {
    path: "xtask/src/compose/lock.rs",
    what: "`lsof`, to resolve the working directory of the process holding the worktree lock",
    why: "it is not a container-runtime child, so no daemon sits between the call and its answer, \
          and the result decides only which of three descriptions a refusal carries - a host with \
          neither `/proc` nor `lsof` already reports an unidentified holder and behaves \
          identically. A bound there would be a bound on a local process listing",
}];

/// A file in the tier, and every wait site in it.
struct Waiting {
    /// Repo-relative path.
    path: String,
    /// 1-based line, and the needle found there.
    sites: Vec<(usize, &'static str)>,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-bounded-wait: could not determine the repo root");
        return Verdict::Fail;
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
        let waiter = waiting.iter().find(|found| in_runtime(&found.path));
        let sites = waiter.map_or(0, |found| found.sites.len());
        let named = waiter.map_or("", |found| found.path.as_str());
        println!(
            "xtask check-bounded-wait: ok - {scanned} file(s) in the compose tier, one waiter \
             ({named}, {sites} site(s)), {} declared allowance(s) beside it",
            ALLOWED.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-bounded-wait: FAILED - the compose tier does not wait in one place:");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    explain();
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
            "no Rust in scope, so this gate checked nothing - it expected {TIER_ROOT} and the \
             files under {TIER_DIR}, so either the tier moved or the listing it reads is empty"
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
            "nothing in {RUNTIME_ROOT} or under {RUNTIME_DIR} waits on a child process - the wait \
             this gate is about has left the module, or the scan stopped finding it"
        )),
        1 => {}
        found => problems.push(format!(
            "{found} files in the docker module wait on a child process, and the property is that \
             ONE does: {}",
            runtime.join(", ")
        )),
    }

    for allowance in stale(waiting) {
        problems.push(format!(
            "{} is declared here as waiting deliberately ({}) and no longer waits - delete the \
             entry rather than leaving it to widen what is permitted",
            allowance.path, allowance.what
        ));
    }

    for found in waiting {
        if in_runtime(&found.path) || allowance_for(&found.path).is_some() {
            continue;
        }
        for (line, needle) in &found.sites {
            problems.push(format!("{}:{line}: `{needle}` waits on a child process", found.path));
        }
    }

    problems
}

/// The allowances whose file no longer waits.
fn stale(waiting: &[Waiting]) -> impl Iterator<Item = &'static Allowance> {
    ALLOWED
        .iter()
        .filter(move |allowance| !waiting.iter().any(|found| found.path == allowance.path))
}

/// The declared allowance for `rel`, if there is one.
fn allowance_for(rel: &str) -> Option<&'static Allowance> {
    ALLOWED.iter().find(|allowance| allowance.path == rel)
}

/// Printed on failure. A gate that only says no gets worked around.
fn explain() {
    eprintln!("A gate that HANGS prints nothing, so it reads as a slow machine rather than as a");
    eprintln!("failure - measured here at 38 min, 50 min and an 864s SIGTERM before the cause was");
    eprintln!("understood. The cause is a subprocess wait with no timeout, and the fix routes every");
    eprintln!("such wait through one function carrying a deadline. This gate holds the half a wait");
    eprintln!("loop cannot hold about itself: that there is only ONE of it in this tier.");
    eprintln!();
    eprintln!("So the fix is not a second loop. Call the docker module's waiter, giving it a budget");
    eprintln!("for the subcommand being run.");
    eprintln!();
    eprintln!("If a wait outside that module is genuinely right, the entry in");
    eprintln!("xtask/src/bounded_wait.rs is what has to change, and that is an architecture");
    eprintln!("decision: it should be a visible diff with the argument in it. The allowances today:");
    for allowance in ALLOWED {
        eprintln!("  {}: {}", allowance.path, allowance.what);
        eprintln!("    Why: {}", allowance.why);
    }
}

/// Is this file part of the compose tier?
fn in_tier(rel: &str) -> bool {
    (rel == TIER_ROOT || rel.starts_with(TIER_DIR)) && is_rust(rel)
}

/// Is this file part of the docker module?
fn in_runtime(rel: &str) -> bool {
    rel == RUNTIME_ROOT || rel.starts_with(RUNTIME_DIR)
}

/// Case-insensitive, because a case-sensitive extension test is wrong on a case-insensitive
/// filesystem - the reason `causality`'s own path test gives.
fn is_rust(rel: &str) -> bool {
    std::path::Path::new(rel)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Every wait site in `code`, as a 1-based line number and the needle found.
///
/// Whitespace inside a line is removed before matching, so `command . output ()` is one site. A
/// call broken across two lines is not - see the limit in this module's header, and the mechanism
/// that owns that shape.
fn wait_sites(code: &[String]) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (index, line) in code.iter().enumerate() {
        let dense: String = line.chars().filter(|character| !character.is_whitespace()).collect();
        for needle in WAITS {
            if dense.contains(needle) {
                found.push((index.saturating_add(1), *needle));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{ALLOWED, RUNTIME_ROOT, TIER_ROOT, WAITS, Waiting, in_runtime, in_tier, judged, wait_sites};
    use crate::serde_parse::scan::code_lines;

    /// A file with one wait site, as the scan would have reported it.
    fn waits(path: &str) -> Waiting {
        Waiting {
            path: String::from(path),
            sites: vec![(1, ".output()")],
        }
    }

    /// The needles found in `source`, read the way the gate reads a file.
    fn found(source: &str) -> Vec<&'static str> {
        wait_sites(&code_lines(source))
            .into_iter()
            .map(|(_, needle)| needle)
            .collect()
    }

    #[test]
    fn one_waiter_in_the_docker_module_is_the_property() {
        let tree = [waits(RUNTIME_ROOT), waits("xtask/src/compose/lock.rs")];
        assert!(judged(5, &tree).is_empty(), "{:?}", judged(5, &tree));
    }

    #[test]
    fn the_waiter_may_move_into_the_docker_directory() {
        // The reason the docker half is a discovery and not a name: the waiter has already moved
        // once inside that module, and a gate naming the file would have gone red for the move.
        let tree = [
            waits("xtask/src/compose/docker/bounded.rs"),
            waits("xtask/src/compose/lock.rs"),
        ];
        assert!(judged(6, &tree).is_empty(), "{:?}", judged(6, &tree));
    }

    #[test]
    fn a_second_wait_beside_the_waiter_is_the_defect_this_exists_for() {
        // `.output()` written into a sibling: it compiles, it reviews clean, and it restores the
        // unbounded wait that hangs a gate with no output.
        let tree = [
            waits(RUNTIME_ROOT),
            waits("xtask/src/compose/lock.rs"),
            waits("xtask/src/compose/health.rs"),
        ];
        let problems = judged(6, &tree);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("xtask/src/compose/health.rs:1:"), "{problems:?}");
    }

    #[test]
    fn a_wait_in_the_tier_root_is_reported_too() {
        let tree = [waits(RUNTIME_ROOT), waits("xtask/src/compose/lock.rs"), waits(TIER_ROOT)];
        let problems = judged(5, &tree);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains(TIER_ROOT), "{problems:?}");
    }

    #[test]
    fn a_second_file_inside_the_docker_module_is_reported_as_two_waiters() {
        let tree = [
            waits(RUNTIME_ROOT),
            waits("xtask/src/compose/docker/bounded.rs"),
            waits("xtask/src/compose/lock.rs"),
        ];
        let problems = judged(6, &tree);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("2 files in the docker module"), "{problems:?}");
    }

    #[test]
    fn no_waiter_at_all_fails_closed() {
        // Not a pass. Either the wait left the tier - in which case this gate follows it or is
        // deleted - or the scan stopped seeing it, which is the same thing to a reader.
        let problems = judged(5, &[waits("xtask/src/compose/lock.rs")]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("waits on a child process"), "{problems:?}");
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
        let problems = judged(5, &[waits(RUNTIME_ROOT)]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("no longer waits"), "{problems:?}");
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
        assert!(found(source).is_empty(), "{:?}", found(source));
    }

    #[test]
    fn a_wait_inside_a_multi_line_string_is_not_a_wait() {
        // Which is what keeps this module's own fixtures out of the scan it defines.
        let source = "fn fixture() -> &'static str {\n    r#\"\nlet out = command.output()?;\n\"#\n}\n";
        assert!(found(source).is_empty(), "{:?}", found(source));
    }

    #[test]
    fn every_needle_is_found_where_it_is_written() {
        for needle in WAITS {
            let source = format!("fn f() {{\n    let _ = child{needle};\n}}\n");
            assert_eq!(found(&source), vec![*needle], "{needle} was not found");
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
            assert!(
                !allowance.what.is_empty(),
                "{} says nothing about what it runs",
                allowance.path
            );
            assert!(!allowance.why.is_empty(), "{} says nothing about why", allowance.path);
        }
    }
}

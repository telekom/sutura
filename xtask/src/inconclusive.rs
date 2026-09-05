//! Every venue that invokes a gate able to answer INCONCLUSIVE declares what it does with exit 3.
//!
//! `crate::Verdict::Inconclusive` is exit 3, and the whole argument for it is that **the default is
//! closed**: 3 is neither 0 nor 1, so a consumer that has not been taught the code fails on it
//! rather than reading a green check over no evidence - which is what `github.com/telekom/sutura#307`
//! records happening twice on finished branches. [`SITES`] is where each venue says which of the two
//! things it does with 3: branch on it and continue deliberately, or propagate it because nothing in
//! that venue suppresses a status.
//!
//! **That was a fact about today's tree and nothing held it.** A step written as
//! `nix run .#causality -- --since "$BASE" || true`, or a recipe whose invocation stops being the
//! last command under `set -e`, restores exit-0 semantics silently and no gate notices - the exact
//! shape #307 was about, one venue over. So the invocation sites are a **declared list** here:
//! [`SITES`] says which file may invoke the gate and what it does with 3, and a site the scan finds
//! that the list does not name is a failure. A NEW venue is a decision, not a diff.
//!
//! Three rules, and each one is a way the argument has failed or could:
//!
//! * **[`Handling::Propagates`] means no `||` on the invocation line.** `|| true`, `|| :` and
//!   `|| exit 0` all read as ordinary shell and all turn 3 into 0. A site that captures the status
//!   has to say so by declaring the other handling.
//! * **[`Handling::BranchesOnThree`] means the capture is real.** The line has to end in
//!   `|| <var>=$?` and the file has to compare `<var>` against 3 - a declared branch nobody wrote
//!   is a swallowed status wearing a declaration.
//! * **A branching site still propagates everything else.** The same file has to `exit "$<var>"`,
//!   because handling 3 and dropping 1 is a worse gate than not handling 3 at all.
//!
//! **What it does not reach**, stated next to the claim because a text scan always has a rim:
//!
//! * **A wrapper.** This reads the file that spells the invocation. A venue that calls a script
//!   which calls the gate declares nothing here, and the script's own handling is invisible - the
//!   scan would name the script as the site and the outer venue would not appear.
//! * **`continue-on-error` on a workflow step.** That makes a job green over ANY status, which is a
//!   property of the step rather than of the invocation line, and the two workflow-shaped venues
//!   here do not use it. A gate over it belongs with the step parser in `crate::workflows`.
//! * **Whether the branch a site declares is ever taken.** `ci.yml`'s exit-3 branch has executed in
//!   no run of this repository. This holds that the code path exists and is not a suppression; that
//!   it works is `Verdict::exit_code`'s unit test plus the measurement that `nix run` and `just`
//!   both propagate 3 unchanged.
//! * **A gate other than `test-causality`.** `Verdict::Inconclusive` is available to every task in
//!   the registry, and only this one returns it today. When a second does, its invocations join
//!   [`NEEDLES`] and the list grows - which is the same decision made once more, out loud.

use std::path::Path;

use crate::Verdict;
use crate::repo;
use crate::workflows::sources::{Source, ci_sources};

/// How a venue spells an invocation of a gate that can answer INCONCLUSIVE.
///
/// The xtask task name covers `cargo run -p xtask -- test-causality` however the profile and flags
/// are spelled; the flake app is the form CI reads, and it is a different string because `nix run`
/// is the boundary the exit code crosses there. `just causality` is deliberately NOT a needle: the
/// recipe body is one of these two, and matching the recipe name as well would make an `echo` that
/// names it for a reader look like an invocation - `ci.yml` prints exactly that sentence.
const NEEDLES: &[&str] = &["test-causality", "nix run .#causality"];

/// What a venue does with exit 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handling {
    /// The status reaches this venue's own caller unchanged: a bare command, so `set -e`, `exec` or
    /// the process boundary carries it.
    Propagates,
    /// The venue captures the status and branches on 3 explicitly, then propagates anything else.
    BranchesOnThree,
}

/// One venue that may invoke the gate, and what it does with exit 3.
struct Site {
    /// The label the scan produces for the file - what a reader can open.
    label: &'static str,
    /// What this venue does with 3.
    handling: Handling,
    /// Why, in one line, for the failure a change to this venue produces.
    because: &'static str,
}

/// Every venue that may invoke the gate. A site the scan finds and this does not name fails.
const SITES: &[Site] = &[
    Site {
        label: "justfile",
        handling: Handling::Propagates,
        because: "a developer typing the task reads the verdict and the shell reads the status",
    },
    Site {
        label: "flake.nix",
        handling: Handling::Propagates,
        because: "the flake app is a passthrough: `exec`, so `nix run .#causality` IS the gate's status",
    },
    Site {
        label: "devenv.nix",
        handling: Handling::BranchesOnThree,
        because: "`ship-check` retains the verdict and runs the remaining hooks, which a push still needs",
    },
    Site {
        label: "ci.yml",
        handling: Handling::BranchesOnThree,
        because: "the changes that land on an inconclusive arm are legitimate, and a gate that reddens correct work gets disabled",
    },
];

/// One invocation the scan found: where it is, and the line that spells it.
struct Found<'s> {
    /// The source's label.
    label: &'s str,
    /// The line number, for a failure a reader can open.
    number: usize,
    /// The invocation line, trimmed.
    line: &'s str,
}

/// `check-inconclusive`.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-inconclusive: could not determine the repo root");
        return Verdict::Fail;
    };
    let Some(sources) = sources(&root) else {
        return Verdict::Fail;
    };

    let found = invocations(&sources);
    // FAIL CLOSED. This gate's whole subject is a status nobody handled, so a scan that found
    // nothing to check must not be the way it goes unhandled again: the needles are wrong, or the
    // walk lost a venue, and either is the scan breaking rather than the tree being clean.
    if found.is_empty() {
        eprintln!("xtask check-inconclusive: no invocation of the causality gate found anywhere");
        eprintln!("  the scan is broken, not the tree - `NEEDLES` or the source walk has drifted");
        return Verdict::Fail;
    }

    let mut problems = Vec::new();
    for one in &found {
        match SITES.iter().find(|site| site.label == one.label) {
            Some(site) => problems.extend(judge(site, one, &sources)),
            None => problems.push(format!(
                "{}:{} invokes the causality gate and no site declares what it does with exit 3 \
                 - `xtask/src/inconclusive.rs` is where a venue says `Propagates` or \
                 `BranchesOnThree`, and adding one is a decision rather than a diff: {}",
                one.label, one.number, one.line
            )),
        }
    }
    for site in SITES {
        if !found.iter().any(|one| one.label == site.label) {
            problems.push(format!(
                "{} is declared as an invocation site and invokes nothing - delete the site, or the \
                 invocation moved somewhere this scan cannot see it (a wrapper script is invisible \
                 here, see the module header)",
                site.label
            ));
        }
    }

    if problems.is_empty() {
        println!(
            "xtask check-inconclusive: ok - {} invocation(s) across {} declared venue(s)",
            found.len(),
            SITES.len()
        );
        return Verdict::Pass;
    }
    eprintln!("xtask check-inconclusive: exit 3 is not handled where it is invoked");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("Exit 3 means the gate measured nothing. The default is CLOSED because 3 is neither 0");
    eprintln!("nor 1, so a venue that suppresses it hands its reader a green check over no evidence -");
    eprintln!("which is what `github.com/telekom/sutura#307` records happening twice. `just hygiene`");
    eprintln!("runs this gate.");
    Verdict::Fail
}

/// Every file that could spell an invocation, in one walk each side.
///
/// `crate::workflows::sources` for the CI half, because a second walk over the same three places is
/// a second answer to *where does CI invoke things from* - the drift that module's header records
/// twice. The local half is the justfile, `devenv.nix`, `flake.nix` and `nix/*.nix`: absent is not
/// legitimate for the justfile or the flake, so a read failure there is the scan breaking.
fn sources(root: &Path) -> Option<Vec<Source>> {
    let mut out = ci_sources(root)?;

    for name in ["justfile", "devenv.nix", "flake.nix"] {
        match std::fs::read_to_string(root.join(name)) {
            Ok(text) => out.push(Source {
                label: String::from(name),
                text,
            }),
            Err(e) => {
                eprintln!("xtask check-inconclusive: cannot read {name}: {e}");
                return None;
            }
        }
    }

    for entry in std::fs::read_dir(root.join("nix")).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("nix") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let named = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        out.push(Source {
            label: format!("nix/{named}"),
            text,
        });
    }

    Some(out)
}

/// Every invocation line, with the comments dropped.
///
/// A `#` line is prose in all four of these languages, and every one of these files names the gate
/// in prose: `flake.nix` documents `nix run .#causality --since <ref>` above the app, the justfile
/// heads the recipe with it, and `nix/cargo-env.nix` records a reproduction with it. Reading those
/// as invocations would make the declared list a list of files that mention a gate.
fn invocations(sources: &[Source]) -> Vec<Found<'_>> {
    let mut out = Vec::new();
    for source in sources {
        for (index, line) in source.text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }
            if NEEDLES.iter().any(|needle| trimmed.contains(needle)) {
                out.push(Found {
                    label: &source.label,
                    number: index.saturating_add(1),
                    line: trimmed,
                });
            }
        }
    }
    out
}

/// What is wrong with `one`, given what `site` declared.
fn judge(site: &Site, one: &Found<'_>, sources: &[Source]) -> Vec<String> {
    match site.handling {
        Handling::Propagates => propagation_problems(site, one),
        Handling::BranchesOnThree => branch_problems(site, one, sources),
    }
}

/// A declared passthrough with a `||` on it is not a passthrough.
fn propagation_problems(site: &Site, one: &Found<'_>) -> Vec<String> {
    if !one.line.contains("||") {
        return Vec::new();
    }
    vec![format!(
        "{}:{} declares that it PROPAGATES exit 3 ({}) and the invocation carries a `||`, which \
         turns 3 into whatever follows it: {}",
        one.label, one.number, site.because, one.line
    )]
}

/// A declared branch has to capture the status, compare it against 3, and pass the rest on.
fn branch_problems(site: &Site, one: &Found<'_>, sources: &[Source]) -> Vec<String> {
    let Some(captured) = captured_status(one.line) else {
        return vec![format!(
            "{}:{} declares that it BRANCHES ON 3 ({}) and the invocation does not capture the \
             status - a branch needs `|| <var>=$?`, and `|| true` or a bare command is not one: {}",
            one.label, one.number, site.because, one.line
        )];
    };

    let text = sources
        .iter()
        .find(|source| source.label == one.label)
        .map_or("", |source| source.text.as_str());
    let mut problems = Vec::new();

    if !text.lines().any(|line| line.contains("-eq 3") && line.contains(&captured)) {
        problems.push(format!(
            "{}:{} captures the status into `{captured}` and no line compares it against 3 - the \
             declared branch does not exist, so exit 3 is swallowed",
            one.label, one.number
        ));
    }

    // HANDLING 3 AND DROPPING 1 IS THE WORSE GATE. A venue that captures the status has taken it
    // out of the shell's hands for every value, not only for 3, so the re-raise is part of the
    // declaration rather than a detail of it.
    if !text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("exit") && trimmed.contains(&captured)
    }) {
        problems.push(format!(
            "{}:{} captures the status into `{captured}` and never re-raises it - handling 3 while \
             dropping a real failure is a worse gate than not handling 3 at all",
            one.label, one.number
        ));
    }

    problems
}

/// The variable a `|| <var>=$?` capture writes, if the line is one.
fn captured_status(line: &str) -> Option<String> {
    let (_, after) = line.split_once("||")?;
    let (name, rest) = after.trim().split_once('=')?;
    if !rest.starts_with("$?") || name.is_empty() {
        return None;
    }
    let named = name.trim();
    named
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        .then(|| String::from(named))
}

#[cfg(test)]
mod tests {
    use super::{Found, Handling, SITES, Site, captured_status, invocations, judge};
    use crate::workflows::sources::Source;

    /// One source, as the walk would produce it.
    fn source(label: &str, text: &str) -> Source {
        Source {
            label: String::from(label),
            text: String::from(text),
        }
    }

    /// The declared site with this label.
    fn site(label: &str) -> &'static Site {
        SITES.iter().find(|one| one.label == label).expect("a declared site")
    }

    /// `line` as the scan would have found it in `label`.
    fn found<'s>(label: &'s str, line: &'s str) -> Found<'s> {
        Found { label, number: 1, line }
    }

    #[test]
    fn a_suppressed_status_fails_at_a_venue_that_declared_it_propagates() {
        // THE FAILURE THIS GATE EXISTS FOR, and it was held by nothing: `|| true` on the recipe's
        // invocation restores exit-0 semantics for the whole venue, which is #307 one venue over.
        let just = site("justfile");
        assert_eq!(just.handling, Handling::Propagates);
        assert!(
            judge(
                just,
                &found("justfile", "cargo run -q -p xtask -- test-causality --since HEAD~1"),
                &[]
            )
            .is_empty(),
            "a bare command propagates"
        );
        for suppressed in [
            "cargo run -q -p xtask -- test-causality --since HEAD~1 || true",
            "cargo run -q -p xtask -- test-causality --since HEAD~1 || :",
            "cargo run -q -p xtask -- test-causality --since HEAD~1 || exit 0",
        ] {
            let problems = judge(just, &found("justfile", suppressed), &[]);
            assert_eq!(problems.len(), 1, "`{suppressed}` turns 3 into 0: {problems:?}");
        }
    }

    #[test]
    fn a_declared_branch_that_does_not_compare_against_three_fails() {
        // A DECLARATION IS NOT THE MECHANISM. Capturing the status and never testing it is the
        // same swallow as `|| true` wearing a site entry, and a reader of the site list would read
        // it as handled.
        let ci = site("ci.yml");
        assert_eq!(ci.handling, Handling::BranchesOnThree);
        let line = "nix run .#causality -- --since \"$BASE\" || status=$?";

        let handled = [source(
            "ci.yml",
            "nix run .#causality -- --since \"$BASE\" || status=$?\nif [ \"$status\" -eq 3 ]; then\n  exit 0\nfi\nexit \"$status\"\n",
        )];
        assert!(
            judge(ci, &found("ci.yml", line), &handled).is_empty(),
            "captured, compared against 3, and re-raised"
        );

        let swallowed = [source(
            "ci.yml",
            "nix run .#causality -- --since \"$BASE\" || status=$?\nexit \"$status\"\n",
        )];
        let problems = judge(ci, &found("ci.yml", line), &swallowed);
        assert!(
            problems.iter().any(|p| p.contains("compares it against 3")),
            "a capture with no comparison is a swallow: {problems:?}"
        );

        // And the other half: handling 3 while dropping 1 is the worse gate.
        let dropped = [source(
            "ci.yml",
            "nix run .#causality -- --since \"$BASE\" || status=$?\nif [ \"$status\" -eq 3 ]; then\n  exit 0\nfi\n",
        )];
        let problems = judge(ci, &found("ci.yml", line), &dropped);
        assert!(
            problems.iter().any(|p| p.contains("never re-raises")),
            "a captured status has to be re-raised: {problems:?}"
        );

        // A branching site that does not capture at all is the third way.
        let problems = judge(
            ci,
            &found("ci.yml", "nix run .#causality -- --since \"$BASE\" || true"),
            &handled,
        );
        assert!(
            problems.iter().any(|p| p.contains("does not capture the status")),
            "`|| true` is not a branch: {problems:?}"
        );
    }

    #[test]
    fn a_gate_named_in_prose_is_not_an_invocation_site() {
        // THE REASON `just causality` IS NOT A NEEDLE, measured on this tree: `ci.yml` echoes that
        // exact phrase to the step summary, and every one of the four venues names the gate in a
        // comment. A scan that read those would turn the declared list into a list of files that
        // mention a gate, and the list would then need entries that handle nothing.
        let sources = vec![
            source(
                "flake.nix",
                "# `nix run .#causality -- --since <ref>` - the red-before-green gate.\n            exec cargo run -q --profile ci -p xtask -- test-causality \"$@\"\n",
            ),
            source("ci.yml", "          echo \"scoped per commit - see \\`just causality\\`\"\n"),
        ];
        let found = invocations(&sources);
        assert_eq!(
            found.len(),
            1,
            "one invocation, not three: {:?}",
            found.iter().map(|f| f.line).collect::<Vec<_>>()
        );
        assert_eq!(found[0].label, "flake.nix");
        assert_eq!(found[0].number, 2);
    }

    #[test]
    fn only_a_real_capture_reads_as_one() {
        assert_eq!(captured_status("x -- test-causality || status=$?").as_deref(), Some("status"));
        assert_eq!(
            captured_status("cargo run -- test-causality --since \"$m\" || causality_status=$?").as_deref(),
            Some("causality_status")
        );
        assert_eq!(captured_status("x -- test-causality").as_deref(), None);
        assert_eq!(captured_status("x -- test-causality || true").as_deref(), None);
        assert_eq!(captured_status("x -- test-causality || exit 0").as_deref(), None);
        // `$!` is the last background pid, not the status - a plausible typo that would make the
        // branch compare against something no gate produced.
        assert_eq!(captured_status("x -- test-causality || status=$!").as_deref(), None);
    }

    #[test]
    fn this_repository_declares_every_venue_that_invokes_the_gate() {
        // THE LIVE ASSERTION, so the fixtures above cannot be the whole of it: the real tree's
        // sites resolve, and the whole gate passes over them. `run` is what CI and `just hygiene`
        // call, and its own fail-closed arms are what a broken scan reaches.
        let root = crate::repo::root().expect("the repo root");
        let sources = super::sources(&root).expect("the venue walk");
        let found = invocations(&sources);
        assert!(!found.is_empty(), "the scan found no invocation at all");
        for one in &found {
            assert!(
                SITES.iter().any(|site| site.label == one.label),
                "{}:{} invokes the gate and is not declared: {}",
                one.label,
                one.number,
                one.line
            );
        }
        assert_eq!(super::run(&[]), crate::Verdict::Pass);
    }
}

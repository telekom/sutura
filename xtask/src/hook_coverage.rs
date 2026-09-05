//! What a diff-scoped hook run COVERED, said out loud beside its verdict.
//!
//! `AGENTS.md` nominates `just ship-check` as the thing to run *before saying done*, so its green
//! is read by every reader - human and agent - as *the gates covered this change*. It is not that.
//! `prek` filters each hook by the changed file set, which is what makes the task fast enough to
//! run before every push and is the right BEHAVIOUR; what was wrong is the REPORTING. Measured on
//! a branch whose diff was one workflow file and one README: five of ten commit-stage hooks printed
//! `(no files to check)Skipped` - no format check, no clippy, no `cargo check`, no CRAP score, no
//! shellcheck - and the summary still said `ship-check: green`. It already knew both numbers and
//! printed neither.
//!
//! # Why the total is DERIVED from the hook config rather than counted from the output
//!
//! Because a silenced hook prints no row at all. **Measured against prek 0.4.14:**
//! `PREK_SKIP=hygiene,rust-tests prek run --dry-run` emits eight rows where an unset run emits
//! ten - the two named hooks are absent, not marked. So a verdict computed from the rows would
//! have reported *8 of 8 ran* over a run with two gates switched off from the environment, which
//! is the same defect one level down from the one this module exists for. The declared set comes
//! from [`crate::hooks`], the one parser of `.pre-commit-config.yaml`, and a declared hook with no
//! row is [`Coverage::Unreported`] - a FAILED verdict rather than a number nobody can attribute.
//!
//! # The sharper case, and the surface no hook reaches
//!
//! A hook's `files:` filter decides what it sees, and one surface this repository ships has no
//! hook at all: the shell inside `.github/actions/*/action.yml`. `zizmor` is pointed at
//! `.github/workflows`, the shellcheck hook globs `*.sh`, and a `run:` block is neither - so a
//! shell defect there passes a diff-scoped run twice over, once by being filtered out and once by
//! the filter never reaching it. `just lint-workflows` is the only task that does. [`SURFACES`]
//! carries that, and `--surface-tasks` is how `ship-check` learns which extra task a diff needs
//! BEFORE it can be judged - so the gap is closed rather than described.
//!
//! # What this does NOT reach
//!
//! * **Whether a hook that ran was RIGHT.** It reads prek's status column; `Failed` counts as
//!   *inspected the diff*, because the question here is coverage and not correctness. The run's
//!   own exit code is what says whether anything failed.
//! * **A hook whose `files:` regex is too narrow.** [`SURFACES`] pairs a path glob with the hook
//!   IDs that CLAIM it, and that pairing is written here rather than derived: `xtask` has no regex
//!   engine, and re-implementing prek's filter would be a gate measured against documentation
//!   instead of against the tool. What holds it is the other direction - every ID named here must
//!   be a hook the config declares, so a rename fails this gate rather than silently emptying a
//!   surface's claim.
//! * **A surface nothing in this repository lints.** The Python under `test-infra/` is the live
//!   example. Absent from [`SURFACES`] on purpose: a row with no covering task would fail every
//!   diff that touched it, and a gate that fails a correct tree is one somebody disables.

use std::collections::BTreeMap;

use crate::Verdict;
use crate::changes;
use crate::hooks;
use crate::repo;

/// What a declared hook contributed to one run.
///
/// Three states rather than a bool, for the reason `check-venues` splits `can` from `unrun`: *ran*
/// and *was filtered out* are both honest outcomes a reader can act on, and *never reported* is
/// neither - it is the state a count of output rows cannot see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coverage {
    /// A row said `Passed`, `Failed` or `Dry Run`: the hook inspected this diff.
    Ran,
    /// A row said `Skipped`: prek's `files:` filter matched nothing in the diff.
    NoMatchingFiles,
    /// Declared for this stage and ABSENT from the output. `SKIP` / `PREK_SKIP` removes the row
    /// entirely - measured, see the module header - so this is the silencing a row count misses.
    Unreported,
}

impl Coverage {
    /// Did the hook inspect the diff?
    const fn inspected(self) -> bool {
        matches!(self, Self::Ran)
    }
}

/// prek's status column, and what each status says about coverage.
///
/// MEASURED against prek 0.4.14 rather than taken from its documentation: these four are literals
/// in the binary, and the row format is `<name><dots>[(<reason>)]<status>` with no space anywhere
/// in the padding.
const STATUSES: &[(&str, Coverage)] = &[
    ("Passed", Coverage::Ran),
    ("Failed", Coverage::Ran),
    ("Dry Run", Coverage::Ran),
    ("Skipped", Coverage::NoMatchingFiles),
];

/// A kind of file a change can touch, and what inspects it.
struct Surface {
    /// What a reader would call it.
    label: &'static str,
    /// Path globs, matched by [`repo::matches`].
    paths: &'static [&'static str],
    /// The hook IDs that claim it. **EMPTY is the sharp case**: nothing a diff-scoped hook run
    /// invokes reaches this surface at all.
    hooks: &'static [&'static str],
    /// The `just` task that reaches it when no hook did.
    reached_by: &'static str,
}

/// Every surface, and the reason each row is where it is.
const SURFACES: &[Surface] = &[
    Surface {
        // The extension, not a directory: `crates/`, `xtask/` and `examples/` all carry Rust, and
        // a directory list here is a list to forget the day a fourth appears.
        label: "Rust source",
        paths: &["*.rs"],
        hooks: &["rust-fmt", "rust-clippy", "rust-check-changed", "rust-tests", "rust-doctests"],
        reached_by: "lint",
    },
    Surface {
        label: "shell script",
        paths: &["*.sh"],
        hooks: &["shellcheck"],
        reached_by: "lint-workflows",
    },
    Surface {
        // `zizmor`'s own `files:` is `^\.github/workflows/.*\.ya?ml$`, so this row and that
        // regex agree by construction rather than by coincidence.
        label: "workflow YAML",
        paths: &[".github/workflows/*.yml", ".github/workflows/*.yaml"],
        hooks: &["zizmor"],
        reached_by: "lint-workflows",
    },
    Surface {
        // NO HOOK, and that is the finding rather than an omission here. `actionlint` cannot read
        // a composite action at the pinned version, `zizmor` is pointed elsewhere, and a `run:`
        // block is not a `.sh` file - so this surface is invisible to every hook in the config.
        label: "composite-action shell",
        paths: &[".github/actions/*/action.yml", ".github/actions/*/action.yaml"],
        hooks: &[],
        reached_by: "lint-workflows",
    },
];

/// How this gate was invoked.
struct Invocation {
    /// The base ref whose diff against `HEAD` is the change under judgement.
    base: String,
    /// `(stage, log path)` for each prek run to read.
    logs: Vec<(String, String)>,
    /// Tasks that ran OUTSIDE prek, so a surface no hook claims can still be covered.
    ran: Vec<String>,
    /// Print the tasks this diff needs and stop. `ship-check` asks before it can judge.
    surface_tasks: bool,
}

/// `cargo xtask hook-coverage` - what a diff-scoped hook run left uninspected.
pub(crate) fn run(args: &[String]) -> Verdict {
    let invocation = match parse(args) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("xtask hook-coverage: {message}");
            eprintln!("  usage: hook-coverage --since <ref> [--surface-tasks]");
            eprintln!("                       [--log <stage>:<path>]... [--ran <task>]...");
            return Verdict::Usage;
        }
    };
    let Some(root) = repo::root() else {
        eprintln!("xtask hook-coverage: could not locate the repo root");
        return Verdict::Fail;
    };
    let Some(changed) = changes::changed_paths(&invocation.base) else {
        // FAIL CLOSED, unlike `classify` next door, and the direction is the opposite one on
        // purpose: that gate widens what RUNS when it cannot read a diff, and this one reports
        // what a run covered. An unreadable diff there costs a CI minute; here it would silently
        // declare every surface untouched and every gap absent.
        eprintln!("xtask hook-coverage: git could not diff against `{}`", invocation.base);
        eprintln!("  Without the changed file set there is nothing to say about coverage, and");
        eprintln!("  saying it anyway would report no gaps over a diff nobody read.");
        return Verdict::Fail;
    };

    let declared = match declared_hooks(&root) {
        Ok(declared) => declared,
        Err(message) => {
            eprintln!("xtask hook-coverage: {message}");
            return Verdict::Fail;
        }
    };

    if invocation.surface_tasks {
        for surface in touched(SURFACES, &changed) {
            if surface.hooks.is_empty() {
                println!("{}", surface.reached_by);
            }
        }
        return Verdict::Pass;
    }
    decide(&invocation, &declared, &changed)
}

/// Arguments, or the sentence to print instead.
fn parse(args: &[String]) -> Result<Invocation, String> {
    let mut base = None;
    let mut logs = Vec::new();
    let mut ran = Vec::new();
    let mut surface_tasks = false;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--surface-tasks" => surface_tasks = true,
            "--since" => base = Some(rest.next().ok_or("--since needs a git ref")?.clone()),
            "--ran" => ran.push(rest.next().ok_or("--ran needs a task name")?.clone()),
            "--log" => {
                let value = rest.next().ok_or("--log needs <stage>:<path>")?;
                let (stage, path) = value
                    .split_once(':')
                    .ok_or_else(|| format!("`{value}` is not <stage>:<path>"))?;
                logs.push((String::from(stage), String::from(path)));
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(Invocation {
        base: base.ok_or("--since is required")?,
        logs,
        ran,
        surface_tasks,
    })
}

/// Every hook the config declares, or the sentence saying why nothing could be read.
///
/// FAILS CLOSED on an empty parse, for `check-hook-tiers`' reason one file over: a reader that
/// stopped matching this file would otherwise report a run as fully covered by declaring nothing.
fn declared_hooks(root: &std::path::Path) -> Result<Vec<hooks::Hook>, String> {
    let text = std::fs::read_to_string(root.join(hooks::CONFIG))
        .map_err(|error| format!("could not read {}: {error}", hooks::CONFIG))?;
    let declared = hooks::hooks(&text);
    if declared.is_empty() {
        return Err(format!(
            "read no hooks at all out of {} - the reader stopped matching the file",
            hooks::CONFIG
        ));
    }
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    for hook in &declared {
        *names.entry(hook.name.as_str()).or_default() += 1;
    }
    // A NAME IS THE KEY, so a collision is a key that cannot answer - the shape the causality gate
    // learned when 23 duplicated test-function names satisfied both its filter and its comparison.
    let collided: Vec<&str> = names.iter().filter(|(_, count)| **count > 1).map(|(name, _)| *name).collect();
    if collided.is_empty() {
        Ok(declared)
    } else {
        Err(format!(
            "two hooks share a display name, so a row cannot be joined to one: {collided:?}"
        ))
    }
}

/// One hook's row in one stage's output.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    /// The display name prek printed.
    name: String,
    /// What its status column said.
    coverage: Coverage,
}

/// The hook rows in one prek run's output.
///
/// Anything that is not a row - a hook's own stdout, a notice, a blank line - yields nothing. The
/// padding is what makes a row recognisable: a status keyword preceded either by the dot run or by
/// a parenthesised reason that is itself preceded by one.
fn rows(text: &str) -> Vec<Row> {
    text.lines().filter_map(row).collect()
}

/// One line, if it is a hook row.
fn row(line: &str) -> Option<Row> {
    let line = line.trim_end();
    let (head, coverage) = STATUSES
        .iter()
        .find_map(|(status, coverage)| Some((line.strip_suffix(status)?, *coverage)))?;
    // A reason is `(...)` sitting directly on the dot padding. Requiring the dot is what tells it
    // from a hook whose NAME ends in a parenthesis - two of them do in this repository.
    let head = match head.strip_suffix(')').and_then(|inner| inner.rsplit_once('(')) {
        Some((before, _)) if before.ends_with('.') => before,
        _ => head,
    };
    let name = head.trim_end_matches('.');
    // At least one dot of padding, and a name left over. Both halves matter: without the dot every
    // line of a hook's own output is a candidate row, and a row that parses to an empty name would
    // join to nothing and read as a hook nobody declared.
    (name.len() < head.len() && !name.is_empty()).then(|| Row {
        name: String::from(name),
        coverage,
    })
}

/// The surfaces a change touches, in table order.
fn touched<'a>(surfaces: &'a [Surface], changed: &[String]) -> Vec<&'a Surface> {
    surfaces
        .iter()
        .filter(|surface| changed.iter().any(|path| repo::matches_any(surface.paths, path)))
        .collect()
}

/// One declared hook and what one stage's output said about it.
///
/// A named struct rather than a tuple, because `clippy::type_complexity` refuses the tuple - and it
/// is right to for `crate::shipped::Reconciliation`'s reason: three fields side by side say nothing
/// about which is the key, which is the display name and which is the verdict.
struct Inspected {
    /// The hook's id, which is what a reader edits in the config.
    id: String,
    /// Its display name, which is what prek printed.
    name: String,
    /// What that stage's output said.
    coverage: Coverage,
}

/// What one stage's run covered: the declared hooks for that stage, paired with what its output
/// said about each.
fn coverage_of(declared: &[hooks::Hook], stage: &str, rows: &[Row]) -> Vec<Inspected> {
    declared
        .iter()
        .filter(|hook| hook.runs_at(stage))
        .map(|hook| Inspected {
            id: hook.id.clone(),
            name: hook.name.clone(),
            coverage: rows
                .iter()
                .find(|row| row.name == hook.name)
                .map_or(Coverage::Unreported, |row| row.coverage),
        })
        .collect()
}

/// Every stage's verdict, then every touched surface's, then the line a reader quotes.
fn decide(invocation: &Invocation, declared: &[hooks::Hook], changed: &[String]) -> Verdict {
    let mut failures: Vec<String> = Vec::new();
    let mut inspected: Vec<String> = Vec::new();

    for (stage, path) in &invocation.logs {
        let Ok(text) = std::fs::read_to_string(path) else {
            failures.push(format!("could not read the {stage} log at {path}"));
            continue;
        };
        let read = rows(&text);
        if read.is_empty() {
            // A log with no parsable row is a reader that stopped matching prek's output, not a
            // stage with no hooks - and the first must not be reported as full coverage.
            failures.push(format!("no hook row parsed out of the {stage} log at {path}"));
            continue;
        }
        let per_hook = coverage_of(declared, stage, &read);
        if per_hook.is_empty() {
            failures.push(format!("no hook is declared at stage `{stage}`, so its log measures nothing"));
            continue;
        }
        report_stage(stage, &per_hook);
        for hook in &per_hook {
            if hook.coverage.inspected() {
                inspected.push(hook.id.clone());
            }
            if hook.coverage == Coverage::Unreported {
                failures.push(format!(
                    "`{}` is declared at `{stage}` and printed no row - SKIP / PREK_SKIP silences a hook by removing it",
                    hook.id
                ));
            }
        }
    }

    failures.extend(unknown_hook_ids(declared));
    failures.extend(surface_gaps(changed, &inspected, &invocation.ran));
    verdict(&failures, changed.len())
}

/// One stage's line: how many of its declared hooks ran, and the names of the ones that did not.
fn report_stage(stage: &str, per_hook: &[Inspected]) {
    let ran = per_hook.iter().filter(|hook| hook.coverage.inspected()).count();
    let filtered: Vec<&str> = per_hook
        .iter()
        .filter(|hook| hook.coverage == Coverage::NoMatchingFiles)
        .map(|hook| hook.name.as_str())
        .collect();
    let missing: Vec<&str> = per_hook
        .iter()
        .filter(|hook| hook.coverage == Coverage::Unreported)
        .map(|hook| hook.id.as_str())
        .collect();
    print!(
        "xtask hook-coverage: {stage} - {ran} of {} declared hook(s) ran",
        per_hook.len()
    );
    if !filtered.is_empty() {
        print!(", {} skipped (no matching files: {})", filtered.len(), filtered.join(", "));
    }
    if !missing.is_empty() {
        print!(", {} NOT REPORTED ({})", missing.len(), missing.join(", "));
    }
    println!();
}

/// Hook IDs [`SURFACES`] claims that the config does not declare.
///
/// The anti-rot half, and the only direction available: nothing here can re-implement a `files:`
/// regex, so what is held instead is that every ID this table leans on still exists. A renamed
/// hook would otherwise quietly empty a surface's claim and turn its row into a permanent gap or a
/// permanent pass, depending on which way the rename went.
fn unknown_hook_ids(declared: &[hooks::Hook]) -> Vec<String> {
    SURFACES
        .iter()
        .flat_map(|surface| surface.hooks.iter().map(move |id| (surface.label, *id)))
        .filter(|(_, id)| !declared.iter().any(|hook| hook.id == *id))
        .map(|(label, id)| {
            format!(
                "the `{label}` surface names hook `{id}`, which {} does not declare",
                hooks::CONFIG
            )
        })
        .collect()
}

/// Every touched surface, reported, and a sentence for each one that was not fully inspected.
///
/// **EVERY claiming hook, not any of them.** A surface's hooks are not interchangeable: for Rust
/// the set is the format check, clippy, `cargo check` and both test runs, and *one of them ran* is
/// precisely the sentence this whole module exists to stop being printed as coverage. `just <task>`
/// having run covers the surface whatever the hooks did, which is what stops the arm firing on a
/// tree somebody has already checked the other way.
fn surface_gaps(changed: &[String], inspected: &[String], ran: &[String]) -> Vec<String> {
    let mut gaps = Vec::new();
    for surface in touched(SURFACES, changed) {
        let by_task = ran.iter().any(|task| task == surface.reached_by);
        let missing: Vec<&str> = surface
            .hooks
            .iter()
            .filter(|id| !inspected.iter().any(|inspected| inspected == *id))
            .copied()
            .collect();
        let covered = by_task || (!surface.hooks.is_empty() && missing.is_empty());
        if covered {
            let how = if missing.is_empty() && !surface.hooks.is_empty() {
                format!("hook(s) {}", surface.hooks.join(", "))
            } else {
                format!("`just {}`", surface.reached_by)
            };
            println!("  surface `{}`: covered by {how}", surface.label);
            continue;
        }
        let what = if surface.hooks.is_empty() {
            String::from("no hook reaches it at all")
        } else {
            format!("these hooks did not run: {}", missing.join(", "))
        };
        println!("  surface `{}`: NOT COVERED - {what}", surface.label);
        gaps.push(format!(
            "the `{}` this diff touches was not fully inspected ({what}) - `just {}` is what reaches it",
            surface.label, surface.reached_by
        ));
    }
    gaps
}

/// The line a reader quotes, and the exit code behind it.
fn verdict(failures: &[String], changed: usize) -> Verdict {
    if failures.is_empty() {
        println!("xtask hook-coverage: ok - every surface these {changed} changed file(s) touch was inspected");
        return Verdict::Pass;
    }
    eprintln!(
        "xtask hook-coverage: FAILED - {} coverage gap(s) over {changed} changed file(s)",
        failures.len()
    );
    for failure in failures {
        eprintln!("  {failure}");
    }
    eprintln!();
    eprintln!("  A diff-scoped run is fast because it skips hooks no changed file matches, which is");
    eprintln!("  correct. What is not correct is calling that green without saying so: the summary");
    eprintln!("  above is the coverage, and each line names the task that closes its gap.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{Coverage, Row, Surface};

    /// prek's real commit-stage output on a diff of one README, captured from prek 0.4.14 with
    /// `--color never`. Byte-for-byte: the padding is what the row parser keys on, so a
    /// hand-tidied fixture would test a format prek does not print.
    const COMMIT_LOG: &str = concat!(
        "cargo fmt.............................................(no files to check)Skipped\n",
        "cargo clippy (-D warnings, all features, on stable)...(no files to check)Skipped\n",
        "structural gates.........................................................Dry Run\n",
        "cargo check (changed packages only)...................(no files to check)Skipped\n",
        "cargo nextest (all features).............................................Dry Run\n",
        "doctests.................................................................Dry Run\n",
        "CRAP score (complexity weighted by coverage)..........(no files to check)Skipped\n",
        "shell scripts.........................................(no files to check)Skipped\n",
        "detect hardcoded secrets (CI is authoritative)...........................Dry Run\n",
        "GitHub Actions static analysis........................(no files to check)Skipped\n",
    );

    #[test]
    fn a_skipped_hook_is_told_apart_from_one_that_ran() {
        // The whole defect in one assertion: five of these ten inspected nothing, and the number
        // was available the entire time.
        let rows = super::rows(COMMIT_LOG);
        assert_eq!(rows.len(), 10, "{rows:?}");
        assert_eq!(rows.iter().filter(|row| row.coverage.inspected()).count(), 5);
        assert_eq!(rows.iter().filter(|row| row.coverage == Coverage::NoMatchingFiles).count(), 5);
    }

    #[test]
    fn a_name_ending_in_a_parenthesis_is_not_read_as_a_reason() {
        // Two hooks in this repository are named that way, and the reason is recognised by the DOT
        // in front of it rather than by the bracket - without that, `cargo clippy (-D warnings,
        // all features, on stable)` would parse to `cargo clippy` and join to no declared hook.
        let ran = super::row("cargo clippy (-D warnings, all features, on stable)......................Passed");
        assert_eq!(
            ran,
            Some(Row {
                name: String::from("cargo clippy (-D warnings, all features, on stable)"),
                coverage: Coverage::Ran,
            })
        );
        let skipped = super::row("cargo clippy (-D warnings, all features, on stable)...(no files to check)Skipped");
        assert_eq!(
            skipped.map(|row| row.name).as_deref(),
            Some("cargo clippy (-D warnings, all features, on stable)")
        );
    }

    #[test]
    fn a_line_that_is_not_a_row_yields_nothing() {
        // A hook's own output sits between the rows, and a false row would join to no declared
        // hook - or worse, to one, and report coverage the run never had.
        assert_eq!(super::row("error: something Failed"), None);
        assert_eq!(super::row("- files were modified by this hook"), None);
        assert_eq!(super::row(""), None);
        // No padding at all is not a row either. Fail-closed: the hook it would have named reads
        // as unreported, which is a verdict rather than a silent pass.
        assert_eq!(super::row("cargo fmtPassed"), None);
    }

    #[test]
    fn a_hook_silenced_from_the_environment_is_a_failure_and_not_a_smaller_denominator() {
        // MEASURED on prek 0.4.14: `PREK_SKIP=hygiene,rust-tests` removes the two rows outright.
        // So the denominator has to come from the config, and the missing rows have to be named.
        let declared = declared();
        let silenced = COMMIT_LOG.replace(
            "structural gates.........................................................Dry Run\n",
            "",
        );
        let rows = super::rows(&silenced);
        assert_eq!(rows.len(), 9);
        let per_hook = super::coverage_of(&declared, super::hooks::COMMIT, &rows);
        let hygiene = per_hook.iter().find(|hook| hook.id == "hygiene").expect("the hygiene hook");
        assert_eq!(hygiene.coverage, Coverage::Unreported);
        // And with the row present it is Ran, which is the direction that proves the arm above is
        // about the silencing rather than about the join failing everywhere.
        let full = super::coverage_of(&declared, super::hooks::COMMIT, &super::rows(COMMIT_LOG));
        let hygiene = full.iter().find(|hook| hook.id == "hygiene").expect("the hygiene hook");
        assert_eq!(hygiene.coverage, Coverage::Ran);
    }

    #[test]
    fn the_surface_no_hook_reaches_is_reported_and_the_others_are_not() {
        // The sharp case: a composite action's inline shell is invisible to every hook in the
        // config, so a diff touching one always has a gap until the covering task has run.
        let changed = vec![String::from(".github/actions/build-artefacts/action.yml")];
        let gaps = super::surface_gaps(&changed, &[], &[]);
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        assert!(
            gaps.first().is_some_and(|gap| gap.contains("just lint-workflows")),
            "{gaps:?}"
        );
        // Named as having run, the same diff has no gap - which is what stops this being a gate
        // that fails a tree somebody has already checked.
        assert!(super::surface_gaps(&changed, &[], &[String::from("lint-workflows")]).is_empty());
        // A Rust diff is covered when EVERY hook claiming Rust ran, and not before: `one of them
        // ran` is the sentence this module exists to stop being printed as coverage.
        let rust = vec![String::from("crates/sutura-domain/src/lib.rs")];
        let all: Vec<String> = super::SURFACES
            .iter()
            .find(|surface| surface.label == "Rust source")
            .expect("the Rust surface")
            .hooks
            .iter()
            .map(|id| String::from(*id))
            .collect();
        assert!(super::surface_gaps(&rust, &all, &[]).is_empty());
        // Clippy alone is a gap, and the gap NAMES the four that did not run.
        let partial = super::surface_gaps(&rust, &[String::from("rust-clippy")], &[]);
        assert_eq!(partial.len(), 1, "{partial:?}");
        assert!(partial.first().is_some_and(|gap| gap.contains("rust-fmt")), "{partial:?}");
        // And with nothing at all, which is the state the measured run on a workflow-only branch
        // was in for five of its ten hooks.
        assert_eq!(super::surface_gaps(&rust, &[], &[]).len(), 1);
    }

    /// The surface table after a hook rename nobody carried here.
    const GONE: &[Surface] = &[Surface {
        label: "Rust source",
        paths: &["*.rs"],
        hooks: &["rust-clippy-renamed"],
        reached_by: "lint",
    }];

    #[test]
    fn a_surface_naming_a_hook_the_config_does_not_declare_fails() {
        // The anti-rot half. Nothing here can read a `files:` regex, so what is held is that the
        // IDs this table leans on still exist - a rename would otherwise empty a claim in silence.
        assert!(super::unknown_hook_ids(&declared()).is_empty());
        let declared = declared();
        let unknown: Vec<String> = GONE
            .iter()
            .flat_map(|surface| surface.hooks.iter().map(move |id| (surface.label, *id)))
            .filter(|(_, id)| !declared.iter().any(|hook| hook.id == *id))
            .map(|(label, id)| format!("{label} names {id}"))
            .collect();
        assert_eq!(unknown.len(), 1, "{unknown:?}");
    }

    #[test]
    fn every_surface_the_real_config_claims_still_exists() {
        // Over the REAL file, because the fixtures above prove the reader and not the tree. This is
        // the assertion that reddens when a hook is renamed in `.pre-commit-config.yaml`.
        assert!(super::unknown_hook_ids(&declared()).is_empty());
        // And the composite-action row still claims nothing, because the day a hook covers it this
        // module's header stops being true.
        let uncovered: Vec<&str> = super::SURFACES
            .iter()
            .filter(|s| s.hooks.is_empty())
            .map(|s| s.label)
            .collect();
        assert_eq!(uncovered, vec!["composite-action shell"]);
    }

    /// The real hook config, parsed by the one parser of it.
    fn declared() -> Vec<super::hooks::Hook> {
        let root = crate::repo::root().expect("the repo root");
        let text = std::fs::read_to_string(root.join(super::hooks::CONFIG)).expect("the hook config");
        let declared = super::hooks::hooks(&text);
        assert!(!declared.is_empty());
        declared
    }
}

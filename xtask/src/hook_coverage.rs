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
//! the filter never reaching it. `just lint-workflows` is the task that does. [`SURFACES`]
//! carries that, and `--surface-tasks` is how `ship-check` learns which extra task a diff needs
//! BEFORE it can be judged - so the gap is closed rather than described.
//!
//! **There are TWO such surfaces, and this sentence was false for one release.** The shell inside
//! `devenv.nix`'s script bodies is a Nix string, so every row above filters it out the same way.
//! Its first row claimed `hygiene`, which **could never report a gap** - that hook is
//! `always_run: true`, so the coverage line printed whatever the diff was
//! (`github.com/telekom/sutura#402`). Both rows declare an EMPTY hook set now, and
//! `every_surface_the_real_config_claims_still_exists` holds both that pair and the general form:
//! no row may claim an `always_run` hook.
//!
//! # Three ways a row said more than it knew, and all three printed a full house
//!
//! Every one of these produced output a reader could not tell from a real run, which is this
//! module's own subject turned on itself.
//!
//! * **A stage handed no `--log` was neither measured nor mentioned.** The loop was over the logs
//!   it was GIVEN, so `hook-coverage --since HEAD` printed `ok` having read no prek output at all.
//!   The stage denominator is derived from the config now, the way the hook denominator already
//!   was: see [`MEASURED_STAGES`].
//! * **`Dry Run` counted as *inspected this diff*.** Measured: both stage logs captured with
//!   `prek run --dry-run` printed `pre-commit - 8 of 10 declared hook(s) ran`, `pre-push - 4 of 4`,
//!   every surface covered and `ok`, exit 0 - **character for character** the lines this branch's
//!   real `ship-check` run printed, with not one hook having executed. It is its own
//!   [`Coverage::DryRun`] and a FAILED verdict now: a dry run is neither *ran* nor *filtered out*.
//! * **A hook that decides for itself not to run prints `Passed`.** Eight of the fifteen do.
//!   [`abstain`] carries that, the measurement, and why the notice cannot be the mechanism.
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

use std::collections::{BTreeMap, BTreeSet};

use crate::Verdict;
use crate::changes;
use crate::hooks;
use crate::repo;

mod abstain;

/// What a declared hook contributed to one run.
///
/// Three states rather than a bool, for the reason `check-venues` splits `can` from `unrun`: *ran*
/// and *was filtered out* are both honest outcomes a reader can act on, and *never reported* is
/// neither - it is the state a count of output rows cannot see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coverage {
    /// A row said `Passed` or `Failed` and the hook could have run here: it inspected this diff.
    Ran,
    /// A row said `Skipped`: prek's `files:` filter matched nothing in the diff.
    NoMatchingFiles,
    /// Declared for this stage and ABSENT from the output. `SKIP` / `PREK_SKIP` removes the row
    /// entirely - measured, see the module header - so this is the silencing a row count misses.
    Unreported,
    /// A row said `Dry Run`: prek listed the hook and executed nothing. **Neither *ran* nor
    /// *filtered out*** - a third thing, and the one that printed a full house over a run in which
    /// nothing happened.
    DryRun,
    /// A row said `Passed` and the hook's own shell could not have reached its tool on this host,
    /// so what it printed was its skip notice. See [`abstain`].
    SelfSkipped,
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
    ("Dry Run", Coverage::DryRun),
    ("Skipped", Coverage::NoMatchingFiles),
];

/// The stages a diff-scoped prek run covers, each of which needs a `--log`.
///
/// **DERIVED IN BOTH DIRECTIONS, which is the half that was missing.** A stage named here with no
/// log handed to this run is a FAILED verdict, and a stage the config declares that is in neither
/// this list nor [`UNMEASURED_STAGES`] is one too - so a new tier is a decision rather than a
/// silence. The hook denominator already worked this way; the stage list did not exist at all.
const MEASURED_STAGES: &[&str] = &[hooks::COMMIT, hooks::PUSH];

/// Stages a diff-scoped run cannot be asked about, with the reason each is out of scope.
///
/// `commit-msg` inspects the commit MESSAGE. It reads no file, so it covers no surface and a log
/// of it would measure nothing about a diff - and `ship-check` judges a committed range rather
/// than writing a commit, so there is no message for it to run against.
const UNMEASURED_STAGES: &[&str] = &["commit-msg"];

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
    Surface {
        // The same shape one file over, and it had no row at all: a 74-line change to `devenv.nix`
        // used to produce a surface list that said nothing about it. The shell in this file is the
        // `scripts.<name>.exec` bodies and `enterShell`, none of which is a tracked `*.sh` file, a
        // workflow or a composite action - so every row above filters it out.
        //
        // `hooks` IS EMPTY, and it was `["hygiene"]` for one release - a claim that could never
        // report a gap, because that hook is `always_run: true`. No hook's `files:` filter reaches
        // `devenv.nix` at all, so empty is the honest value.
        //
        // It does NOT mean nothing checks this file: `check-devenv-shell` runs inside `hygiene` on
        // every commit and every pull request. What no hook reaches is the LINTER - ShellCheck
        // runs when the dev shell is BUILT, and nothing a diff-scoped run invokes builds one - so
        // `reached_by` is that task and `just ship-check` runs it rather than describing the gap.
        label: "devenv script shell",
        paths: &["devenv.nix"],
        hooks: &[],
        reached_by: "devenv-linter",
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
    decide(&root, &invocation, &declared, &changed)
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
#[derive(Debug)]
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
fn coverage_of(declared: &[hooks::Hook], stage: &str, rows: &[Row], unavailable: &BTreeSet<String>) -> Vec<Inspected> {
    declared
        .iter()
        .filter(|hook| hook.runs_at(stage))
        .map(|hook| {
            let reported = rows
                .iter()
                .find(|row| row.name == hook.name)
                .map_or(Coverage::Unreported, |row| row.coverage);
            // A HOOK THAT COULD NOT HAVE RUN HERE DID NOT INSPECT THE DIFF, whatever its row said.
            // Downgrading rather than trusting the column is the whole of the eight-hook finding:
            // a self-skip exits 0 and prek prints `Passed`.
            let coverage = if reported.inspected() && unavailable.contains(&hook.id) {
                Coverage::SelfSkipped
            } else {
                reported
            };
            Inspected {
                id: hook.id.clone(),
                name: hook.name.clone(),
                coverage,
            }
        })
        .collect()
}

/// Every stage's verdict, then every touched surface's, then the line a reader quotes.
fn decide(root: &std::path::Path, invocation: &Invocation, declared: &[hooks::Hook], changed: &[String]) -> Verdict {
    let mut failures: Vec<String> = Vec::new();
    let mut inspected: Vec<String> = Vec::new();

    let abstentions = match abstain::over(root, declared) {
        Ok(abstentions) => abstentions,
        Err(message) => {
            eprintln!("xtask hook-coverage: {message}");
            return Verdict::Fail;
        }
    };
    failures.extend(abstentions.unreadable.iter().cloned());
    if abstentions.deciding == 0 {
        // FAIL CLOSED, and this rule's own floor: no declared hook decides for itself whether to
        // run, so every pass reads as coverage again and nothing here would ever fire.
        failures.push(String::from(
            "no declared hook was read as deciding for itself whether to run - the authorities for that are the hook entries and nix/run-gate.sh, and this reader matched neither",
        ));
    }
    failures.extend(unmeasured_stages(declared, &invocation.logs));

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
        let per_hook = coverage_of(declared, stage, &read, &abstentions.unavailable);
        if per_hook.is_empty() {
            failures.push(format!("no hook is declared at stage `{stage}`, so its log measures nothing"));
            continue;
        }
        report_stage(stage, &per_hook);
        for hook in &per_hook {
            if hook.coverage.inspected() {
                inspected.push(hook.id.clone());
            }
            failures.extend(why_it_measured_nothing(stage, hook));
        }
    }

    failures.extend(unknown_hook_ids(declared));
    let (reported, gaps) = surface_gaps(changed, &inspected, &invocation.ran);
    failures.extend(gaps);
    verdict(&failures, reported, changed.len())
}

/// Stages the config declares that this run was handed nothing for, or has not classified at all.
///
/// **The *cannot look at all* half.** An unreadable log fails and an unparsable log fails; a stage
/// with no log was neither measured nor mentioned, so `hook-coverage --since HEAD` printed `ok`
/// having read no prek output. The denominator is the config either way, which is the rule the
/// hook count already followed one level down.
fn unmeasured_stages(declared: &[hooks::Hook], logs: &[(String, String)]) -> Vec<String> {
    let mut problems = Vec::new();
    for stage in MEASURED_STAGES {
        if !declared.iter().any(|hook| hook.runs_at(stage)) {
            problems.push(format!(
                "no hook is declared at stage `{stage}`, which this task measures - the reader stopped matching {}, or the tier is gone and this list did not move",
                hooks::CONFIG
            ));
            continue;
        }
        if !logs.iter().any(|(given, _)| given == *stage) {
            problems.push(format!(
                "stage `{stage}` was handed no --log, so nothing was read about the hooks declared there - a stage nobody measured is not a stage with no gaps"
            ));
        }
    }
    for stage in declared.iter().flat_map(|hook| hook.stages.iter()) {
        if !MEASURED_STAGES.contains(&stage.as_str()) && !UNMEASURED_STAGES.contains(&stage.as_str()) {
            problems.push(format!(
                "{} declares a hook at stage `{stage}`, and this task classifies it as neither measured nor out of scope - say which",
                hooks::CONFIG
            ));
        }
    }
    problems.sort();
    problems.dedup();
    problems
}

/// The sentence for one hook whose row was not coverage, if there is one.
fn why_it_measured_nothing(stage: &str, hook: &Inspected) -> Option<String> {
    match hook.coverage {
        Coverage::Ran | Coverage::NoMatchingFiles => None,
        Coverage::Unreported => Some(format!(
            "`{}` is declared at `{stage}` and printed no row - the environment can silence a hook by removing it",
            hook.id
        )),
        Coverage::DryRun => Some(format!(
            "`{}` reported `Dry Run` at `{stage}` - prek listed it and executed nothing, so this log measures no coverage at all",
            hook.id
        )),
        Coverage::SelfSkipped => Some(format!(
            "`{}` reported a pass at `{stage}` and could not have run on this host - its own shell falls through to a skip notice and exits 0, which prek prints as a pass",
            hook.id
        )),
    }
}

/// One stage's line: how many of its declared hooks ran, and the names of the ones that did not.
///
/// **Every state gets a clause**, which is what makes a real run's line different text from a
/// hollow run's. Before, `Dry Run` and a self-skip both landed in the `ran` count, so a
/// `--dry-run` capture printed the identical sentence.
fn report_stage(stage: &str, per_hook: &[Inspected]) {
    let ran = per_hook.iter().filter(|hook| hook.coverage.inspected()).count();
    print!(
        "xtask hook-coverage: {stage} - {ran} of {} declared hook(s) ran",
        per_hook.len()
    );
    let labels = |state: Coverage| -> Vec<&str> {
        per_hook
            .iter()
            .filter(|hook| hook.coverage == state)
            .map(|hook| hook.name.as_str())
            .collect()
    };
    for (state, clause) in [
        (Coverage::NoMatchingFiles, "skipped (no matching files"),
        (Coverage::Unreported, "NOT REPORTED (silenced from the environment"),
        (Coverage::DryRun, "MEASURED NOTHING (dry run"),
        (Coverage::SelfSkipped, "COULD NOT RUN HERE (self-skipped on a missing tool"),
    ] {
        let named = labels(state);
        if !named.is_empty() {
            print!(", {} {clause}: {})", named.len(), named.join(", "));
        }
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
fn surface_gaps(changed: &[String], inspected: &[String], ran: &[String]) -> (usize, Vec<String>) {
    let mut gaps = Vec::new();
    let mut reported = 0_usize;
    for surface in touched(SURFACES, changed) {
        reported = reported.saturating_add(1);
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
    (reported, gaps)
}

/// The line a reader quotes, and the exit code behind it.
///
/// `reported` is the number of surfaces this diff touched, and it is in the sentence for
/// `check-venues`' reason one gate over: *every surface* over an empty set is a claim about
/// nothing, and a reader has no way to tell four from zero without the number.
fn verdict(failures: &[String], reported: usize, changed: usize) -> Verdict {
    if failures.is_empty() {
        println!("xtask hook-coverage: ok - {reported} surface(s) these {changed} changed file(s) touch, each inspected");
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

    /// prek 0.4.14's **`--dry-run`** commit-stage output over a diff of one README, `--color
    /// never`. Captured 2026-09-05 and pasted verbatim.
    ///
    /// **Its doc used to call this prek's real output, and it is not** - reported in review and
    /// re-measured here: `prek run --color never --dry-run --files README.md` reproduces all ten
    /// rows byte for byte. The correction matters because a dry run is the state
    /// [`Coverage::DryRun`] exists for, so this fixture is the RED arm for that rule rather than
    /// evidence about a real run. [`REAL_COMMIT_LOG`] is the real one.
    ///
    /// Byte-for-byte either way: the padding is what the row parser keys on, so a hand-tidied
    /// fixture would test a format prek does not print.
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

    /// prek 0.4.14's REAL commit-stage output, `--color never`, over a diff of one README on a
    /// host with nix - so `structural gates`, the suite, the doctests and the secret scan actually
    /// executed. Captured on 2026-09-05, exit 0, and pasted verbatim.
    ///
    /// The point of having both is that they are told apart. The two captures are the same ten
    /// rows in the same order over the same diff: six identical, and four differing in the status
    /// column alone - plus the one dot of padding that column's width costs. That is exactly why
    /// the column is not evidence on its own. Over this one the verdict is a pass; over
    /// [`COMMIT_LOG`] it is a failure naming every dry-run row. Before, the two produced
    /// character-for-character the same lines.
    const REAL_COMMIT_LOG: &str = concat!(
        "cargo fmt.............................................(no files to check)Skipped\n",
        "cargo clippy (-D warnings, all features, on stable)...(no files to check)Skipped\n",
        "structural gates..........................................................Passed\n",
        "cargo check (changed packages only)...................(no files to check)Skipped\n",
        "cargo nextest (all features)..............................................Passed\n",
        "doctests..................................................................Passed\n",
        "CRAP score (complexity weighted by coverage)..........(no files to check)Skipped\n",
        "shell scripts.........................................(no files to check)Skipped\n",
        "detect hardcoded secrets (CI is authoritative)............................Passed\n",
        "GitHub Actions static analysis........................(no files to check)Skipped\n",
    );

    #[test]
    fn a_dry_run_log_and_a_real_one_do_not_produce_the_same_verdict() {
        // THE HEADLINE DEFECT. Measured in review on the version before this: both stage logs
        // captured with `prek run --dry-run` printed `pre-commit - 8 of 10 declared hook(s) ran`,
        // every surface covered and `ok`, exit 0 - character for character the lines the real
        // `ship-check` run printed, with not one hook having executed.
        let declared = declared();
        let none = super::BTreeSet::new();
        let dry = super::coverage_of(&declared, super::hooks::COMMIT, &super::rows(COMMIT_LOG), &none);
        // FOUR rows measured nothing, and none of them is counted as having run.
        assert_eq!(
            dry.iter().filter(|hook| hook.coverage == Coverage::DryRun).count(),
            4,
            "{dry:?}"
        );
        assert_eq!(dry.iter().filter(|hook| hook.coverage.inspected()).count(), 0, "{dry:?}");
        // Each one is a FAILED sentence rather than a smaller denominator nobody can attribute.
        let refusals: Vec<String> = dry
            .iter()
            .filter_map(|hook| super::why_it_measured_nothing(super::hooks::COMMIT, hook))
            .collect();
        assert_eq!(refusals.len(), 4, "{refusals:?}");
        assert!(refusals.iter().all(|line| line.contains("Dry Run")), "{refusals:?}");
        // And the real capture of the SAME diff, on which those four did run: no refusal at all.
        let real = super::coverage_of(&declared, super::hooks::COMMIT, &super::rows(REAL_COMMIT_LOG), &none);
        assert_eq!(real.iter().filter(|hook| hook.coverage.inspected()).count(), 4, "{real:?}");
        assert!(
            real.iter()
                .all(|hook| super::why_it_measured_nothing(super::hooks::COMMIT, hook).is_none()),
            "{real:?}"
        );
    }

    #[test]
    fn a_hook_that_could_not_have_run_here_is_not_coverage_whatever_its_row_said() {
        // MEASURED: the `shellcheck` entry verbatim with `nix` off PATH prints its notice and
        // exits 0, so prek prints a pass. Eight of the fifteen declared hooks are written that
        // way, three of the four on push - so on a host with no nix the verdict was
        // `pre-push - 4 of 4 declared hook(s) ran` over hooks that announced their own skip.
        let declared = declared();
        let rows = super::rows(REAL_COMMIT_LOG);
        let unavailable = super::BTreeSet::from([String::from("secret-sweep"), String::from("betterleaks")]);
        let per_hook = super::coverage_of(&declared, super::hooks::COMMIT, &rows, &unavailable);
        let leaks = per_hook
            .iter()
            .find(|hook| hook.id == "betterleaks")
            .expect("the betterleaks hook");
        assert_eq!(leaks.coverage, Coverage::SelfSkipped);
        assert!(!leaks.coverage.inspected());
        assert!(
            super::why_it_measured_nothing(super::hooks::COMMIT, leaks)
                .is_some_and(|line| line.contains("could not have run on this host")),
            "{leaks:?}"
        );
        // AND THE ARM THAT STILL FIRES: with the tool present the same row is coverage, which is
        // what keeps this from failing every host that has nix.
        let none = super::BTreeSet::new();
        let present = super::coverage_of(&declared, super::hooks::COMMIT, &rows, &none);
        let leaks = present
            .iter()
            .find(|hook| hook.id == "betterleaks")
            .expect("the betterleaks hook");
        assert_eq!(leaks.coverage, Coverage::Ran);
    }

    #[test]
    fn a_stage_handed_no_log_is_a_failure_rather_than_a_silence() {
        // Measured in review: `hook-coverage --since HEAD` printed
        // `ok - every surface these 0 changed file(s) touch was inspected`, exit 0, having read no
        // prek output at all. An unreadable log failed and an unparsable log failed; a stage nobody
        // handed over was neither measured nor mentioned.
        let declared = declared();
        let nothing = super::unmeasured_stages(&declared, &[]);
        assert_eq!(nothing.len(), 2, "{nothing:?}");
        assert!(
            nothing.iter().any(|line| line.contains("`pre-commit` was handed no --log")),
            "{nothing:?}"
        );
        assert!(
            nothing.iter().any(|line| line.contains("`pre-push` was handed no --log")),
            "{nothing:?}"
        );
        // One log is still half a measurement, and the half is named.
        let one = super::unmeasured_stages(&declared, &[(String::from(super::hooks::COMMIT), String::from("/dev/null"))]);
        assert_eq!(one.len(), 1, "{one:?}");
        // AND THE ARM THAT STILL FIRES: both handed over, no complaint.
        let both = super::unmeasured_stages(
            &declared,
            &[
                (String::from(super::hooks::COMMIT), String::from("/dev/null")),
                (String::from(super::hooks::PUSH), String::from("/dev/null")),
            ],
        );
        assert!(both.is_empty(), "{both:?}");
    }

    #[test]
    fn a_stage_the_config_declares_and_this_task_has_not_classified_fails() {
        // The other direction, so a new tier is a decision rather than a silence. `commit-msg` is
        // classified out of scope and must not be reported; an unclassified one must.
        let declared = declared();
        let logs = [
            (String::from(super::hooks::COMMIT), String::from("/dev/null")),
            (String::from(super::hooks::PUSH), String::from("/dev/null")),
        ];
        assert!(super::unmeasured_stages(&declared, &logs).is_empty());
        let text = concat!(
            "default_stages: [pre-commit]\n",
            "      - id: a\n",
            "        name: a\n",
            "        entry: cargo clippy\n",
            "      - id: b\n",
            "        name: b\n",
            "        entry: cargo clippy\n",
            "        stages: [pre-push]\n",
            "      - id: c\n",
            "        name: c\n",
            "        entry: cargo clippy\n",
            "        stages: [post-checkout]\n",
        );
        let added = super::unmeasured_stages(&super::hooks::hooks(text), &logs);
        assert_eq!(added.len(), 1, "{added:?}");
        assert!(added.iter().any(|line| line.contains("`post-checkout`")), "{added:?}");
    }

    #[test]
    fn a_skipped_hook_is_told_apart_from_one_that_ran() {
        // The whole defect in one assertion: SIX of these ten inspected nothing, and the number
        // was available the entire time.
        //
        // Six rather than the five the report measured, and the difference is the point: that run
        // touched a workflow file so the Actions analysis ran, and this capture touched only a
        // README so it did not. The counts are a property of the DIFF - which is exactly why a
        // reader cannot infer them and the verdict has to print them.
        let rows = super::rows(REAL_COMMIT_LOG);
        assert_eq!(rows.len(), 10, "{rows:?}");
        assert_eq!(rows.iter().filter(|row| row.coverage.inspected()).count(), 4);
        assert_eq!(rows.iter().filter(|row| row.coverage == Coverage::NoMatchingFiles).count(), 6);
        // Over the DRY-RUN capture of the same diff the four are `DryRun` instead, which is the
        // distinction the row parser has to carry for the verdict to be able to make it.
        let dry = super::rows(COMMIT_LOG);
        assert_eq!(dry.iter().filter(|row| row.coverage.inspected()).count(), 0);
        assert_eq!(dry.iter().filter(|row| row.coverage == Coverage::DryRun).count(), 4);
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
        let none = super::BTreeSet::new();
        let silenced = REAL_COMMIT_LOG.replace(
            "structural gates..........................................................Passed\n",
            "",
        );
        let rows = super::rows(&silenced);
        assert_eq!(rows.len(), 9);
        let per_hook = super::coverage_of(&declared, super::hooks::COMMIT, &rows, &none);
        let hygiene = per_hook.iter().find(|hook| hook.id == "hygiene").expect("the hygiene hook");
        assert_eq!(hygiene.coverage, Coverage::Unreported);
        // And with the row present it is Ran, which is the direction that proves the arm above is
        // about the silencing rather than about the join failing everywhere.
        let full = super::coverage_of(&declared, super::hooks::COMMIT, &super::rows(REAL_COMMIT_LOG), &none);
        let hygiene = full.iter().find(|hook| hook.id == "hygiene").expect("the hygiene hook");
        assert_eq!(hygiene.coverage, Coverage::Ran);
    }

    #[test]
    fn the_surface_no_hook_reaches_is_reported_and_the_others_are_not() {
        // The sharp case: a composite action's inline shell is invisible to every hook in the
        // config, so a diff touching one always has a gap until the covering task has run.
        let changed = vec![String::from(".github/actions/build-artefacts/action.yml")];
        let (reported, gaps) = super::surface_gaps(&changed, &[], &[]);
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        // The COUNT is in the verdict for `check-venues`' reason: `every surface` over an empty set
        // is a claim about nothing, and a reader cannot tell four from zero without it.
        assert_eq!(reported, 1);
        assert_eq!(super::surface_gaps(&[], &[], &[]).0, 0);
        assert!(
            gaps.first().is_some_and(|gap| gap.contains("just lint-workflows")),
            "{gaps:?}"
        );
        // Named as having run, the same diff has no gap - which is what stops this being a gate
        // that fails a tree somebody has already checked.
        assert!(
            super::surface_gaps(&changed, &[], &[String::from("lint-workflows")])
                .1
                .is_empty()
        );
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
        assert!(super::surface_gaps(&rust, &all, &[]).1.is_empty());
        // Clippy alone is a gap, and the gap NAMES the four that did not run.
        let (_, partial) = super::surface_gaps(&rust, &[String::from("rust-clippy")], &[]);
        assert_eq!(partial.len(), 1, "{partial:?}");
        assert!(partial.first().is_some_and(|gap| gap.contains("rust-fmt")), "{partial:?}");
        // And with nothing at all, which is the state the measured run on a workflow-only branch
        // was in for five of its ten hooks.
        assert_eq!(super::surface_gaps(&rust, &[], &[]).1.len(), 1);
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
        // And the two rows that claim nothing are exactly the two the header says there are. The
        // day a hook covers one of them this assertion is what says the header stopped being true
        // - which is the direction that matters, because a row claiming a hook that cannot report
        // a gap is what `github.com/telekom/sutura#402` measured on the second of these.
        let uncovered: Vec<&str> = super::SURFACES
            .iter()
            .filter(|s| s.hooks.is_empty())
            .map(|s| s.label)
            .collect();
        assert_eq!(uncovered, vec!["composite-action shell", "devenv script shell"]);
        // AND NO ROW MAY CLAIM AN `always_run` HOOK - the general form of that defect: such a hook
        // runs whatever the diff contains, so naming it is a claim nothing can falsify. Derived
        // from the config, and `hygiene` is asserted to BE one, or this rule is over an empty set.
        let unconditional: Vec<String> = declared()
            .iter()
            .filter(|hook| hook.always_run)
            .map(|hook| hook.id.clone())
            .collect();
        assert!(unconditional.iter().any(|id| id == "hygiene"), "{unconditional:?}");
        for surface in super::SURFACES {
            for id in surface.hooks {
                assert!(
                    !unconditional.iter().any(|declared| declared == id),
                    "surface `{}` claims `{id}`, which runs on every diff and so can never report \
                     a gap",
                    surface.label
                );
            }
        }
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

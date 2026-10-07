//! The fuzz harness's own gate: is every target declared, seeded, and actually run?
//!
//! **Why this exists rather than a sentence somewhere.** Issue #146 measured it: a sweep over the
//! tree found no fuzz target at all, so *nothing would notice one arriving or disappearing*. A
//! `fuzz_targets/*.rs` with no `[[bin]]` entry compiles for nobody; a `[[bin]]` the workflow's
//! matrix does not name is a target CI never runs; a target with no tracked seed starts from
//! nothing every time and cannot replay a crash somebody already fixed. All three leave every other
//! check green, which is the definition of coverage that is not there.
//!
//! **Static, and cheap on purpose.** It compiles nothing and runs no fuzzer, so it belongs in the
//! hygiene sweep on every pull request - unlike fuzzing itself, which `.github/workflows/fuzz.yml`
//! argues has to stay off the merge path.
//!
//! **And it refuses the fuzzer inside the RELEASE workflow**, which is the same argument one venue
//! over: fuzzing runs on the release tag now, and a job that ran it inside `release.yml` would be
//! one `needs:` away from letting a fresh random finding block every release. See
//! [`RELEASE_WORKFLOW`] for why that is worse than the cost it would save, and
//! [`FUZZ_INVOCATIONS`] for what the refusal cannot see.
//!
//! **Fails closed.** No target directory, no manifest, no matrix block and an empty target list are
//! all failures rather than a pass over silence.
//!
//! **A dictionary libFuzzer refuses is a run that executes nothing.** `-dict=` is validated at
//! startup and an unparseable line is fatal: libFuzzer prints `ParseDictionaryFile: error in line
//! N` and exits 1 - but only AFTER the release build the job already paid for, so the leg looks
//! like a fuzz run, costs like one, and mutates zero inputs. Nothing else in the tree reads these
//! files, so the only witness was a log line. Measured against a libFuzzer binary rather than read
//! off its source: a name is optional, the value must be double-quoted, `\\`, `\"` and `\xAB` are
//! the only escapes, an EMPTY value is refused, and a trailing `# comment` after the closing quote
//! is refused too - a `#` only starts a comment at the head of a line.
//!
//! **The limit, next to the claim.** All three readers are line-oriented over surface syntax - a
//! TOML table header, a YAML sequence and a dictionary entry - rather than a parse into a document
//! model, because `xtask` carries neither a TOML nor a YAML dependency and runs inside a nix
//! sandbox. So a `[[bin]]` spelled as an inline table, or a matrix in flow style, is invisible to
//! this gate. It holds the shapes this repository writes, and the unit tests below fix those
//! shapes. The dictionary reader checks the line shape and the escapes, not the token libFuzzer
//! builds from them, and it says nothing about whether a dictionary is USEFUL to its target. It
//! does not read what a target's code DOES - that a target reaches the parser it claims is a
//! review question, and each target's header is where the claim and its limits are written.

use std::collections::BTreeSet;

mod deny_wiring;
mod hook_paths;
mod lock_drift;

use crate::Verdict;
use crate::repo;

/// The fuzz crate's manifest, relative to the repository root.
const MANIFEST: &str = "fuzz/Cargo.toml";

/// Where the target sources live.
const TARGETS_DIR: &str = "fuzz/fuzz_targets";

/// Where the TRACKED seeds live, one directory per target.
///
/// Not `fuzz/corpus/`, which is libFuzzer's writable working corpus and is gitignored - see
/// `fuzz/.gitignore` for why the two are separate directories rather than one.
const SEEDS_DIR: &str = "fuzz/seeds";

/// Where the OPTIONAL libFuzzer dictionaries live, one `<target>.dict` per target that has one.
///
/// Optional on purpose - `nix/run-fuzz.sh` drops `-dict=` when the file is absent, so a target
/// without one runs without one. Present and unparseable is the case this gate exists for.
const DICTIONARIES_DIR: &str = "fuzz/dictionaries";

/// The workflow whose matrix decides which targets CI spends a budget on.
const WORKFLOW: &str = ".github/workflows/fuzz.yml";

/// The workflow that publishes a release, held against the fuzzer never running INSIDE it.
///
/// **Why this is a refusal and not a sentence in a header.** Fuzzing is unbounded by nature: a
/// target that finds nothing today finds something on a path nobody has generated yet, and
/// `sql_expression` has produced a real defect on runs before now. A fuzz job inside this file is
/// one `needs:` away from deciding whether a tag may ship, and at that point the first fresh
/// finding blocks every release until somebody fixes it - which is a far worse outcome than the
/// cost of running the fuzzer where it cannot. So [`WORKFLOW`] runs on the same `v*` tag as its own
/// SEPARATE run: no job in one workflow can depend on another workflow's run, so the structure
/// carries the property rather than a reviewer remembering it.
const RELEASE_WORKFLOW: &str = ".github/workflows/release.yml";

/// The three ways the fuzzer is invoked in this repository: the runner script, the flake app, and
/// `cargo-fuzz` itself.
///
/// **A NAME REFUSAL AND NOTHING MORE**, and the limits matter more than the list. It reads one
/// file's text, so a fuzz job in a workflow `release.yml` CALLS is invisible to it. It says nothing
/// about [`WORKFLOW`]'s own triggers: a `pull_request` or `merge_group` leg added there is held
/// elsewhere, by `check-workflows`, which requires every job that reports on a gating event to be
/// classified in `devco/required-contexts`. And a re-added `schedule` or push-to-`main` leg is held
/// by nothing here on purpose - that is a COST decision, reversible in one line, not a way for
/// fuzzing to block a release.
const FUZZ_INVOCATIONS: [&str; 3] = ["run-fuzz.sh", "nix run .#fuzz", "cargo fuzz"];

/// The lock the fuzz crate resolves against.
///
/// Its own, because `fuzz/` is a workspace of its own - see the manifest's header. A root-level
/// `cargo deny` never reads it: [`deny_wiring`] holds a fuzz-scoped run at every site that runs the
/// root one, and [`lock_drift`] holds every pin it shares with the root lock to the root's version.
const LOCK: &str = "fuzz/Cargo.lock";

/// The root workspace manifest, whose single `version = "..."` line
/// `.github/workflows/version-bump.yml`'s "Set the workspace version" step writes.
///
/// **`3ab86ad77`, the v0.6.0 release commit**: that step ran `cargo update --workspace`
/// against the ROOT manifest only, so the release left [`LOCK`] pinning every `sutura-*` crate at
/// the PREVIOUS version while `ROOT_MANIFEST` already declared the new one - green until the next
/// PR's `check-boundaries` ran `cargo metadata --locked` against `fuzz/Cargo.toml` and refused the
/// drift. This is the PR-time half: it catches the same drift on every pull request, not only at a
/// version bump, so a stale [`LOCK`] fails here before it ever reaches a release.
///
/// `pub(crate)` for `crate::boundaries::second_workspace`, which reads it for the same reason
/// [`version_gaps`] is `pub(crate)`: one owner for "which file holds the workspace version"
/// rather than a second literal that could disagree with this one.
pub(crate) const ROOT_MANIFEST: &str = "Cargo.toml";

/// The prefix that marks a `[[package]]` in [`LOCK`] as a first-party crate rather than a
/// crates.io dependency - a path dependency's stanza carries no `source = ` line, but the name
/// prefix is cheaper to check and does not depend on stanza order.
const FIRST_PARTY_PREFIX: &str = "sutura";

/// The fuzz crate's OWN package - [`MANIFEST`]'s `[package]` name, versioned `0.0.0` on purpose
/// (the manifest's header) and never bumped alongside [`ROOT_MANIFEST`]. Excluded from
/// [`first_party_pins`] by name rather than by "is this the crate under test", because it is the
/// one `sutura`-prefixed stanza [`LOCK`] carries that this gate must NOT compare to the workspace
/// version.
const FUZZ_CRATE: &str = "sutura-fuzz";

/// The macro that makes a file a libFuzzer target.
///
/// It is also, separately, the exact string OSS Scorecard's Rust fuzzing detector looks for in a
/// `*.rs` file - so the shape that finds bugs and the shape that is detectable are the same shape,
/// and neither was chosen for the other.
const HARNESS_MACRO: &str = "libfuzzer_sys";

/// The panic strategy the harness has to compile under.
///
/// The whole argument for `fuzz/` is that a panic on untrusted input is process death on every
/// shipped profile. A harness that unwound would be measuring a build nobody runs.
const PANIC_ABORT: &str = "panic = \"abort\"";

/// A `[[bin]]` section being read: either key may not have arrived yet.
#[derive(Default)]
struct PendingBin {
    name: Option<String>,
    path: Option<String>,
}

impl PendingBin {
    /// The complete pair, or nothing - a section missing either key declares no binary.
    fn complete(self) -> Option<(String, String)> {
        Some((self.name?, self.path?))
    }
}

/// Every `[[bin]]` the manifest declares, as `(name, path)`.
fn declared_bins(manifest: &str) -> Vec<(String, String)> {
    let mut bins = Vec::new();
    let mut open: Option<PendingBin> = None;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if let Some(complete) = open.take().and_then(PendingBin::complete) {
                bins.push(complete);
            }
            open = (trimmed == "[[bin]]").then(PendingBin::default);
            continue;
        }
        if let Some(fields) = &mut open {
            if let Some(value) = quoted_value(trimmed, "name") {
                fields.name = Some(value);
            } else if let Some(value) = quoted_value(trimmed, "path") {
                fields.path = Some(value);
            }
        }
    }
    if let Some(complete) = open.and_then(PendingBin::complete) {
        bins.push(complete);
    }
    bins
}

/// The double-quoted value of `key` on one `key = "value"` line.
fn quoted_value(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
    let inner = rest.strip_prefix('"')?;
    let end = inner.find('"')?;
    Some(String::from(inner.split_at(end).0))
}

/// The target names the workflow's matrix expands over.
///
/// Fails closed: a workflow with no `target:` sequence under a `matrix:` returns an error rather
/// than an empty set, because "the matrix names none of them" and "this reader did not find the
/// matrix" must not be the same answer.
fn matrix_targets(workflow: &str) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    let mut inside = false;
    let mut indent = 0_usize;
    for line in workflow.lines() {
        let trimmed = line.trim();
        if inside {
            if let Some(item) = trimmed.strip_prefix("- ") {
                names.insert(String::from(item.trim()));
                continue;
            }
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Any other content at or left of the sequence's own key ends it.
            if line.len().saturating_sub(trimmed.len()) <= indent {
                inside = false;
            }
            continue;
        }
        if trimmed == "target:" {
            inside = true;
            indent = line.len().saturating_sub(trimmed.len());
        }
    }
    if names.is_empty() {
        return Err(format!("{WORKFLOW} declares no `target:` sequence - the matrix runs nothing"));
    }
    Ok(names)
}

/// The 1-indexed lines of the release workflow that invoke the fuzzer, with the form each used.
///
/// Comment lines are skipped: this file's own argument for the split names the runner script, and a
/// gate that refused the sentence explaining it would make the explanation unwritable.
fn release_invocations(workflow: &str) -> Vec<(usize, &'static str)> {
    workflow
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with('#'))
        .flat_map(|(index, line)| {
            FUZZ_INVOCATIONS
                .iter()
                .filter(move |needle| line.contains(**needle))
                .map(move |needle| (index.saturating_add(1), *needle))
        })
        .collect()
}

/// Every package a lock pins, as `(name, version)` - the one parse both this gate's first-party
/// check and [`lock_drift`]'s third-party one read.
///
/// Same parse `shared_client::versions_of` uses: within a `[[package]]` stanza, `name` precedes
/// `version`, both `key = "value"` on their own line. Not shared with it - that scan matches one
/// exact name, this one reads the whole file.
fn pins(lock: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut current: Option<&str> = None;
    for line in lock.lines() {
        if let Some(value) = line.strip_prefix("name = ") {
            current = unquote(value);
        } else if let Some(value) = line.strip_prefix("version = ")
            && let Some(seen) = current.take()
            && let Some(version) = unquote(value)
        {
            found.push((seen, version));
        }
    }
    found
}

/// Every first-party package [`LOCK`] pins, as `(name, version)`.
fn first_party_pins(lock: &str) -> Vec<(String, String)> {
    pins(lock)
        .into_iter()
        .filter(|(name, _)| name.starts_with(FIRST_PARTY_PREFIX) && *name != FUZZ_CRATE)
        .map(|(name, version)| (String::from(name), String::from(version)))
        .collect()
}

/// Strip the surrounding quotes from a lock-file or manifest value, or `None` if it is not
/// quoted.
///
/// Takes the FIRST quoted span and ignores anything after its closing quote, rather than
/// requiring the value to end on one - TOML allows a `# comment` after a value on the same
/// line, and `version = "0.6.0" # workspace` is legal, parseable TOML that the earlier
/// exact-suffix match refused, silently skipping the drift check below it rather than reading
/// the version (`telekom/sutura#1150` review, mutation 3c). [`first_party_pins`]' lock lines
/// never carry a trailing comment, so the same relaxation there costs nothing.
fn unquote(value: &str) -> Option<&str> {
    let inner = value.strip_prefix('"')?;
    let end = inner.find('"')?;
    inner.get(..end)
}

/// [`ROOT_MANIFEST`]'s single `version = "..."` line, unquoted.
///
/// `None` is the documented, accepted case ONLY when no line starts `version = ` at all -
/// `xtask/tests/check_fuzz.rs`'s fixture tree is a bare `[workspace]\n` with no shared version,
/// and a workspace that declares none has nothing for [`version_gaps`] to compare against. Any
/// line that DOES start `version = ` is read by [`unquote`], comment or not.
fn workspace_version(root_manifest: &str) -> Option<&str> {
    root_manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = ").and_then(unquote))
}

/// Every [`LOCK`] pin that disagrees with [`ROOT_MANIFEST`]'s workspace version, each already
/// formatted as a failure line - the pure comparison [`run`] wires into [`report`]'s `gaps`,
/// and the one [`crate::boundaries`] calls to put the same hint on its own `fuzz/Cargo.toml`
/// `cargo metadata --locked` failure, which the drift trips FIRST in the `hygiene` sweep
/// (`check-boundaries` runs before `check-fuzz` - `telekom/sutura#1150` review, finding 3a: the
/// sweep stops at the first failure, so this gate's own readable message was never reached
/// there).
///
/// Extracted to a pure `(text, text) -> Vec<String>` function, rather than left inline in `run`,
/// for a reason that is not tidiness: the version this gate shipped with (`telekom/sutura#1150`
/// review, finding 4) tested `report`'s `gaps` parameter with a HAND-WRITTEN string and
/// never called `first_party_pins`, `workspace_version` or this filter at all - so a mutated
/// filter (`pinned != workspace` weakened to `pinned != workspace && false`) passed all 2065
/// tests in the workspace. A test that calls this function directly, on a real stale lock, is
/// killed by that same mutation; a test that calls `report` with a pre-written string cannot be.
pub(crate) fn version_gaps(root_manifest: &str, lock: &str) -> Vec<String> {
    let Some(workspace) = workspace_version(root_manifest) else {
        return Vec::new();
    };
    first_party_pins(lock)
        .into_iter()
        .filter(|(_, pinned)| pinned != workspace)
        .map(|(name, pinned)| {
            format!(
                "{LOCK} pins {name} at \"{pinned}\", but {ROOT_MANIFEST} declares the \
                 workspace version \"{workspace}\" - run `cargo update --workspace \
                 --offline --manifest-path fuzz/Cargo.toml` after bumping the version"
            )
        })
        .collect()
}

/// Does the manifest compile the harness with the shipped panic strategy?
fn aborts_on_panic(manifest: &str) -> bool {
    manifest.lines().any(|line| line.trim() == PANIC_ABORT)
}

/// Would libFuzzer accept this one dictionary line?
///
/// Everything before the first `"` is the optional name and is ignored, exactly as libFuzzer
/// ignores it; the last non-space character has to be the closing `"`; the value has to be
/// non-empty and may escape only `\\`, `\"` and `\xAB` - a lowercase `x` and either hex case.
/// Blank and comment lines never reach here.
fn dictionary_entry_is_parseable(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(open) = trimmed.find('"') else { return false };
    let Some(value) = trimmed.get(open + 1..).and_then(|rest| rest.strip_suffix('"')) else {
        return false;
    };
    if value.is_empty() {
        return false;
    }
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            continue;
        }
        match chars.next() {
            Some('\\' | '"') => (),
            // Lowercase `x` ONLY: `\X41` is refused, measured against a libFuzzer binary and
            // against this file's first reading of libFuzzer's source, which had it as either case.
            Some('x') => match (chars.next(), chars.next()) {
                (Some(high), Some(low)) if high.is_ascii_hexdigit() && low.is_ascii_hexdigit() => (),
                _ => return false,
            },
            _ => return false,
        }
    }
    true
}

/// The 1-INDEXED lines of one dictionary that libFuzzer would refuse.
///
/// One-indexed because the runner's own error is, and a gate that renumbered the defect would cost
/// the reader the one thing the log already told them.
fn unparseable_entries(dictionary: &str) -> Vec<usize> {
    dictionary
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let head = line.trim_start();
            !head.is_empty() && !head.starts_with('#')
        })
        .filter(|(_, line)| !dictionary_entry_is_parseable(line))
        .map(|(index, _)| index + 1)
        .collect()
}

/// `github.com/telekom/sutura#867`: every crate a target's source names must be reachable through
/// the "fuzzed tree" surface, the one pattern that decides whether `just ship-check` replays the
/// seeds for a diff. Fails closed if no surface is reached by `fuzz-smoke`.
fn surface_crate_gaps(root: &std::path::Path, sources: &BTreeSet<String>) -> Result<Vec<String>, String> {
    let surface_paths = crate::hook_coverage::SURFACES
        .iter()
        .find(|surface| surface.reached_by == "fuzz-smoke")
        .map(|surface| surface.paths)
        .ok_or_else(|| String::from("no surface in xtask/src/hook_coverage/surfaces.rs is reached by `fuzz-smoke`"))?;

    let mut gaps = Vec::new();
    for name in sources {
        let Ok(source) = std::fs::read_to_string(root.join(TARGETS_DIR).join(format!("{name}.rs"))) else {
            continue;
        };
        for crate_dir in hook_paths::target_crates(&source) {
            if hook_paths::missing_from_surface(&crate_dir, surface_paths) {
                gaps.push(format!(
                    "{TARGETS_DIR}/{name}.rs imports `{crate_dir}`, which the `fuzzed tree` row in \
                     xtask/src/hook_coverage/surfaces.rs never names - a change there makes \
                     `just ship-check` skip the seed replay"
                ));
            }
        }
    }
    Ok(gaps)
}

/// The verdict, given everything read off the tree.
fn report(
    sources: &BTreeSet<String>,
    bins: &[(String, String)],
    seeded: &BTreeSet<String>,
    harnessed: &BTreeSet<String>,
    matrix: &BTreeSet<String>,
    refused: &[(String, usize)],
    in_release: &[(usize, &str)],
    locked: bool,
    aborts: bool,
    gaps: &[String],
) -> Verdict {
    let mut failures = Vec::new();
    failures.extend(gaps.iter().cloned());
    if sources.is_empty() {
        failures.push(format!("{TARGETS_DIR} holds no target - a green fuzz run over nothing"));
    }
    let declared: BTreeSet<String> = bins.iter().map(|(name, _)| name.clone()).collect();
    for name in sources.difference(&declared) {
        failures.push(format!(
            "{TARGETS_DIR}/{name}.rs has no `[[bin]]` entry in {MANIFEST} - it is never built"
        ));
    }
    for (name, path) in bins {
        if !sources.contains(name) {
            failures.push(format!("{MANIFEST} declares `{name}` at `{path}`, which is not a file"));
        } else if path != &format!("fuzz_targets/{name}.rs") {
            failures.push(format!(
                "{MANIFEST} points `{name}` at `{path}` rather than at its own source"
            ));
        }
    }
    for name in sources.difference(harnessed) {
        failures.push(format!(
            "{TARGETS_DIR}/{name}.rs does not name `{HARNESS_MACRO}` - it fuzzes nothing"
        ));
    }
    for name in sources.difference(seeded) {
        failures.push(format!(
            "{SEEDS_DIR}/{name}/ is missing or empty - the target starts from nothing"
        ));
    }
    for name in sources.difference(matrix) {
        failures.push(format!("{WORKFLOW} never runs `{name}` - CI spends no budget on it"));
    }
    for name in matrix.difference(sources) {
        failures.push(format!("{WORKFLOW} runs `{name}`, which is not a target"));
    }
    for (dictionary, line) in refused {
        failures.push(format!(
            "{DICTIONARIES_DIR}/{dictionary}: error in line {line} - not a `name=\"value\"` entry, so libFuzzer refuses the dictionary after the build and the target executes nothing"
        ));
    }
    for (line, form) in in_release {
        failures.push(format!(
            "{RELEASE_WORKFLOW}:{line} invokes the fuzzer (`{form}`) - a fuzz job there is one `needs:` away from deciding whether a tag ships, and the first finding on a fresh random path would then block every release. {WORKFLOW} runs it on the same `v*` tag as a separate run, which no release job can depend on"
        ));
    }
    if !locked {
        failures.push(format!(
            "{LOCK} is absent - a fuzz run against an unlocked graph is not reproducible"
        ));
    }
    if !aborts {
        failures.push(format!(
            "{MANIFEST} does not declare `{PANIC_ABORT}` - the harness would not measure the shipped panic strategy"
        ));
    }
    if failures.is_empty() {
        println!(
            "xtask check-fuzz: ok - {} target(s) declared, seeded and named in the workflow matrix",
            sources.len()
        );
        return Verdict::Pass;
    }
    eprintln!("xtask check-fuzz: {} problem(s)", failures.len());
    for failure in &failures {
        eprintln!("  {failure}");
    }
    eprintln!();
    eprintln!("A fuzz invocation in the release workflow belongs in .github/workflows/fuzz.yml,");
    eprintln!("which runs on the same tag without a release job depending on it. Otherwise: add");
    eprintln!("the target to fuzz/Cargo.toml, seed fuzz/seeds/<target>/, and name it in the");
    eprintln!("matrix of .github/workflows/fuzz.yml. An entry in fuzz/dictionaries/<target>.dict is");
    eprintln!("`name=\"value\"`, escaping only \\\\, \\\" and \\xAB. `just fuzz-smoke` replays what is");
    eprintln!("committed.");
    Verdict::Fail
}

/// Reads the tree and hands [`report`] the answer.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-fuzz: not inside a git repository");
        return Verdict::Fail;
    };
    let Ok(manifest) = std::fs::read_to_string(root.join(MANIFEST)) else {
        eprintln!("xtask check-fuzz: {MANIFEST} is unreadable - the fuzz crate is the thing this gate is about");
        return Verdict::Fail;
    };
    let Ok(workflow) = std::fs::read_to_string(root.join(WORKFLOW)) else {
        eprintln!("xtask check-fuzz: {WORKFLOW} is unreadable - nothing would run a target");
        return Verdict::Fail;
    };
    // Fails closed for the reason every other read here does: a release workflow this gate cannot
    // open is one whose fuzz job it cannot refuse.
    let Ok(release) = std::fs::read_to_string(root.join(RELEASE_WORKFLOW)) else {
        eprintln!(
            "xtask check-fuzz: {RELEASE_WORKFLOW} is unreadable - a fuzz job inside it would gate every release, and this is what refuses one"
        );
        return Verdict::Fail;
    };

    // The set of targets is the *.rs files under fuzz_targets - the same derivation run-fuzz.sh
    // uses, so the gate and the runner agree on what a target is by construction.
    let targets_dir = root.join(TARGETS_DIR);
    let sources: BTreeSet<String> = std::fs::read_dir(&targets_dir).map_or_else(
        |_| BTreeSet::new(),
        |entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
                .filter_map(|e| e.file_name().into_string().ok())
                .filter_map(|name| name.strip_suffix(".rs").map(String::from))
                .collect()
        },
    );

    let seeded: BTreeSet<String> = std::fs::read_dir(root.join(SEEDS_DIR)).map_or_else(
        |_| BTreeSet::new(),
        |entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| {
                    // A seed directory is only a seed directory if it holds at least one file.
                    std::fs::read_dir(root.join(SEEDS_DIR).join(name)).is_ok_and(|mut d| d.next().is_some())
                })
                .collect()
        },
    );

    let harnessed: BTreeSet<String> = sources
        .iter()
        .filter(|name| {
            std::fs::read_to_string(root.join(TARGETS_DIR).join(format!("{name}.rs")))
                .is_ok_and(|src| src.contains(HARNESS_MACRO))
        })
        .cloned()
        .collect();

    // Only `<target>.dict` files, the same derivation run-fuzz.sh uses to build its `-dict=`. An
    // absent directory is not a failure: the dictionaries are an optimisation, not a requirement.
    let dictionaries_dir = root.join(DICTIONARIES_DIR);
    let mut names: Vec<String> = std::fs::read_dir(&dictionaries_dir).map_or_else(
        |_| Vec::new(),
        |entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "dict"))
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        },
    );
    names.sort();
    let mut refused = Vec::new();
    for name in names {
        let Ok(dictionary) = std::fs::read_to_string(dictionaries_dir.join(&name)) else {
            eprintln!(
                "xtask check-fuzz: {DICTIONARIES_DIR}/{name} is unreadable - a dictionary nothing can check is one libFuzzer may refuse"
            );
            return Verdict::Fail;
        };
        refused.extend(unparseable_entries(&dictionary).into_iter().map(|line| (name.clone(), line)));
    }

    let bins = declared_bins(&manifest);
    let locked = root.join(LOCK).is_file();
    let aborts = aborts_on_panic(&manifest);
    let in_release = release_invocations(&release);
    let mut gaps = match surface_crate_gaps(&root, &sources) {
        Ok(gaps) => gaps,
        Err(message) => {
            eprintln!("xtask check-fuzz: {message}");
            return Verdict::Fail;
        }
    };
    let Ok(root_manifest) = std::fs::read_to_string(root.join(ROOT_MANIFEST)) else {
        eprintln!("xtask check-fuzz: {ROOT_MANIFEST} is unreadable - the workspace version could not be checked");
        return Verdict::Fail;
    };
    // Merged into `gaps` rather than given `report` an 11th parameter: `clippy.toml` caps
    // `too-many-arguments-threshold` at 10, already the shape's own limit.
    let lock_text = std::fs::read_to_string(root.join(LOCK)).unwrap_or_default();
    gaps.extend(version_gaps(&root_manifest, &lock_text));
    gaps.extend(lock_drift::gaps_in(&root, &lock_text));
    gaps.extend(deny_wiring::gaps(&root));
    match matrix_targets(&workflow) {
        Ok(matrix) => report(
            &sources,
            &bins,
            &seeded,
            &harnessed,
            &matrix,
            &refused,
            &in_release,
            locked,
            aborts,
            &gaps,
        ),
        Err(message) => {
            eprintln!("xtask check-fuzz: {message}");
            Verdict::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bins(spec: &[(&str, &str)]) -> Vec<(String, String)> {
        spec.iter().map(|(a, b)| (String::from(*a), String::from(*b))).collect()
    }
    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn a_complete_target_set_passes() {
        let verdict = report(
            &set(&["sql_expression", "question_body"]),
            &bins(&[
                ("sql_expression", "fuzz_targets/sql_expression.rs"),
                ("question_body", "fuzz_targets/question_body.rs"),
            ]),
            &set(&["sql_expression", "question_body"]),
            &set(&["sql_expression", "question_body"]),
            &set(&["sql_expression", "question_body"]),
            &[],
            &[],
            true,
            true,
            &[],
        );
        assert!(verdict == Verdict::Pass, "a coherent set must pass");
    }

    #[test]
    fn a_target_without_a_bin_entry_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &[],
            &[],
            true,
            true,
            &[],
        );
        assert!(verdict == Verdict::Fail, "an unbuilt target must fail");
    }

    #[test]
    fn a_target_the_matrix_never_runs_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &set(&[]),
            &[],
            &[],
            true,
            true,
            &[],
        );
        assert!(verdict == Verdict::Fail, "a target CI never runs must fail");
    }

    #[test]
    fn a_target_with_no_seed_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
            &set(&[]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &[],
            &[],
            true,
            true,
            &[],
        );
        assert!(verdict == Verdict::Fail, "an unseeded target must fail");
    }

    #[test]
    fn a_missing_lock_or_missing_panic_abort_fails() {
        for (locked, aborts) in [(false, true), (true, false)] {
            let verdict = report(
                &set(&["sql_expression"]),
                &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
                &set(&["sql_expression"]),
                &set(&["sql_expression"]),
                &set(&["sql_expression"]),
                &[],
                &[],
                locked,
                aborts,
                &[],
            );
            assert!(verdict == Verdict::Fail, "an unlocked or unwinding harness must fail");
        }
    }

    #[test]
    fn a_single_bin_section_is_read() {
        let manifest =
            "[package]\nname = \"x\"\n\n[[bin]]\nname = \"sql_expression\"\npath = \"fuzz_targets/sql_expression.rs\"\n";
        assert_eq!(
            declared_bins(manifest),
            vec![(String::from("sql_expression"), String::from("fuzz_targets/sql_expression.rs"))]
        );
    }

    #[test]
    fn a_matrix_sequence_is_read_and_fails_closed_when_absent() {
        let workflow = "jobs:\n  fuzz:\n    strategy:\n      matrix:\n        target:\n          - sql_expression\n          - question_body\n";
        assert_eq!(matrix_targets(workflow).unwrap(), set(&["question_body", "sql_expression"]));
        matrix_targets("jobs:\n  fuzz:\n    strategy: {}\n").expect_err("a workflow with no matrix must fail closed");
    }

    #[test]
    fn a_target_is_read_off_its_suffix() {
        assert_eq!("sql_expression.rs".strip_suffix(".rs"), Some("sql_expression"));
    }

    #[test]
    fn first_party_pins_are_read_and_third_party_ones_are_not() {
        let lock = "[[package]]\nname = \"arraydeque\"\nversion = \"0.5.1\"\nsource = \"registry+x\"\n\n\
                    [[package]]\nname = \"sutura-domain\"\nversion = \"0.5.1\"\ndependencies = []\n";
        assert_eq!(
            first_party_pins(lock),
            vec![(String::from("sutura-domain"), String::from("0.5.1"))]
        );
    }

    /// The fuzz crate pins itself in its own lock, at its own independent `0.0.0` scheme - a
    /// real shape in `fuzz/Cargo.lock` today, and the one this gate must not flag.
    #[test]
    fn the_fuzz_crates_own_stanza_is_excluded() {
        let lock = "[[package]]\nname = \"sutura-fuzz\"\nversion = \"0.0.0\"\ndependencies = []\n";
        assert_eq!(first_party_pins(lock), Vec::<(String, String)>::new());
    }

    #[test]
    fn workspace_version_is_the_manifests_own_version_line() {
        assert_eq!(
            workspace_version("[workspace.package]\nversion = \"0.6.0\"\nedition = \"2024\"\n"),
            Some("0.6.0")
        );
        assert_eq!(workspace_version("[workspace.package]\nedition = \"2024\"\n"), None);
    }

    /// `3ab86ad77`, the v0.6.0 release commit: a `fuzz/Cargo.lock` a release left behind at the
    /// previous version, with the root manifest already bumped, is exactly this shape.
    ///
    /// Calls [`version_gaps`] directly rather than hand-writing a `gaps` string for
    /// `report`. `telekom/sutura#1150` review, finding 4: the earlier version of this test did
    /// the latter, so it never called [`first_party_pins`], [`workspace_version`] or the
    /// comparison at all, and a mutated filter (weakened to always find no gap) still passed
    /// every test in the workspace. This one is killed by that same mutation.
    #[test]
    fn a_stale_first_party_pin_fails() {
        let root_manifest = "[workspace.package]\nversion = \"0.6.0\"\n";
        let lock = "[[package]]\nname = \"sutura-domain\"\nversion = \"0.5.1\"\ndependencies = []\n";
        let gaps = version_gaps(root_manifest, lock);
        assert_eq!(gaps.len(), 1, "a stale first-party pin must be reported exactly once");
        assert!(gaps[0].contains("sutura-domain"));
        assert!(gaps[0].contains("\"0.5.1\""));
        assert!(gaps[0].contains("\"0.6.0\""));
    }

    #[test]
    fn a_matching_pin_reports_no_gap() {
        let root_manifest = "[workspace.package]\nversion = \"0.6.0\"\n";
        let lock = "[[package]]\nname = \"sutura-domain\"\nversion = \"0.6.0\"\ndependencies = []\n";
        assert_eq!(version_gaps(root_manifest, lock), Vec::<String>::new());
    }

    /// `telekom/sutura#1150` review, finding 3c: a trailing `# comment` after the version's
    /// closing quote is legal TOML, and the earlier `unquote` refused to parse it - so
    /// `workspace_version` returned `None` and this whole check was skipped without saying so,
    /// on a manifest this gate could read perfectly well otherwise.
    #[test]
    fn a_trailing_comment_on_the_version_line_does_not_skip_the_check() {
        let root_manifest = "[workspace.package]\nversion = \"0.6.0\" # workspace\n";
        let lock = "[[package]]\nname = \"sutura-domain\"\nversion = \"0.5.1\"\ndependencies = []\n";
        assert_eq!(version_gaps(root_manifest, lock).len(), 1);
    }

    /// No `version = ` line at all is the one case this gate accepts silently -
    /// `xtask/tests/check_fuzz.rs`'s fixture tree is exactly this shape.
    #[test]
    fn no_workspace_version_line_reports_no_gap() {
        let root_manifest = "[workspace]\n";
        let lock = "[[package]]\nname = \"sutura-domain\"\nversion = \"0.5.1\"\ndependencies = []\n";
        assert_eq!(version_gaps(root_manifest, lock), Vec::<String>::new());
    }

    fn stanza(name: &str, version: &str) -> String {
        format!("[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = \"registry+x\"\n\n")
    }

    /// `yoke-derive` 0.8.3 in the fuzz lock against the root's 0.8.2, the drift crates.io's yank
    /// of 0.8.3 turned into a red supply-chain check on every pull request.
    #[test]
    fn a_shared_package_at_another_version_fails() {
        let gaps = lock_drift::gaps(&stanza("yoke-derive", "0.8.2"), &stanza("yoke-derive", "0.8.3"));
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        assert!(gaps[0].contains("yoke-derive") && gaps[0].contains("\"0.8.3\"") && gaps[0].contains("\"0.8.2\""));
    }

    #[test]
    fn a_shared_package_at_a_root_version_passes() {
        let root = [stanza("syn", "2.0.119"), stanza("syn", "3.0.4")].concat();
        assert_eq!(lock_drift::gaps(&root, &stanza("syn", "3.0.4")), Vec::<String>::new());
    }

    #[test]
    fn a_package_only_the_fuzz_lock_pins_passes() {
        let fuzz = [stanza("libfuzzer-sys", "0.4.12"), stanza("syn", "3.0.4")].concat();
        assert_eq!(lock_drift::gaps(&stanza("syn", "3.0.4"), &fuzz), Vec::<String>::new());
    }

    #[test]
    fn a_dictionary_libfuzzer_would_refuse_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &[(String::from("sql_expression.dict"), 17)],
            &[],
            true,
            true,
            &[],
        );
        assert!(verdict == Verdict::Fail, "a dictionary libFuzzer refuses must fail");
    }

    /// The oracle is a real libFuzzer binary, not this file's reading of libFuzzer's source: every
    /// row was run through `-dict=` with a one-line dictionary and the verdict recorded.
    #[test]
    fn the_shapes_libfuzzer_accepts_and_the_ones_it_refuses() {
        for accepted in [
            r#""abc""#,
            r#"name="abc""#,
            r#"name = "abc""#,
            r#"anything before the quote "abc""#,
            r#"name="ab"cd""#,
            r#"name="\\""#,
            r#"name="\"""#,
            r#"name="\x41""#,
            r#"date="\x22\\d{4}-\\d{2}-\\d{2}\x22""#,
        ] {
            assert!(dictionary_entry_is_parseable(accepted), "libFuzzer accepts {accepted}");
        }
        for refused in [
            "foo",                      // a bare unquoted token
            r#"""#,                     // one quote
            r#""a"#,                    // unterminated
            r#""""#,                    // an empty value
            r#"name="""#,               // an empty value with a name
            r#"name="abc" # trailing"#, // a `#` is only a comment at the head of a line
            r#"name="\q""#,             // not an escape
            r#"name="\""#,              // a trailing backslash
            r#"name="\x2""#,            // a short hex escape
            r#"name="\xZZ""#,           // not hex
            r#"name="\X41""#,           // the hex marker is lowercase-only
            r#"open={"\x22"#,           // the two committed lines that cost every scheduled run
            r#"close=\x22"}"#,
        ] {
            assert!(!dictionary_entry_is_parseable(refused), "libFuzzer refuses {refused}");
        }
    }

    #[test]
    fn refused_lines_are_reported_one_indexed_past_comments_and_blanks() {
        let dictionary = "# a header comment\n\nmetric=\"\\x22metric\\x22\"\nopen={\"\\x22\nclose=\\x22\"}\n";
        assert_eq!(
            unparseable_entries(dictionary),
            vec![4, 5],
            "the line numbers must be the ones libFuzzer's own error prints"
        );
        assert!(
            unparseable_entries("# only a comment\n\n\tsum=\"SUM(\"\n").is_empty(),
            "comments, blank lines and a valid entry are not defects"
        );
    }

    #[test]
    fn a_fuzz_invocation_in_the_release_workflow_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &[],
            &[(41, "run-fuzz.sh")],
            true,
            true,
            &[],
        );
        assert!(verdict == Verdict::Fail, "a fuzz job inside the release workflow must fail");
    }

    /// Every form a release job could reach the fuzzer by, and the one shape that must stay
    /// writable: the comment explaining why the split exists names the runner script itself.
    #[test]
    fn each_invocation_form_is_found_and_a_comment_about_one_is_not() {
        assert_eq!(
            release_invocations("jobs:\n  fuzz:\n    steps:\n      - run: bash nix/run-fuzz.sh run 900 x\n"),
            vec![(4, "run-fuzz.sh")]
        );
        assert_eq!(
            release_invocations("      - run: nix run .#fuzz -- run\n"),
            vec![(1, "nix run .#fuzz")]
        );
        assert_eq!(
            release_invocations("      - run: cargo fuzz run x\n"),
            vec![(1, "cargo fuzz")]
        );
        assert!(
            release_invocations("# fuzzing runs on the same tag via nix/run-fuzz.sh, in its own workflow\n").is_empty(),
            "a comment naming the runner is the explanation, not an invocation"
        );
    }

    #[test]
    fn the_release_workflow_in_this_tree_invokes_no_fuzzer() {
        let Some(root) = repo::root() else { return };
        let Ok(release) = std::fs::read_to_string(root.join(RELEASE_WORKFLOW)) else {
            panic!("{RELEASE_WORKFLOW} has to be readable - the rule fails closed on it");
        };
        assert!(
            release_invocations(&release).is_empty(),
            "the release workflow must not run the fuzzer - see RELEASE_WORKFLOW for why"
        );
    }
}

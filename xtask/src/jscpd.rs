//! The jscpd copy/paste gate (issue #474): a duplicated block is a refactor invitation
//! two maintainers have often independently ignored, and this makes the finding mechanical.
//!
//! WHAT IT RUNS. This gate shells to the `jscpd` binary - a nix-native Rust engine built by
//! `nix/jscpd.nix` from the pinned `jscpd-src` input. It scans the whole tree's Rust (with
//! `--format rust`) and reports every clone with `file:line` on both sides. The verdict is NOT
//! "how much duplication there is" but "are there any clones above the thresholds that an
//! auditable reason does not cover".
//!
//! WHY IT IS A HYGIENE GATE, AND WHEN IT REFUSES. `checks.hygiene` carries `jscpd` on PATH (see
//! flake.nix), so in CI - and in `just validate` / `just test` through it - this gate scans and
//! fails on a clone. If `jscpd` is not on PATH the gate FAILS CLOSED (refuses to attest): that is
//! the `github.com/telekom/sutura#371` rule every hygiene gate is held to - a green it cannot
//! back is a green that says nothing, enforced by `every_registered_hygiene_gate_refuses_a_tree_it_cannot_attest`.
//! The DEDICATED `jscpd` pre-commit hook (`.pre-commit-config.yaml`, via `nix/run-gate.sh`) tiers
//! the local path: jscpd on PATH runs the gate, nix provisions the SAME locked pin, and a host
//! with neither skips with a notice (the run-gate.sh tier-3 contract the shellcheck / zizmor hooks
//! already use) - so a jscpd-able host checks hard and CI always does.
//!
//! THE ALLOWLIST. `devco/dup-ignore` is a policy file in the `devco/max-lines-ignore` shape: one
//! audited clone per non-comment line, each carrying a human reason (the only thing a reviewer
//! can judge), keyed on the two fragments' `path:start-line`. An intentional clone is added there
//! with a reason, never by loosening the threshold - exactly how `devco/max-lines-ignore` grants
//! file-length exemptions. The matching is deliberately simple (file + start line, in either
//! order) because the cost of a wrong match is a clone reported that was meant to be allowed, and
//! that is a visible false positive rather than a silent one.
//!
//! LIMIT, NEXT TO THE CLAIM. This scans the whole tree's Rust (`--format rust`) and cannot tell
//! an intentional pattern from an accidental one - that is what the allowlist's written reasons
//! are for. It does not see duplication in other languages. Thresholds (lines/tokens) are
//! constants here; changing what trips the gate is a source change, reviewed.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Verdict, repo, repo::Unmigrated};

/// The minimum lines of a clone this gate reports (jscpd's `--min-lines`).
///
/// Calibrated on this tree (measured): at the jscpd defaults (5 lines / 50 tokens) the first-
/// party Rust reports dozens of clones; at 30 lines / 250 tokens it reports NONE - the largest
/// pre-existing clone is 41 lines / 246 tokens. So 30/250 catches substantial copy-paste
/// while starting green, the repo's convention for a new gate. A clone must meet BOTH limits.
const MIN_LINES: usize = 30;
/// The minimum tokens of a clone this gate reports (jscpd's `--min-tokens`).
const MIN_TOKENS: usize = 250;
/// jscpd's format name for Rust, which also makes paths report repo-relative when scanning the
/// repo root (drive which files it tokenizes under `-f rust`).
const FORMAT: &str = "rust";
/// The allowlist policy file, granting auditable reasons for intentional clones.
const IGNORE_FILE: &str = "devco/dup-ignore";
/// Generated/build output jscpd must never scan, as comma-separated globs (`--ignore`).
/// `wholeTree` in the nix sandbox carries `target/` build artifacts, so relying on gitignore (a
/// `.git`-less source does not apply it) would let a 250-line generated CRC table flag itself.
const IGNORE_GLOBS: &str = "target/**,site/**,result/**,result-*/**,.pixi/**,.sutura-dev/**,report/**,**/.prek-cache/**";
/// First-party source that the allowlist may never excuse, mirroring `max_lines::is_unexemptable`
/// (there under `UNEXEMPTABLE_PREFIXES`): an exemption list that can swallow `crates/` or `xtask/`
/// is a gate that has quietly stopped gating.
const UNEXEMPTABLE_PREFIXES: &[&str] = &["crates/", "xtask/"];

/// One fragment of a clone, as jscpd's JSON reporter names it.
#[derive(Debug, Clone)]
struct Fragment {
    name: String,
    start: u64,
}

/// One clone as jscpd's `json` reporter emits it - stripped to what the gate decides on.
#[derive(Debug, Clone)]
struct Duplicate {
    first: Fragment,
    second: Fragment,
    lines: u64,
    tokens: u64,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-jscpd: could not determine the repo root");
        return Verdict::Fail;
    };

    // Single owner of the scan set and thresholds: `nix/run-gate.sh`'s tier-2 jscpd arm carries
    // an inline `--ignore` list and `--min-lines`/`--min-tokens`/`--format` it cannot import, so
    // this gate re-parses that invocation line and refuses any drift.
    if let Err(msg) = check_run_gate_globs(&root) {
        eprintln!("xtask check-jscpd: FAILED - {msg}");
        return Verdict::Fail;
    }

    // CENSUS FLOOR, decided before `jscpd` exists and independent of `--min-tokens`: this gate must
    // scan real Rust, and an `--ignore` glob that starts matching it lets a clone hide in the
    // skipped half. `git ls-files '*.rs'` is the independent oracle - `report.statistics.total.sources`
    // drops files below `--min-tokens` (measured), so a sources floor would be counted off the same
    // threshold as the scan and would not be a floor at all.
    let files = match repo::all_files() {
        Ok(census) => match census.into_listing(Unmigrated::Jscpd) {
            Ok((_root, files)) => files,
            Err(why) => {
                eprintln!("xtask check-jscpd: FAILED - {}", why.describe());
                return Verdict::Fail;
            }
        },
        Err(why) => {
            eprintln!("xtask check-jscpd: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let rs_files: Vec<&str> = files.iter().map(String::as_str).filter(|f| is_rust(f)).collect();
    if let Err(msg) = check_census(&rs_files, IGNORE_GLOBS) {
        eprintln!("xtask check-jscpd: FAILED - {msg}");
        return Verdict::Fail;
    }

    // FAIL CLOSED when `jscpd` is absent: a hygiene gate that cannot attest must refuse, per
    // `github.com/telekom/sutura#371` (a green it cannot back is a green that says nothing) -
    // enforced by `every_registered_hygiene_gate_refuses_a_tree_it_cannot_attest`. Hosts running
    // this without jscpd get the finding at the gate instead of a silent pass; CI
    // (`checks.hygiene`) carries jscpd on PATH, and a host with nix reaches the same locked pin
    // with `cargo run -p xtask` after the run-gate.sh `jscpd` tier puts it there.
    let Some(jscpd) = which("jscpd") else {
        eprintln!(
            "xtask check-jscpd: FAILED - `jscpd` is not on PATH, so this gate cannot attest, and per #371 it refuses rather than passes."
        );
        eprintln!("  Reach the locked pin with nix: `nix run .#jscpd`. CI (checks.hygiene) carries the same binary.");
        return Verdict::Fail;
    };

    let allow = match load_allowlist(&root, &files) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("xtask check-jscpd: FAILED - {e}");
            return Verdict::Fail;
        }
    };

    let clones = match scan(&root, &jscpd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("xtask check-jscpd: FAILED - {e}");
            eprintln!("  A run that produced no verdict has judged nothing.");
            return Verdict::Fail;
        }
    };

    decide(&clones, &allow)
}

/// Is `path` a Rust source file, by extension (case-insensitive, like `git ls-files '*.rs'`)?
fn is_rust(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rs"))
}

/// The census floor: the gate must scan real Rust, and an `--ignore` glob must never match an
/// in-scope `.rs` (or a clone there could never be reported). Pure and unit-tested, so a mutation
/// that drops or weakens it reddens the suite. `git ls-files '*.rs'` is the independent oracle -
/// `report.statistics.total.sources` drops sub-`--min-tokens` files and would not be a floor.
fn check_census(rs_files: &[&str], ignore_globs: &str) -> Result<(), String> {
    if rs_files.is_empty() {
        return Err(String::from(
            "found zero in-scope `.rs` files, so this gate would attest over no Rust at all",
        ));
    }
    let globs: Vec<&str> = ignore_globs.split(',').map(str::trim).collect();
    if let Some(hidden) = rs_files.iter().copied().find(|f| repo::matches_any(&globs, f)) {
        return Err(format!(
            "`{ignore_globs}` matches {hidden}, so a clone in that file could never be reported"
        ));
    }
    Ok(())
}

/// The marker token that identifies the tier-2 jscpd invocation line in `nix/run-gate.sh` - the
/// single line whose `--ignore`, `--min-lines`, `--min-tokens` and `--format` this gate owns.
const RUN_GATE_TIER2_TOKEN: &str = "nix run .#jscpd";

/// The tier-2 jscpd invocation line of `nix/run-gate.sh` - the first non-comment line whose
/// trimmed text carries [`RUN_GATE_TIER2_TOKEN`]. Anchoring to the line (rather than a whole-file
/// `find`) makes the parse a property of the invocation, not of whatever text happens to sit
/// elsewhere in the file: a comment above the arm or a decoy in another arm quoting the same
/// tokens must not move it.
fn tier2_line(run_gate_text: &str) -> Option<&str> {
    run_gate_text.lines().find(|line| {
        let trimmed = line.trim_start();
        !trimmed.starts_with('#') && trimmed.contains(RUN_GATE_TIER2_TOKEN)
    })
}

/// The value a `--flag` carries on the tier-2 invocation line: the single-quoted body of
/// `--ignore '<globs>'`, or the bare token of `--min-lines` / `--min-tokens` / `--format`.
fn tier2_value<'a>(line: &'a str, flag: &str) -> Option<&'a str> {
    let needle = format!("{flag} ");
    let rest = line.split_once(&needle)?.1.trim_start();
    rest.strip_prefix('\'').map_or_else(
        || {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            rest.get(..end)
        },
        |quoted| quoted.split('\'').next(),
    )
}

/// The glob list the tier-2 jscpd arm of `nix/run-gate.sh` carries, between `--ignore '` and the
/// closing quote on the anchored invocation line - the ONLY source of truth for that tier's
/// `--ignore`.
fn tier2_globs(line: &str) -> Option<&str> {
    tier2_value(line, "--ignore")
}

/// Does `nix/run-gate.sh`'s tier-2 jscpd arm agree with the gate's declared set - the `--ignore`
/// list AND the `--min-lines` / `--min-tokens` / `--format` thresholds?
///
/// run-gate.sh cannot import the Rust constants, so its inline copies would silently drift (one
/// side adds `result-*/**`, the other does not, and the two tiers scan different trees; a
/// `--min-lines 900` here would quietly scan a laxer tree than the gate). This gate is the single
/// owner: it re-parses the anchored invocation line and refuses any drift, held by a unit test.
fn check_run_gate_globs(root: &Path) -> Result<(), String> {
    let path = root.join("nix/run-gate.sh");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    globs_agree(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The pure comparison: does the tier-2 jscpd arm equal the gate's [`IGNORE_GLOBS`] list and its
/// [`MIN_LINES`] / [`MIN_TOKENS`] / [`FORMAT`] thresholds?
fn globs_agree(run_gate_text: &str) -> Result<(), String> {
    let Some(line) = tier2_line(run_gate_text) else {
        return Err(String::from(
            "run-gate.sh has no non-comment tier-2 `nix run .#jscpd` invocation line, so its scan set and thresholds cannot be verified",
        ));
    };
    let Some(listed) = tier2_globs(line) else {
        return Err(String::from(
            "the tier-2 jscpd arm no longer passes `--ignore '<globs>'`, so its scan set cannot be verified against IGNORE_GLOBS",
        ));
    };

    let mut problems: Vec<String> = Vec::new();

    let gate: std::collections::BTreeSet<&str> = IGNORE_GLOBS.split(',').map(str::trim).filter(|g| !g.is_empty()).collect();
    let tier2: std::collections::BTreeSet<&str> = listed.split(',').map(str::trim).filter(|g| !g.is_empty()).collect();
    if gate != tier2 {
        let missing: Vec<&str> = gate.difference(&tier2).copied().collect();
        let extra: Vec<&str> = tier2.difference(&gate).copied().collect();
        problems.push(format!(
            "`--ignore` drifted from IGNORE_GLOBS (missing {missing:?}, extra {extra:?})"
        ));
    }

    // The three thresholds on the same line are inline copies of MIN_LINES / MIN_TOKENS / FORMAT;
    // hold them to the constants so a laxer shell copy refuses instead of silently passing.
    for (flag, expect) in [
        ("--min-lines", MIN_LINES.to_string()),
        ("--min-tokens", MIN_TOKENS.to_string()),
    ] {
        match tier2_value(line, flag) {
            Some(got) if got == expect => {}
            Some(got) => problems.push(format!("`{flag}` is {got:?}, but the gate scans with {expect}")),
            None => problems.push(format!("the tier-2 jscpd arm no longer passes `{flag}`")),
        }
    }
    match tier2_value(line, "--format") {
        Some(got) if got == FORMAT => {}
        Some(got) => problems.push(format!("`--format` is {got:?}, but the gate scans with {FORMAT:?}")),
        None => problems.push("the tier-2 jscpd arm no longer passes `--format`".to_owned()),
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the tier-2 jscpd arm drifted from the gate's declared set ({}); keep the two in agreement",
            problems.join("; ")
        ))
    }
}

/// The gate's decision, computed from the raw clones and the parsed allowlist.
///
/// Pure and unit-tested with synthetic [`Duplicate`]s - no `jscpd` binary - so the exit code is
/// held by a test: flipping the `Fail` arm here reddens the suite, which is the #371 refusal held
/// by a mechanism rather than by recall (the falsifier cannot reach this branch, because
/// `check-jscpd` refuses earlier on the missing binary and the missing allowlist file).
fn decide(clones: &[Duplicate], allow: &Allowlist) -> Verdict {
    let findings = findings(clones, allow);
    if findings.is_empty() {
        println!(
            "xtask check-jscpd: ok - {} clone(s) over the repo's Rust, all within {IGNORE_FILE}",
            clones.len()
        );
        return Verdict::Pass;
    }

    eprintln!(
        "xtask check-jscpd: FAILED - {} clone(s) not excused in {IGNORE_FILE}",
        findings.len()
    );
    for d in &findings {
        eprintln!(
            "  {}:{} - {}:{}  ({lines} lines, {tokens} tokens)  [duplicate]",
            d.first.name,
            d.first.start,
            d.second.name,
            d.second.start,
            lines = d.lines,
            tokens = d.tokens,
        );
        eprintln!("      deduplicate it, or add an audited reason to {IGNORE_FILE}");
    }
    Verdict::Fail
}

/// The clones the allowlist does not excuse.
fn findings(clones: &[Duplicate], allow: &Allowlist) -> Vec<Duplicate> {
    clones.iter().filter(|c| !allowed(c, allow)).cloned().collect()
}

/// `jscpd`'s private temporary working directory, removed when dropped.
///
/// The directory is gone on the success AND the failure path, and even on a panic inside the scan
/// (unwinding runs `Drop`) - a scope guard, without a global panic hook. Removing the `drop` body
/// leaks one `$TMPDIR/jscpd-*` directory per scan, which `the_temp_work_dir_is_removed_when_the_guard_drops`
/// holds red.
struct TempWorkDir(PathBuf);

impl TempWorkDir {
    /// Create a fresh, uniquely named working directory under `$TMPDIR`.
    fn create() -> Result<Self, String> {
        // A per-call sequence keeps concurrently-running scans and unit tests from sharing a name
        // under the same `<pid>-<secs>` second and deleting each other's directory mid-scan.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let out = std::env::temp_dir().join(format!(
            "jscpd-{}-{seq}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        ));
        // A stale directory can share the freshly derived name (a crashed pid in the same second
        // with the same sequence); create_dir_all would silently leave its contents in place, so
        // clear any pre-existing dir first for a genuinely fresh working tree.
        if out.exists() {
            std::fs::remove_dir_all(&out).map_err(|e| format!("could not clear stale temp dir: {e}"))?;
        }
        std::fs::create_dir_all(&out).map_err(|e| format!("could not create temp dir: {e}"))?;
        Ok(Self(out))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempWorkDir {
    fn drop(&mut self) {
        // A failure here is not the scan's concern (the directory is already absent or the OS will
        // reap it), so it must not turn a scan verdict into an error.
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

/// Run jscpd over [`SCAN`] and return the clones it found.
fn scan(root: &Path, jscpd: &Path) -> Result<Vec<Duplicate>, String> {
    let work = TempWorkDir::create()?;
    let out = work.path();

    let mut cmd = Command::new(jscpd);
    cmd.arg("--silent")
        .arg("--no-colors")
        .arg("--reporters")
        .arg("json")
        .arg("--output")
        .arg(out)
        .arg("--format")
        .arg(FORMAT)
        .arg("--min-lines")
        .arg(MIN_LINES.to_string())
        .arg("--min-tokens")
        .arg(MIN_TOKENS.to_string())
        .arg("--ignore")
        .arg(IGNORE_GLOBS)
        // Scan the whole tree's Rust. Since paths then report relative to the scan root, the
        // allowlist's `path:start` entries are repo-relative (`crates/...`, `xtask/...`).
        .arg(root);

    let output = cmd.output().map_err(|e| format!("could not run jscpd: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "jscpd exited {}:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let report_path = out.join("jscpd-report.json");
    let text = std::fs::read_to_string(&report_path).map_err(|e| format!("could not read {}: {e}", report_path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("jscpd JSON report was not valid: {e}"))?;
    parse_report(&value)
}

/// Parse jscpd's `json` report into [`Duplicate`]s. Hand-parsed over `serde_json::Value`
/// rather than `#[derive(Deserialize)]` because the derive would add `serde` as an xtask
/// dependency for one struct; this shape is stable enough that the read is worth the lines.
fn parse_report(value: &serde_json::Value) -> Result<Vec<Duplicate>, String> {
    let duplicates = value
        .get("duplicates")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "report has no `duplicates` array".to_owned())?;
    let mut out = Vec::with_capacity(duplicates.len());
    for d in duplicates {
        let frag = |key: &str| -> Result<Fragment, String> {
            let f = d.get(key).ok_or_else(|| format!("duplicate is missing `{key}`"))?;
            let name = f
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("`{key}.name` is not a string"))?
                .to_owned();
            let start = f
                .get("start")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| format!("`{key}.start` is not a number"))?;
            Ok(Fragment { name, start })
        };
        let first = frag("firstFile")?;
        let second = frag("secondFile")?;
        let lines = d.get("lines").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let tokens = d.get("tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
        out.push(Duplicate {
            first,
            second,
            lines,
            tokens,
        });
    }
    Ok(out)
}

/// One allowlisted fragment's identity: its repo-relative path and 1-based start line.
type AllowEntry = (String, u64);

/// The parsed allowlist: every granted clone as a normalised (path, start) pair.
#[derive(Default)]
struct Allowlist(Vec<[AllowEntry; 2]>);

/// Is this clone excused by the allowlist? Matching is on both fragments' `path:start-line`,
/// in either order, so transposing two files grants the same clone.
fn allowed(d: &Duplicate, allow: &Allowlist) -> bool {
    let a = key(&d.first);
    let b = key(&d.second);
    allow
        .0
        .iter()
        .any(|pair| (pair[0] == a && pair[1] == b) || (pair[0] == b && pair[1] == a))
}

fn key(f: &Fragment) -> AllowEntry {
    (f.name.clone(), f.start)
}

/// Load `devco/dup-ignore`. A malformed entry is an error (fail-closed): a policy the gate
/// cannot read grants nothing and should say so, mirroring the allowlist-discipline rules.
///
/// The documented syntax is `<path>:<start> == <path>:<start> : <reason>` - the trailing
/// ` : <reason>` is REQUIRED, and it is split off before the `==`, so a reason may itself contain
/// an arrow without being mis-parsed (the old code split on `==` first and never read the reason).
/// Each fragment must name a real file `path` (a stale entry fails, there is no inert state) and
/// must not be first-party source under `crates/` or `xtask/`.
fn load_allowlist(root: &Path, tree_files: &[String]) -> Result<Allowlist, String> {
    let path = root.join(IGNORE_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {IGNORE_FILE}: {e}"))?;
    let mut out = Allowlist::default();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // The reason after ` : ` is required, and is peeled off BEFORE the `==` split so it is
        // never confused with the fragment separator.
        let Some((fragments, reason)) = line.split_once(" : ") else {
            return Err(format!(
                "{IGNORE_FILE}:{}: expected `<path>:<start> == <path>:<start> : <reason>` (reason after ` : ` is required)",
                idx + 1
            ));
        };
        if reason.trim().is_empty() {
            return Err(format!("{IGNORE_FILE}:{}: the reason after ` : ` cannot be empty", idx + 1));
        }
        let Some((left, right)) = fragments.split_once("==") else {
            return Err(format!(
                "{IGNORE_FILE}:{}: expected `<path>:<start> == <path>:<start> : <reason>`",
                idx + 1
            ));
        };
        let parse = |s: &str| -> Result<AllowEntry, String> {
            let s = s.trim();
            let (path, start) = s
                .rsplit_once(':')
                .ok_or_else(|| format!("{IGNORE_FILE}:{}: fragment `{s}` has no `:start`", idx + 1))?;
            let start: u64 = start
                .parse()
                .map_err(|_invalid| format!("{IGNORE_FILE}:{}: `{start}` is not a line number", idx + 1))?;
            Ok((path.to_owned(), start))
        };
        let (left_path, left_start) = parse(left)?;
        let (right_path, right_start) = parse(right)?;
        for p in [&left_path, &right_path] {
            if is_unexemptable(p) {
                return Err(format!(
                    "{IGNORE_FILE}:{}: `{p}` is first-party source under `crates/` or `xtask/` and cannot be exempted; split the file instead",
                    idx + 1
                ));
            }
            // Path-level stale check, mirroring `max_lines::inert_entries`: an entry naming a file
            // that is not in the tree is a promise that outlived its subject (renamed or deleted),
            // so it fails rather than lying inert. Path only - not start line - to avoid churn when
            // an audited clone's start line shifts.
            if !tree_files.iter().any(|f| f == p) {
                return Err(format!(
                    "{IGNORE_FILE}:{}: `{p}` names no file in the tree - it was renamed or deleted and the exemption outlived it",
                    idx + 1
                ));
            }
        }
        out.0.push([(left_path, left_start), (right_path, right_start)]);
    }
    Ok(out)
}

/// Can this path be excused by [`IGNORE_FILE`]? First-party source under `crates/` or `xtask/`
/// cannot - mirroring `max_lines::is_unexemptable` (`UNEXEMPTABLE_PREFIXES` there), so an
/// allowlist that swallows first-party code is a gate that has quietly stopped gating.
fn is_unexemptable(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    UNEXEMPTABLE_PREFIXES.iter().any(|prefix| normalized.starts_with(prefix))
}

/// Resolve `jscpd` from PATH (a plain name, or an absolute path if a caller passes one).
fn which(name: &str) -> Option<std::path::PathBuf> {
    if name.contains('/') {
        return std::path::Path::new(name).exists().then(|| std::path::PathBuf::from(name));
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|p| p.join(name)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag(name: &str, start: u64) -> Fragment {
        Fragment {
            name: name.into(),
            start,
        }
    }

    fn dup(a: Fragment, b: Fragment) -> Duplicate {
        Duplicate {
            first: a,
            second: b,
            lines: 6,
            tokens: 60,
        }
    }

    #[test]
    fn an_unexcused_clone_is_a_finding() {
        let allow = Allowlist::default();
        let d = dup(frag("crates/a.rs", 10), frag("crates/b.rs", 12));
        assert!(!allowed(&d, &allow));
    }

    #[test]
    fn a_granted_clone_is_excused_in_either_order() {
        let allow = Allowlist(vec![[("crates/a.rs".into(), 10), ("crates/b.rs".into(), 12)]]);
        let d = dup(frag("crates/a.rs", 10), frag("crates/b.rs", 12));
        assert!(allowed(&d, &allow));
        let swapped = dup(frag("crates/b.rs", 12), frag("crates/a.rs", 10));
        assert!(allowed(&swapped, &allow));
    }

    #[test]
    fn an_entry_for_a_different_start_line_does_not_excuse() {
        let allow = Allowlist(vec![[("crates/a.rs".into(), 10), ("crates/b.rs".into(), 12)]]);
        let d = dup(frag("crates/a.rs", 10), frag("crates/b.rs", 40));
        assert!(!allowed(&d, &allow));
    }

    #[test]
    fn a_comment_only_allowlist_grants_nothing() {
        let allow = load_allowlist_with("comment", "# just a comment\n", &[]);
        assert_eq!(allow.0.len(), 0);
    }

    #[test]
    fn a_documented_entry_parses_with_its_reason() {
        let allow = load_allowlist_with(
            "doc",
            "dev/a.rs:10 == dev/b.rs:12 : intentional mirror of the tokeniser\n",
            &["dev/a.rs", "dev/b.rs"],
        );
        assert_eq!(allow.0.len(), 1);
        assert_eq!(allow.0[0], [("dev/a.rs".into(), 10), ("dev/b.rs".into(), 12)]);
    }

    #[test]
    fn a_reason_may_contain_an_arrow() {
        // The reason is peeled off before the `==` split, so ` == ` inside the reason must not
        // mis-split the line.
        let allow = load_allowlist_with(
            "arrow",
            "dev/a.rs:10 == dev/b.rs:12 : mirror == template, both handwritten\n",
            &["dev/a.rs", "dev/b.rs"],
        );
        assert_eq!(allow.0.len(), 1);
    }

    #[test]
    fn an_entry_without_a_reason_fails_closed() {
        let result = load_allowlist_result("noreason", "dev/a.rs:10 == dev/b.rs:12\n", &["dev/a.rs", "dev/b.rs"]);
        assert!(result.is_err(), "a missing reason must be rejected");
    }

    #[test]
    fn an_entry_with_an_empty_reason_fails_closed() {
        let result = load_allowlist_result("emptyreason", "dev/a.rs:10 == dev/b.rs:12 :   \n", &["dev/a.rs", "dev/b.rs"]);
        assert!(result.is_err(), "a blank reason must be rejected");
    }

    #[test]
    fn an_entry_without_a_separator_fails_closed() {
        let result = load_allowlist_result("nosep", "dev/a.rs:10 dev/b.rs:12 : reason\n", &["dev/a.rs", "dev/b.rs"]);
        assert!(result.is_err(), "a line without ` == ` must be rejected");
    }

    #[test]
    fn first_party_source_cannot_be_exempted() {
        let result = load_allowlist_result(
            "firstparty",
            "crates/a.rs:10 == dev/b.rs:12 : reason\n",
            &["crates/a.rs", "dev/b.rs"],
        );
        assert!(result.is_err(), "a `crates/` fragment must be refused");
        assert!(is_unexemptable("crates/sutura-domain/src/lib.rs"));
        assert!(is_unexemptable("./xtask/src/main.rs"));
        assert!(!is_unexemptable("dev/a.rs"));
    }

    #[test]
    fn an_entry_naming_a_file_absent_from_the_tree_fails() {
        let result = load_allowlist_result("stale", "dev/a.rs:10 == dev/gone.rs:12 : reason\n", &["dev/a.rs"]);
        assert!(result.is_err(), "an entry naming a missing file must be rejected");
    }

    #[test]
    fn census_refuses_an_empty_scan() {
        assert!(check_census(&[], IGNORE_GLOBS).is_err());
    }

    #[test]
    fn census_refuses_an_ignore_glob_that_matches_source() {
        // The trap the floor closes: widening `IGNORE_GLOBS` so it covers real `.rs` would make a
        // clone in the skipped half invisible. This must refuse.
        assert!(check_census(&["crates/sutura-domain/src/lib.rs"], "crates/**").is_err());
        assert!(check_census(&["dev/a.rs"], "dev/**").is_err());
    }

    #[test]
    fn census_passes_on_real_rust_outside_the_ignore_globs() {
        assert_eq!(check_census(&["crates/a.rs", "dev/b.rs"], IGNORE_GLOBS), Ok(()));
    }

    #[test]
    fn the_temp_work_dir_is_removed_when_the_guard_drops() {
        let path = {
            let work = TempWorkDir::create().expect("scratch temp-dir creation");
            let p = work.path().to_path_buf();
            assert!(p.exists(), "the temp dir must exist while the guard is alive");
            p
        };
        // `work` dropped here (end of the block), so `Drop` must have removed the directory.
        assert!(!path.exists(), "the temp working dir must be removed when the guard drops");
    }

    #[test]
    fn run_gate_tier2_globs_match_the_gate_owner() {
        // The tier-2 arm must hand `IGNORE_GLOBS` AND the MIN_LINES / MIN_TOKENS / FORMAT
        // thresholds to bare jscpd; a subset silently scans a different tree, and a laxer
        // threshold scans a different standard. `nix/run-gate.sh` cannot import the consts, so
        // this gate re-parses the anchored invocation line and refuses a drift.
        //
        // The agreeing fixture is BUILT from `IGNORE_GLOBS` and the threshold consts (not a third
        // hardcoded copy), so widening the const in both real owners stays green here - the drift
        // assertions below are where the real owners are held, not in a literal that duplicates
        // the list.
        let agreeing = format!(
            "exec nix run .#jscpd -- --silent --no-colors --format {FORMAT} --min-lines {MIN_LINES} \
             --min-tokens {MIN_TOKENS} --ignore '{IGNORE_GLOBS}' .\n"
        );
        globs_agree(&agreeing).expect("the agreeing set must pass");

        // The old subset that #483 shipped (missing `result-*/**` and `**/.prek-cache/**`) IS the
        // glob drift this holds: either side diverging must refuse.
        let drifted = "exec nix run .#jscpd -- --silent --no-colors --format rust --min-lines 30 \
                       --min-tokens 250 --ignore 'target/**,site/**,result/**,.pixi/**,.sutura-dev/**,report/**' .\n";
        assert!(globs_agree(drifted).is_err());

        // A mutation to a laxer threshold on the SAME line must also refuse - previously a silent
        // pass, because only the `--ignore` list was compared (#506).
        let lax_threshold = format!(
            "exec nix run .#jscpd -- --silent --no-colors --format {FORMAT} --min-lines 900 \
             --min-tokens {MIN_TOKENS} --ignore '{IGNORE_GLOBS}' .\n"
        );
        assert!(globs_agree(&lax_threshold).is_err(), "a laxer --min-lines must refuse");

        // The anchor must be the INVOCATION LINE, not a whole-file `find`: a comment above the arm
        // that quotes the same flag (or any decoy in another arm) must not move the parse.
        let comment_above = format!("# nix run .#jscpd -- --format rust --min-lines 900 --ignore '{IGNORE_GLOBS}'\n{agreeing}");
        globs_agree(&comment_above).expect("a comment carrying the same tokens must be ignored");
        let decoy_arm = format!("exec other-tool -- --format rust --min-lines 900 --ignore 'decoy/**' .\n{agreeing}");
        globs_agree(&decoy_arm).expect("a decoy arm carrying a laxer line must be ignored");

        // An arm that no longer passes `--ignore '<globs>'` cannot be verified at all.
        assert!(globs_agree("exec nix run .#jscpd -- .\n").is_err());
    }

    #[test]
    fn decide_passes_on_no_clones() {
        assert_eq!(decide(&[], &Allowlist::default()), Verdict::Pass);
    }

    #[test]
    fn decide_fails_on_an_unexcused_clone() {
        let d = dup(frag("dev/a.rs", 10), frag("dev/b.rs", 12));
        assert_eq!(decide(&[d], &Allowlist::default()), Verdict::Fail);
    }

    #[test]
    fn decide_passes_on_an_excused_clone() {
        let allow = Allowlist(vec![[("dev/a.rs".into(), 10), ("dev/b.rs".into(), 12)]]);
        let d = dup(frag("dev/a.rs", 10), frag("dev/b.rs", 12));
        assert_eq!(decide(&[d], &allow), Verdict::Pass);
    }

    /// Parse `content` at a scratch `devco/dup-ignore` against `tree_files`, panicking on error -
    /// for entries that are expected to load.
    fn load_allowlist_with(tag: &str, content: &str, tree_files: &[&str]) -> Allowlist {
        load_allowlist_result(tag, content, tree_files).expect("scratch allowlist must parse")
    }

    /// Parse `content` at a scratch `devco/dup-ignore` against `tree_files`, returning the error -
    /// for entries that are expected to be rejected.
    fn load_allowlist_result(tag: &str, content: &str, tree_files: &[&str]) -> Result<Allowlist, String> {
        // Point at a scratch directory (creating the `devco/` subdir IGNORE_FILE lives under)
        // rather than touching the real repo tree. `tag` keeps parallel tests from sharing a dir.
        let dir = std::env::temp_dir().join(format!(
            "jscpd-test-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        ));
        let devco = dir.join("devco");
        std::fs::create_dir_all(&devco).map_err(|e| e.to_string())?;
        std::fs::write(devco.join("dup-ignore"), content).map_err(|e| e.to_string())?;
        let files: Vec<String> = tree_files.iter().map(ToString::to_string).collect();
        let out = crate::jscpd::load_allowlist(&dir, &files);
        let _removed = std::fs::remove_dir_all(&dir);
        out
    }
}

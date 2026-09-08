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

use std::path::Path;
use std::process::Command;

use crate::{Verdict, repo};

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

    let allow = match load_allowlist(&root) {
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

    // A clone is a finding unless the allowlist grants it an audited reason.
    let findings: Vec<Duplicate> = clones.iter().filter(|c| !allowed(c, &allow)).cloned().collect();

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

/// Run jscpd over [`SCAN`] and return the clones it found.
fn scan(root: &Path, jscpd: &Path) -> Result<Vec<Duplicate>, String> {
    let out = std::env::temp_dir().join(format!(
        "jscpd-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    ));
    std::fs::create_dir_all(&out).map_err(|e| format!("could not create temp dir: {e}"))?;

    let mut cmd = Command::new(jscpd);
    cmd.arg("--silent")
        .arg("--no-colors")
        .arg("--reporters")
        .arg("json")
        .arg("--output")
        .arg(&out)
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
fn load_allowlist(root: &Path) -> Result<Allowlist, String> {
    let path = root.join(IGNORE_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {IGNORE_FILE}: {e}"))?;
    let mut out = Allowlist::default();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // Split the two fragments on ` == `.
        let Some((left, right)) = line.split_once("==") else {
            return Err(format!(
                "{IGNORE_FILE}:{}: expected `<path>:<start> == <path>:<start> [ : <reason>]`",
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
        out.0.push([parse(left)?, parse(right)?]);
    }
    Ok(out)
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
        let allow = load_allowlist_with("comment", "# just a comment\n");
        assert_eq!(allow.0.len(), 0);
    }

    fn load_allowlist_with(tag: &str, content: &str) -> Allowlist {
        // Point at a scratch directory (creating the `devco/` subdir IGNORE_FILE lives under)
        // rather than touching the real repo tree. `tag` keeps parallel tests from sharing a dir.
        let dir = std::env::temp_dir().join(format!("jscpd-test-{}-{tag}", std::process::id()));
        let devco = dir.join("devco");
        std::fs::create_dir_all(&devco).unwrap();
        std::fs::write(devco.join("dup-ignore"), content).unwrap();
        let out = crate::jscpd::load_allowlist(&dir).expect("scratch allowlist must parse");
        let _removed = std::fs::remove_dir_all(&dir);
        out
    }

    #[test]
    fn an_entry_without_a_separator_fails_closed() {
        let dir = std::env::temp_dir().join(format!("jscpd-bad-{}", std::process::id()));
        let devco = dir.join("devco");
        std::fs::create_dir_all(&devco).unwrap();
        std::fs::write(devco.join("dup-ignore"), "crates/a.rs:10 crates/b.rs:12 : reason\n").unwrap();
        let result = crate::jscpd::load_allowlist(&dir);
        let _removed = std::fs::remove_dir_all(&dir);
        match result {
            Err(_) => {}
            Ok(_) => panic!("a malformed allowlist entry must be rejected"),
        }
    }
}

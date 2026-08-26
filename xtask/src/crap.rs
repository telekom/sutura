//! The CRAP gate: cyclomatic complexity weighted by the tests that cover it.
//!
//! CRAP(f) = CC(f)^2 * (1 - coverage(f))^3 + CC(f). A branchy function with tests scores near
//! its complexity; the same function untested scores an order of magnitude higher. It is the one
//! number that will not let "complex but fine" and "untested but trivial" cancel out.
//!
//! TWO TASKS, split by what they cost, because the expensive half cannot run everywhere:
//!
//!   `check-crap`  Hygiene. Reads files. Validates the policy, the allowlist discipline and the
//!                 scope, and never compiles anything. Runs on every commit and in the Nix
//!                 sandbox, where there is no network and no `.git`.
//!   `crap`        Standalone. Runs `cargo llvm-cov` over the scoped packages and scores the
//!                 result with `cargo crap`. Needs both tools; says so and FAILS when either is
//!                 missing, because a gate whose tool is absent must not report success.
//!
//! WHY THE VERDICT IS COMPUTED HERE and not taken from `cargo crap --fail-above`'s exit code.
//! Two reasons, and the first is the scar this repo already carries: a gate that "listed files
//! via an absent tool and got an empty list" passed while checking nothing. `cargo crap` exits 0
//! on an empty report, so an analysis that found no functions at all - a renamed crate, a moved
//! source root, a walker that matched nothing - would read as clean. Reading the JSON and
//! refusing an empty or under-populated report is the only way that hole closes. The second is
//! that a pure function over parsed JSON is unit-testable with no tool installed at all, which
//! is what makes this gate itself tested rather than merely written.
//!
//! THE COVERAGE RUN IS ON STABLE, AND THIS TASK MAKES ITSELF SO rather than trusting its caller.
//!
//! Coverage instrumentation is LLVM-specific: under the cranelift backend `-C
//! instrument-coverage` does not exist, so a run that inherited the dev shell's nightly-plus-
//! cranelift cargo would fail - or worse, produce an LCOV with no counters, which would score
//! every function as uncovered and read as a catastrophe rather than as a broken run. That is a
//! correctness requirement, not the channel-consistency preference the lints have.
//!
//! HOW CRANELIFT ARRIVES HERE, checked rather than assumed. In this repo it is two environment
//! variables, `CARGO_UNSTABLE_CODEGEN_BACKEND` and `CARGO_PROFILE_DEV_CODEGEN_BACKEND`, set by
//! `devenv.nix`. There is no `RUSTFLAGS` and no `CARGO_TARGET_*_RUSTFLAGS` in the dev shell at
//! all - verified by reading the environment on both sides of `nix/stable-env.sh` - and the
//! per-target tables in `.cargo/config.toml` carry linker and target-feature flags only. So
//! `stable-env.sh`, which unsets exactly those two variables, already covers the case jubust's
//! gate script strips per-target rustflags for.
//!
//! [`coverage_env`] hardens it anyway, and the reason is not belt-and-braces: this gate is the
//! one whose failure mode is a silent empty report, so it must not depend on a caller having
//! remembered a `source` line. It unsets the two variables, strips the cranelift flag out of any
//! rustflags variable that does carry it while preserving the linker and library flags beside
//! it, pins `RUSTUP_TOOLCHAIN` to the channel in `rust-toolchain.toml`, and puts
//! `SUTURA_STABLE_BIN` in front of `PATH` when the dev shell exported one. The instrumented
//! profile also gets its own target directory, because it shares nothing with the cranelift
//! artifacts the inner loop depends on and this repo's target directory is already tens of
//! gigabytes.

use std::path::Path;
use std::process::Command;

use crate::Verdict;
use crate::repo;

mod report;

use report::{AllowEntry, Entry, POLICY_FILE, Report, offenders, parse_policy, read_report, value_of};

/// The packages the coverage run covers, and the ONLY packages scored.
///
/// `sutura-domain` alone, and this is a measured decision rather than a cautious one.
///
/// THE COST OF THE COVERAGE STEP, measured in the dev container on 16 cores, from a cold
/// coverage profile each time. This is the number that varies with the scope, which is why it is
/// the one tabulated:
///
/// | scope                                          | wall       | CPU      |
/// | ---------------------------------------------- | ---------- | -------- |
/// | `sutura-domain`                                | 11.1 s     | 50.8 s   |
/// | plus `sutura-catalog-local`, `sutura-semantic` | 3 m 27 s   | 15 m 5 s |
/// | `--workspace`                                  | > 6 m 18 s | > 80 m   |
///
/// End to end this task is 42 s cold and about 20 s warm at the scope below: the coverage step
/// plus roughly 2 s of AST analysis and the xtask build itself. Well inside the commit-stage
/// budget this repo already spends on workspace clippy and the full test run.
///
/// The workspace row is a lower bound because that run never reached a test: `DataFusion`, Arrow
/// and `DuckDB` have to be recompiled with `-C instrument-coverage`, which shares nothing with
/// the cached ordinary profile. On a four-vCPU runner that is twenty minutes and up, added to a
/// `ci` job whose cap was already raised from 60 to 120 minutes. It is not affordable, and a
/// gate nobody can afford gets deleted.
///
/// THE OTHER HALF OF THE DECISION, and it is not about cost. Coverage scoped to one package sees
/// only THAT package's tests. For `sutura-domain` that is the whole truth: its 120 unit tests are
/// its real test suite and they live in the crate. For `sutura-semantic` it is not: its real
/// tests are the golden corpus in `sutura-app`, so a `-p sutura-semantic` coverage run reports
/// `resolve` at 0% and scores it CRAP 210 - measured, not hypothesised. Seven such functions
/// appear the moment that crate is added. They are artefacts of where the tests live, not
/// findings, and a gate that cries wolf seven times gets switched off.
///
/// So the rule for adding a name here is not "is it cheap". It is: does this package's own test
/// suite exercise its own code? Until the golden suite can be reached without `DataFusion` and
/// `DuckDB` in the coverage profile, `sutura-domain` is the only package for which the answer is
/// yes.
///
/// WHAT THAT MISSES, stated plainly: every dialect renderer, the resolver, the planner, the
/// catalog loader and both adapters. This gate covers the invariant core and nothing else.
pub(crate) const SCOPE: &[&str] = &["sutura-domain"];

/// Where the two tool versions are pinned. One file, imported by flake.nix and devenv.nix.
const PIN_FILE: &str = "nix/crap.nix";

/// The documentation page whose stated version must match the pin.
const DOC_PAGE: &str = "docs/crap.md";
// ------------------------------------------------------------- check-crap (hygiene) ---

/// The cheap half: is the policy a policy, and does the scope name real packages?
pub(crate) fn run_check(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-crap: could not locate the repo root");
        return Verdict::Fail;
    };

    let text = match std::fs::read_to_string(root.join(POLICY_FILE)) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-crap: could not read {POLICY_FILE}: {error}");
            eprintln!("  The CRAP gate has no policy without it, and a gate with no policy is not a gate.");
            return Verdict::Fail;
        }
    };
    let policy = match parse_policy(&text) {
        Ok(policy) => policy,
        Err(reason) => {
            eprintln!("xtask check-crap: {reason}");
            return Verdict::Fail;
        }
    };

    let unannotated: Vec<&AllowEntry> = policy.allow.iter().filter(|entry| !entry.annotated).collect();
    if !unannotated.is_empty() {
        eprintln!("xtask check-crap: these allowlist entries say what, but not why\n");
        for entry in &unannotated {
            eprintln!("  {POLICY_FILE}:{}  {}", entry.line, entry.pattern);
        }
        eprintln!();
        eprintln!("Put a `#` comment on the line or immediately above it saying why the function or");
        eprintln!("file is exempt. Same rule deny.toml's licence list follows, for the same reason: an");
        eprintln!("unjustified entry becomes pre-approval for whatever is added next to it.");
        return Verdict::Fail;
    }

    if SCOPE.is_empty() {
        eprintln!("xtask check-crap: SCOPE is empty, so `cargo xtask crap` would score nothing");
        return Verdict::Fail;
    }
    if let Err(reason) = scope_names_real_packages() {
        eprintln!("xtask check-crap: {reason}");
        return Verdict::Fail;
    }
    if let Err(reason) = pinned_version_matches_the_docs(&root) {
        eprintln!("xtask check-crap: {reason}");
        return Verdict::Fail;
    }

    println!(
        "xtask check-crap: ok - threshold {}, {} allowlist entry(ies), scope: {}",
        policy.threshold,
        policy.allow.len(),
        SCOPE.join(", ")
    );
    Verdict::Pass
}

/// Every name in [`SCOPE`] is a workspace member.
///
/// A renamed crate would otherwise narrow the expensive gate silently. This turns it into a
/// cheap failure on every commit instead of a message somebody has to be watching a CI log for.
fn scope_names_real_packages() -> Result<(), String> {
    let metadata = crate::cargo_metadata(&["--no-deps"])?;
    let packages = metadata
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| String::from("cargo metadata reported no packages"))?;
    let names: Vec<&str> = packages
        .iter()
        .filter_map(|p| p.get("name").and_then(serde_json::Value::as_str))
        .collect();
    // An empty list here would make the check below pass vacuously, which is the failure mode a
    // membership test is most prone to.
    if names.is_empty() {
        return Err(String::from(
            "cargo metadata named no packages - the check is broken, not the scope",
        ));
    }
    let missing: Vec<&str> = SCOPE.iter().copied().filter(|wanted| !names.contains(wanted)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "SCOPE in xtask/src/crap.rs names packages this workspace does not have: {}",
        missing.join(", ")
    ))
}

/// The cargo-crap version in `nix/crap.nix` is the one the documentation states.
///
/// Documented versions rot: `check-guidance` exists because this repo has already published a
/// compiler version two releases old. The pin is the authority and the page has to follow it.
fn pinned_version_matches_the_docs(root: &Path) -> Result<(), String> {
    let pin = std::fs::read_to_string(root.join(PIN_FILE)).map_err(|error| format!("could not read {PIN_FILE}: {error}"))?;
    let version = pin
        .lines()
        .find_map(|line| value_of(line.trim(), "crapVersion"))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{PIN_FILE} declares no `crapVersion`"))?;

    let page = std::fs::read_to_string(root.join(DOC_PAGE)).map_err(|error| format!("could not read {DOC_PAGE}: {error}"))?;
    if page.contains(&version) {
        return Ok(());
    }
    Err(format!(
        "{PIN_FILE} pins cargo-crap {version}, and {DOC_PAGE} does not mention that version"
    ))
}

// ------------------------------------------------------------- crap (standalone) ---

/// The expensive half: coverage, then scoring.
///
/// Four stages, each its own function. Not decomposition for its own sake: every stage has its
/// OWN refusal, and three of the four are ways this gate could otherwise pass while having
/// judged nothing. Naming them separately is what makes each one reviewable.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask crap: could not locate the repo root");
        return Verdict::Fail;
    };
    let policy = match std::fs::read_to_string(root.join(POLICY_FILE))
        .map_err(|error| format!("could not read {POLICY_FILE}: {error}"))
        .and_then(|text| parse_policy(&text))
    {
        Ok(policy) => policy,
        Err(reason) => {
            eprintln!("xtask crap: {reason}");
            return Verdict::Fail;
        }
    };
    if !tools_are_runnable() {
        return Verdict::Fail;
    }

    // Under `target/` so it is gitignored and swept by the same `cargo clean` as everything
    // else. A dedicated name because the instrumented profile shares nothing with the ordinary
    // one, and mixing them invalidates both on every alternation.
    let work = root.join("target").join("crap");
    if let Err(error) = std::fs::create_dir_all(&work) {
        eprintln!("xtask crap: could not create {}: {error}", work.display());
        return Verdict::Fail;
    }
    let lcov = work.join("coverage.lcov.info");
    let report = work.join("report.json");

    if let Err(reason) = measure_coverage(&root, &work, &lcov) {
        eprintln!("xtask crap: {reason}");
        return Verdict::Fail;
    }
    let entries = match score_coverage(&root, &work, &lcov, &report, policy.threshold) {
        Ok(entries) => entries,
        Err(reason) => {
            eprintln!("xtask crap: {reason}");
            return Verdict::Fail;
        }
    };
    verdict(&entries, policy.threshold)
}

/// Both tools run, or the gate fails saying so.
///
/// A FAILURE and not a skip. The tiering that turns an absent tool into a notice lives in
/// `nix/run-gate.sh`, which is the bypassable layer; here, a run that produced no score has
/// judged nothing and must not report success.
fn tools_are_runnable() -> bool {
    for tool in ["llvm-cov", "crap"] {
        if !tool_present(tool) {
            eprintln!("xtask crap: `cargo {tool}` does not run here.");
            eprintln!();
            eprintln!("  Both tools are pinned by {PIN_FILE} and reachable three ways:");
            eprintln!("    devenv shell -- crap      inside the dev shell");
            eprintln!("    nix run .#crap            with nix and no dev shell");
            eprintln!("    just crap                 whichever of those is available");
            eprintln!();
            eprintln!("  This is a FAILURE and not a skip: the point of the gate is the score,");
            eprintln!("  and a run that produced none has judged nothing.");
            return false;
        }
    }
    true
}

/// Run the tests under coverage and prove the result says something.
fn measure_coverage(root: &Path, work: &Path, lcov: &Path) -> Result<(), String> {
    println!("xtask crap: coverage over {}", SCOPE.join(", "));
    let mut coverage = Command::new(cargo());
    coverage.current_dir(root).arg("llvm-cov").arg("nextest");
    for name in SCOPE {
        coverage.arg("-p").arg(name);
    }
    coverage.arg("--all-features").arg("--lcov").arg("--output-path").arg(lcov);
    coverage_env(&mut coverage, root, &work.join("target"));
    match coverage.status() {
        Ok(status) if status.success() => {}
        Ok(status) => return Err(format!("the coverage run failed ({status}). Nothing was scored.")),
        Err(error) => return Err(format!("could not run cargo llvm-cov: {error}")),
    }
    // An LCOV that exists and says nothing is the same hole as an empty report, one step
    // earlier, and it is cheaper to name here than to infer from a strange score later. Without
    // this, instrumentation that produced no data would score every function as uncovered - or,
    // under a `skip` policy, as absent - and either reading is a lie about the tests.
    match std::fs::read_to_string(lcov) {
        Ok(text) if text.contains("SF:") => Ok(()),
        Ok(_) => Err(format!(
            "the coverage run wrote no source records to {}.\n  Refused rather than scored: \
             every function would read as uncovered.",
            lcov.display()
        )),
        Err(error) => Err(format!("could not read {}: {error}", lcov.display())),
    }
}

/// Score the LCOV and refuse a report that proves nothing.
fn score_coverage(root: &Path, work: &Path, lcov: &Path, report: &Path, threshold: f64) -> Result<Vec<Entry>, String> {
    println!("xtask crap: scoring against threshold {threshold}");
    let mut score = Command::new(cargo());
    score.current_dir(root).arg("crap");
    for name in SCOPE {
        score.arg("-p").arg(name);
    }
    score
        .arg("--lcov")
        .arg(lcov)
        .arg("--format")
        .arg("json")
        .arg("--output")
        .arg(report);
    // `cargo crap` shells out to `cargo metadata` for `-p`, so an unstable flag left in the
    // environment would fail it here just as surely as in the build above.
    coverage_env(&mut score, root, &work.join("target"));
    // NOT checked for success: `fail-above` in the policy file makes a non-zero exit the NORMAL
    // outcome when something is over the line, and the report is written either way. The verdict
    // comes out of the report, which is the whole reason it is read.
    if let Err(error) = score.status() {
        return Err(format!("could not run cargo crap: {error}"));
    }

    let json = std::fs::read_to_string(report).map_err(|error| format!("could not read {}: {error}", report.display()))?;
    match read_report(&json, SCOPE) {
        Report::Scored(entries) => Ok(entries),
        Report::Empty(reason) => Err(format!(
            "{reason}\n\n  Refused rather than reported clean. A gate that checked nothing and \
             said `ok` is the failure this repo has already had once."
        )),
    }
}

/// The verdict, and the report a person reads when it is a failure.
fn verdict(entries: &[Entry], threshold: f64) -> Verdict {
    let over = offenders(entries, threshold);
    if over.is_empty() {
        println!(
            "xtask crap: ok - {} function(s) scored, none over CRAP {threshold}",
            entries.len()
        );
        return Verdict::Pass;
    }

    eprintln!();
    eprintln!(
        "xtask crap: {} of {} function(s) exceed CRAP {threshold}\n",
        over.len(),
        entries.len()
    );
    eprintln!("     CRAP   CC     COV  FUNCTION");
    for entry in over {
        eprintln!(
            "  {:>7.1}  {:>3.0}  {:>5.1}%  {}",
            entry.crap, entry.cyclomatic, entry.coverage, entry.function
        );
        eprintln!("           {}:{}", short_path(&entry.file), entry.line);
    }
    eprintln!();
    eprintln!("The fix is a TEST, not an allowlist entry. At this threshold a function needs");
    eprintln!("roughly 30% coverage at CC 6 and 55% at CC 10 to pass, and a function above");
    eprintln!("CC {threshold} cannot pass at any coverage and has to be split.");
    eprintln!();
    eprintln!("{POLICY_FILE} says what belongs in `allow` and what does not. An entry there needs");
    eprintln!("a comment saying why, and `cargo xtask check-crap` fails without one.");
    Verdict::Fail
}

/// The cargo that invoked us, else `cargo`. Same reasoning as `crate::cargo_metadata`: `env!`
/// would bake a store path in at compile time.
fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"))
}

/// Every rustflags variable cargo will read. Cargo takes the FIRST match and ignores the rest,
/// so all of them have to be cleaned rather than only the one this repo happens to use.
const RUSTFLAG_VARS: &[&str] = &[
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
    "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
    "CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS",
    "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS",
];

/// The nightly-only flag that selects the cranelift backend, in both spellings cargo accepts.
const CRANELIFT_FLAGS: &[&str] = &["-Zcodegen-backend=cranelift", "-Z codegen-backend=cranelift"];

/// Drop the cranelift backend selection and keep everything else.
///
/// Word-wise rather than by string replacement: `CARGO_ENCODED_RUSTFLAGS` separates flags with
/// `\x1f`, and a naive `replace` on the space-separated spelling would corrupt it. Linker and
/// library flags beside it - `-C link-arg=-fuse-ld=lld`, `-L native=...` - are what a coverage
/// build still needs, so they survive.
pub(crate) fn strip_cranelift(value: &str) -> String {
    let separator = if value.contains('\u{1f}') { '\u{1f}' } else { ' ' };
    let kept: Vec<&str> = value
        .split(separator)
        .filter(|part| {
            let trimmed = part.trim();
            // `-Z` and `codegen-backend=cranelift` can arrive as two words. Dropping the bare
            // `-Z` as well is safe here: nothing else in this repo passes one.
            !CRANELIFT_FLAGS.iter().any(|flag| flag.split(' ').any(|word| word == trimmed))
                && trimmed != "codegen-backend=cranelift"
        })
        .collect();
    kept.join(&separator.to_string())
}

/// The channel from `rust-toolchain.toml`, which is the single source of truth for it.
pub(crate) fn pinned_channel(toolchain_toml: &str) -> Option<String> {
    toolchain_toml
        .lines()
        .find_map(|line| value_of(line.trim(), "channel"))
        .filter(|value| !value.is_empty())
}

/// Put `command` on the stable toolchain with no cranelift, in its own target directory.
///
/// Applied to BOTH child processes, because `cargo crap` shells out to `cargo metadata` and an
/// unstable flag there would fail it just as surely.
fn coverage_env(command: &mut Command, root: &Path, target_dir: &Path) {
    // Cranelift, as this repo actually injects it.
    command.env_remove("CARGO_UNSTABLE_CODEGEN_BACKEND");
    command.env_remove("CARGO_PROFILE_DEV_CODEGEN_BACKEND");

    // Cranelift as a rustflag, which nothing here sets today. Cleaned rather than removed: the
    // linker selection lives in the same variable on a host that does set it.
    for name in RUSTFLAG_VARS {
        if let Ok(value) = std::env::var(name)
            && CRANELIFT_FLAGS.iter().any(|flag| value.contains(flag))
        {
            command.env(name, strip_cranelift(&value));
        }
    }

    // For a bare-rustup host. Inside the dev shell cargo comes from nix and ignores this, which
    // is why the PATH line below exists as well.
    if let Ok(text) = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        && let Some(channel) = pinned_channel(&text)
    {
        command.env("RUSTUP_TOOLCHAIN", channel);
    }
    if let Ok(stable_bin) = std::env::var("SUTURA_STABLE_BIN")
        && !stable_bin.is_empty()
    {
        let path = std::env::var("PATH").unwrap_or_default();
        command.env(
            "PATH",
            format!("{stable_bin}{}{path}", if path.is_empty() { "" } else { ":" }),
        );
    }

    // Its own directory. The instrumented profile shares nothing with the cranelift artifacts the
    // inner loop depends on, and alternating them in one directory invalidates both.
    command.env("CARGO_LLVM_COV_TARGET_DIR", target_dir);
    command.env("CARGO_TARGET_DIR", target_dir);
}

/// Is `cargo <subcommand>` runnable?
///
/// THROUGH CARGO, not by executing `cargo-<subcommand>` directly, and not by walking PATH. Both
/// alternatives were wrong. A PATH walk misses a subcommand provided by a shim. Executing the
/// binary directly and passing `--version` misses that a cargo subcommand expects its own name as
/// the first argument: `cargo-llvm-cov --version` fails while `cargo llvm-cov --version` succeeds,
/// so the direct form reported an installed tool as absent - the gate refusing to run on a host
/// where it would have worked. Asking cargo is also the only form that proves what the gate is
/// about to do, since that is how it invokes them.
fn tool_present(subcommand: &str) -> bool {
    Command::new(cargo())
        .arg(subcommand)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// `cargo crap` reports absolute paths. Trim to the crate-relative part so a failure message is
/// the same on a runner and on a laptop, and short enough to read.
fn short_path(path: &str) -> &str {
    path.split_once("crates/").map_or(path, |(_, rest)| rest)
}

#[cfg(test)]
mod tests {
    use super::short_path;

    #[test]
    fn the_scope_is_not_empty_and_names_no_duplicates() {
        // An empty scope would make the expensive gate a green no-op, which is the same failure
        // the empty-report refusal exists for, arrived at from the configuration side.
        assert!(!super::SCOPE.is_empty());
        let mut names: Vec<&str> = super::SCOPE.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), super::SCOPE.len(), "duplicate package in SCOPE");
    }

    #[test]
    fn every_scoped_package_is_a_workspace_member() {
        // The same assertion `check-crap` makes, as a unit test, so a rename fails the test
        // suite as well as the gate.
        super::scope_names_real_packages().expect("SCOPE must name real packages");
    }

    #[test]
    fn the_pinned_version_and_the_documented_one_agree() {
        let root = crate::repo::root().expect("repo root");
        super::pinned_version_matches_the_docs(&root).expect("pin and docs must agree");
    }

    #[test]
    fn the_cranelift_flag_is_stripped_and_the_linker_flags_are_not() {
        use super::strip_cranelift;
        // The whole point: instrumentation is LLVM-only, but the build still needs its linker.
        assert_eq!(
            strip_cranelift("-Zcodegen-backend=cranelift -C link-arg=-fuse-ld=lld"),
            "-C link-arg=-fuse-ld=lld"
        );
        // Both spellings cargo accepts, and the flag in the middle of a list.
        assert_eq!(
            strip_cranelift("-C target-feature=+lse -Z codegen-backend=cranelift -L native=/x"),
            "-C target-feature=+lse -L native=/x"
        );
        // CARGO_ENCODED_RUSTFLAGS is unit-separated; splitting on spaces would corrupt it.
        assert_eq!(
            strip_cranelift("-C\u{1f}link-arg=-fuse-ld=lld\u{1f}-Zcodegen-backend=cranelift"),
            "-C\u{1f}link-arg=-fuse-ld=lld"
        );
        // Nothing to strip is a no-op, not an empty string.
        assert_eq!(strip_cranelift("-C link-arg=-fuse-ld=lld"), "-C link-arg=-fuse-ld=lld");
    }

    #[test]
    fn the_pinned_channel_is_read_from_the_toolchain_file() {
        use super::pinned_channel;
        let root = crate::repo::root().expect("repo root");
        let text = std::fs::read_to_string(root.join("rust-toolchain.toml")).expect("toolchain");
        let channel = pinned_channel(&text).expect("rust-toolchain.toml must name a channel");
        // A stable channel, because coverage instrumentation is what this pins the run to. A
        // nightly here would mean the gate had quietly moved back onto the inner-loop compiler,
        // where `-C instrument-coverage` does not exist at all.
        assert!(
            !channel.contains("nightly"),
            "the coverage run must not be pinned to {channel}"
        );
        assert!(pinned_channel("[toolchain]\nprofile = \"minimal\"\n").is_none());
    }

    #[test]
    fn a_reported_path_is_trimmed_to_the_crate_relative_part() {
        assert_eq!(
            short_path("/home/x/.stax/wt/crates/sutura-domain/src/model.rs"),
            "sutura-domain/src/model.rs"
        );
        // A path with no `crates/` segment is left alone rather than mangled.
        assert_eq!(short_path("xtask/src/crap.rs"), "xtask/src/crap.rs");
    }
}

//! The pre-push stage runs ONLY the security checks, and none of them compiles.
//!
//! This gate is the mirror of the one it replaces. Before, `.pre-commit-config.yaml` carried a
//! `pre-push` clippy hook so that a `git rebase` - which runs no commit hook - could not push a
//! conflict resolution nobody had compiled. That tier is **deliberately retired**: pre-push now
//! runs only `secret-sweep` (`run-gate.sh secrets`) and `cargo-deny` (`run-gate.sh supply-chain`),
//! and an uncompiled rebase reaches CI rather than being caught locally. This gate holds the new
//! shape, and it fails CLOSED so a push stage that silently stops being security-only cannot pass
//! unnoticed.
//!
//! WHAT THE RETIREMENT DOES NOT COVER, SAID OUT LOUD. The compile is a COMMIT-stage concern now:
//! every compiling gate in `.pre-commit-config.yaml` sits on the commit stage, where it always
//! did, and a rebase is the one path that skips them all. That is the cost of dropping the push
//! clippy, and it is a deliberate weakening of the old rebase-safety tier rather than an oversight.
//!
//! WHY IT NEEDS A GATE AND NOT A COMMENT. The push hooks are blocks of YAML. Adding one that
//! compiles, or widening the stage to a non-security hook, breaks nothing visible: every other
//! gate stays green and the tier table in `.agents/skills/sutura/gates` goes on saying the push
//! stage is security-only. A mechanism that reads as enforcement and checks nothing is the shape
//! this repo keeps deleting invariant rows over. Note the tier it guards is BYPASSABLE, with
//! `--no-verify`, so this is not an invariant and `AGENTS.md` does not carry it as one; what it
//! guarantees is that the documented tiers are the tiers the file declares.
//!
//! TWO RULES, over `.pre-commit-config.yaml` and nothing else:
//!
//! * **the pre-push stage runs ONLY the security checks** - every hook staged `pre-push` is one of
//!   `secret-sweep` (`run-gate.sh secrets`) or `cargo-deny` (`run-gate.sh supply-chain`).
//! * **none of it compiles first-party code** - no pre-push hook's entry runs a cargo subcommand
//!   that builds the tree (`cargo clippy`, `cargo check`, `cargo build`, `cargo nextest`,
//!   `cargo test`). Security checks parse or scan; they do not compile.
//!
//! HOW IT READS THEM. Text, for the reason `pins.rs` gives - `xtask` has two dependencies and no
//! YAML parser, and this has to run on a host with no nix. It resolves the two YAML features this
//! file actually uses: an anchored scalar (`entry: &name value`) and an alias to one
//! (`entry: *name`), plus folded block scalars, because a rule comparing `*clippy` against a
//! command as text would compare two things neither of which is an invocation.
//!
//! FAIL CLOSED, in the directions a text scan fails silently: no hooks parsed, or no `pre-push`
//! hooks found. Both mean the reader stopped matching this file rather than the file being clean,
//! and a hook-reading gate's worst outcome is to find no hooks and say `ok`.

use std::collections::BTreeMap;

use crate::Verdict;
use crate::repo;

/// The one file this reads.
pub(crate) const CONFIG: &str = ".pre-commit-config.yaml";

/// The stage a push hook declares.
pub(crate) const PUSH: &str = "pre-push";

/// The stage a commit hook declares, or inherits from `default_stages`.
pub(crate) const COMMIT: &str = "pre-commit";

/// cargo subcommands that COMPILE first-party code. A pre-push hook whose entry names one proves
/// the security-only rule was broken - the push stage must not compile anything.
///
/// `cargo fmt` and `cargo run -q -p xtask` are deliberately absent: the first parses and the
/// second is how a gate gets invoked. `cargo nextest` and `cargo test` are here because running the
/// suite has certainly compiled the tree.
const COMPILES: &[&str] = &["cargo clippy", "cargo check", "cargo build", "cargo nextest", "cargo test"];

/// The ONLY commands a `pre-push` hook may run: the two whole-tree security checks. Each is
/// written exactly as its `entry:` is resolved, so an allowed push hook's resolved entry matches
/// one of these byte for byte.
const SECURITY: &[&str] = &["bash nix/run-gate.sh secrets", "bash nix/run-gate.sh supply-chain"];

/// One hook: its id, its display name, the line it starts on, its resolved entry and the stages
/// it runs at.
///
/// `pub(crate)` with `name` on it because `crate::hook_coverage` reads the SAME file for a
/// different question - which hooks a diff-scoped run reported - and two parsers of one
/// declaration are two answers nobody can reconcile, which is the argument
/// `shipped::declared` makes about `nix/shipped.nix`. `name` is what prek prints in its output,
/// so it is the only key a row can be joined on.
pub(crate) struct Hook {
    pub(crate) id: String,
    pub(crate) name: String,
    line: usize,
    /// The resolved command. `pub(crate)` because `crate::hook_coverage::abstain` reads it for a
    /// different question - whether this hook decides for ITSELF that it will not run - and the
    /// entry is where that decision is written.
    pub(crate) entry: String,
    pub(crate) stages: Vec<String>,
    /// `always_run: true` - it runs whatever the diff contains.
    ///
    /// `pub(crate)` because `crate::hook_coverage` needs it to answer a question this file does
    /// not ask: whether a SURFACE claiming this hook could ever report a gap. It cannot, and
    /// `github.com/telekom/sutura#402` measured a row that did exactly that - the coverage line
    /// printed on every diff, so the claim asserted nothing.
    pub(crate) always_run: bool,
    /// Does it declare a `files:` or `types:` filter?
    ///
    /// The OTHER spelling of the same hazard, and holding only `always_run` was holding one
    /// spelling of it: a hook with no filter matches every path, so prek runs it on every diff and
    /// a surface claiming it can never report a gap either. Two such hooks are in this config and
    /// both were claimed by a row, so the rule that reads only `always_run` passed over the same
    /// defect written the other way.
    pub(crate) filtered: bool,
}

impl Hook {
    /// Does this hook run whatever the diff contains?
    ///
    /// The two spellings as one question, so a caller cannot hold half of it.
    pub(crate) const fn unconditional(&self) -> bool {
        self.always_run || !self.filtered
    }

    /// Does this hook's entry compile first-party code?
    fn compiles(&self) -> bool {
        COMPILES.iter().any(|needle| self.entry.contains(needle))
    }

    /// Does it run at `stage`?
    pub(crate) fn runs_at(&self, stage: &str) -> bool {
        self.stages.iter().any(|declared| declared == stage)
    }
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-hook-tiers: could not locate the repo root");
        return Verdict::Fail;
    };
    let text = match std::fs::read_to_string(root.join(CONFIG)) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-hook-tiers: could not read {CONFIG}: {error}");
            return Verdict::Fail;
        }
    };
    decide(&hooks(&text))
}

/// What to say about the hooks that were read.
///
/// Separated from [`run`] so both verdicts are reachable from a test. A gate only ever exercised
/// green is a gate nobody has seen fail.
fn decide(hooks: &[Hook]) -> Verdict {
    if hooks.is_empty() {
        eprintln!("xtask check-hook-tiers: read no hooks at all out of {CONFIG}\n");
        eprintln!("This gate reads hooks as text and found none, so it has checked NOTHING. That is");
        eprintln!("a failure rather than a pass: the file's shape changed and the reader did not.");
        return Verdict::Fail;
    }

    let on_push: Vec<&Hook> = hooks.iter().filter(|hook| hook.runs_at(PUSH)).collect();
    if on_push.is_empty() {
        eprintln!("xtask check-hook-tiers: read no PRE-PUSH hooks out of {CONFIG}\n");
        eprintln!("A push stage with no parsed hook is a reader that stopped matching this file, not");
        eprintln!("a stage with nothing to check - it fails rather than passes over nothing.");
        return Verdict::Fail;
    }

    // The push stage must not COMPILE first-party code: the old push clippy - which caught a
    // rebase that ran no commit hook - is deliberately retired, and nothing may re-introduce it.
    if let Some(hook) = on_push.iter().find(|hook| hook.compiles()) {
        eprintln!("xtask check-hook-tiers: the pre-push stage compiles first-party code\n");
        eprintln!("  {CONFIG}:{}: `{}` runs", hook.line, hook.id);
        eprintln!("      {}", hook.entry);
        eprintln!();
        eprintln!("The push stage runs ONLY the security checks; nothing on push may compile.");
        eprintln!("An uncompiled rebase used to be caught locally by a push clippy, and that tier is");
        eprintln!("deliberately retired - a rebase that never compiled now reaches CI instead. A");
        eprintln!("hook staged `pre-push` that compiles re-introduces a cost this config no longer");
        eprintln!("intends to pay at push time.");
        return Verdict::Fail;
    }

    // And it must run ONLY the two security checks - nothing else belongs in the stage.
    if let Some(hook) = on_push
        .iter()
        .find(|hook| !SECURITY.iter().any(|allowed| hook.entry == *allowed))
    {
        eprintln!("xtask check-hook-tiers: the pre-push stage runs more than the security checks\n");
        eprintln!("  {CONFIG}:{}: `{}` runs", hook.line, hook.id);
        eprintln!("      {}", hook.entry);
        eprintln!();
        let allowed = SECURITY.join("` and `");
        eprintln!("The push stage runs ONLY `{allowed}`. Anything else on push is");
        eprintln!("a cost this config no longer intends to pay at push time.");
        return Verdict::Fail;
    }

    let pushed: Vec<&str> = on_push.iter().map(|hook| hook.id.as_str()).collect();
    println!(
        "xtask check-hook-tiers: ok - {} hook(s), the pre-push stage runs only {}",
        hooks.len(),
        pushed.join(", ")
    );
    Verdict::Pass
}

/// Every hook in the file, with its entry resolved and its stages filled in.
///
/// One pass. `default_stages:` is read where it stands, which is above every hook in this file and
/// is the only place YAML would accept it as a top-level key.
pub(crate) fn hooks(text: &str) -> Vec<Hook> {
    let mut anchors: BTreeMap<String, String> = BTreeMap::new();
    let mut defaults: Vec<String> = Vec::new();
    let mut out: Vec<Hook> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();

    for (index, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(list) = trimmed.strip_prefix("default_stages:") {
            defaults = stages(list);
            continue;
        }
        if let Some(id) = trimmed.strip_prefix("- id:") {
            out.push(Hook {
                id: String::from(id.trim()),
                name: String::new(),
                line: index.saturating_add(1),
                entry: String::new(),
                stages: defaults.clone(),
                always_run: false,
                filtered: false,
            });
            continue;
        }
        let Some(hook) = out.last_mut() else {
            continue;
        };
        if let Some(list) = trimmed.strip_prefix("stages:") {
            hook.stages = stages(list);
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("always_run:") {
            hook.always_run = value.trim() == "true";
            continue;
        }
        if trimmed.starts_with("files:") || trimmed.starts_with("types:") {
            hook.filtered = true;
            continue;
        }
        // The DISPLAY name, which is what prek prints and therefore the only thing a line of its
        // output can be joined back to an id on. Read here rather than in a second scan for the
        // reason this struct's doc gives.
        if let Some(value) = trimmed.strip_prefix("name:") {
            hook.name = String::from(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("entry:") {
            let indent = raw.len().saturating_sub(trimmed.len());
            let value = value.trim();
            hook.entry = match value {
                // A folded or literal block scalar: the entry is the more-indented lines below it.
                ">-" | ">" | "|" | "|-" => folded(&lines, index, indent),
                _ => value.to_owned(),
            };
            // An anchored scalar defines the name; an alias spends it. Both are resolved here so
            // every later rule compares commands rather than YAML.
            if let Some((name, anchored)) = anchor(&hook.entry) {
                anchors.insert(name, anchored.clone());
                hook.entry = anchored;
            } else if let Some(name) = hook.entry.strip_prefix('*') {
                hook.entry = anchors.get(name.trim()).cloned().unwrap_or_default();
            }
        }
    }
    out
}

/// The lines of a block scalar: everything below `at` indented deeper than the key, folded into
/// one line the way `>-` folds them.
fn folded(lines: &[&str], at: usize, indent: usize) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for raw in lines.iter().skip(at.saturating_add(1)) {
        let trimmed = raw.trim_start();
        if trimmed.is_empty() {
            break;
        }
        if raw.len().saturating_sub(trimmed.len()) <= indent {
            break;
        }
        parts.push(trimmed);
    }
    parts.join(" ")
}

/// The anchor name and the value it anchors, for `&name the rest of the line`.
fn anchor(value: &str) -> Option<(String, String)> {
    let rest = value.strip_prefix('&')?;
    let (name, anchored) = rest.split_once(' ')?;
    if name.is_empty() || anchored.trim().is_empty() {
        return None;
    }
    Some((String::from(name), String::from(anchored.trim())))
}

/// The stage names in a `[a, b]` flow sequence.
fn stages(list: &str) -> Vec<String> {
    list.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|name| String::from(name.trim()))
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::Verdict;

    #[cfg(unix)]
    #[test]
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "The fixture contains shell parameter expansions, not Rust format arguments."
    )]
    fn unconfigured_rust_hooks_use_pinned_nix_without_probing_host_cargo() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::process::Command;

        let root = crate::repo::root().expect("the repository root");
        let tree = crate::falsifier::falsifier_tree();
        let shell = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", "command -v bash"])
            .env_remove("BASH_ENV")
            .output()
            .expect("find the fixture's interpreter");
        assert!(shell.status.success());
        let shell = String::from_utf8(shell.stdout).expect("the interpreter path is text");
        let mut wrong = Vec::new();
        for with_nix in [false, true] {
            let bin = tree.join(if with_nix { "with-nix" } else { "without-nix" });
            std::fs::create_dir_all(&bin).expect("the fake PATH");
            let mut tools = vec![("cargo", "printf 'probed\\n' >\"${SUTURA_GATE_MARKER:?}\"\nexit 1\n")];
            if with_nix {
                tools.push((
                    "nix",
                    "printf '<%s>' \"$@\" >>\"${SUTURA_GATE_NIX_LOG:?}\"\n\
                     printf '\\n' >>\"$SUTURA_GATE_NIX_LOG\"\n\
                     if [ \"$1\" = eval ]; then printf 'fixture-system'; else \
                     printf 'fixture-nix backend=%s/%s\\n' \
                     \"${CARGO_UNSTABLE_CODEGEN_BACKEND-unset}\" \
                     \"${CARGO_PROFILE_DEV_CODEGEN_BACKEND-unset}\"; exit \"${SUTURA_GATE_NIX_EXIT:?}\"; fi\n",
                ));
            }
            for (name, body) in tools {
                let path = bin.join(name);
                std::fs::write(&path, format!("#!{}\n{body}", shell.trim())).expect("write the fake command");
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("make the fake executable");
            }
            for (gate, expected) in [
                (
                    "tests",
                    "<eval><--raw><--impure><--expr><builtins.currentSystem>\n<build><.#checks.fixture-system.nextest><-L>\n",
                ),
                ("supply-chain", "<run><.#deny>\n"),
                ("crap", "<run><.#crap>\n"),
                (
                    "secrets",
                    "<run><.#betterleaks><--><dir><.><--config><devco/gitleaks.toml><--redact>\n",
                ),
                (
                    "fmt-parity",
                    "<eval><--raw><--impure><--expr><builtins.currentSystem>\n<build><.#checks.fixture-system.fmt><-L>\n",
                ),
            ] {
                // The Rust arms probe host cargo; the secrets and fmt-parity arms never do. The
                // fake `cargo` writes the marker on any invocation, so the marker's presence tells
                // whether the probe ran.
                let probes_cargo = matches!(gate, "tests" | "supply-chain" | "crap");
                if !with_nix && matches!(gate, "secrets" | "fmt-parity") {
                    continue;
                }
                let exits: &[i32] = if with_nix && matches!(gate, "tests" | "supply-chain" | "crap") {
                    &[0, 23]
                } else {
                    &[0]
                };
                for nix_exit in exits {
                    let marker = tree.join(format!("{gate}-{with_nix}-{nix_exit}.cargo"));
                    let nix_log = tree.join(format!("{gate}-{with_nix}-{nix_exit}.nix"));
                    let output = Command::new(shell.trim())
                        .args(["--noprofile", "--norc", "nix/run-gate.sh", gate])
                        .current_dir(&root)
                        .env_remove("BASH_ENV")
                        .env("PATH", &bin)
                        .env("SUTURA_GATE_MARKER", &marker)
                        .env("SUTURA_GATE_NIX_LOG", &nix_log)
                        .env("SUTURA_GATE_NIX_EXIT", nix_exit.to_string())
                        .env("CARGO_UNSTABLE_CODEGEN_BACKEND", "true")
                        .env("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "cranelift")
                        .output()
                        .expect("execute the real tiered hook with fake commands");
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let invoked = std::fs::read_to_string(&nix_log).unwrap_or_default();
                    let correct = (marker.exists() == probes_cargo)
                        && if with_nix {
                            output.status.code() == Some(*nix_exit)
                                && stdout.contains("fixture-nix backend=unset/unset")
                                && invoked == expected
                        } else {
                            // Only the cargo-probing Rust gates reach here: without nix, and with
                            // a broken cargo, they skip with a notice rather than refusing.
                            output.status.success() && invoked.is_empty() && stdout.contains("SKIPPED")
                        };
                    if !correct {
                        wrong.push(format!(
                            "{gate}, nix={with_nix}, exit={nix_exit}: {:?}; cargo={}; argv={invoked:?}; {stdout:?}; {stderr:?}",
                            output.status.code(),
                            marker.exists()
                        ));
                    }
                }
            }
        }
        std::fs::remove_dir_all(tree).expect("remove the owned fixture");
        assert!(wrong.is_empty(), "unpinned execution or lost pinned fallback: {wrong:#?}");
    }

    /// The push stage as it is now: ONLY the two security checks, neither of which compiles. This
    /// is the shape the gate holds green.
    const SECURITY_ONLY: &str = concat!(
        "default_stages: [pre-commit]\n",
        "repos:\n",
        "  - repo: local\n",
        "    hooks:\n",
        "      - id: secret-sweep\n",
        "        entry: bash nix/run-gate.sh secrets\n",
        "        stages: [pre-push]\n",
        "      - id: cargo-deny\n",
        "        entry: bash nix/run-gate.sh supply-chain\n",
        "        stages: [pre-push]\n",
    );

    /// The OLD push stage, brought back: a whole-tree secret scan plus a `pre-push` clippy that
    /// COMPILES first-party code. This is the tier the retirement removed, and the gate has to be
    /// red on it.
    const PUSH_COMPILES: &str = concat!(
        "default_stages: [pre-commit]\n",
        "repos:\n",
        "  - repo: local\n",
        "    hooks:\n",
        "      - id: secret-sweep\n",
        "        entry: bash nix/run-gate.sh secrets\n",
        "        stages: [pre-push]\n",
        "      - id: rust-clippy-push\n",
        "        entry: bash -c 'exec cargo clippy --workspace --all-targets --all-features -- -D warnings'\n",
        "        stages: [pre-push]\n",
    );

    #[test]
    fn a_push_stage_that_compiles_is_the_failure_this_gate_is_for() {
        // Red against the OLD rule: a `pre-push` clippy re-introduces the retired tier, where a
        // rebase that ran no commit hook was compiled again before reaching the remote. Nothing on
        // push may compile now.
        assert_eq!(super::decide(&super::hooks(PUSH_COMPILES)), Verdict::Fail);
        // And GREEN with the NEW rule: only the two security checks, nothing that compiles.
        assert_eq!(super::decide(&super::hooks(SECURITY_ONLY)), Verdict::Pass);
    }

    #[test]
    fn a_push_stage_that_runs_more_than_the_security_checks_fails() {
        // The pre-push stage as it was even before the push clippy: a secret scan and a formatting
        // check, neither compiling. Both are outside the security-only set, so the gate is red for
        // running MORE than it is allowed to, independently of the compile half.
        const FORMAT_ON_PUSH: &str = concat!(
            "default_stages: [pre-commit]\n",
            "      - id: secret-sweep\n",
            "        entry: bash nix/run-gate.sh secrets\n",
            "        stages: [pre-push]\n",
            "      - id: fmt-parity\n",
            "        entry: bash nix/run-gate.sh fmt-parity\n",
            "        stages: [pre-push]\n",
        );
        assert_eq!(super::decide(&super::hooks(FORMAT_ON_PUSH)), Verdict::Fail);
    }

    #[test]
    fn an_alias_is_resolved_to_the_command_it_names() {
        // The anchor/alias machinery is still how the reader resolves entries; a rule comparing a
        // literal `*clippy` against an invocation would compare two things neither of which is a
        // command. The config no longer USES an anchor, so this exercises the machinery directly.
        const ANCHORED: &str = concat!(
            "default_stages: [pre-commit]\n",
            "      - id: rust-clippy\n",
            "        entry: &clippy bash -c 'exec cargo clippy --workspace --all-targets --all-features -- -D warnings'\n",
            "        language: system\n",
            "      - id: secret-sweep\n",
            "        entry: bash nix/run-gate.sh secrets\n",
            "        stages: [pre-push]\n",
            "      - id: rust-clippy-push\n",
            "        entry: *clippy\n",
            "        stages: [pre-push]\n",
        );
        let read = super::hooks(ANCHORED);
        let pushed = read.iter().find(|hook| hook.id == "rust-clippy-push").expect("the push hook");
        assert!(pushed.entry.contains("cargo clippy --workspace"), "{}", pushed.entry);
        assert!(pushed.compiles());
        assert!(pushed.runs_at(super::PUSH));
        // A pushed hook that RESOLVES to a compiling command is red on the compile half.
        assert_eq!(super::decide(&super::hooks(ANCHORED)), Verdict::Fail);
        // An alias naming nothing resolves to nothing rather than to itself, so it fails the
        // security-only rule instead of passing on the strength of the word `clippy`.
        let dangling = ANCHORED.replace("entry: *clippy", "entry: *nothingAnchorsThis");
        assert_eq!(super::decide(&super::hooks(&dangling)), Verdict::Fail);
    }

    #[test]
    fn a_stage_declared_by_default_is_read_as_the_commit_stage() {
        // `rust-clippy` declares no `stages:`, so its tier comes from `default_stages` - and if
        // that inheritance were missed, the reader would mis-tire the real file.
        const TIERED: &str = concat!(
            "default_stages: [pre-commit]\n",
            "      - id: rust-clippy\n",
            "        entry: devenv shell cargo clippy --workspace --all-targets --all-features -- -D warnings\n",
            "        language: system\n",
            "      - id: secret-sweep\n",
            "        entry: bash nix/run-gate.sh secrets\n",
            "        stages: [pre-push]\n",
            "      - id: cargo-deny\n",
            "        entry: bash nix/run-gate.sh supply-chain\n",
            "        stages: [pre-push]\n",
        );
        let read = super::hooks(TIERED);
        let commit = read.iter().find(|hook| hook.id == "rust-clippy").expect("the commit hook");
        assert!(commit.runs_at(super::COMMIT), "{:?}", commit.stages);
        assert!(!commit.runs_at(super::PUSH));
        assert_eq!(super::decide(&super::hooks(TIERED)), Verdict::Pass);
    }

    #[test]
    fn a_folded_block_scalar_is_one_command() {
        // The shellcheck hook is written this way. Read line by line it would be three fragments,
        // and an entry this gate cannot reassemble is one it cannot compare.
        const BLOCK: &str = concat!(
            "      - id: shellcheck\n",
            "        entry: >-\n",
            "          bash -c 'command -v nix >/dev/null 2>&1\n",
            "          && exec nix run .#shellcheck -- -x \"$@\"\n",
            "          || echo skipped' --\n",
            "        language: system\n",
        );
        let read = super::hooks(BLOCK);
        let hook = read.first().expect("the hook");
        assert_eq!(
            hook.entry,
            "bash -c 'command -v nix >/dev/null 2>&1 && exec nix run .#shellcheck -- -x \"$@\" || echo skipped' --"
        );
        // `language: system` is a key at the entry's own indentation and must not be folded in.
        assert!(!hook.entry.contains("language"));
    }

    #[test]
    fn a_file_this_gate_cannot_read_is_red_rather_than_green() {
        // No hooks parsed at all, and hooks parsed but NONE of them on pre-push: both are a reader
        // that stopped matching, and neither must pass by having nothing to compare.
        const NO_PUSH: &str = concat!(
            "default_stages: [pre-commit]\n",
            "      - id: rust-clippy\n",
            "        entry: devenv shell cargo clippy --workspace --all-targets --all-features -- -D warnings\n",
            "        language: system\n",
        );
        assert_eq!(super::decide(&[]), Verdict::Fail);
        assert_eq!(super::decide(&super::hooks(NO_PUSH)), Verdict::Fail);
    }

    #[test]
    fn the_real_file_still_has_the_shape_this_gate_reads() {
        // Caught here and not only on a branch: a reader that matches nothing makes its gate pass
        // vacuously, and the verdict on the real file is the gate's own output. This asserts the
        // file is still SHAPED the way the reader expects - hooks found, the commit stage still
        // compiles, and the push stage runs exactly the two security checks and nothing that
        // compiles.
        let root = crate::repo::root().expect("the repo root");
        let text = std::fs::read_to_string(root.join(super::CONFIG)).expect("the hook config");
        let read = super::hooks(&text);
        assert!(read.len() > 8, "read {} hooks", read.len());
        assert!(
            read.iter().any(|hook| hook.runs_at(super::COMMIT) && hook.compiles()),
            "the commit stage still compiles"
        );
        let push: Vec<&super::Hook> = read.iter().filter(|hook| hook.runs_at(super::PUSH)).collect();
        let push_ids: Vec<&str> = push.iter().map(|hook| hook.id.as_str()).collect();
        assert_eq!(
            push.len(),
            2,
            "the push stage has exactly the two security hooks: {push_ids:?}"
        );
        assert!(
            push.iter()
                .all(|hook| !hook.compiles() && super::SECURITY.iter().any(|allowed| hook.entry == *allowed)),
            "every push hook is security-only and compiles nothing: {push_ids:?}"
        );
        // Every hook has a DISPLAY name, because `crate::hook_coverage` joins prek's output rows
        // back to this file on it - a nameless hook there is a row nothing can be matched to, and
        // a hook silenced by `SKIP` prints no row at all, so the join is the only thing that can
        // see it.
        let nameless: Vec<&str> = read
            .iter()
            .filter(|hook| hook.name.is_empty())
            .map(|hook| hook.id.as_str())
            .collect();
        assert!(nameless.is_empty(), "hooks with no name: {nameless:?}");
    }
}

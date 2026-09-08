//! The push stage compiles, and it compiles the way the commit stage does.
//!
//! This gate exists because of a measured hole rather than a hypothetical one. Every compiling
//! gate in `.pre-commit-config.yaml` was on the COMMIT stage, and the push stage ran a secret
//! sweep, `cargo-deny` and a formatting check - none of which compiles anything. `git rebase` and
//! `git rebase --continue` run no commit hook at all, so a conflict resolution went straight to the
//! remote with nothing having compiled it. It happened three times in one day: a hand-resolved
//! merge whose `sutura-config` did not build, a CLEAN merge of a signature change against a new
//! call site written for the old one, and a branch red on CI's format step.
//!
//! WHY IT NEEDS A GATE AND NOT A COMMENT. The push hook is one block of YAML. Deleting it, or
//! moving it to the commit stage, breaks nothing visible: every other gate stays green, the tier
//! table in `.agents/skills/sutura/gates` goes on saying the push stage compiles, and the next rebase pushes a
//! tree nobody built. That is the shape this repo keeps deleting invariant rows over - a mechanism
//! that reads as enforcement and checks nothing. Note the tier it guards is BYPASSABLE, with
//! `--no-verify`, so this is not an invariant and `AGENTS.md` does not carry it as one; what it
//! guarantees is that the documented tiers are the tiers the file declares.
//!
//! TWO RULES, over `.pre-commit-config.yaml` and nothing else:
//!
//! * **the push stage compiles** - some hook staged `pre-push` runs a cargo subcommand that
//!   compiles first-party code.
//! * **it is the commit stage's own invocation** - that hook's entry is byte-equal to a
//!   commit-stage entry. Not tidiness: cargo keys its fingerprints on the invocation, so a
//!   divergence of one flag stops the push stage reusing what the commit hook built and turns a
//!   one-second no-op into a full workspace check. The file expresses the equality as a YAML
//!   anchor, which cannot drift; this rule is what fails if somebody replaces the alias with a
//!   copy and then edits one of them.
//!
//! HOW IT READS THEM. Text, for the reason `pins.rs` gives - `xtask` has two dependencies and no
//! YAML parser, and this has to run on a host with no nix. It resolves the two YAML features this
//! file actually uses: an anchored scalar (`entry: &name value`) and an alias to one
//! (`entry: *name`), plus folded block scalars, because a rule comparing `*clippy` against a
//! command as text would compare two things neither of which is an invocation.
//!
//! FAIL CLOSED, in the directions a text scan fails silently: no hooks parsed, or no compiling
//! entry found at the COMMIT stage either. Both mean the reader stopped matching this file rather
//! than the file being clean, and a hook-reading gate's worst outcome is to find no hooks and say
//! `ok`.

use std::collections::BTreeMap;

use crate::Verdict;
use crate::repo;

/// The one file this reads.
pub(crate) const CONFIG: &str = ".pre-commit-config.yaml";

/// The stage a push hook declares.
pub(crate) const PUSH: &str = "pre-push";

/// The stage a commit hook declares, or inherits from `default_stages`.
pub(crate) const COMMIT: &str = "pre-commit";

/// cargo subcommands that COMPILE first-party code, so an entry naming one proves the stage
/// compiles.
///
/// `cargo fmt` and `cargo run -q -p xtask` are deliberately absent: the first parses and the
/// second is how a gate gets invoked. `cargo nextest` and `cargo test` are here because a stage
/// running the suite has certainly compiled the tree - not because either is the gate this file
/// puts on push today.
const COMPILES: &[&str] = &["cargo clippy", "cargo check", "cargo build", "cargo nextest", "cargo test"];

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

    let compiling_at =
        |stage: &'static str| -> Vec<&Hook> { hooks.iter().filter(|hook| hook.runs_at(stage) && hook.compiles()).collect() };
    let on_commit = compiling_at(COMMIT);
    if on_commit.is_empty() {
        eprintln!("xtask check-hook-tiers: no COMMIT-stage hook in {CONFIG} compiles anything\n");
        eprintln!("That is either a reader that stopped matching this file or a commit tier that");
        eprintln!("stopped compiling, and this gate cannot tell which - so it fails rather than");
        eprintln!("comparing the push stage against nothing. Entries it looks for: {COMPILES:?}");
        return Verdict::Fail;
    }

    let on_push = compiling_at(PUSH);
    if on_push.is_empty() {
        let names: Vec<&str> = on_commit.iter().map(|hook| hook.id.as_str()).collect();
        eprintln!("xtask check-hook-tiers: the PUSH stage compiles nothing\n");
        eprintln!("  {CONFIG}: every compiling hook is commit-stage: {}", names.join(", "));
        eprintln!();
        eprintln!("A rebase runs no commit hook - `git rebase --continue` writes a commit with none");
        eprintln!("of them - so with the push stage compiling nothing, a conflict resolution reaches");
        eprintln!("the remote unbuilt. Measured: a clean merge of a signature change and a call site");
        eprintln!("written for the old one, pushed green, red on CI.");
        eprintln!();
        eprintln!("Stage one of the compiling hooks at `pre-push` as well. Alias its `entry:` rather");
        eprintln!("than copying it, so both stages issue the identical command and the push run");
        eprintln!("reuses the fingerprints the commit run just produced.");
        return Verdict::Fail;
    }

    let mut drifted = Vec::new();
    for hook in &on_push {
        if !on_commit.iter().any(|twin| twin.entry == hook.entry) {
            drifted.push(hook);
        }
    }
    if let Some(hook) = drifted.first() {
        eprintln!("xtask check-hook-tiers: the push stage compiles with an invocation no commit hook uses\n");
        eprintln!("  {CONFIG}:{}: `{}` runs", hook.line, hook.id);
        eprintln!("      {}", hook.entry);
        eprintln!("  and no commit-stage hook runs that command.");
        eprintln!();
        eprintln!("cargo keys its fingerprints on the invocation, so this is not a style point: a");
        eprintln!("push-stage command differing by one flag rebuilds instead of reusing what the");
        eprintln!("commit hook built, and a gate that makes `git push` slow gets bypassed with");
        eprintln!("`--no-verify`. Point the push hook's `entry:` at the commit hook's anchor.");
        return Verdict::Fail;
    }

    let pushed: Vec<&str> = on_push.iter().map(|hook| hook.id.as_str()).collect();
    println!(
        "xtask check-hook-tiers: ok - {} hook(s), the push stage compiles through {}",
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
    fn stable_gate_entries_refuse_before_an_unconfigured_host_tool_can_run() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::process::Command;

        const TOOLS: &[&str] = &[
            "cargo",
            "rustc",
            "rustdoc",
            "cargo-fmt",
            "rustfmt",
            "cargo-clippy",
            "clippy-driver",
        ];
        let root = crate::repo::root().expect("the repository root");
        let tree = crate::falsifier::falsifier_tree();
        let shell = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", "command -v bash"])
            .env_remove("BASH_ENV")
            .output()
            .expect("find the fixture's interpreter");
        assert!(shell.status.success());
        let shell = String::from_utf8(shell.stdout).expect("the interpreter path is text");
        let write_tool = |path: &std::path::Path, origin: &str| {
            let body = format!(
                "#!{}\nprintf 'executed\\n' >\"${{SUTURA_GATE_MARKER:?}}\"\n\
                 printf 'fixture-{origin} target=%s codegen=%s/%s\\n' \
                 \"${{CARGO_TARGET_DIR:-}}\" \"${{CARGO_UNSTABLE_CODEGEN_BACKEND-unset}}\" \
                 \"${{CARGO_PROFILE_DEV_CODEGEN_BACKEND-unset}}\"\n",
                shell.trim()
            );
            std::fs::write(path, body).expect("write a fake tool");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("make the fake executable");
        };
        let host = tree.join("host-bin");
        std::fs::create_dir_all(&host).expect("the fallback directory");
        write_tool(&host.join("cargo"), "host");
        let stable = tree.join("stable bin");
        std::fs::create_dir_all(&stable).expect("the configured directory");
        for tool in TOOLS {
            write_tool(&stable.join(tool), "stable");
        }
        let mut configurations = vec![
            (String::from("missing"), None, false),
            (String::from("empty"), Some(std::path::PathBuf::new()), false),
            (String::from("nonexistent directory"), Some(tree.join("absent-bin")), false),
            (String::from("configured"), Some(stable), true),
        ];
        for omitted in TOOLS {
            for defect in ["missing", "not-executable", "directory"] {
                let label = format!("{omitted}-{defect}");
                let bin = tree.join(&label);
                std::fs::create_dir_all(&bin).expect("the incomplete toolchain directory");
                for tool in TOOLS {
                    if tool != omitted || defect == "not-executable" {
                        write_tool(&bin.join(tool), "stable");
                    }
                }
                if defect == "not-executable" {
                    std::fs::set_permissions(bin.join(omitted), std::fs::Permissions::from_mode(0o644))
                        .expect("remove execute permission from one member");
                } else if defect == "directory" {
                    std::fs::create_dir_all(bin.join(omitted)).expect("a directory is not a tool");
                }
                configurations.push((label, Some(bin), false));
            }
        }
        let config = std::fs::read_to_string(root.join(super::CONFIG)).expect("the hook configuration");
        let declared = super::hooks(&config);
        let mut entries = vec![(
            String::from("source without errexit"),
            String::from("source nix/stable-env.sh; printf 'continued\\n' >\"${SUTURA_GATE_CONTINUED:?}\"; cargo"),
        )];
        for id in [
            "rust-fmt",
            "rust-clippy",
            "hygiene",
            "rust-check-changed",
            "rust-doctests",
            "rust-clippy-push",
        ] {
            let hook = declared.iter().find(|hook| hook.id == id).expect("the gate hook is declared");
            entries.push((String::from(id), hook.entry.clone()));
        }
        for task in ["fmt", "lint"] {
            let body = crate::tasks::recipe_body(&root, task).expect("the gate recipe").join("\n");
            entries.push((format!("just {task}"), body));
        }
        let inherited_path = std::env::var("PATH").expect("the test runner has a PATH");
        let path = format!("{}:{inherited_path}", host.display());
        let mut wrong = Vec::new();
        let marker = tree.join("entry-marker");
        let continued = tree.join("continuation-marker");
        for (configuration, bin, valid) in configurations {
            for (entry, body) in &entries {
                for prior in [&marker, &continued] {
                    if prior.exists() {
                        std::fs::remove_file(prior).expect("remove the prior invocation's marker");
                    }
                }
                let mut command = Command::new(shell.trim());
                command
                    .args(["--noprofile", "--norc", "-c", body])
                    .current_dir(&root)
                    .env_remove("BASH_ENV")
                    .env_remove("SUTURA_STABLE_BIN")
                    .env("PATH", &path)
                    .env("SUTURA_GATE_MARKER", &marker)
                    .env("SUTURA_GATE_CONTINUED", &continued)
                    .env("CARGO_TARGET_DIR", "fixture-target")
                    .env("CARGO_UNSTABLE_CODEGEN_BACKEND", "true")
                    .env("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "cranelift");
                if let Some(bin) = &bin {
                    command.env("SUTURA_STABLE_BIN", bin);
                }
                let output = command.output().expect("execute the actual gate entry with fake tools");
                let stdout = String::from_utf8_lossy(&output.stdout);
                let correct = if valid {
                    output.status.success()
                        && marker.exists()
                        && continued.exists() == (entry == "source without errexit")
                        && stdout.contains("fixture-stable target=fixture-target/stable codegen=unset/unset")
                        && !stdout.contains("fixture-host")
                } else {
                    !output.status.success() && !marker.exists() && !continued.exists()
                };
                if !correct {
                    wrong.push(format!(
                        "{configuration} / {entry}: {:?}; continued={}; tool={}; stdout={stdout:?}; stderr={:?}",
                        output.status.code(),
                        continued.exists(),
                        marker.exists(),
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
            }
        }
        std::fs::remove_dir_all(tree).expect("remove the owned fixture");
        assert!(wrong.is_empty(), "gate entries used an unsupported toolchain: {wrong:#?}");
    }

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
                        .env_remove("SUTURA_STABLE_BIN")
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
                    let correct = !marker.exists()
                        && if with_nix {
                            output.status.code() == Some(*nix_exit)
                                && stdout.contains("fixture-nix backend=unset/unset")
                                && invoked == expected
                        } else {
                            !output.status.success() && invoked.is_empty()
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

    /// The push stage as it was before this gate: a whole-tree secret scan and a formatting check,
    /// neither of which compiles. This is the base behaviour, and the gate has to be red on it.
    const BEFORE: &str = concat!(
        "default_stages: [pre-commit]\n",
        "repos:\n",
        "  - repo: local\n",
        "    hooks:\n",
        "      - id: rust-clippy\n",
        "        entry: bash -c 'source nix/stable-env.sh; exec cargo clippy --workspace --all-targets --all-features -- -D warnings'\n",
        "        language: system\n",
        "      - id: secret-sweep\n",
        "        entry: bash nix/run-gate.sh secrets\n",
        "        stages: [pre-push]\n",
        "      - id: fmt-parity\n",
        "        entry: bash nix/run-gate.sh fmt-parity\n",
        "        stages: [pre-push]\n",
    );

    /// The same file with the push-stage hook added as an ALIAS of the commit-stage entry.
    const AFTER: &str = concat!(
        "default_stages: [pre-commit]\n",
        "repos:\n",
        "  - repo: local\n",
        "    hooks:\n",
        "      - id: rust-clippy\n",
        "        entry: &clippy bash -c 'source nix/stable-env.sh; exec cargo clippy --workspace --all-targets --all-features -- -D warnings'\n",
        "        language: system\n",
        "      - id: secret-sweep\n",
        "        entry: bash nix/run-gate.sh secrets\n",
        "        stages: [pre-push]\n",
        "      - id: rust-clippy-push\n",
        "        entry: *clippy\n",
        "        stages: [pre-push]\n",
    );

    #[test]
    fn a_push_stage_that_compiles_nothing_is_the_failure_this_gate_is_for() {
        // Red against the base behaviour, which is the whole point: every compiling hook on
        // commit, and `git rebase --continue` runs none of them.
        assert_eq!(super::decide(&super::hooks(BEFORE)), Verdict::Fail);
        assert_eq!(super::decide(&super::hooks(AFTER)), Verdict::Pass);
    }

    #[test]
    fn an_alias_is_resolved_to_the_command_it_names() {
        // Without this the rule below would compare the string `*clippy` against an invocation and
        // report drift on a file that has none.
        let read = super::hooks(AFTER);
        let pushed = read.iter().find(|hook| hook.id == "rust-clippy-push").expect("the push hook");
        assert!(pushed.entry.contains("cargo clippy --workspace"), "{}", pushed.entry);
        assert!(pushed.compiles());
        assert!(pushed.runs_at(super::PUSH));
        // An alias naming nothing resolves to nothing rather than to itself, so it fails the
        // compiling rule instead of passing it on the strength of the word `clippy`.
        let dangling = AFTER.replace("entry: *clippy", "entry: *nothingAnchorsThis");
        assert_eq!(super::decide(&super::hooks(&dangling)), Verdict::Fail);
    }

    #[test]
    fn a_copy_that_diverges_by_one_flag_fails() {
        // The cost is not tidiness. cargo keys fingerprints on the invocation, so this push hook
        // would rebuild the workspace rather than reuse what the commit hook built.
        let copied = AFTER.replace(
            "entry: *clippy",
            "entry: bash -c 'source nix/stable-env.sh; exec cargo clippy --workspace --all-features -- -D warnings'",
        );
        assert_eq!(super::decide(&super::hooks(&copied)), Verdict::Fail);
        // An identical copy is not drift, and must not be reported as any: it is the same command,
        // just spelled twice. The anchor is how the file avoids that, not what the rule demands.
        let twin = AFTER.replace(
            "entry: *clippy",
            "entry: bash -c 'source nix/stable-env.sh; exec cargo clippy --workspace --all-targets --all-features -- -D warnings'",
        );
        assert_eq!(super::decide(&super::hooks(&twin)), Verdict::Pass);
    }

    #[test]
    fn a_stage_declared_by_default_is_read_as_the_commit_stage() {
        // `rust-clippy` declares no `stages:`, so its tier comes from `default_stages` - and if
        // that inheritance were missed the commit stage would look empty and the gate would fail
        // closed on a clean file.
        let read = super::hooks(AFTER);
        let commit = read.iter().find(|hook| hook.id == "rust-clippy").expect("the commit hook");
        assert!(commit.runs_at(super::COMMIT), "{:?}", commit.stages);
        assert!(!commit.runs_at(super::PUSH));
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
        // Hooks parsed, none of them compiling: that is a reader that stopped matching, and it
        // must not pass by having nothing to compare.
        const NONE: &str = concat!(
            "default_stages: [pre-commit]\n",
            "      - id: secret-sweep\n",
            "        entry: bash nix/run-gate.sh secrets\n",
            "        stages: [pre-push]\n",
        );
        assert_eq!(super::decide(&[]), Verdict::Fail);
        assert_eq!(super::decide(&super::hooks(NONE)), Verdict::Fail);
    }

    #[test]
    fn the_real_file_still_has_the_shape_this_gate_reads() {
        // Caught here and not only on a branch: a reader that matches nothing makes its gate pass
        // vacuously, and the verdict on the real file is the gate's own output. This asserts the
        // file is still SHAPED the way the reader expects - hooks found, both tiers populated.
        let root = crate::repo::root().expect("the repo root");
        let text = std::fs::read_to_string(root.join(super::CONFIG)).expect("the hook config");
        let read = super::hooks(&text);
        assert!(read.len() > 8, "read {} hooks", read.len());
        assert!(read.iter().any(|hook| hook.runs_at(super::COMMIT) && hook.compiles()));
        assert!(read.iter().any(|hook| hook.runs_at(super::PUSH) && hook.compiles()));
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

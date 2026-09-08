//! Whether a hook COULD have inspected the diff on this host, derived from the shell that decides.
//!
//! **The finding this exists for: nine of the sixteen declared hooks print `Passed` after deciding
//! not to run.** `shellcheck` and `zizmor` are
//! `command -v nix >/dev/null 2>&1 && exec ... || echo "... skipped ..."`; `betterleaks` is the same
//! on its own tool; `rust-tests`, `rust-crap`, `secret-sweep`, `cargo-deny` and `fmt-parity` route
//! through `nix/run-gate.sh`, whose tier 3 prints `run-gate: SKIPPED ...` and falls off the end of
//! the `case` with status 0. Measured with the `shellcheck` entry verbatim and `nix` off `PATH`: the
//! notice printed, exit status 0. prek reads that as `Passed`, [`super::Coverage::Ran`] read that as
//! *inspected the diff*, and `just ship-check` printed `pre-push - 4 of 4 declared hook(s) ran`
//! while three of the four announced their own skip - on a configuration
//! `.pre-commit-config.yaml` argues for at length for a MISSING TOOL, *"a hook that cannot run
//! must not be a wall"*. It no longer argues it for a missing toolchain: `nix/run-gate.sh` refuses
//! a host with neither configured stable tools nor nix, so the eight are eight only where the
//! abstention is a tool's. This paragraph is prose in `.rs` and `check-guidance`'s scope stops at
//! the file extension, so it is held by review - the registered half is in
//! `xtask/src/guidance/claims/contradicted.rs`.
//!
//! # Why this does not read the notice, which is the remedy the report asked for
//!
//! **MEASURED against prek 0.4.14: a passing hook's own output is not in the log.** A minimal
//! repository with one hook whose entry self-skips and exits 0:
//!
//! ```text
//! shell scripts............................................................Passed
//! exit=0
//! ```
//!
//! and nothing else. The notice appears only with `verbose: true` on the hook, which would also
//! dump every hook's full output on every run - `cargo nextest` over the workspace included - and
//! noise is how a gate gets switched off. So the notice cannot be the mechanism, and the honest
//! authority for *could this hook have run here* is the shell that decides it plus this host's
//! `PATH`.
//!
//! # What it reads, and why it is a derivation rather than a table
//!
//! Every self-skip in this repository is guarded by one of two probe shapes - `command -v <tool>`
//! and `<tool> --version` - and both are in the text that DECIDES: the hook's own `entry:` for the
//! three inline ones, and `nix/run-gate.sh`'s `case` arm for the five tiered ones. So the tools are
//! read out of that text rather than listed here. A list would be a second copy of `run-gate.sh`'s
//! tiers, and two owners for one decision is the shape `sutura/gates` records under *one owner per
//! concern*.
//!
//! One line is one CONJUNCTION and the lines are ALTERNATIVES, which is what makes the crap tier
//! come out right: `if cargo llvm-cov --version ... && cargo crap --version ...` needs both, and
//! the `elif command -v nix ...` below it needs only nix.
//!
//! # Fails closed three ways
//!
//! * **a hook routed through `nix/run-gate.sh` whose `case` arm is not there** - the argument was
//!   renamed, or the reader stopped matching the file;
//! * **a deciding text that announces a skip and yields no probe** - the guard was rewritten into a
//!   shape this reader does not know, so its notice would be decorative again;
//! * **no declared hook deciding anything at all** - the reader stopped matching both authorities,
//!   and every `Passed` would read as coverage. If the tiering is genuinely gone, this rule goes
//!   with it rather than staying green over nothing.
//!
//! # What it does NOT reach
//!
//! * **The PATH it probes is THIS PROCESS's.** That is the same shell prek ran in only because
//!   `ship-check` runs both, one after the other. Handed a log captured on another host, the probe
//!   answers about the wrong machine, and nothing here can tell.
//! * **A tool present but broken**, or present and refusing to run: tier 1 is entered and fails,
//!   which is a `Failed` row and the run's own exit code.
//! * **Which tier actually ran.** The question answered is whether ANY branch could have been
//!   taken, so a hook whose tier 1 tool is absent and whose nix is present is *not* an abstention -
//!   correctly, because `run-gate.sh` says that is the same check reached differently.
//! * **A probe written any third way** - a `[ -x ... ]` test, a `type`, a `hash`. Such a guard
//!   sitting next to a skip notice is the second refusal above rather than a silent pass.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::hooks;

/// The one script the tiered hooks route through, and the authority for their tiers.
const RUN_GATE: &str = "nix/run-gate.sh";

/// The word every self-skip in this repository prints, in either case. It is what tells a deciding
/// text apart from a probe that merely selects between two ways of running.
const ANNOUNCES: &str = "skipped";

/// Words that end a command, so a probe's tool name cannot be read through one.
const SEPARATORS: &[&str] = &[
    "if", "elif", "then", "else", "fi", "while", "until", "do", "done", "exec", "&&", "||",
];

/// Tools, ALL of which one host needs for a single branch of a hook's decision to be taken.
type Group = Vec<String>;

/// What the declared hooks can decide for themselves on this host.
pub(super) struct Abstentions {
    /// Hook ids that could not have inspected the diff here: every branch is unsatisfiable.
    pub(super) unavailable: BTreeSet<String>,
    /// One sentence per hook whose own decision could not be read. FAIL CLOSED.
    pub(super) unreadable: Vec<String>,
    /// How many declared hooks decide anything at all - this rule's own floor witness.
    pub(super) deciding: usize,
}

/// Every declared hook's decision, resolved against this host.
pub(super) fn over(root: &Path, declared: &[hooks::Hook]) -> Result<Abstentions, String> {
    let script = std::fs::read_to_string(root.join(RUN_GATE))
        .map_err(|error| format!("could not read {RUN_GATE}, which decides five hooks' tiers: {error}"))?;
    let mut unavailable = BTreeSet::new();
    let mut unreadable = Vec::new();
    let mut deciding = 0_usize;
    let mut present: BTreeMap<String, bool> = BTreeMap::new();
    for hook in declared {
        let deciding_text = match deciding_text(&hook.entry, &script) {
            Ok(text) => text,
            Err(message) => {
                unreadable.push(format!("`{}` {message}", hook.id));
                continue;
            }
        };
        let groups = probe_groups(&deciding_text);
        if groups.is_empty() {
            if deciding_text.to_lowercase().contains(ANNOUNCES) {
                unreadable.push(format!(
                    "`{}` announces a skip and no `command -v <tool>` or `<tool> --version` probe could be read out of the text that decides it - the guard was rewritten and its notice is decorative again",
                    hook.id
                ));
            }
            continue;
        }
        deciding = deciding.saturating_add(1);
        let satisfiable = groups.iter().any(|group| {
            group
                .iter()
                .all(|tool| *present.entry(tool.clone()).or_insert_with(|| on_path(tool)))
        });
        if !satisfiable {
            unavailable.insert(hook.id.clone());
        }
    }
    Ok(Abstentions {
        unavailable,
        unreadable,
        deciding,
    })
}

/// The shell text that decides whether one hook runs: its `case` arm where it is tiered, else its
/// own entry.
fn deciding_text(entry: &str, script: &str) -> Result<String, String> {
    let Some(gate) = gate_argument(entry) else {
        return Ok(String::from(entry));
    };
    case_arm(script, &gate).ok_or_else(|| {
        format!("routes through {RUN_GATE} as `{gate}` and that script declares no `{gate})` arm - the argument was renamed, or the reader stopped matching it")
    })
}

/// The gate one entry hands to [`RUN_GATE`], if it routes through it.
fn gate_argument(entry: &str) -> Option<String> {
    let mut words = entry.split_whitespace().skip_while(|word| !word.ends_with(RUN_GATE));
    words.next()?;
    words.next().map(String::from)
}

/// One `case` arm of a shell script: the lines between `<name>)` and its `;;`.
fn case_arm(script: &str, name: &str) -> Option<String> {
    let opener = format!("{name})");
    let mut lines = script.lines().skip_while(|line| line.trim() != opener);
    lines.next()?;
    Some(lines.take_while(|line| line.trim() != ";;").collect::<Vec<&str>>().join("\n"))
}

/// Every alternative group of tools one deciding text probes for.
fn probe_groups(text: &str) -> Vec<Group> {
    text.lines()
        .filter_map(|line| {
            let group = probes(line);
            (!group.is_empty()).then_some(group)
        })
        .collect()
}

/// The tools one line probes for. All of them are needed, because one line is one conjunction.
fn probes(line: &str) -> Group {
    let words: Vec<&str> = line
        .split_whitespace()
        .map(|word| word.trim_matches(|c| c == '\'' || c == '"'))
        .collect();
    let mut group: Group = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let tool = if *word == "-v" && index.checked_sub(1).and_then(|before| words.get(before)) == Some(&"command") {
            words
                .get(index.saturating_add(1))
                .filter(|next| is_word(next))
                .map(|next| String::from(*next))
        } else if *word == "--version" {
            subcommand_before(&words, index)
        } else {
            None
        };
        if let Some(tool) = tool
            && !group.contains(&tool)
        {
            group.push(tool);
        }
    }
    group
}

/// The executable a `<words> --version` probe resolves to, read backwards from the flag.
///
/// `cargo nextest --version` is `cargo-nextest` on `PATH`, which is how cargo itself finds a
/// subcommand - so the two-word form is not a special case, it is the answer.
fn subcommand_before(words: &[&str], flag: usize) -> Option<String> {
    let command: Vec<&str> = words
        .iter()
        .take(flag)
        .rev()
        .take_while(|word| is_word(word) && !SEPARATORS.contains(word))
        .copied()
        .collect();
    match command.as_slice() {
        [only] => Some(String::from(*only)),
        [subcommand, .., "cargo"] => Some(format!("cargo-{subcommand}")),
        _ => None,
    }
}

/// Is this a plain command word rather than a redirection, an expansion or a flag?
fn is_word(word: &str) -> bool {
    !word.is_empty()
        && !word.starts_with('-')
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '+')
}

/// Is this executable on this process's `PATH`?
fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(tool).is_file()))
}

#[cfg(test)]
mod tests {
    /// The `crap` arm of `nix/run-gate.sh`, verbatim: the one tier that needs TWO tools, which is
    /// what makes a line a conjunction rather than a list.
    const CRAP_ARM: &str = concat!(
        "    if cargo llvm-cov --version >/dev/null 2>&1 && cargo crap --version >/dev/null 2>&1; then\n",
        "        exec cargo run -q -p xtask -- crap\n",
        "    elif command -v nix >/dev/null 2>&1; then\n",
        "        echo \"run-gate: cargo-crap or cargo-llvm-cov absent, using nix (same pin as CI)\"\n",
        "        exec nix run .#crap\n",
        "    else\n",
        "        echo \"run-gate: SKIPPED the CRAP score - no cargo-crap/cargo-llvm-cov and no nix here.\"\n",
        "    fi\n",
    );

    #[test]
    fn one_line_is_a_conjunction_and_the_lines_are_alternatives() {
        // Both halves matter. Read as one flat list, a host with `cargo-llvm-cov` and neither
        // `cargo-crap` nor nix would come out as *could have run*, which is the over-claiming
        // direction. Read as alternatives per line, a host with nix comes out right.
        let groups = super::probe_groups(CRAP_ARM);
        assert_eq!(
            groups,
            vec![
                vec![String::from("cargo-llvm-cov"), String::from("cargo-crap")],
                vec![String::from("nix")],
            ],
            "{groups:?}"
        );
    }

    #[test]
    fn a_cargo_subcommand_probe_resolves_to_the_executable_path_would_hold() {
        // `cargo nextest --version` succeeds when `cargo-nextest` is on PATH, which is how cargo
        // finds a subcommand - so probing `nextest` would answer about a binary nobody installs.
        assert_eq!(
            super::probes("if cargo nextest --version >/dev/null 2>&1; then"),
            vec![String::from("cargo-nextest")]
        );
        assert_eq!(
            super::probes("if cargo deny --version >/dev/null 2>&1; then"),
            vec![String::from("cargo-deny")]
        );
        // A plain tool keeps its own name, and the shell keyword in front of it is not part of it.
        assert_eq!(
            super::probes("elif command -v nix >/dev/null 2>&1; then"),
            vec![String::from("nix")]
        );
        // And a quoted `bash -c '...'` entry is read through the quote, which is the shape all
        // three inline hooks are written in.
        assert_eq!(
            super::probes(
                "bash -c 'command -v nix >/dev/null 2>&1 && exec nix run .#shellcheck -- -x \"$@\" || echo \"skipped\"' --"
            ),
            vec![String::from("nix")]
        );
        // A line with no probe yields nothing, so it cannot contribute an empty group that every
        // host satisfies.
        assert!(super::probes("        exec cargo run -q -p xtask -- crap").is_empty());
    }

    #[test]
    fn a_case_arm_ends_at_its_own_double_semicolon() {
        const SCRIPT: &str = concat!(
            "case \"$gate\" in\n",
            "tests)\n",
            "    if cargo nextest --version >/dev/null 2>&1; then\n",
            "        cargo nextest run\n",
            "    fi\n",
            "    ;;\n",
            "secrets)\n",
            "    if command -v betterleaks >/dev/null 2>&1; then\n",
            "        exec betterleaks dir .\n",
            "    fi\n",
            "    ;;\n",
            "esac\n",
        );
        // Without the boundary the `tests` arm would inherit `secrets`' probe and a host with
        // betterleaks would read as able to run the test suite.
        assert_eq!(
            super::probe_groups(&super::case_arm(SCRIPT, "tests").expect("the arm")),
            vec![vec![String::from("cargo-nextest")]]
        );
        assert_eq!(
            super::probe_groups(&super::case_arm(SCRIPT, "secrets").expect("the arm")),
            vec![vec![String::from("betterleaks")]]
        );
        assert_eq!(super::case_arm(SCRIPT, "renamed"), None);
    }

    #[test]
    fn a_hook_routed_through_a_gate_that_does_not_exist_is_unreadable() {
        // FAIL CLOSED: the argument was renamed, or the reader stopped matching the script. Either
        // way every `Passed` from that hook would read as coverage again.
        let entry = "bash nix/run-gate.sh renamed-gate";
        let error = super::deciding_text(entry, "tests)\n    :\n    ;;\n").expect_err("the arm is absent");
        assert!(error.contains("declares no `renamed-gate)` arm"), "{error}");
        // And a hook that routes nowhere is decided by its own entry.
        let own = super::deciding_text("bash -c 'command -v nix >/dev/null 2>&1 || echo skipped'", "").expect("its own entry");
        assert_eq!(super::probe_groups(&own), vec![vec![String::from("nix")]]);
    }

    #[test]
    fn every_hook_the_real_config_tiers_is_read_and_the_others_decide_nothing() {
        // Over the REAL two authorities, because the fixtures above prove the reader and not the
        // tree. This is the assertion that reddens when a tier is rewritten into a shape this
        // reader does not know - which is exactly when its notice would stop being load-bearing.
        let root = crate::repo::root().expect("the repo root");
        let text = std::fs::read_to_string(root.join(crate::hooks::CONFIG)).expect("the hook config");
        let declared = crate::hooks::hooks(&text);
        let read = super::over(&root, &declared).expect("the run-gate script");
        assert!(read.unreadable.is_empty(), "{:?}", read.unreadable);
        // NINE of the sixteen, which is the count the report measured by hand.
        assert_eq!(read.deciding, 9, "{} hook(s) decide", read.deciding);
        // And the tools come out per hook, so a tier silently losing its nix fallback is visible.
        let script = std::fs::read_to_string(root.join(super::RUN_GATE)).expect(super::RUN_GATE);
        let tools = |id: &str| -> Vec<super::Group> {
            let hook = declared.iter().find(|hook| hook.id == id).expect(id);
            super::probe_groups(&super::deciding_text(&hook.entry, &script).expect("the deciding text"))
        };
        assert_eq!(tools("shellcheck"), vec![vec![String::from("nix")]]);
        assert_eq!(tools("zizmor"), vec![vec![String::from("nix")]]);
        assert_eq!(tools("betterleaks"), vec![vec![String::from("betterleaks")]]);
        assert_eq!(tools("jscpd"), vec![vec![String::from("jscpd")], vec![String::from("nix")]]);
        assert_eq!(
            tools("rust-tests"),
            vec![vec![String::from("cargo-nextest")], vec![String::from("nix")]]
        );
        assert_eq!(
            tools("rust-crap"),
            vec![
                vec![String::from("cargo-llvm-cov"), String::from("cargo-crap")],
                vec![String::from("nix")],
            ]
        );
        assert_eq!(tools("fmt-parity"), vec![vec![String::from("nix")]]);
        // A hook that only sources the stable environment decides nothing, so its `Passed` is
        // still read as coverage - which is what keeps this from failing a correct tree.
        assert!(tools("rust-clippy").is_empty());
        assert!(tools("hygiene").is_empty());
    }

    #[test]
    fn a_tool_no_host_has_is_not_on_path_and_a_directory_is_not_an_executable() {
        assert!(!super::on_path("sutura-tool-that-does-not-exist"));
        // A DIRECTORY named like the tool must not answer yes, which is what `is_file` buys.
        assert!(!super::on_path("."));
    }
}

//! A loop over the shipped set refuses an EMPTY set, and an empty set is never spelled out.
//!
//! **Its own file for `refusal.rs`' reason: the parent is against the unexemptable 1000-line cap.**
//! Its fixtures come with it, so nothing here can be orphaned by reverting the parent.
//!
//! # The hole
//!
//! Twelve steps across two workflows and two composite actions loop over the shipped-binary set,
//! and none of them guarded the loop. **An EMPTY set is a green job that built nothing** - zero
//! iterations, no `assert-linked.sh`, no `rust-audit-info`, no `syft`, and a green cross matrix
//! under a notice saying the `ci` profile linked. Measured in the exact form the steps are
//! written:
//!
//! ```text
//! $ BINARIES="" bash -c 'set -euo pipefail; for bin in $BINARIES; do echo "would build $bin"; done; echo reached'
//! reached
//! exit=0
//! ```
//!
//! The ABSENT case already fails closed - `set -u` catches an unset variable - which is worth
//! knowing before treating this as bigger than it is. The empty case did not.
//!
//! # Why the parent's own rules could not reach it
//!
//! `is_literal_set` opens with `!value.is_empty()`, so `BINARIES: ""` was neither a declaration nor
//! a failure: it was skipped. And the parent's `checked == 0` refusal - *finding none means this
//! gate is reading nothing* - describes a state the tree cannot reach, because the two composite
//! actions each carry a `default:` on their `binaries` input and those are counted, so `checked`
//! never drops below 2 whatever the workflows say. *At least one row* defends nothing about WHICH
//! row, which is the shape `refusal.rs` records one file over.
//!
//! # The two rules
//!
//! * **an explicitly empty declaration is a declaration**, and it disagrees with `nix/shipped.nix`;
//! * **a `run:` block that loops over the set refuses an empty one, in that block.** This is the
//!   rule that covers the route text cannot see: the input arrives as `${{ inputs.binaries }}`, an
//!   expression, so no literal rule can say what it will resolve to.
//!
//! The refusal is recognised by an allowlist of two spellings, each of which fails the step by
//! itself on an empty value under the `-eo pipefail` a GitHub `shell: bash` step runs with.
//! MEASURED rather than reasoned:
//!
//! ```text
//! test -n "$BINARIES" || { echo "::error::..."; exit 1; }
//! non-empty exit=0     empty exit=1
//! ```
//!
//! # What it does NOT reach
//!
//! * **A third honest spelling.** `if [ -z "$BINARIES" ]; then ... exit 1; fi` is correct and is
//!   REPORTED, because deciding whether a guard reaches an exit is the `GUARD_WINDOW` problem
//!   `crate::venues::acceptance` already carries, and one gate should not own two answers to it.
//!   Under-permissive by choice: the allowlist is what makes *did this fail the step* a property of
//!   the shape rather than of a reading.
//! * **A loop laundered into a `.sh` file or a helper**, and a loop over a differently named
//!   variable. The needle is the variable this set travels in; a rename fails the parent's literal
//!   rule instead.
//! * **Whether the value is CORRECT.** That is the parent's rule. This one says a step cannot go
//!   green having iterated zero times.
//! * **`default: ""` in a file that declares no `binaries` input.** Not this set's default, so it
//!   is out of scope rather than guessed at.

use std::collections::BTreeMap;

use crate::Verdict;

/// The variable the shipped set travels in - a workflow's `env`, and a step `env` inside an
/// action.
const VARIABLE: &str = "BINARIES";

/// The keys a set is spelled under, plus the input default's own key.
const KEYS: [&str; 3] = ["BINARIES:", "binaries:", "default:"];

/// Refusals that fail the step by themselves on an empty value. See the header for the
/// measurement, and for why the list is deliberately short.
const REFUSALS: [&str; 2] = ["test -n \"$BINARIES\"", "${BINARIES:?"];

/// Is this value an explicitly empty string?
///
/// `""` and `''` only. A key with NO value is a YAML null and, in an action's `inputs:`, is how the
/// input block itself opens - so reading that as an empty set would fail every correct action.
fn is_empty_set(value: &str) -> bool {
    value == "\"\"" || value == "''"
}

/// One problem, as a report line.
type Problem = String;

/// Both rules over every workflow and local composite action. Prints its own verdict.
pub(super) fn verdict(files: &BTreeMap<String, String>) -> Verdict {
    let mut problems: Vec<Problem> = Vec::new();
    let mut guarded = 0_usize;
    for (name, text) in files {
        for line in empty_declarations(text) {
            problems.push(format!("{name}:{line} spells the shipped set as an EMPTY string"));
        }
        for (line, body) in run_blocks(text) {
            if !loops_over_the_set(&body) {
                continue;
            }
            if REFUSALS.iter().any(|refusal| body.contains(refusal)) {
                guarded = guarded.saturating_add(1);
            } else {
                problems.push(format!(
                    "{name}:{line} loops over ${VARIABLE} and does not refuse an empty one"
                ));
            }
        }
    }
    if problems.is_empty() {
        println!("xtask check-shipped-binaries: ok - {guarded} step(s) loop over ${VARIABLE}, each refusing an empty one");
        return Verdict::Pass;
    }
    eprintln!(
        "xtask check-shipped-binaries: FAILED - {} step(s) could run over an EMPTY shipped set",
        problems.len()
    );
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("  A `for` loop over an empty variable runs ZERO times and exits 0, so the job is");
    eprintln!("  green having linked, audited and inventoried nothing. `set -u` covers the variable");
    eprintln!("  being UNSET and nothing covered it being empty; the input arrives as an expression,");
    eprintln!("  so no literal rule can say what it resolves to. Add, before the loop:");
    eprintln!();
    eprintln!("    {} || {{ echo \"::error::{VARIABLE} is empty\"; exit 1; }}", REFUSALS[0]);
    Verdict::Fail
}

/// Lines that declare the set as an explicitly empty string, 1-based.
///
/// `default:` is read only in a file that declares a `binaries` input, because that is the only
/// file where that key is this set's default. One `contains`, rather than a second parser of the
/// input block the parent's `spelled` already walks.
fn empty_declarations(text: &str) -> Vec<usize> {
    let declares_input = text.lines().any(|line| line.trim() == "binaries:");
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                return false;
            }
            KEYS.iter()
                .filter(|key| declares_input || **key != "default:")
                .filter_map(|key| trimmed.strip_prefix(key))
                .any(|value| is_empty_set(value.trim()))
        })
        .map(|(index, _)| index.saturating_add(1))
        .collect()
}

/// Does this shell body iterate over the set?
///
/// Comment lines are blanked first, which is load-bearing here: both workflows explain the loop in
/// prose directly above it, and a comment quoting the loop would otherwise read as one.
fn loops_over_the_set(body: &str) -> bool {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .any(|line| line.starts_with("for ") && line.contains(VARIABLE))
}

/// Every `run:` block in one YAML file, as `(1-based line of the key, body)`.
///
/// The body is the lines indented deeper than the `run:` key, which is the only thing about YAML
/// this needs to know - the same rule the parent's `spelled` uses for an input block. A one-line
/// `run: <command>` is its own body, so a loop written on the key is not lost.
fn run_blocks(text: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (index, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        let key = trimmed.strip_prefix("- run:").or_else(|| trimmed.strip_prefix("run:"));
        let Some(value) = key else {
            continue;
        };
        let indent = raw.len().saturating_sub(trimmed.len());
        // `- run:` puts the key two columns right of the list marker, and its body is indented
        // deeper than THAT rather than deeper than the dash.
        let indent = if trimmed.starts_with("- ") {
            indent.saturating_add(2)
        } else {
            indent
        };
        let value = value.trim();
        let line = index.saturating_add(1);
        if !value.is_empty() && !matches!(value, "|" | "|-" | "|+" | ">" | ">-" | ">+") {
            blocks.push((line, String::from(value)));
            continue;
        }
        let mut body = Vec::new();
        for below in lines.iter().skip(line) {
            let inner = below.trim_start();
            if inner.is_empty() {
                continue;
            }
            if below.len().saturating_sub(inner.len()) <= indent {
                break;
            }
            body.push(*below);
        }
        blocks.push((line, body.join("\n")));
    }
    blocks
}

#[cfg(test)]
mod tests {
    use crate::Verdict;
    use std::collections::BTreeMap;

    /// The step as every one of them was written: a loop over the set, no guard.
    const BEFORE: &str = concat!(
        "    steps:\n",
        "      - name: Link check\n",
        "        env:\n",
        "          BINARIES: ${{ inputs.binaries }}\n",
        "        run: |\n",
        "          # One line per shipped binary, in BINARIES order.\n",
        "          for bin in $BINARIES; do\n",
        "            bash nix/assert-linked.sh \"$bin\"\n",
        "          done\n",
        "      - name: Say so\n",
        "        run: echo done\n",
    );

    #[test]
    fn a_loop_with_no_refusal_is_the_failure_this_rule_is_for() {
        // Red against the base behaviour: this is the shape all twelve steps had, and an empty set
        // made every one of them a green job that built nothing.
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), String::from(BEFORE))]);
        assert_eq!(super::verdict(&files), Verdict::Fail);
        // Green with the guard, and only the accepted spelling counts.
        let guarded = BEFORE.replace(
            "          for bin in $BINARIES; do\n",
            "          test -n \"$BINARIES\" || { echo \"::error::empty\"; exit 1; }\n          for bin in $BINARIES; do\n",
        );
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), guarded)]);
        assert_eq!(super::verdict(&files), Verdict::Pass);
    }

    #[test]
    fn a_guard_in_a_different_block_does_not_count() {
        // LOCALITY is the claim, and it is the honest one: a refusal in another step runs in
        // another shell and cannot fail this one.
        let elsewhere = BEFORE.replace("        run: echo done\n", "        run: test -n \"$BINARIES\"\n");
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), elsewhere)]);
        assert_eq!(super::verdict(&files), Verdict::Fail);
    }

    #[test]
    fn a_comment_quoting_the_loop_is_not_a_loop() {
        // Both workflows explain the loop in prose right above it. Read as code, one sentence would
        // turn a guarded step into an unguarded second one.
        let body = concat!(
            "      - name: Notice\n",
            "        run: |\n",
            "          # for bin in $BINARIES; do ... - what the step below does\n",
            "          echo nothing to do here\n",
        );
        let files = BTreeMap::from([(String::from(".github/workflows/x.yml"), String::from(body))]);
        assert_eq!(super::verdict(&files), Verdict::Pass);
    }

    #[test]
    fn an_empty_set_spelled_out_is_a_declaration_and_not_a_skip() {
        // `is_literal_set` opens with `!value.is_empty()`, so this was neither compared nor
        // reported - it was skipped, which is how an empty cross matrix would have gone green.
        let workflow = concat!("env:\n", "  BINARIES: \"\"\n", "jobs:\n");
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), String::from(workflow))]);
        assert_eq!(super::verdict(&files), Verdict::Fail);
        // An input's empty DEFAULT is the same declaration, and is read only where the input is
        // declared - a `default: ""` elsewhere is not this set's default.
        let action = concat!("inputs:\n", "  binaries:\n", "    default: \"\"\n");
        let files = BTreeMap::from([(String::from(".github/actions/x/action.yml"), String::from(action))]);
        assert_eq!(super::verdict(&files), Verdict::Fail);
        let unrelated = concat!("inputs:\n", "  minimum-gb:\n", "    default: \"\"\n");
        let files = BTreeMap::from([(String::from(".github/actions/x/action.yml"), String::from(unrelated))]);
        assert_eq!(super::verdict(&files), Verdict::Pass);
        // And a key with NO value is a YAML null, which is how an input block OPENS - reading that
        // as an empty set would fail every correct action in the repository.
        let opening = concat!("inputs:\n", "  binaries:\n", "    description: the set\n");
        let files = BTreeMap::from([(String::from(".github/actions/x/action.yml"), String::from(opening))]);
        assert_eq!(super::verdict(&files), Verdict::Pass);
    }

    #[test]
    fn a_one_line_run_is_its_own_body() {
        // Without this a loop written on the `run:` key itself would be outside every block, which
        // is the shape `crate::venues::acceptance` records as a whole step escaping a scan.
        let inline = "      - run: for bin in $BINARIES; do echo \"$bin\"; done\n";
        let files = BTreeMap::from([(String::from(".github/workflows/x.yml"), String::from(inline))]);
        assert_eq!(super::verdict(&files), Verdict::Fail);
    }
}

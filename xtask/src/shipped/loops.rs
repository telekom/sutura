//! A `run:` block that reads the shipped set refuses an EMPTY set, and an empty set is never
//! spelled out.
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
//! * **a `run:` block that READS the set refuses an empty one, in that block.** This is the rule
//!   that covers the route text cannot see: the input arrives as `${{ inputs.binaries }}`, an
//!   expression, so no literal rule can say what it will resolve to.
//!
//! # READS, not *loops over*, and the shape list is what forced that
//!
//! The first version keyed on a line beginning `for ` and naming the variable, and **a `while read
//! -r bin; do ... done <<< "$BINARIES"` step was invisible to it** - reproduced in review as an
//! unguarded step in a new workflow with the gate still printing `ok - 8 step(s) ...` and exiting
//! 0. An empty here-string feeds ZERO lines, so that is the gate's own defect reached by a shape
//! its limit list did not name: the same variable, spelled identically, iterated by a different
//! construct. `xargs`, a pipe into `while read`, and `for bin in $(echo $BINARIES)` are three more.
//!
//! **A list of iteration constructs is the wrong shape of answer** - it is under-inclusive by
//! construction, and the missing entry is only ever found by somebody writing it. So the needle is
//! the VARIABLE: a block that names it, other than in a refusal, is a block whose behaviour depends
//! on the set. MEASURED before it was taken, on this tree: *reads the set* and *loops over the set*
//! select the SAME eight `run:` blocks, so the widening costs nothing today. What it can cost later
//! is one line in a step that only REPORTS the set and would now be asked for a guard - and that is
//! the direction to be wrong in, against a shape list that provably missed one.
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
//! # Both rules read the CODE half, and the first version read it on one side only
//!
//! The loop search blanked comment lines and the refusal search read the raw body, so **a guard
//! that existed only in a comment satisfied the rule.** Measured in review: the guard deleted from
//! `cross-link.yml`'s `Link check` step and this gate's own printed remedy pasted into a comment
//! above the loop gave `ok - 8 step(s) ... each refusing an empty one`, exit 0, over one unguarded
//! step. That is the likely edit rather than a contrived one - the FAILED verdict PRINTS the guard
//! as the remedy, and every one of the eight blocks already carries a prose paragraph directly
//! above the loop for it to land in. One [`code`] per block feeds both sides now.
//!
//! # The floor
//!
//! `guarded == 0` used to print *0 step(s) loop over $BINARIES, each refusing an empty one* and
//! return `Pass` - a verdict over an empty scan, inside the fix for verdicts over empty scans.
//! Launder every read into a helper script and delete the eight guards in one edit and that was the
//! whole output. The parent's `checked == 0` refusal cannot reach it, for the reason above: two
//! counted `default:` keys keep `checked` at 2 or more whatever the workflows say. So this rule
//! carries its own floor, keyed on the READS rather than on the literals, because the reads are
//! what it measures. **Both halves are needed and neither substitutes for the other:** the floor
//! alone still counted a `while read` step as one of the eight, and the widened needle alone still
//! permitted `ok - 0 step(s)`.
//!
//! # What it does NOT reach
//!
//! * **A third honest spelling.** `if [ -z "$BINARIES" ]; then ... exit 1; fi` is correct and is
//!   REPORTED, because deciding whether a guard reaches an exit is the `GUARD_WINDOW` problem
//!   `crate::venues::acceptance` already carries, and one gate should not own two answers to it.
//!   Under-permissive by choice: the allowlist is what makes *did this fail the step* a property of
//!   the shape rather than of a reading.
//! * **A read laundered into a `.sh` file or a helper that inherits the variable from the step's
//!   `env:`**, and a read of a differently named variable. The needle is the variable this set
//!   travels in, so the block has to NAME it; a script re-reading it from the environment names it
//!   nowhere in the block, and a rename fails the parent's literal rule instead. This is the one
//!   escape the widened needle does not close, and it is the one that moves the code out of the
//!   workflow, where `just lint-workflows` reaches the extracted script instead.
//! * **Whether the value is CORRECT.** That is the parent's rule. This one says a step cannot go
//!   green having iterated zero times.
//! * **`default: ""` in a file that declares no `binaries` input.** Not this set's default, so it
//!   is out of scope rather than guessed at.
//! * **Whether the RUNNER renders a null `BINARIES:` as an empty string.** Unmeasured - no runner
//!   to ask - so the refusal below rests on the declaration rather than on the resolution: an
//!   environment variable declared with no value is not a shipped set whatever it resolves to.

use std::collections::BTreeMap;

use crate::Verdict;

/// The variable the shipped set travels in - a workflow's `env`, and a step `env` inside an
/// action.
const VARIABLE: &str = "BINARIES";

/// The environment variable's own key. A null under it is an empty set, because nothing opens a
/// block under an `env:` entry.
const ENV_KEY: &str = "BINARIES:";

/// The keys a set is spelled under, plus the input default's own key.
const KEYS: [&str; 3] = [ENV_KEY, "binaries:", "default:"];

/// Refusals that fail the step by themselves on an empty value. See the header for the
/// measurement, and for why the list is deliberately short.
const REFUSALS: [&str; 2] = ["test -n \"$BINARIES\"", "${BINARIES:?"];

/// Is this value an explicitly empty set?
///
/// `""` and `''` under any of [`KEYS`], and NOTHING AT ALL under [`ENV_KEY`]. The null exemption is
/// what the input keys need and what the environment variable does not: `binaries:` with no value
/// is how an action's input block OPENS, so reading that as an empty set would fail every correct
/// action, while `BINARIES:` with no value opens nothing and declares no set. Measured with the
/// exemption applied to both: a null under `cross-link.yml`'s `BINARIES:` left this rule at `ok`
/// and silently dropped the parent's literal comparison from four to three.
fn is_empty_set(key: &str, value: &str) -> bool {
    value == "\"\"" || value == "''" || (key == ENV_KEY && value.is_empty())
}

/// One problem, as a report line.
type Problem = String;

/// What one pass over the files found: every problem, and how many guarded reads were counted.
///
/// The count is a field rather than a printed number for the floor's sake - `guarded == 0` is a
/// verdict about nothing, so the thing that decides it has to be visible to a test.
struct Scan {
    problems: Vec<Problem>,
    guarded: usize,
    /// Files that still spell the set at all, which is what makes the floor's message evidence.
    spelling: usize,
}

/// Both rules over every workflow and local composite action.
fn scan(files: &BTreeMap<String, String>) -> Scan {
    let mut problems: Vec<Problem> = Vec::new();
    let mut guarded = 0_usize;
    let mut spelling = 0_usize;
    for (name, text) in files {
        if text.contains(VARIABLE) {
            spelling = spelling.saturating_add(1);
        }
        for line in empty_declarations(text) {
            problems.push(format!("{name}:{line} spells the shipped set as an EMPTY string"));
        }
        for (line, body) in run_blocks(text) {
            // ONE blanked body, read by both rules. Reading the raw text on either side is the
            // defect the header records.
            let code = code(&body);
            if !reads_the_set(&code) {
                continue;
            }
            if REFUSALS.iter().any(|refusal| code.contains(refusal)) {
                guarded = guarded.saturating_add(1);
            } else {
                problems.push(format!("{name}:{line} reads ${VARIABLE} and does not refuse an empty one"));
            }
        }
    }
    Scan {
        problems,
        guarded,
        spelling,
    }
}

/// Both rules over every workflow and local composite action. Prints its own verdict.
pub(super) fn verdict(files: &BTreeMap<String, String>) -> Verdict {
    let scan = scan(files);
    if scan.problems.is_empty() {
        if scan.guarded == 0 {
            return no_reads(&scan);
        }
        println!(
            "xtask check-shipped-binaries: ok - {} step(s) read ${VARIABLE}, each refusing an empty one",
            scan.guarded
        );
        return Verdict::Pass;
    }
    eprintln!(
        "xtask check-shipped-binaries: FAILED - {} place(s) where an EMPTY shipped set goes unrefused",
        scan.problems.len()
    );
    for problem in &scan.problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("  Every construct that iterates an empty variable runs ZERO times and exits 0 - a");
    eprintln!("  `for` loop, an `xargs`, a `while read` fed a `<<< \"$VAR\"` here-string - so the job");
    eprintln!("  is green having linked, audited and inventoried nothing. `set -u` covers the");
    eprintln!("  variable being UNSET and nothing covered it being empty; the input arrives as an");
    eprintln!("  expression, so no literal rule can say what it resolves to. Add, before the read:");
    eprintln!();
    eprintln!("    {} || {{ echo \"::error::{VARIABLE} is empty\"; exit 1; }}", REFUSALS[0]);
    Verdict::Fail
}

/// THE FLOOR: not one `run:` block reads the set, so the guard rule measured nothing.
fn no_reads(scan: &Scan) -> Verdict {
    eprintln!("xtask check-shipped-binaries: FAILED - no `run:` block reads ${VARIABLE}\n");
    eprintln!(
        "  {} file(s) still spell ${VARIABLE}, and this rule found no read to hold to a guard, so",
        scan.spelling
    );
    eprintln!("  its verdict would have been about an empty set of steps. That is the defect this");
    eprintln!("  rule exists for, one level up. Either the read was laundered out of the `run:`");
    eprintln!("  block - into a helper script inheriting the variable from the step `env:`, which");
    eprintln!("  is this rule's one stated blind spot - or the set no longer travels in a shell");
    eprintln!("  variable at all, in which case delete this rule rather than leaving it green.");
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
                .filter_map(|key| trimmed.strip_prefix(key).map(|value| (*key, value)))
                .any(|(key, value)| is_empty_set(key, value.trim()))
        })
        .map(|(index, _)| index.saturating_add(1))
        .collect()
}

/// The CODE half of a shell body: comment lines dropped, each remaining line trimmed.
///
/// **Load-bearing on both sides of the guard test**, which is the whole point of it being one
/// function called once: both workflows explain the loop in prose directly above it, so a comment
/// quoting the loop reads as a loop and a comment quoting the guard reads as a guard. The second is
/// the measured one - see the header.
fn code(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<&str>>()
        .join("\n")
}

/// Does this shell code read the set for anything other than refusing it?
///
/// The needle is the VARIABLE and not an iteration construct, for the reason the header measures: a
/// list of constructs missed `while read ... done <<< "$BINARIES"`, and the next list would miss the
/// next one. A line that IS a refusal does not count as a read, so a block whose only mention of
/// the set is its own guard is not counted as a step that uses it.
fn reads_the_set(code: &str) -> bool {
    code.lines()
        .filter(|line| !REFUSALS.iter().any(|refusal| line.contains(refusal)))
        .any(|line| line.contains(VARIABLE))
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

    /// `BEFORE` with the accepted refusal added above the loop.
    fn guarded_files() -> BTreeMap<String, String> {
        BTreeMap::from([(
            String::from(".github/workflows/cross-link.yml"),
            BEFORE.replace(
                "          for bin in $BINARIES; do\n",
                "          test -n \"$BINARIES\" || { echo \"::error::empty\"; exit 1; }\n          for bin in $BINARIES; do\n",
            ),
        )])
    }

    #[test]
    fn a_read_with_no_refusal_is_the_failure_this_rule_is_for() {
        // Red against the base behaviour: this is the shape all twelve steps had, and an empty set
        // made every one of them a green job that built nothing.
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), String::from(BEFORE))]);
        assert_eq!(super::verdict(&files), Verdict::Fail);
        // Green with the guard, and only the accepted spelling counts.
        assert_eq!(super::verdict(&guarded_files()), Verdict::Pass);
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
    fn a_comment_quoting_the_loop_is_not_a_read() {
        // Both workflows explain the loop in prose right above it. Read as code, one sentence would
        // turn a guarded step into an unguarded second one.
        let body = concat!(
            "      - name: Notice\n",
            "        run: |\n",
            "          # for bin in $BINARIES; do ... - what the step below does\n",
            "          echo nothing to do here\n",
        );
        let files = BTreeMap::from([(String::from(".github/workflows/x.yml"), String::from(body))]);
        let scan = super::scan(&files);
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        assert_eq!(scan.guarded, 0);
    }

    #[test]
    fn a_guard_that_exists_only_in_a_comment_does_not_count() {
        // MEASURED IN REVIEW on the version before this: the guard deleted from `cross-link.yml`'s
        // `Link check` step and this gate's own printed remedy pasted into the paragraph above the
        // loop gave `ok - 8 step(s) ... each refusing an empty one` and exit 0. The loop search
        // blanked comments and the guard search read the raw body, so the blanking was load-bearing
        // on one side of the same `if`.
        let commented = BEFORE.replace(
            "          # One line per shipped binary, in BINARIES order.\n",
            "          # Add: test -n \"$BINARIES\" || { echo \"::error::BINARIES is empty\"; exit 1; }\n",
        );
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), commented)]);
        let scan = super::scan(&files);
        assert_eq!(scan.guarded, 0);
        assert_eq!(scan.problems.len(), 1, "{:?}", scan.problems);
        assert_eq!(super::verdict(&files), Verdict::Fail);
        // AND THE ARM THAT STILL FIRES: the same spelling as CODE is a guard, which is what keeps
        // this from being a rule that refuses the remedy it prints.
        assert_eq!(super::scan(&guarded_files()).guarded, 1);
    }

    #[test]
    fn a_while_read_over_the_set_is_a_read_and_the_shape_list_missed_it() {
        // MEASURED IN REVIEW on the version before this: an unguarded
        // `while read -r bin; do ... done <<< "$BINARIES"` step in a new workflow left the gate at
        // `ok - 8 step(s) ...` and exit 0. An empty here-string feeds ZERO lines, so that is the
        // defect this rule exists for, reached by a construct its limit list did not name.
        let here_string = BEFORE.replace(
            concat!(
                "          for bin in $BINARIES; do\n",
                "            bash nix/assert-linked.sh \"$bin\"\n",
                "          done\n",
            ),
            concat!(
                "          while read -r bin; do\n",
                "            bash nix/assert-linked.sh \"$bin\"\n",
                "          done <<< \"$BINARIES\"\n",
            ),
        );
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), here_string.clone())]);
        let scan = super::scan(&files);
        assert_eq!(scan.problems.len(), 1, "{:?}", scan.problems);
        assert_eq!(scan.guarded, 0);
        assert_eq!(super::verdict(&files), Verdict::Fail);
        // AND THE ARM THAT STILL FIRES: the same construct WITH the guard is counted, so the
        // widening is about the needle and not about refusing a shape.
        let guarded = here_string.replace(
            "          while read -r bin; do\n",
            "          test -n \"$BINARIES\" || { echo \"::error::empty\"; exit 1; }\n          while read -r bin; do\n",
        );
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), guarded)]);
        assert_eq!(super::scan(&files).guarded, 1);
        assert_eq!(super::verdict(&files), Verdict::Pass);
    }

    #[test]
    fn not_one_read_of_the_set_is_a_failure_and_not_a_verdict_about_nothing() {
        // THE FLOOR. Measured in review: launder every read out of the `run:` blocks and delete all
        // eight guards in one edit, and the whole output was
        // `ok - 0 step(s) loop over $BINARIES, each refusing an empty one`, exit 0 - the empty-scan
        // defect this rule exists to refuse, inside the rule. The parent's `checked == 0` cannot
        // reach it: two counted `default:` keys hold `checked` at 2 or more.
        //
        // Laundered into a helper inheriting the variable from the step `env:`, which is this
        // rule's one stated blind spot - every shape that NAMES `$BINARIES` in the block is a read
        // now, so this is what an invisible one has to look like.
        let laundered = BEFORE.replace(
            concat!(
                "          for bin in $BINARIES; do\n",
                "            bash nix/assert-linked.sh \"$bin\"\n",
                "          done\n",
            ),
            "          bash nix/assert-every-shipped-binary.sh\n",
        );
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), laundered)]);
        let scan = super::scan(&files);
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        assert_eq!(scan.guarded, 0);
        // The file still spells the set, which is what the refusal's message cites as evidence.
        assert_eq!(scan.spelling, 1);
        assert_eq!(super::verdict(&files), Verdict::Fail);
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
        assert!(super::scan(&files).problems.is_empty());
        // And a key with NO value is a YAML null, which is how an input block OPENS - reading that
        // as an empty set would fail every correct action in the repository.
        let opening = concat!("inputs:\n", "  binaries:\n", "    description: the set\n");
        let files = BTreeMap::from([(String::from(".github/actions/x/action.yml"), String::from(opening))]);
        assert!(super::scan(&files).problems.is_empty());
    }

    #[test]
    fn a_null_under_the_environment_key_is_an_empty_set() {
        // The null exemption is what the INPUT keys need and what the environment variable does
        // not - nothing opens a block under an `env:` entry. Measured in review with the exemption
        // applied to both: a null under `cross-link.yml`'s `BINARIES:` left this rule at `ok` and
        // silently dropped the parent's literal comparison from four to three, with no line saying
        // a comparison it used to make had stopped.
        let workflow = concat!("env:\n", "  BINARIES:\n", "jobs:\n");
        let files = BTreeMap::from([(String::from(".github/workflows/cross-link.yml"), String::from(workflow))]);
        let scan = super::scan(&files);
        assert_eq!(scan.problems.len(), 1, "{:?}", scan.problems);
        assert_eq!(super::verdict(&files), Verdict::Fail);
        // AND THE ARM THAT STILL FIRES: a null under the two INPUT keys still opens a block, which
        // is every correct action in the repository.
        let action = concat!("inputs:\n", "  binaries:\n", "    default:\n", "    description: the set\n");
        let files = BTreeMap::from([(String::from(".github/actions/x/action.yml"), String::from(action))]);
        assert!(super::scan(&files).problems.is_empty(), "{:?}", super::scan(&files).problems);
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

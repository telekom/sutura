//! The shell inside a local composite action, extracted so a linter can read it.
//!
//! WHY THIS EXISTS. `ci.yml`'s *Workflow static analysis* step runs three tools, and between them
//! they cover every line of shell this repository ships EXCEPT the shell inside
//! `.github/actions/*/action.yml`:
//!
//! * `zizmor` is pointed at `.github/workflows`.
//! * `actionlint` - which is the one that would otherwise carry this, because it shells each
//!   workflow's `run:` block out to `shellcheck` itself - **cannot read a composite action at all**
//!   at the pinned version. MEASURED rather than assumed, against `actionlint` 1.7.12: handed
//!   `.github/actions/attest-and-sign/action.yml` it reports `"jobs" section is missing in
//!   workflow`, `"on" section is missing in workflow` and `unexpected key "runs" for "workflow"
//!   section` - it parses the file as a workflow and rejects it.
//! * `shellcheck` is run over `find . -name '*.sh'`, and a `run:` block is not a `.sh` file.
//!
//! So the release path's own signing sequence - `nix run .#cosign` over every asset - was shell
//! that nothing had ever linted. **The gap is not new and it is not this file's fault**, but it
//! widens every time a step moves out of a workflow to stay under the 1000-line cap, which is
//! exactly what this repository does.
//!
//! # Why extraction rather than moving the shell into `.sh` files
//!
//! Putting each step's body in a script beside the action would need no new mechanism at all - the
//! existing `files: \.sh$` hook would lint it. It was declined for one reason: a composite action's
//! step is read as configuration, and a reader following `release.yml` into the action to see what
//! `cosign` is invoked with should find the invocation rather than a path to it. Nine scripts across
//! three actions is a worse file to review than three actions.
//!
//! The cost of extracting instead is that this parser can be wrong, which is why it is Rust with
//! tests rather than `awk` in the workflow, and why it FAILS CLOSED: no action directory, no
//! `action.yml`, or a composite action with no extractable step is a non-zero exit rather than a
//! quiet zero-script run that reads as coverage.
//!
//! # What it does NOT check
//!
//! Everything `actionlint` would have said about the action itself - that an input exists, that an
//! expression is well formed, that a `shell:` is declared. This recovers the `shellcheck` half only.
//! Closing the other half needs an `actionlint` that reads composite actions; the measurement above
//! is what makes that a version to watch rather than a hope.

use std::path::Path;

use crate::Verdict;
use crate::repo;

/// Where the local composite actions live, relative to the repo root.
const ACTIONS: &str = ".github/actions";

/// How many lines the emitted header occupies before the extracted body starts.
///
/// Pinned as a constant because the header PRINTS it: a `shellcheck` finding is reported against
/// the extracted file, and the only way back to the `action.yml` line is arithmetic. A header that
/// grew a line without this moving would make every one of those pointers off by one.
const HEADER_LINES: usize = 5;

/// One extracted step.
#[derive(Debug, PartialEq, Eq)]
struct Extracted {
    /// The step's `- name:`, or `unnamed` where it has none.
    step: String,
    /// 1-based line in the source file where the body's first line sits.
    first_body_line: usize,
    /// The body, dedented, with GitHub expressions replaced.
    body: String,
}

/// Replace every `${{ ... }}` with a bare identifier.
///
/// `shellcheck` cannot parse `${{`, and an action's `run:` body may legitimately contain one. The
/// substitution is a token rather than a removal so the surrounding syntax still balances - a
/// deleted expression inside `"$(...)"` would change the parse rather than simplify it.
fn strip_expressions(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find("${{") {
        out.push_str(rest.get(..open).unwrap_or_default());
        let after = rest.get(open.saturating_add(3)..).unwrap_or_default();
        if let Some(close) = after.find("}}") {
            out.push_str("GHA_EXPRESSION");
            rest = after.get(close.saturating_add(2)..).unwrap_or_default();
        } else {
            // An unterminated expression is not ours to repair: keep the rest verbatim so the
            // linter sees the real text and says so.
            out.push_str("${{");
            rest = after;
            break;
        }
    }
    out.push_str(rest);
    out
}

/// Every `run:` block in one `action.yml`.
///
/// The shape this relies on is the one YAML guarantees for a block scalar: `run: |` introduces a
/// body indented deeper than the `run:` key itself, and the body ends at the first non-blank line
/// that is not. A blank line inside a body is kept, because it is part of the script.
///
/// A single-line `run: cmd` is extracted too. It is not a shape this repository uses today, and
/// skipping it would mean a step added in that form was silently unlinted - which is the whole
/// failure mode this module exists to remove.
fn extract(text: &str) -> Vec<Extracted> {
    let mut found = Vec::new();
    let mut step = String::from("unnamed");
    let lines: Vec<&str> = text.lines().collect();
    let mut index = 0_usize;
    while index < lines.len() {
        let line = lines.get(index).copied().unwrap_or_default();
        let trimmed = line.trim_start();
        let indent = line.len().saturating_sub(trimmed.len());

        if let Some(name) = trimmed.strip_prefix("- name:") {
            step = String::from(name.trim());
        } else if let Some(value) = trimmed.strip_prefix("run:") {
            let value = value.trim();
            if value == "|" || value == "|-" || value == ">" || value == ">-" {
                let mut body = String::new();
                let mut cursor = index.saturating_add(1);
                let mut body_indent = None;
                while cursor < lines.len() {
                    let candidate = lines.get(cursor).copied().unwrap_or_default();
                    let candidate_indent = candidate.len().saturating_sub(candidate.trim_start().len());
                    if candidate.trim().is_empty() {
                        body.push('\n');
                        cursor = cursor.saturating_add(1);
                        continue;
                    }
                    if candidate_indent <= indent {
                        break;
                    }
                    let strip = *body_indent.get_or_insert(candidate_indent);
                    body.push_str(&strip_expressions(candidate.get(strip..).unwrap_or_default()));
                    body.push('\n');
                    cursor = cursor.saturating_add(1);
                }
                if !body.trim().is_empty() {
                    found.push(Extracted {
                        step: step.clone(),
                        first_body_line: index.saturating_add(2),
                        body,
                    });
                }
                index = cursor;
                continue;
            } else if !value.is_empty() {
                found.push(Extracted {
                    step: step.clone(),
                    first_body_line: index.saturating_add(1),
                    body: format!("{}\n", strip_expressions(value)),
                });
            }
        }
        index = index.saturating_add(1);
    }
    found
}

/// The emitted script: a shebang, three pointers back to the source, and the body.
///
/// `# shellcheck shell=bash` as well as the shebang, because a composite action's step declares
/// `shell: bash` and the extracted file should not depend on the shebang being believed.
fn script(source: &str, extracted: &Extracted) -> String {
    // The body starts at script line `HEADER_LINES + 1`, so the shift that turns a script line
    // into a source line is `first_body_line - (HEADER_LINES + 1)`. Written out because the
    // obvious `first_body_line - HEADER_LINES` is off by one, and it was: the first version of
    // this said 104 for a block whose body starts at source line 109 and script line 6, which
    // sends every reported finding one line past the code that caused it.
    let offset = extracted.first_body_line.saturating_sub(HEADER_LINES.saturating_add(1));
    format!(
        "#!/usr/bin/env bash\n\
         # shellcheck shell=bash\n\
         # EXTRACTED from {source}, step: {step}\n\
         # Its first line is line {first} there. Add {offset} to a line number here.\n\
         # Do not edit: `just lint-actions` regenerates it into a temporary directory.\n\
         {body}",
        step = extracted.step,
        first = extracted.first_body_line,
        body = extracted.body,
    )
}

/// `cargo xtask action-shell <dir>` - write every composite action's shell into `<dir>`.
pub(crate) fn run(args: &[String]) -> Verdict {
    let Some(out) = args.first() else {
        eprintln!("usage: cargo xtask action-shell <output-directory>");
        eprintln!("  `just lint-actions` is the way to call it - it also runs shellcheck.");
        return Verdict::Usage;
    };
    let Some(root) = repo::root() else {
        eprintln!("xtask action-shell: could not determine the repo root");
        return Verdict::Fail;
    };
    let out = Path::new(out);
    if let Err(error) = std::fs::create_dir_all(out) {
        eprintln!("xtask action-shell: could not create {}: {error}", out.display());
        return Verdict::Fail;
    }

    let dir = root.join(ACTIONS);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("xtask action-shell: FAILED - cannot read {ACTIONS}");
        eprintln!("  Fails rather than passes: a scan that found nowhere to look has checked");
        eprintln!("  nothing, and a zero-script run reads as coverage.");
        return Verdict::Fail;
    };

    let mut actions = 0_usize;
    let mut written = 0_usize;
    let mut silent = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let action = entry.path();
        let label = action
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        for candidate in ["action.yml", "action.yaml"] {
            let path = action.join(candidate);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            actions = actions.saturating_add(1);
            names.push(label.clone());
            let source = format!("{ACTIONS}/{label}/{candidate}");
            let steps = extract(&text);
            // A COMPOSITE action with no extractable step is the parser being wrong or the file
            // having changed shape; a `using: node20` action legitimately has none, so the
            // distinction is read off the file rather than assumed.
            if steps.is_empty() && text.contains("using: composite") {
                silent.push(source.clone());
                continue;
            }
            for (n, step) in steps.iter().enumerate() {
                let target = out.join(format!("{label}-{n}.sh"));
                if let Err(error) = std::fs::write(&target, script(&source, step)) {
                    eprintln!("xtask action-shell: could not write {}: {error}", target.display());
                    return Verdict::Fail;
                }
                written = written.saturating_add(1);
            }
        }
    }

    if actions == 0 {
        eprintln!("xtask action-shell: FAILED - no action.yml under {ACTIONS}");
        return Verdict::Fail;
    }
    if !silent.is_empty() {
        eprintln!("xtask action-shell: FAILED - composite action(s) with no extractable shell:");
        for source in &silent {
            eprintln!("  {source}");
        }
        eprintln!("  Either the file stopped using `run:` steps, or this parser no longer reads");
        eprintln!("  the shape it is written in. Both mean that shell is unlinted, which is the");
        eprintln!("  state this task exists to prevent - so it is a failure, not a note.");
        return Verdict::Fail;
    }

    names.sort();
    names.dedup();
    println!(
        "xtask action-shell: wrote {written} script(s) from {actions} action(s) ({}) into {}",
        names.join(", "),
        out.display()
    );
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use super::{HEADER_LINES, extract, script, strip_expressions};

    #[test]
    fn a_block_scalar_is_dedented_and_located() {
        let text = concat!(
            "runs:\n",
            "  using: composite\n",
            "  steps:\n",
            "    - name: Pack\n",
            "      shell: bash\n",
            "      run: |\n",
            "        set -eu\n",
            "        echo hi\n",
        );
        let found = extract(text);
        assert_eq!(found.len(), 1);
        let first = found.first().expect("one step");
        assert_eq!(first.step, "Pack");
        // Line 7 of the file is `set -eu`. A pointer that is off by one sends a reader to the
        // wrong line of a hand-written config file, which is worse than no pointer.
        assert_eq!(first.first_body_line, 7);
        assert_eq!(first.body, "set -eu\necho hi\n");
    }

    #[test]
    fn a_block_ends_at_the_next_key_and_keeps_its_blank_lines() {
        // The two halves of the boundary rule. A blank line is part of a script; the next key at
        // or below the `run:` indent is not - and reading past it would splice YAML into bash.
        let text = concat!(
            "    - name: One\n",
            "      run: |\n",
            "        a\n",
            "\n",
            "        b\n",
            "    - name: Two\n",
            "      run: |\n",
            "        c\n",
        );
        let found = extract(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found.first().expect("first").body, "a\n\nb\n");
        assert_eq!(found.get(1).expect("second").step, "Two");
        assert_eq!(found.get(1).expect("second").body, "c\n");
    }

    #[test]
    fn a_single_line_run_is_extracted_too() {
        // Not a shape this repository uses, and skipping it would mean a step added in that form
        // was silently unlinted - the one failure this module exists to remove.
        let found = extract("    - name: Quick\n      run: echo hi\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found.first().expect("one").body, "echo hi\n");
    }

    #[test]
    fn a_github_expression_becomes_a_token_rather_than_a_hole() {
        // Deleting it would change the parse of whatever encloses it; `shellcheck` cannot read
        // `${{` at all, so leaving it is not an option either.
        assert_eq!(strip_expressions("X=${{ inputs.target }}"), "X=GHA_EXPRESSION");
        assert_eq!(
            strip_expressions("a ${{ x }} b ${{ y }} c"),
            "a GHA_EXPRESSION b GHA_EXPRESSION c"
        );
        // An unterminated one is handed to the linter verbatim rather than repaired here.
        assert_eq!(strip_expressions("X=${{ oops"), "X=${{ oops");
        assert_eq!(strip_expressions("plain"), "plain");
    }

    #[test]
    fn the_header_offset_lands_on_the_source_line() {
        // WHAT THIS CAUGHT, which is the reason it is written as arithmetic rather than as a
        // string comparison: the first version computed `first_body_line - HEADER_LINES` and was
        // off by one, so every shellcheck finding would have pointed one line past its cause.
        //
        // Padded so the body does not start above `HEADER_LINES` - a three-line fixture makes the
        // subtraction saturate and the relation untestable, which is a property of the fixture and
        // not of the code.
        let padding = "# a
"
        .repeat(20);
        let text = format!(
            "{padding}    - name: Pack
      run: |
        set -eu
        echo hi
"
        );
        let found = extract(&text);
        let step = found.first().expect("one step");
        let emitted = script(".github/actions/x/action.yml", step);
        let lines: Vec<&str> = emitted.lines().collect();
        assert_eq!(
            lines.len().saturating_sub(step.body.lines().count()),
            HEADER_LINES,
            "the header is not {HEADER_LINES} lines: {lines:?}"
        );

        // The header states the shift; applying it to the script line where the body starts has to
        // land on the source line the body really starts at.
        let offset = step.first_body_line.saturating_sub(HEADER_LINES.saturating_add(1));
        let stated = format!("Add {offset} to a line number here");
        assert!(emitted.contains(&stated), "the header does not state the shift: {emitted}");
        let body_starts_at = HEADER_LINES.saturating_add(1);
        assert_eq!(body_starts_at.saturating_add(offset), step.first_body_line);

        // And the source line it names really is the first line of the body.
        let source_lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            source_lines.get(step.first_body_line.saturating_sub(1)).map(|l| l.trim()),
            Some("set -eu")
        );
    }

    #[test]
    fn every_local_composite_action_yields_shell() {
        // The parser against the tree it guards, `shared_client`'s reason: a refactor can pass its
        // own fixtures and read nothing out of the real files. This is also the assertion that
        // would have caught the gap in the first place - `attest-and-sign`'s `cosign` invocations
        // are shell that nothing had ever linted.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(root.join(super::ACTIONS)) else {
            panic!("no .github/actions directory");
        };
        let mut checked = 0_usize;
        for entry in entries.flatten() {
            let path = entry.path().join("action.yml");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if !text.contains("using: composite") {
                continue;
            }
            checked = checked.saturating_add(1);
            assert!(
                !extract(&text).is_empty(),
                "{} is a composite action and no shell was extracted from it",
                path.display()
            );
        }
        assert!(checked >= 3, "expected at least three composite actions, found {checked}");
    }
}

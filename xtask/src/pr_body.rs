//! Refuses a pull-request body where a negation sits in front of a GitHub closing keyword and
//! its issue reference - the parser links and closes the issue regardless of the negation.
//!
//! **THE HOLE, measured on this repo (GraphQL `ClosedEvent.closer`).** #1609's body "... It does
//! not close #137." closed #137 on merge (2026-10-26); #1596's "... This does not close #828: ..."
//! closed #828 (2026-10-26); #1484's "... This does not close #138." closed #138 (2026-10-17).
//! GitHub's own grammar for linking a pull request to an issue ("Linking a pull request to an
//! issue", docs.github.com, checked 2026-10-27) names nine keywords - `close`, `closes`,
//! `closed`, `fix`, `fixes`, `fixed`, `resolve`, `resolves`, `resolved` - "followed by colons or
//! in uppercase", each taking `#ISSUE-NUMBER` or `OWNER/REPOSITORY#ISSUE-NUMBER`. Nothing in that
//! grammar reads a preceding "not"; the keyword and the reference alone decide.
//!
//! # What this holds, and what it cannot
//!
//! * **Over the whole body as one word stream, within a short window before the keyword.** A PR
//!   body is routinely hard-wrapped, so "... It does not\nclose #137." carries its negation and its
//!   keyword on two different lines - [`findings`] does not split on lines; it takes the
//!   [`WINDOW`] words right before the keyword, wherever the whitespace between them falls, and
//!   judges only those.
//! * **The window crosses a sentence end**, so "We will not ship X. Fixes #12" is refused even
//!   though the negation and the keyword belong to different clauses - closed on this rule's own
//!   terms rather than left open, at the cost of one rewrite for a body phrased that way.
//! * **The negation vocabulary is a closed list** (`not`, `n't`, `never`, `nor`, `without`) that a
//!   paraphrase can still walk past - "fails to close #10" reads as a close to GitHub and is not
//!   caught here. Widening the list costs nothing against a false pass; narrowing the KEYWORD list
//!   would instead miss a real GitHub keyword, which is the asymmetry this gate is built against.
//! * **The reference forms are `#N`, `OWNER/REPO#N`, and a `github.com/.../issues|pull/N` URL** -
//!   the three the task names. `GH-N` autolinks on GitHub ("Autolinked references and URLs") but
//!   is not one of the closing-keyword's own reference forms, so it is out of this gate's scope.
//! * **The workflow's `grep -E` is a form of this rule; this subcommand is its oracle.** `pr-title.yml`
//!   fetches the pull request's CURRENT body at run time (never the frozen event payload, so an
//!   edit after the last push is judged too - #1249's reason the title half of that workflow
//!   exists) and judges it with one `grep -Eiq`, with `[^[:space:]]*`/`[^[:alnum:][:space:]]*` runs
//!   flanking the negation and keyword groups so punctuation sitting right against either word
//!   ("not, close #5", "not (closes #6)") agrees with [`negated_window`]'s `\b`-bounded match
//!   rather than being missed by a stricter, whitespace-only ERE. `the_workflow_regex_agrees_with_the_rule`
//!   runs that step's own body under a fake `gh`, body by body, to hold it to this rule. This
//!   subcommand is a local check only - no CI step calls it.
//! * **On refusal the step logs only the matched span, never the body.** The body is
//!   attacker-controlled text; echoing it raw would let an embedded `::stop-commands::` or
//!   `::add-mask::` line be read back as a workflow command rather than displayed. The step
//!   extracts the same `grep -Eio` match and prefixes every line with `pr-body: refused: ` instead
//!   of printing the body itself; `a_refusal_never_echoes_a_raw_body_line_as_a_workflow_command`
//!   holds it.

use std::sync::LazyLock;

use regex::Regex;

use crate::Verdict;

/// How many words before a closing keyword this reads for a negation. Every measured case needs
/// one ("does **not** close"); four gives headroom for "This PR does not close #N" without
/// reaching into an earlier, unrelated clause.
const WINDOW: usize = 4;

/// A negation, case-insensitively, or the `n't` a contraction always carries.
const NEGATION: &str = r"(?i)\b(?:not|never|nor|without)\b|n't\b";

/// GitHub's own vocabulary - "Linking a pull request to an issue" names exactly these nine,
/// case-insensitively, optionally followed by a colon.
const KEYWORD: &str = "(?:close|closes|closed|fix|fixes|fixed|resolve|resolves|resolved)";

/// `#N`, `OWNER/REPO#N`, or an issue/pull URL - the three forms the task names.
const REFERENCE: &str = r"(?:https?://github\.com/[\w.-]+/[\w.-]+/(?:issues|pull)/\d+|[\w.-]+/[\w.-]+#\d+|#\d+)";

/// `None` only if the static pattern fails to compile, which a test below would catch; callers
/// then see no match rather than a panic. `\s` (not a line anchor) joins the keyword to its
/// reference across a hard wrap the same way it joins a negation to the keyword in
/// [`negated_window`].
fn keyword_reference() -> Option<&'static Regex> {
    static PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(&format!(r"(?i)\b{KEYWORD}\b:?\s+{REFERENCE}")).ok());
    PATTERN.as_ref()
}

fn negation() -> Option<&'static Regex> {
    static PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(NEGATION).ok());
    PATTERN.as_ref()
}

/// The [`WINDOW`] words of `body` right before byte offset `start`, joined back with a single
/// space, if they carry a negation - `None` otherwise. Whitespace-splitting the whole body rather
/// than one line is what reaches a negation a hard wrap put on the line above the keyword.
fn negated_window(body: &str, start: usize) -> Option<String> {
    let negation = negation()?;
    let prefix = body.get(..start)?;
    let words: Vec<&str> = prefix.split_whitespace().collect();
    let from = words.len().saturating_sub(WINDOW);
    let window = words.get(from..)?.join(" ");
    negation.is_match(&window).then_some(window)
}

/// One negated closing keyword: the words leading into it plus the keyword and its reference, so
/// the refusal can quote exactly what a reviewer would see rather than paraphrase it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) text: String,
}

/// Every negated closing keyword in `body`. One finding per occurrence - a body naming the same
/// issue twice this way is still two places to fix.
pub(crate) fn findings(body: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    let Some(pattern) = keyword_reference() else { return found };
    for hit in pattern.find_iter(body) {
        if let Some(window) = negated_window(body, hit.start()) {
            found.push(Finding {
                text: format!("{window} {}", hit.as_str()),
            });
        }
    }
    found
}

pub(crate) fn run(args: &[String]) -> Verdict {
    let Some(body) = args.first() else {
        eprintln!("xtask check-pr-body: expected the pull request body as one argument");
        return Verdict::Usage;
    };
    let found = findings(body);
    if found.is_empty() {
        println!("xtask check-pr-body: ok");
        return Verdict::Pass;
    }
    eprintln!("xtask check-pr-body: FAILED - a negation precedes a closing keyword, and GitHub ignores the negation:");
    for finding in &found {
        eprintln!("  {}", finding.text);
    }
    eprintln!();
    eprintln!("GitHub still links and closes the referenced issue. Write `Part of #N` or `Refs #N` instead.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{Finding, findings, run};
    use crate::Verdict;

    /// THE THREE REAL BODIES, their offending sentence quoted verbatim so this is a reproduction
    /// rather than a paraphrase. RED ON BASE: the module, and the rule, do not exist there.
    #[test]
    fn the_three_bodies_that_closed_an_issue_anyway_are_refused() {
        for (body, closed) in [
            (
                "This is the bounded inbound Flight framing slice of #137, following `docs/adr/20261017032054-leg-transport-between-sutura-nodes.md`. It does not close #137.",
                137,
            ),
            (
                "Advances the independent M2 slice of #828. The previous smoke stopped at readiness, so it could pass without a query round-trip. This does not close #828: distributed execution, the multi-machine network and identity venue, and benchmark evidence remain outstanding.",
                828,
            ),
            ("Part of #138. Refs #1482. This does not close #138.", 138),
        ] {
            let found = findings(body);
            assert_eq!(found.len(), 1, "{body:?} -> {found:?}");
            assert!(found[0].text.contains(&format!("#{closed}")), "{found:?}");
            assert_eq!(run(&[String::from(body)]), Verdict::Fail);
        }
    }

    /// A PR body is routinely hard-wrapped, so the negation and the keyword a line-based rule would
    /// have paired land on two different lines here. RED against a per-line rule; green here because
    /// `findings` whitespace-splits the whole body rather than one line at a time.
    #[test]
    fn a_hard_wrapped_negation_is_still_refused() {
        let body = "This repository hard-wraps a pull-request body routinely. It does\nnot close #137.";
        let found = findings(body);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].text.contains("#137"), "{found:?}");
        assert_eq!(run(&[String::from(body)]), Verdict::Fail);
    }

    /// The forms this repo writes when it genuinely means to close something.
    #[test]
    fn an_unnegated_closing_keyword_passes() {
        for ok in [
            "Closes #1610",
            "Closes #1605.",
            "The owner approved the fixture approach. Closes #1398.",
            "Fixes octo-org/octo-repo#100",
            "Resolves https://github.com/telekom/sutura/issues/42",
        ] {
            assert!(findings(ok).is_empty(), "{ok:?}");
            assert_eq!(run(&[String::from(ok)]), Verdict::Pass);
        }
    }

    /// A negation with nothing after the keyword to close is not a finding: there is no issue to
    /// mislead GitHub about.
    #[test]
    fn a_body_with_no_reference_passes() {
        let body = "This does not fix anything by itself - a follow-up will.";
        assert_eq!(findings(body), []);
        assert_eq!(run(&[String::from(body)]), Verdict::Pass);
    }

    /// The window is a few words, not the whole body: a negation several clauses earlier must not
    /// taint a later, unrelated close.
    #[test]
    fn the_negation_window_does_not_reach_an_unrelated_sentence() {
        let body = "This does not implement a Flight SQL service. Closes #1610.";
        assert!(findings(body).is_empty(), "{body:?}");
        assert_eq!(run(&[String::from(body)]), Verdict::Pass);
    }

    /// Documented as a cost rather than a surprise: the window reaches across a sentence end, so a
    /// negation about something else entirely still refuses a genuine close right after it.
    #[test]
    fn the_window_crosses_a_sentence_end() {
        let body = "We will not ship it. Fixes #12.";
        assert_eq!(findings(body).len(), 1);
        assert_eq!(run(&[String::from(body)]), Verdict::Fail);
    }

    #[test]
    fn an_absent_argument_is_usage_not_a_pass() {
        assert_eq!(run(&[]), Verdict::Usage);
    }

    #[test]
    fn a_finding_quotes_the_offending_window() {
        assert_eq!(
            findings("Padding words here. It does not close #137.\ntrailer"),
            vec![Finding {
                text: String::from("here. It does not close #137")
            }]
        );
    }

    /// The body of the live `pr-title.yml` step named `PR body`.
    #[cfg(unix)]
    fn body_step() -> String {
        let root = crate::repo::root().expect("the xtask binary discovers the repo root");
        let text =
            std::fs::read_to_string(root.join(".github/workflows/pr-title.yml")).expect("the live pr-title.yml is readable");
        crate::action_shell::extract(&text)
            .into_iter()
            .find(|extracted| extracted.step == "PR body")
            .map(|extracted| extracted.body)
            .expect("pr-title.yml has a step named `PR body`")
    }

    /// What one run of the step did: its exit code and every `gh` call it made.
    #[cfg(unix)]
    #[derive(Debug, PartialEq, Eq)]
    struct Ran {
        code: Option<i32>,
        gh: Vec<String>,
    }

    /// Runs the step body under a fake `gh` that answers every call with `fake_output`, returning
    /// its exit code and the raw stderr text - the lower-level half of [`run_step`], kept separate
    /// so a test can read the full log rather than only the `gh` invocations [`run_step`] filters
    /// it down to.
    #[cfg(unix)]
    fn run_step_raw(body: &str, event: &str, git_ref: &str, fake_output: &str) -> (Option<i32>, String) {
        const FAKE: &str = r#"set -eu
gh() { printf 'gh %s\n' "$*" >&2; printf '%s\n' "$FAKE_OUTPUT"; }
export -f gh
bash --noprofile --norc -c "$STEP"
"#;
        let out = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", FAKE])
            .env_remove("BASH_ENV")
            .env("STEP", body)
            .env("FAKE_OUTPUT", fake_output)
            .env("GITHUB_EVENT_NAME", event)
            .env("GITHUB_REF", git_ref)
            .env("GITHUB_REPOSITORY", "telekom/sutura")
            .output()
            .expect("bash is present in a unix test");
        (out.status.code(), String::from_utf8_lossy(&out.stderr).into_owned())
    }

    /// Runs the step body under a fake `gh` that answers every call with `fake_output`, and returns
    /// the exit code and every `gh` invocation the step made.
    #[cfg(unix)]
    fn run_step(body: &str, event: &str, git_ref: &str, fake_output: &str) -> Ran {
        let (code, stderr) = run_step_raw(body, event, git_ref, fake_output);
        let invoked = stderr
            .lines()
            .filter(|line| line.starts_with("gh "))
            .map(str::to_owned)
            .collect();
        Ran { code, gh: invoked }
    }

    /// The required check judges the body with one `grep -Eiq`, so it needs no toolchain - this
    /// runs that step's own body under a fake `gh` and asserts it agrees with `run` body by body.
    /// RED ON BASE because the workflow carries no such step there.
    #[cfg(unix)]
    #[test]
    fn the_workflow_regex_agrees_with_the_rule() {
        let body = body_step();
        let mut bodies: Vec<String> = vec![
            String::from("This is the bounded inbound Flight framing slice of #137. It does not close #137."),
            String::from("Advances the independent M2 slice of #828. This does not close #828: more remains."),
            String::from("Part of #138. Refs #1482. This does not close #138."),
            String::from("This repository hard-wraps a pull-request body routinely. It does\nnot close #137."),
            String::from("Closes #1610"),
            String::from("Closes #1605."),
            String::from("The owner approved the fixture approach. Closes #1398."),
            String::from("Fixes octo-org/octo-repo#100"),
            String::from("Resolves https://github.com/telekom/sutura/issues/42"),
            String::from("This does not fix anything by itself - a follow-up will."),
            String::from("This does not implement a Flight SQL service. Closes #1610."),
            String::from("We will not ship it. Fixes #12."),
            String::from("Part of #10, no closing keyword at all."),
            String::new(),
            // Punctuation sitting directly against the negation or the keyword, with no space
            // between: the Rust rule reads these via `\b`, which does not require a space on
            // either side, so the ERE needs the optional `[^[:space:]]*`/`[^[:alnum:][:space:]]*`
            // runs flanking it to agree.
            String::from("This does not, close #5."),
            String::from("This does not (closes #6)."),
        ];
        bodies.push(String::from("This doesn't close #9000."));
        for body_text in bodies {
            let shell = run_step(&body, "pull_request", "refs/pull/7/merge", &body_text).code == Some(0);
            let rule = run(core::slice::from_ref(&body_text)) == Verdict::Pass;
            assert_eq!(
                shell, rule,
                "{body_text:?}: the workflow says {shell}, check-pr-body says {rule}"
            );
        }
    }

    /// The verdict follows what `gh` answers for the pull request the event names, so it is the
    /// live body and not the (frozen) event payload; an event naming no pull request is refused
    /// before `gh` is asked.
    #[cfg(unix)]
    #[test]
    fn the_workflow_asks_for_the_queued_pull_request_and_refuses_any_other_event() {
        let body = body_step();
        assert_eq!(
            run_step(&body, "pull_request", "refs/pull/7/merge", "Closes #1"),
            Ran {
                code: Some(0),
                gh: vec![String::from("gh api repos/telekom/sutura/pulls/7 --jq .body")]
            }
        );
        assert_eq!(
            run_step(
                &body,
                "merge_group",
                "refs/heads/gh-readonly-queue/main/pr-1257-0123abcd",
                "Closes #1"
            ),
            Ran {
                code: Some(0),
                gh: vec![String::from("gh api repos/telekom/sutura/pulls/1257 --jq .body")]
            }
        );
        assert_eq!(
            run_step(&body, "push", "refs/heads/main", "Closes #1"),
            Ran {
                code: Some(1),
                gh: vec![]
            }
        );
        assert_eq!(
            run_step(&body, "merge_group", "refs/heads/main", "Closes #1"),
            Ran {
                code: Some(1),
                gh: vec![]
            }
        );
        assert_eq!(
            run_step(&body, "pull_request", "refs/pull/7/merge", "It does not close #1."),
            Ran {
                code: Some(1),
                gh: vec![String::from("gh api repos/telekom/sutura/pulls/7 --jq .body")]
            }
        );
    }

    /// The body is attacker-controlled text. If a refusal ever echoed it raw, a line in it starting
    /// `::stop-commands::` or `::add-mask::` would be read back as a GitHub Actions workflow command
    /// rather than displayed - the step must log only the matched span behind its own fixed prefix,
    /// never a body line verbatim (the title step is safe only because a title carries no newline).
    #[cfg(unix)]
    #[test]
    fn a_refusal_never_echoes_a_raw_body_line_as_a_workflow_command() {
        let body = body_step();
        let hostile = "::stop-commands::pwned\nIt does not close #137.";
        let (code, stderr) = run_step_raw(&body, "pull_request", "refs/pull/7/merge", hostile);
        assert_eq!(code, Some(1), "{stderr:?}");
        assert!(
            stderr.lines().all(|line| !line.starts_with("::")),
            "a workflow-command-shaped line reached the log: {stderr:?}"
        );
    }
}

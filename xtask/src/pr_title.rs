//! The conventional-commit vocabulary, held on the one subject that becomes permanent history.
//!
//! **THE HOLE, measured on `main`** - `github.com/telekom/sutura#935`. `crate::commit_msg` is the
//! `commit-msg` hook's rule and it judges commits made in a checkout. The merge queue SQUASHES,
//! and the squash takes its subject from the **pull-request title** - a string no local hook ever
//! sees, composed by GitHub at merge time. So the type vocabulary was enforced on every commit
//! except the only one that lands.
//!
//! Counted over the last hundred subjects on `main` at `d5d0448e`, six are outside the vocabulary
//! `crate::commit_msg::TYPES` declares: `batch:` three times, `batch C:` once, `spike(metadata):`
//! once, and one subject with no type at all. The hook passed for each of them, the queue merged
//! each of them, and nothing reported the contradiction.
//!
//! # What this holds, and what it cannot
//!
//! * **The vocabulary and the shape, not the length.** 69 of those hundred subjects are longer
//!   than `commit_msg`'s limit, because GitHub appends ` (#N)` to a title written for a reader
//!   rather than for `git log --oneline`. `commit_msg::check_shape` is the half that applies.
//! * **The CURRENT title, fetched at run time, not the frozen event payload.** `pr-title.yml`
//!   fetches it over the API (not a `ci` step) and fires on `edited`, so a RENAME alone starts a
//!   fresh verdict on the live title (#1249), never the payload that is frozen when a run starts.
//!   On `merge_group` it judges the queued pull request's title.
//! * **An event naming no pull request is refused before `gh` is asked.** The step reads the pull
//!   request number from the ref on the two events that carry one, and exits 1 (stopping the step)
//!   on any other event - so an absent title is a red run before the fetch, never a silent pass.
//! * **The workflow's `grep -E` is a form of this rule; this subcommand is its oracle.** The step
//!   judges the title with one `grep -E`, and `the_workflow_regex_agrees_with_the_rule` runs that
//!   step body under a fake `gh` title by title to hold it to this rule. This subcommand is a local
//!   check only - no CI step calls it.

use crate::Verdict;
use crate::commit_msg::{SubjectVerdict, TYPES, check_shape};

/// A pull-request title: the subject GitHub's squash will land, minus the ` (#N)` it appends.
///
/// **A newtype because the empty string is a VENUE mistake and not a bad title.** Handed to the
/// subject rule it would come back as one more `SubjectVerdict`, a verdict about a convention over
/// an input nobody wrote. Parsing refuses it here, so the two failures carry different messages.
#[derive(Debug, PartialEq, Eq)]
struct Title<'a>(&'a str);

impl<'a> Title<'a> {
    /// The title `text` carries, or `None` when it carries none.
    fn parse(text: &'a str) -> Option<Self> {
        let trimmed = text.trim();
        (!trimmed.is_empty()).then_some(Self(trimmed))
    }

    /// What the convention says about it.
    fn judge(&self) -> SubjectVerdict {
        check_shape(self.0)
    }
}

pub(crate) fn run(args: &[String]) -> Verdict {
    let Some(text) = args.first() else {
        eprintln!("xtask check-pr-title: expected the pull request's title as one argument");
        return Verdict::Usage;
    };
    let Some(title) = Title::parse(text) else {
        eprintln!("xtask check-pr-title: FAILED - an empty title is not a pass");
        return Verdict::Fail;
    };
    match title.judge() {
        SubjectVerdict::Ok | SubjectVerdict::Exempt => {
            println!("xtask check-pr-title: ok - `{}`", title.0);
            Verdict::Pass
        }
        other => {
            eprintln!("xtask check-pr-title: FAILED - {}", other.explain());
            eprintln!("  title: {}", title.0);
            eprintln!();
            eprintln!("The merge queue squashes and takes the landed subject from this title, so it is");
            eprintln!("the one subject no local hook sees. Expected `<type>[(scope)][!]: <subject>`.");
            eprintln!("Types: {}", TYPES.join(", "));
            eprintln!("The length is NOT judged here - GitHub appends ` (#N)` and most landed subjects");
            eprintln!("are longer than the commit-msg limit.");
            Verdict::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TYPES, Title, run};
    use crate::Verdict;
    use crate::commit_msg::SubjectVerdict;

    /// **THE SUBJECT THAT LANDED.** `spike(metadata): what a BPMN file actually carries (#154)` is
    /// on `main` as `ed650549` with the queue's ` (#926)` appended, and `spike` is not in the
    /// vocabulary. Nothing judged that string, which is the whole of #935.
    #[test]
    fn the_type_that_reached_main_unjudged_is_refused() {
        let title = Title::parse("spike(metadata): what a BPMN file actually carries (#154)").expect("a title");
        assert_eq!(title.judge(), SubjectVerdict::UnknownType(String::from("spike")));
        assert_eq!(
            run(&[String::from("spike(metadata): what a BPMN file carries")]),
            Verdict::Fail
        );
        // The other shape `main` carries, and the one a batch pull request is titled with today.
        assert_eq!(
            Title::parse("batch: three gate holes").expect("a title").judge(),
            SubjectVerdict::UnknownType(String::from("batch"))
        );
    }

    /// The length is deliberately not this gate's: 69 of the last hundred landed subjects exceed
    /// the commit-msg limit, so holding it here would refuse most of this repository's own merges.
    #[test]
    fn a_title_longer_than_a_commit_subject_is_not_refused() {
        let long = format!("fix(xtask): {}", "x".repeat(120));
        assert_eq!(Title::parse(&long).expect("a title").judge(), SubjectVerdict::Ok);
        assert_eq!(run(&[long]), Verdict::Pass);
    }

    /// Fails CLOSED where the venue, not the author, is wrong: an expression that expanded to
    /// nothing must not reach the subject rule and come back as a convention verdict.
    #[test]
    fn an_absent_title_is_not_a_pass() {
        assert_eq!(Title::parse(""), None);
        assert_eq!(Title::parse("   \n"), None);
        assert_eq!(run(&[String::new()]), Verdict::Fail);
        assert_eq!(run(&[]), Verdict::Usage);
    }

    /// A revert pull request is titled the way GitHub writes it, and refusing that would block a
    /// revert nobody typed a subject for.
    #[test]
    fn the_forms_this_repo_merges_pass() {
        for ok in [
            "fix(xtask): hold the landed subject to the vocabulary",
            "feat!: rename the port",
            "docs: explain why the gate exists",
            "Revert \"feat(identity): the thing\"",
        ] {
            assert_eq!(run(&[String::from(ok)]), Verdict::Pass, "{ok}");
        }
    }

    /// The body of the live `pr-title.yml` step named `PR title`.
    #[cfg(unix)]
    fn title_step() -> String {
        let root = crate::repo::root().expect("the xtask binary discovers the repo root");
        let text =
            std::fs::read_to_string(root.join(".github/workflows/pr-title.yml")).expect("the live pr-title.yml is readable");
        crate::action_shell::extract(&text)
            .into_iter()
            .find(|extracted| extracted.step == "PR title")
            .map(|extracted| extracted.body)
            .expect("pr-title.yml has a step named `PR title`")
    }

    /// What one run of the step did: its exit code and every `gh` call it made.
    #[cfg(unix)]
    #[derive(Debug, PartialEq, Eq)]
    struct Ran {
        code: Option<i32>,
        gh: Vec<String>,
    }

    /// Runs the step body under a fake `gh` - the same way the agreement cell exercises it - and
    /// returns the exit code and every `gh` invocation the step made.
    #[cfg(unix)]
    fn run_step(body: &str, event: &str, git_ref: &str, title: &str) -> Ran {
        const FAKE: &str = r#"set -eu
gh() { printf 'gh %s\n' "$*" >&2; printf '%s\n' "$FAKE_TITLE"; }
export -f gh
bash --noprofile --norc -c "$STEP"
"#;
        let out = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", FAKE])
            .env_remove("BASH_ENV")
            .env("STEP", body)
            .env("FAKE_TITLE", title)
            .env("GITHUB_EVENT_NAME", event)
            .env("GITHUB_REF", git_ref)
            .env("GITHUB_REPOSITORY", "telekom/sutura")
            .output()
            .expect("bash is present in a unix test");
        let invoked = String::from_utf8_lossy(&out.stderr)
            .lines()
            .filter(|line| line.starts_with("gh "))
            .map(str::to_owned)
            .collect();
        Ran {
            code: out.status.code(),
            gh: invoked,
        }
    }

    /// The required check judges the title with one `grep -E`, so it needs no toolchain - this runs
    /// that step's own body under a fake `gh` and asserts it agrees with `run` title by title. RED
    /// ON BASE because the workflow does not exist there. LIMIT: agreement is over this corpus, and
    /// non-ASCII whitespace is outside it - `str::trim` drops it and `[[:space:]]` may not.
    #[cfg(unix)]
    #[test]
    fn the_workflow_regex_agrees_with_the_rule() {
        let body = title_step();
        assert!(
            body.contains(&format!("({})", TYPES.join("|"))),
            "the step judges the same vocabulary as `run`"
        );
        let mut titles: Vec<String> = TYPES.iter().map(|ty| format!("{ty}: x")).collect();
        for title in [
            "fix(xtask): hold the landed subject to the vocabulary",
            "feat!: rename the port",
            "fix(a)!: x",
            "fix(!): x",
            "fix(a(b)): x",
            "fix(a)): x",
            "fix((a): x",
            "  fix: padded  ",
            "\tfix: tabbed",
            "fix: a: b",
            "fix:  x",
            "fix: x\nbatch: y",
            "Revert \"feat(identity): the thing\"",
            "Merge branch 'main' into feat/x",
            "Merge  x",
            "fixup! feat: x",
            "squash!",
            "amend! x",
            "",
            "   ",
            "Merge",
            "Merge ",
            "Revert ",
            "spike(metadata): what a BPMN file actually carries",
            "batch: three gate holes",
            "batch C: two",
            "no type here",
            "fix:x",
            "fix: ",
            "fix:",
            "fix: x.",
            "fix: x. ",
            "fix:  .",
            "fix(): x",
            "fix( ): x",
            "fix(a:b): c",
            "fix!!: x",
            "fix!(a): x",
            "fix(a)b: x",
            "fix(a)!!: x",
            "Fix: x",
            "fixx: x",
            "fix(a: x",
            "batch: y\nfix: x",
            "fix: x\n.",
        ] {
            titles.push(String::from(title));
        }
        for title in titles {
            let shell = run_step(&body, "pull_request", "refs/pull/7/merge", &title).code == Some(0);
            let rule = run(core::slice::from_ref(&title)) == Verdict::Pass;
            assert_eq!(
                shell, rule,
                "{title:?}: the workflow says {shell}, check-pr-title says {rule}"
            );
        }
    }

    /// The verdict follows what `gh` answers for the pull request the event names, so it is the live
    /// title and not the payload; an event naming no pull request is refused before `gh` is asked.
    #[cfg(unix)]
    #[test]
    fn the_workflow_asks_for_the_queued_pull_request_and_refuses_any_other_event() {
        let body = title_step();
        assert_eq!(
            run_step(&body, "pull_request", "refs/pull/7/merge", "fix: x"),
            Ran {
                code: Some(0),
                gh: vec![String::from("gh api repos/telekom/sutura/pulls/7 --jq .title")]
            }
        );
        assert_eq!(
            run_step(
                &body,
                "merge_group",
                "refs/heads/gh-readonly-queue/main/pr-1257-0123abcd",
                "fix: x"
            ),
            Ran {
                code: Some(0),
                gh: vec![String::from("gh api repos/telekom/sutura/pulls/1257 --jq .title")]
            }
        );
        assert_eq!(
            run_step(&body, "push", "refs/heads/main", "fix: x"),
            Ran {
                code: Some(1),
                gh: vec![]
            }
        );
        assert_eq!(
            run_step(&body, "merge_group", "refs/heads/main", "fix: x"),
            Ran {
                code: Some(1),
                gh: vec![]
            }
        );
        assert_eq!(
            run_step(&body, "pull_request", "refs/pull/7/merge", "batch: x"),
            Ran {
                code: Some(1),
                gh: vec![String::from("gh api repos/telekom/sutura/pulls/7 --jq .title")]
            }
        );
    }
}

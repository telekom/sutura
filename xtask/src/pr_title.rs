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
//! * **The title as it stood when the run started.** A title EDITED after the last push does not
//!   start a `ci` run - the workflow's `pull_request` trigger takes the default activity types, and
//!   `edited` is not among them - so the verdict on the pull request is about the title that was
//!   there, and a later edit is not re-judged by this gate. Naming the limit rather than implying
//!   the surface is closed: the merge queue's own event may carry the composed subject, which would
//!   close it, and that is unverified here rather than assumed.
//! * **Fail closed on no title at all.** The workflow expression that supplies it expands to the
//!   empty string on any event without a pull request, so [`Title::parse`] refuses that rather than
//!   letting an absent title reach the subject rule and read as one more verdict.

use crate::Verdict;
use crate::commit_msg::{SubjectVerdict, TYPES, check_shape};

/// A pull-request title: the subject GitHub's squash will land, minus the ` (#N)` it appends.
///
/// **A newtype because the empty string is a VENUE mistake and not a bad title.**
/// `github.event.pull_request.title` expands to nothing on a `merge_group` or `push` run, and an
/// empty string handed to the subject rule comes back as one more `SubjectVerdict` - a verdict
/// about a convention, over an input nobody wrote. Parsing refuses it here, so the two failures
/// carry different messages and a step wired to the wrong event cannot read as a vocabulary
/// problem.
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
        eprintln!("  ci.yml passes `github.event.pull_request.title`; there is nothing to read locally");
        return Verdict::Usage;
    };
    let Some(title) = Title::parse(text) else {
        eprintln!("xtask check-pr-title: FAILED - no title was handed to this gate");
        eprintln!("  An empty argument is what `github.event.pull_request.title` expands to on an");
        eprintln!("  event that carries no pull request, so this is a step on the wrong event rather");
        eprintln!("  than a title anybody wrote - and it is not a pass.");
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
    use super::{Title, run};
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
}

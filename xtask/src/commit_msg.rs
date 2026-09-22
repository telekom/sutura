//! Conventional-commit check for the `commit-msg` hook.
//!
//! `.pre-commit-config.yaml` declares `commit-msg` in `default_install_hook_types`, and
//! `AGENTS.md` states the convention. Neither of those *checks* anything, so this exists to
//! make the convention a mechanism rather than a request - the repo history is read by
//! humans and by release tooling, and a subject line that does not say what changed costs
//! someone else the archaeology.
//!
//! Deliberately narrow. It judges the SUBJECT line only: the type, an optional scope, the
//! breaking-change marker, and length. Body content is a writing question, not a gate's.

/// The types this repo uses. `feat`/`fix`/`refactor`/`chore`/`test`/`docs` are the set named
/// in `AGENTS.md`; the rest are the conventional-commit types that come up in a repo with
/// CI and packaging, and rejecting them would only teach people to bypass the hook.
pub(crate) const TYPES: &[&str] = &[
    "feat", "fix", "refactor", "chore", "test", "docs", "perf", "ci", "build", "style", "revert",
];

/// Subject lines longer than this get truncated by `git log --oneline`, by GitHub's commit
/// list and by most review tools, so the tail is written for nobody.
const MAX_SUBJECT: usize = 72;

use crate::Verdict as TaskVerdict;

/// Why a subject line was rejected. A separate type so the checking logic is testable
/// without a file, a git repo or a process exit.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SubjectVerdict {
    Ok,
    /// A merge, revert or fixup subject that git itself generates.
    Exempt,
    Empty,
    NoColon,
    UnknownType(String),
    EmptyScope,
    EmptySubject,
    NoSpaceAfterColon,
    TrailingPeriod,
    TooLong(usize),
}

impl SubjectVerdict {
    pub(crate) fn explain(&self) -> String {
        match *self {
            Self::Ok | Self::Exempt => String::new(),
            Self::Empty => String::from("the subject line is empty"),
            Self::NoColon => String::from("no `type: ` prefix - expected `feat: ...`"),
            Self::UnknownType(ref t) => {
                format!("unknown type `{t}` - expected one of: {}", TYPES.join(", "))
            }
            Self::EmptyScope => String::from("empty scope - write `feat(scope):` or `feat:`"),
            Self::EmptySubject => String::from("nothing after the colon"),
            Self::NoSpaceAfterColon => String::from("missing space after the colon"),
            Self::TrailingPeriod => String::from("subject ends with `.`"),
            Self::TooLong(n) => format!("subject is {n} chars, limit is {MAX_SUBJECT}"),
        }
    }
}

/// Judge one subject line: the shape, and then the length.
///
/// The length is asked LAST, so a subject that is both malformed and long is reported by what is
/// wrong with it rather than by how far it ran on.
pub(crate) fn check_subject(line: &str) -> SubjectVerdict {
    let subject = line.trim_end();
    match check_shape(subject) {
        SubjectVerdict::Ok => {
            let chars = subject.chars().count();
            if chars > MAX_SUBJECT {
                SubjectVerdict::TooLong(chars)
            } else {
                SubjectVerdict::Ok
            }
        }
        other => other,
    }
}

/// Judge the TYPE, the scope and the subject text - everything except the length.
///
/// **Split out for `crate::pr_title`, and the split is a measurement rather than a tidy-up.** The
/// subject that lands on `main` is composed by GitHub from the pull-request title, and
/// [`MAX_SUBJECT`] is a rule only local commits obey: of the last hundred subjects on `main`, 69
/// are longer than it. A title gate holding the length would refuse most of this repository's real
/// merges, and a gate that reddens correct work gets switched off - so that gate holds the
/// vocabulary and this one holds both.
pub(crate) fn check_shape(subject: &str) -> SubjectVerdict {
    if subject.trim().is_empty() {
        return SubjectVerdict::Empty;
    }
    // git writes these itself; failing them would block a merge nobody typed. A revert pull request
    // is titled the same way, which is why this list is on the shared side of the split.
    for prefix in ["Merge ", "Revert ", "fixup!", "squash!", "amend!"] {
        if subject.starts_with(prefix) {
            return SubjectVerdict::Exempt;
        }
    }

    let Some((prefix, rest)) = subject.split_once(':') else {
        return SubjectVerdict::NoColon;
    };

    // `!` marks a breaking change and is allowed on either `type!` or `type(scope)!`.
    let prefix = prefix.strip_suffix('!').unwrap_or(prefix);

    let type_part = match prefix.split_once('(') {
        Some((ty, scope_part)) => {
            let Some(scope) = scope_part.strip_suffix(')') else {
                return SubjectVerdict::NoColon;
            };
            if scope.trim().is_empty() {
                return SubjectVerdict::EmptyScope;
            }
            ty
        }
        None => prefix,
    };

    if !TYPES.contains(&type_part) {
        return SubjectVerdict::UnknownType(String::from(type_part));
    }
    if rest.is_empty() {
        return SubjectVerdict::EmptySubject;
    }
    if !rest.starts_with(' ') {
        return SubjectVerdict::NoSpaceAfterColon;
    }
    let text = rest.trim();
    if text.is_empty() {
        return SubjectVerdict::EmptySubject;
    }
    if text.ends_with('.') {
        return SubjectVerdict::TrailingPeriod;
    }
    SubjectVerdict::Ok
}

/// The subject line is the first line that is not a comment. `git` puts its template
/// comments after the message, but an editor-abandoned message can start with them.
fn subject_of(message: &str) -> &str {
    message.lines().find(|l| !l.starts_with('#')).unwrap_or("")
}

pub(crate) fn run(args: &[String]) -> TaskVerdict {
    let Some(path) = args.first() else {
        eprintln!("xtask commit-msg: expected the path to the commit message file");
        eprintln!("  (the `commit-msg` hook passes it; run it via prek, not by hand)");
        return TaskVerdict::Usage;
    };
    // The path is read AS HANDED, and that is measured rather than assumed: git passes this hook
    // an absolute path exactly when `.git` is a file, which is every linked worktree, and a
    // relative one only in the primary checkout where `.git` is a directory. So there is nothing
    // here to resolve. A fallback that retried a relative path against `git rev-parse --git-dir`
    // was built and then deleted: no caller can produce the input it existed for.
    let message = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(why) => {
            // The CAUSE, not just the path. `Not a directory` versus `No such file` is the whole
            // difference between a path that resolved somewhere wrong and a message that is absent.
            eprintln!("xtask commit-msg: could not read {path}: {why}");
            return TaskVerdict::Fail;
        }
    };

    let subject = subject_of(&message);
    match check_subject(subject) {
        SubjectVerdict::Ok | SubjectVerdict::Exempt => {
            println!("xtask commit-msg: ok");
            TaskVerdict::Pass
        }
        other => {
            eprintln!("xtask commit-msg: FAILED - {}", other.explain());
            eprintln!("  subject: {subject}");
            eprintln!();
            eprintln!("Expected `<type>[(scope)][!]: <subject>`, at most {MAX_SUBJECT} chars.");
            eprintln!("Types: {}", TYPES.join(", "));
            eprintln!("Example: fix(semantic): reject a dimension absent from the pinned bundle");
            TaskVerdict::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SubjectVerdict, check_subject, subject_of};

    #[test]
    fn accepts_the_forms_this_repo_uses() {
        assert_eq!(check_subject("feat: add the thing"), SubjectVerdict::Ok);
        assert_eq!(check_subject("fix(semantic): reject unknown dimensions"), SubjectVerdict::Ok);
        assert_eq!(check_subject("refactor!: rename the port"), SubjectVerdict::Ok);
        assert_eq!(check_subject("chore(ci)!: drop devenv"), SubjectVerdict::Ok);
        assert_eq!(check_subject("docs: explain why the gate exists"), SubjectVerdict::Ok);
    }

    #[test]
    fn rejects_a_missing_or_wrong_type() {
        assert_eq!(check_subject("add the thing"), SubjectVerdict::NoColon);
        assert_eq!(
            check_subject("feet: typo in the type"),
            SubjectVerdict::UnknownType(String::from("feet"))
        );
    }

    #[test]
    fn rejects_malformed_prefixes() {
        assert_eq!(check_subject("feat(): empty scope"), SubjectVerdict::EmptyScope);
        assert_eq!(check_subject("feat:"), SubjectVerdict::EmptySubject);
        assert_eq!(check_subject("feat:no space"), SubjectVerdict::NoSpaceAfterColon);
        assert_eq!(check_subject("feat: trailing period."), SubjectVerdict::TrailingPeriod);
        assert_eq!(check_subject(""), SubjectVerdict::Empty);
        assert_eq!(check_subject("   "), SubjectVerdict::Empty);
    }

    #[test]
    fn rejects_a_subject_git_log_would_truncate() {
        let long = format!("feat: {}", "x".repeat(80));
        assert!(matches!(check_subject(&long), SubjectVerdict::TooLong(_)));
        // Exactly at the limit is fine - an off-by-one here would be invisible and
        // permanently annoying.
        let exact = format!("feat: {}", "x".repeat(72 - 6));
        assert_eq!(exact.chars().count(), 72);
        assert_eq!(check_subject(&exact), SubjectVerdict::Ok);
    }

    #[test]
    fn exempts_what_git_writes_itself() {
        assert_eq!(check_subject("Merge branch 'main' into feat/x"), SubjectVerdict::Exempt);
        assert_eq!(check_subject("fixup! feat: add the thing"), SubjectVerdict::Exempt);
        // A revert git generates is exempt even though it would otherwise be too long.
        let revert = format!("Revert \"{}\"", "x".repeat(90));
        assert_eq!(check_subject(&revert), SubjectVerdict::Exempt);
    }

    #[test]
    fn skips_leading_comment_lines() {
        assert_eq!(
            subject_of("# please enter a message\nfeat: real subject\n"),
            "feat: real subject"
        );
        assert_eq!(subject_of("feat: first line wins\nbody\n"), "feat: first line wins");
    }
}

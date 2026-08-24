//! Conventional-commit check for the `commit-msg` hook.
//!
//! `.pre-commit-config.yaml` declares `commit-msg` in `default_install_hook_types`, and
//! `AGENTS.md` states the convention. Neither of those *checks* anything, so this exists to
//! make the convention a mechanism rather than a request — the repo history is read by
//! humans and by release tooling, and a subject line that does not say what changed costs
//! someone else the archaeology.
//!
//! Deliberately narrow. It judges the SUBJECT line only: the type, an optional scope, the
//! breaking-change marker, and length. Body content is a writing question, not a gate's.

use std::process::ExitCode;

/// The types this repo uses. `feat`/`fix`/`refactor`/`chore`/`test`/`docs` are the set named
/// in `AGENTS.md`; the rest are the conventional-commit types that come up in a repo with
/// CI and packaging, and rejecting them would only teach people to bypass the hook.
const TYPES: &[&str] = &[
    "feat", "fix", "refactor", "chore", "test", "docs", "perf", "ci", "build", "style", "revert",
];

/// Subject lines longer than this get truncated by `git log --oneline`, by GitHub's commit
/// list and by most review tools, so the tail is written for nobody.
const MAX_SUBJECT: usize = 72;

/// Why a subject line was rejected. A separate type so the checking logic is testable
/// without a file, a git repo or a process exit.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
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

impl Verdict {
    fn explain(&self) -> String {
        match *self {
            Self::Ok | Self::Exempt => String::new(),
            Self::Empty => String::from("the subject line is empty"),
            Self::NoColon => String::from("no `type: ` prefix — expected `feat: ...`"),
            Self::UnknownType(ref t) => {
                format!("unknown type `{t}` — expected one of: {}", TYPES.join(", "))
            }
            Self::EmptyScope => String::from("empty scope — write `feat(scope):` or `feat:`"),
            Self::EmptySubject => String::from("nothing after the colon"),
            Self::NoSpaceAfterColon => String::from("missing space after the colon"),
            Self::TrailingPeriod => String::from("subject ends with `.`"),
            Self::TooLong(n) => format!("subject is {n} chars, limit is {MAX_SUBJECT}"),
        }
    }
}

/// Judge one subject line.
pub(crate) fn check_subject(line: &str) -> Verdict {
    let subject = line.trim_end();
    if subject.trim().is_empty() {
        return Verdict::Empty;
    }
    // git writes these itself; failing them would block a merge nobody typed.
    for prefix in ["Merge ", "Revert ", "fixup!", "squash!", "amend!"] {
        if subject.starts_with(prefix) {
            return Verdict::Exempt;
        }
    }
    let chars = subject.chars().count();
    if chars > MAX_SUBJECT {
        return Verdict::TooLong(chars);
    }

    let Some((prefix, rest)) = subject.split_once(':') else {
        return Verdict::NoColon;
    };

    // `!` marks a breaking change and is allowed on either `type!` or `type(scope)!`.
    let prefix = prefix.strip_suffix('!').unwrap_or(prefix);

    let type_part = match prefix.split_once('(') {
        Some((ty, scope_part)) => {
            let Some(scope) = scope_part.strip_suffix(')') else {
                return Verdict::NoColon;
            };
            if scope.trim().is_empty() {
                return Verdict::EmptyScope;
            }
            ty
        }
        None => prefix,
    };

    if !TYPES.contains(&type_part) {
        return Verdict::UnknownType(String::from(type_part));
    }
    if rest.is_empty() {
        return Verdict::EmptySubject;
    }
    if !rest.starts_with(' ') {
        return Verdict::NoSpaceAfterColon;
    }
    let text = rest.trim();
    if text.is_empty() {
        return Verdict::EmptySubject;
    }
    if text.ends_with('.') {
        return Verdict::TrailingPeriod;
    }
    Verdict::Ok
}

/// The subject line is the first line that is not a comment. `git` puts its template
/// comments after the message, but an editor-abandoned message can start with them.
fn subject_of(message: &str) -> &str {
    message.lines().find(|l| !l.starts_with('#')).unwrap_or("")
}

pub(crate) fn run(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("xtask commit-msg: expected the path to the commit message file");
        eprintln!("  (the `commit-msg` hook passes it; run it via prek, not by hand)");
        return ExitCode::from(2);
    };
    let Ok(message) = std::fs::read_to_string(path) else {
        eprintln!("xtask commit-msg: could not read {path}");
        return ExitCode::FAILURE;
    };

    let subject = subject_of(&message);
    match check_subject(subject) {
        Verdict::Ok | Verdict::Exempt => {
            println!("xtask commit-msg: ok");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("xtask commit-msg: FAILED — {}", other.explain());
            eprintln!("  subject: {subject}");
            eprintln!();
            eprintln!("Expected `<type>[(scope)][!]: <subject>`, at most {MAX_SUBJECT} chars.");
            eprintln!("Types: {}", TYPES.join(", "));
            eprintln!("Example: fix(semantic): reject a dimension absent from the pinned bundle");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Verdict, check_subject, subject_of};

    #[test]
    fn accepts_the_forms_this_repo_uses() {
        assert_eq!(check_subject("feat: add the thing"), Verdict::Ok);
        assert_eq!(check_subject("fix(semantic): reject unknown dimensions"), Verdict::Ok);
        assert_eq!(check_subject("refactor!: rename the port"), Verdict::Ok);
        assert_eq!(check_subject("chore(ci)!: drop devenv"), Verdict::Ok);
        assert_eq!(check_subject("docs: explain why the gate exists"), Verdict::Ok);
    }

    #[test]
    fn rejects_a_missing_or_wrong_type() {
        assert_eq!(check_subject("add the thing"), Verdict::NoColon);
        assert_eq!(
            check_subject("feet: typo in the type"),
            Verdict::UnknownType(String::from("feet"))
        );
    }

    #[test]
    fn rejects_malformed_prefixes() {
        assert_eq!(check_subject("feat(): empty scope"), Verdict::EmptyScope);
        assert_eq!(check_subject("feat:"), Verdict::EmptySubject);
        assert_eq!(check_subject("feat:no space"), Verdict::NoSpaceAfterColon);
        assert_eq!(check_subject("feat: trailing period."), Verdict::TrailingPeriod);
        assert_eq!(check_subject(""), Verdict::Empty);
        assert_eq!(check_subject("   "), Verdict::Empty);
    }

    #[test]
    fn rejects_a_subject_git_log_would_truncate() {
        let long = format!("feat: {}", "x".repeat(80));
        assert!(matches!(check_subject(&long), Verdict::TooLong(_)));
        // Exactly at the limit is fine — an off-by-one here would be invisible and
        // permanently annoying.
        let exact = format!("feat: {}", "x".repeat(72 - 6));
        assert_eq!(exact.chars().count(), 72);
        assert_eq!(check_subject(&exact), Verdict::Ok);
    }

    #[test]
    fn exempts_what_git_writes_itself() {
        assert_eq!(check_subject("Merge branch 'main' into feat/x"), Verdict::Exempt);
        assert_eq!(check_subject("fixup! feat: add the thing"), Verdict::Exempt);
        // A revert git generates is exempt even though it would otherwise be too long.
        let revert = format!("Revert \"{}\"", "x".repeat(90));
        assert_eq!(check_subject(&revert), Verdict::Exempt);
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

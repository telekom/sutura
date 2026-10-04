//! The named exemptions from the refusal of an added test with no base result.
//!
//! An added test the base run produced no result for (`not run at base`) is red on neither tree,
//! so `base::report` refuses it unless [`PATH`] names it with a reason. The file is parsed ONCE,
//! by [`Exemptions::read`] at the top of `causality::run`, so a malformed or stale entry is refused
//! on every arm and not only on a run that reaches the base run.
//!
//! STALE means the name matches no `fn` in the working tree. It is judged against the tree, not
//! against one diff's added tests: an entry another change committed refuses a later diff only
//! when that diff's tree lost the fn. Limits: the key is the bare fn name, so one entry exempts
//! every scoped test of that name in any package, and any `fn` of that name keeps it fresh, test
//! or not; and an entry whose test DOES produce a base result exempts nothing and is not refused.

use std::path::Path;

use super::worktree;

/// The list's path from the repository root.
pub(crate) const PATH: &str = "devco/causality-no-base-exemptions";

/// The names [`PATH`] exempts, each with a reason and each a `fn` the tree still declares.
#[derive(Debug, Default)]
pub(crate) struct Exemptions(Vec<String>);

/// Why [`PATH`] is refused instead of read.
#[derive(Debug)]
pub(crate) enum ExemptionsError {
    /// The file exists and could not be read. Only an absent file means "nothing exempted".
    Unread(std::io::Error),
    /// An entry with no `# reason`.
    NoReason(String),
    /// An entry naming no `fn` in the tree.
    Stale(String),
}

impl std::fmt::Display for ExemptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unread(e) => write!(f, "{PATH} could not be read: {e}"),
            Self::NoReason(name) => write!(f, "{PATH}: `{name}` has no reason - write `{name} # <reason>`"),
            Self::Stale(name) => write!(f, "{PATH}: `{name}` names no fn in the tree - remove the stale line"),
        }
    }
}

impl Exemptions {
    /// [`PATH`] under `root`, empty when absent.
    pub(crate) fn read(root: &Path) -> Result<Self, ExemptionsError> {
        match std::fs::read_to_string(root.join(PATH)) {
            Ok(text) => Self::parse(&text, |name| worktree::declares_fn(root, name)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ExemptionsError::Unread(e)),
        }
    }

    /// One `<name> # <reason>` per line; blank and `#` lines are skipped. `declared` answers
    /// whether the tree still has a `fn` of that name.
    pub(crate) fn parse(text: &str, declared: impl Fn(&str) -> bool) -> Result<Self, ExemptionsError> {
        let mut names = Vec::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let (name, reason) = line.split_once('#').unwrap_or((line, ""));
            let name = name.trim().to_owned();
            if reason.trim().is_empty() {
                return Err(ExemptionsError::NoReason(name));
            }
            if !declared(&name) {
                return Err(ExemptionsError::Stale(name));
            }
            names.push(name);
        }
        Ok(Self(names))
    }

    pub(crate) fn exempts(&self, name: &str) -> bool {
        self.0.iter().any(|one| one == name)
    }
}

#[cfg(test)]
mod tests {
    use super::{Exemptions, ExemptionsError, PATH};

    #[test]
    fn an_entry_with_a_reason_naming_a_declared_fn_exempts_it() {
        let parsed = Exemptions::parse("# header\n\nskipped_a # only built with rdbms\n", |_| true).unwrap();
        assert!(parsed.exempts("skipped_a"));
        assert!(!parsed.exempts("ran"));
    }

    #[test]
    fn an_entry_without_a_reason_is_refused_with_its_bare_name() {
        for text in ["ran\n", "ran #   \n"] {
            let refused = Exemptions::parse(text, |_| true).unwrap_err();
            assert!(
                matches!(refused, ExemptionsError::NoReason(ref name) if name == "ran"),
                "{text:?}: {refused:?}"
            );
            assert!(refused.to_string().contains("write `ran # <reason>`"), "{refused}");
        }
    }

    #[test]
    fn an_entry_naming_no_fn_in_the_tree_is_stale() {
        let refused = Exemptions::parse("gone # the test was deleted\n", |name| name != "gone").unwrap_err();
        assert!(
            matches!(refused, ExemptionsError::Stale(ref name) if name == "gone"),
            "{refused:?}"
        );
    }

    #[test]
    fn an_unreadable_list_is_refused_and_an_absent_one_exempts_nothing() {
        let dir = std::env::temp_dir().join(format!("sutura-no-base-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        assert!(!Exemptions::read(&dir).unwrap().exempts("ran"));
        std::fs::create_dir_all(dir.join(PATH)).unwrap();
        assert!(matches!(Exemptions::read(&dir), Err(ExemptionsError::Unread(_))));
        drop(std::fs::remove_dir_all(&dir));
    }
}

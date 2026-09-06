//! Where a stacked branch's own diff BEGINS, which is not where the trunk's merge base is.
//!
//! `super::provenance::Commit` closed the half of `github.com/telekom/sutura#303` about a ref that
//! MOVES. This is the other half, and it is the failing direction. On the second branch of a stack
//! the merge base with `origin/main` is the fork point of the WHOLE stack, so the diff carries the
//! branch below's implementation: the gate reverts that, finds this branch's tests green against a
//! behaviour nobody on this branch changed, and prints
//! *FAILED - green against base behaviour* about two halves that do not read each other. Measured
//! on `github.com/telekom/sutura#293`, where a `sutura-cli` page test was paired with an `xtask`
//! change; the same commit with the base pointed at the stack parent was green. A gate that
//! reddens correct work gets disabled, and stacking is how this repository is asked to work
//! (`git-ops/stacked-branches`, where rebasing a stack is forbidden), so the default has to be
//! right rather than overridable.
//!
//! THE PARENT IS RECORDED, so it is read rather than guessed. The branch tool this repository
//! pins writes one blob per branch at `refs/branch-metadata/<branch>`, carrying the parent branch
//! name; [`recorded_parent`] is the whole reader and it is a pure parse over that text.
//!
//! WHAT STOPS A BAD PARENT FROM MOVING THE BASE, which is the part worth the type. A recorded
//! parent is metadata, so it can be stale, retargeted by hand, or name a branch off to one side of
//! this one - and a base commit HEAD does not descend from would make the gate revert files this
//! branch never touched. So the narrowing is not "trust the metadata": [`Base::of`] takes the
//! merge base of the two candidate commits and moves the base only when it EQUALS the named one,
//! which is exactly the statement *the named ref's merge base is an ancestor of the parent's*.
//! The consequences, and they are the reason this shape was chosen over an ancestry query:
//!
//! | Recorded parent | What this does | Why that is safe |
//! | --- | --- | --- |
//! | none, or unparseable | measures the ref the caller named | today's answer, unchanged |
//! | the trunk | [`Origin::Agrees`] - same commit, said out loud | nothing narrowed, and the line says the metadata was read |
//! | the branch below in a stack | [`Origin::Narrowed`] - this branch's own fork point | strictly forward of the named base and still an ancestor of HEAD, so the diff can only SHRINK |
//! | an unrelated branch | measures the ref the caller named | their merge base is neither commit, so the equality fails |
//! | a branch behind the named ref | measures the ref the caller named | same equality, other direction |
//!
//! WHICH VENUES THIS CHANGES. `just causality` and `ship-check` both default to `origin/main` and
//! both run where the metadata is, so both get the derived base. CI has no such metadata at all and
//! passes the pull request's own base branch, where the merge base is already the right value, so
//! the derivation finds nothing there and the fallback IS the correct answer rather than a
//! degradation of one.
//!
//! WHAT IT STILL DOES NOT FIX, stated because a control that is claimed wider than it reaches is
//! itself the defect: nothing here asks whether the reverted implementation is something the
//! measured test could even READ. `#293`'s pairing was also wrong in that second way - a `-cli`
//! page test against an `xtask` change - and a correct base does not rule that out. That is
//! `github.com/telekom/sutura#358`'s second candidate shape, it is checkable from `cargo metadata`,
//! and it is not taken here because it needs the frequency measurement over real branch diffs
//! first.

use super::provenance::Commit;

/// A branch name safe to put on a git command line.
///
/// PARSES for the reason [`Commit`] does: the value comes out of a metadata blob and goes into
/// `git merge-base`, so a name carrying an option-looking prefix or a revision suffix would be
/// read by git as something other than a branch. The alphabet is the one this repository's branch
/// names use, and anything outside it reads as *no recorded parent* - which is the fallback, so
/// the direction of a refusal here is the behaviour that shipped before this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BranchRef(String);

impl BranchRef {
    /// The branch `raw` names, if it is one.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let legal = |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.');
        let shaped = !raw.is_empty()
            && raw.len() <= 255
            && raw.chars().all(legal)
            && !raw.contains("..")
            && !raw.starts_with(['-', '.', '/'])
            && !raw.ends_with(['.', '/', '-']);
        shaped.then(|| Self(String::from(raw)))
    }

    /// The name, for a git command line.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The parent branch a metadata blob records, if it records one.
///
/// The blob is JSON written by the branch tool - `{"parentBranchName":"main", ..}` - so it is read
/// with the same parser the rest of this workspace uses rather than by looking for a substring. An
/// absent ref, a blob that is not JSON, a missing key and an empty value are one answer: `None`,
/// which falls back to the ref the caller named.
pub(crate) fn recorded_parent(blob: &str) -> Option<BranchRef> {
    let value: serde_json::Value = serde_json::from_str(blob).ok()?;
    BranchRef::parse(value.get("parentBranchName")?.as_str()?)
}

/// A recorded stack parent, resolved against this branch.
///
/// Three facts, all from git and none decided here, because the decision is [`Base::of`] and it
/// has to be assertable without a repository.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Parent {
    /// The branch below this one, as the metadata recorded it.
    pub(crate) branch: BranchRef,
    /// `git merge-base <branch> HEAD`: where this branch's own commits begin.
    pub(crate) forked: Commit,
    /// `git merge-base <the named ref's merge base> <forked>`.
    ///
    /// The ONE input that says the parent's fork point is forward of the named one rather than off
    /// to one side. Carried rather than computed here so this module shells out to nothing.
    pub(crate) common: Option<Commit>,
}

/// Where the commit the diff is measured against came from.
///
/// Exhaustive at the one place that prints it, with no wildcard, for the reason
/// `super::base::earned` is: a fourth way of choosing a base would be a fourth sentence a reader
/// needs, and `error[E0004]` is what makes writing it unavoidable.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The merge base with the ref the caller named. No recorded parent, or one whose fork point
    /// is not forward of it.
    Named,
    /// A parent is recorded and it forked at the SAME commit - the ordinary single-branch case,
    /// where the parent IS the trunk. Said out loud rather than folded into [`Self::Named`],
    /// because *the metadata was read and agreed* and *there was no metadata* are different facts
    /// and only one of them means the derivation is working.
    Agrees(BranchRef),
    /// The recorded parent forked FORWARD of the named ref, so the named ref's merge base is the
    /// whole stack's fork point and this branch's own diff begins later. Carries the commit it
    /// replaced, so the printed line cannot claim a narrowing that did not happen.
    Narrowed { branch: BranchRef, instead_of: Commit },
}

/// The commit the diff is measured against, and WHY it is that commit.
///
/// One value rather than a commit beside a sentence: the sentence is derived from the same choice
/// the commit is, so a line saying *derived from the stack parent* over the trunk's merge base is
/// not writable.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Base {
    at: Commit,
    from: Origin,
}

impl Base {
    /// Which commit to measure against, given the named ref's merge base and what the branch tool
    /// recorded.
    ///
    /// The narrowing happens only when `parent.common == Some(named)`. That equality is the whole
    /// guard: it holds exactly when the named commit is an ancestor of the parent's fork point,
    /// which - since a fork point with HEAD is an ancestor of HEAD by construction - puts the
    /// derived base between the two and makes the diff a SUBSET of what the caller asked for.
    pub(crate) fn of(named: Commit, parent: Option<Parent>) -> Self {
        let Some(parent) = parent else {
            return Self {
                at: named,
                from: Origin::Named,
            };
        };
        if parent.common.as_ref() != Some(&named) {
            return Self {
                at: named,
                from: Origin::Named,
            };
        }
        if parent.forked == named {
            return Self {
                at: named,
                from: Origin::Agrees(parent.branch),
            };
        }
        Self {
            at: parent.forked,
            from: Origin::Narrowed {
                branch: parent.branch,
                instead_of: named,
            },
        }
    }

    /// The commit every consumer of this gate diffs, checks out and searches against.
    pub(crate) const fn at(&self) -> &Commit {
        &self.at
    }

    /// The line the gate prints before anything runs, so every verdict below it is qualified by
    /// which commit was measured and by what chose it.
    pub(crate) fn measured(&self, named_ref: &str) -> String {
        let head = format!("xtask test-causality: measuring the diff against `{named_ref}`");
        match self.from {
            Origin::Named => format!("{head} (merge base {})", self.at.short()),
            Origin::Agrees(ref branch) => format!(
                "{head} (merge base {}; the stack parent `{}` forked at the same commit)",
                self.at.short(),
                branch.as_str()
            ),
            Origin::Narrowed {
                ref branch,
                ref instead_of,
            } => format!(
                "{head} (merge base {} - DERIVED from the stack parent `{}`, because {} is the fork point of the whole stack)",
                self.at.short(),
                branch.as_str(),
                instead_of.short()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Base, BranchRef, Origin, Parent, recorded_parent};
    use crate::causality::provenance::Commit;

    /// An object name from its short form, so a fixture reads as one commit.
    fn commit(short: &str) -> Commit {
        Commit::parse(&format!("{short}{}", "0".repeat(40 - short.len()))).expect("an object name")
    }

    fn branch(name: &str) -> BranchRef {
        BranchRef::parse(name).expect("a branch name")
    }

    #[test]
    fn a_stack_parent_forward_of_the_named_ref_is_where_this_branch_begins() {
        // THE DEFECT. `origin/main`'s merge base is the fork point of the whole stack, so the diff
        // carried the branch below's implementation and the gate blamed this branch's tests for
        // being green against it. The parent's fork point is the commit this branch's own diff
        // starts at, and it is forward of the named one - which their merge base being the named
        // one is exactly what says.
        let stack_fork = commit("aaa1");
        let own_fork = commit("bbb2");
        let base = Base::of(
            stack_fork.clone(),
            Some(Parent {
                branch: branch("2026-09-06-the-parent"),
                forked: own_fork.clone(),
                common: Some(stack_fork.clone()),
            }),
        );
        assert_eq!(*base.at(), own_fork, "the diff begins at this branch's own fork point");
        // The discarded commit is IN the value, so the sentence cannot be manufactured: it names
        // both, and a reader can see which one it did not use.
        let line = base.measured("origin/main");
        assert!(line.contains(own_fork.short()), "{line}");
        assert!(line.contains(stack_fork.short()), "{line}");
        assert!(line.contains("2026-09-06-the-parent"), "{line}");
    }

    #[test]
    fn a_parent_off_to_one_side_may_not_move_the_base() {
        // THE GUARD, and the reason it is an equality rather than trust in the metadata: a parent
        // that was retargeted by hand, or is stale, can name a branch HEAD does not descend
        // through. Its fork point with HEAD is then not forward of the named base, their merge
        // base is some third commit, and moving the base there would revert files this branch
        // never touched.
        let named = commit("aaa1");
        let aside = Base::of(
            named.clone(),
            Some(Parent {
                branch: branch("unrelated"),
                forked: commit("ccc3"),
                common: Some(commit("d0d4")),
            }),
        );
        assert_eq!(*aside.at(), named);
        assert!(!aside.measured("origin/main").contains("unrelated"));
        // And a merge base git could not resolve at all is the same answer, not a narrowing.
        let unresolved = Base::of(
            named.clone(),
            Some(Parent {
                branch: branch("unrelated"),
                forked: commit("ccc3"),
                common: None,
            }),
        );
        assert_eq!(*unresolved.at(), named);
    }

    #[test]
    fn a_parent_that_forked_at_the_same_commit_narrows_nothing_and_says_so() {
        // The ordinary single-branch case: the recorded parent is the trunk, so both candidates
        // are one commit. It must not read as a narrowing - and it must not read as *no metadata*
        // either, because those two are how a reader tells a working derivation from a silent one.
        let named = commit("aaa1");
        let base = Base::of(
            named.clone(),
            Some(Parent {
                branch: branch("main"),
                forked: named.clone(),
                common: Some(named.clone()),
            }),
        );
        assert_eq!(*base.at(), named);
        let line = base.measured("origin/main");
        assert!(line.contains("forked at the same commit"), "{line}");
        assert!(!line.contains("DERIVED"), "{line}");
        // No metadata at all is the answer that shipped before this module, and it is a THIRD
        // state: *the derivation ran and agreed* and *there was nothing to read* are how a reader
        // tells a working derivation from a silent one.
        let untracked = Base::of(named.clone(), None);
        assert_eq!(*untracked.at(), named);
        assert_eq!(
            untracked.measured("origin/main"),
            format!(
                "xtask test-causality: measuring the diff against `origin/main` (merge base {})",
                named.short()
            )
        );
        assert!(matches!(untracked, Base { from: Origin::Named, .. }));
    }

    #[test]
    fn the_parent_is_read_out_of_the_metadata_blob_rather_than_guessed() {
        // The blob as the branch tool writes it, copied off `refs/branch-metadata/<branch>` in
        // this repository. Parsed as JSON rather than searched for a substring, so a value that
        // merely CONTAINS the key name is not one.
        let blob = concat!(
            r#"{"parentBranchName":"main","#,
            r#""parentBranchRevision":"5ae3bdedd28754a1e5cb8f4ba20b60a34e75e0b0","frozen":false}"#,
        );
        assert_eq!(
            recorded_parent(blob).map(|b| String::from(b.as_str())),
            Some(String::from("main"))
        );
        // Every shape that records no usable parent, each of which falls back to the named ref.
        for none in [
            "",
            "\n",
            "not json",
            r#"{"frozen":false}"#,
            r#"{"parentBranchName":null}"#,
            r#"{"parentBranchName":""}"#,
            r#"{"parentBranchName":"--upload-pack=x"}"#,
            r#"{"parentBranchName":"main~1"}"#,
            r#"{"parentBranchName":"a..b"}"#,
            r#"{"parentBranchName":"has space"}"#,
        ] {
            assert_eq!(recorded_parent(none), None, "records no parent: {none:?}");
        }
    }

    #[test]
    fn a_branch_name_that_is_not_one_git_would_read_as_a_branch_is_refused() {
        // What keeps a revision expression and an option out of the command line. The names this
        // repository writes are the alphabet; a refusal here is the pre-existing behaviour.
        for one in ["main", "feat/thing", "2026-09-06-causality-remainder", "a_b.c"] {
            assert!(BranchRef::parse(one).is_some(), "a branch name: {one}");
        }
        for not in [
            "", "-x", "/x", ".x", "x/", "x.", "a..b", "HEAD@{1}", "x^", "x~2", "a:b", "a b", "*",
        ] {
            assert!(BranchRef::parse(not).is_none(), "not a branch name: {not}");
        }
    }
}

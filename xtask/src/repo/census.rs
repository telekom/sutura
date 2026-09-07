//! What a discovery walk found, as a witness rather than as a `Vec`.
//!
//! Five instances of one shape were measured across four gates
//! (`github.com/telekom/sutura#414`): a gate's *"how much did I read"* number is derived from the
//! same collection its loop walks, so narrowing the loop narrows the witness too and the gate
//! reports `ok` over a subject it mostly did not read. `.take(100)` dropped 99.46% of one walk at
//! exit 0; another left 152 of 156 files unscanned at exit 0; `chmod 000 .github/actions` produced
//! `ok - 2 literal(s) across 8 file(s)` at exit 0 - **the denominator moved with the numerator**.
//!
//! One sentence explains all of them: every gate-local fix in this tree sits **above the
//! discovery**, and a gate comparing `inspected` against `discovered` where both come off the same
//! collection cannot see a failure that happened before the collection was populated.
//!
//! So the remedy is not a better number. It is that **the count stops being the control**: an
//! unreachable subject refuses whether or not anybody reads a count, and a narrowed loop is not
//! expressible whether or not anybody compares two integers.
//!
//! Generalised from `warm_start::pairing`, which already ships this argument for one gate -
//! `Discovered(Vec<Taking>)` with a private field and one constructor, `Swept::over` refusing a
//! length mismatch, and a verdict printing the witness's own length. That spelling stays where it
//! is on purpose: two independent derivations of one argument, and migrating it would delete the
//! reference implementation at the same moment the generalisation is first relied on.

use std::path::{Path, PathBuf};

/// Everything a discovery walk found, and everything it was meant to reach and could not.
///
/// `of` is PRIVATE and [`Census::found`] is `pub(super)`, so `crate::repo` is the only module that
/// can mint one - **one constructor by visibility, not by convention**. A caller cannot conjure a
/// denominator by reaching fewer subjects, and there is no rule to satisfy: the type checker is the
/// thing asking.
///
/// There is deliberately no `IntoIterator`, no `Deref`, no `Borrow`, no `iter` and no `as_slice`.
/// **A caller that cannot hold the iterator cannot narrow it**, which is the whole mechanism, and
/// it is why [`Census::inspect`] takes a closure rather than returning a sequence.
/// `check-newtype-leaks` already refuses `Deref` and `Borrow` on every `.rs` outside `vendor/`;
/// widening it to the other three is the next PR in that issue's stack, and until it lands the
/// absence here is held by review rather than by a gate.
pub(crate) struct Census {
    /// Repo-relative paths with `/` separators, in discovery order.
    of: Vec<String>,
    /// Absolute root to join one of them to.
    root: PathBuf,
    /// Subjects the walk was meant to reach and could not, already worded for a reader.
    unreachable: Vec<String>,
}

/// What a caller did with one subject.
///
/// **Three arms and no fourth**, so a subject cannot leave the loop unaccounted for: the compiler
/// asks which of the three it was.
///
/// There is deliberately **no early-exit arm**, and that omission is the mechanism rather than an
/// oversight. Verified on this tree before relying on it: `xtask/src` contains **zero labelled
/// loops** and **zero `macro_rules!` definitions outside a `#[cfg(test)]` fixture**, so a `break`
/// cannot be hidden from a grep - and neither of the two gates migrated with this type has a
/// `break` in its walk (`line_endings.rs` has none at all; `threshold_expect.rs`'s three are in
/// `blank_comments`, a per-character lexer below the walk). A `Looked::Stop` variant would be the
/// escape hatch that reopens the whole class; if a future gate needs one, that is an architecture
/// decision rather than a parameter.
///
/// **Not generic, and that was measured rather than assumed.** The first draft carried the
/// caller's per-subject result as a type parameter; both migrated gates instantiated it as `()`
/// and its only accessor was dead outside the tests, because every walk in this tree already
/// accumulates through a captured `&mut Vec<_>`. A type parameter nothing instantiates is a
/// witness carrying no values - the third defect `github.com/telekom/sutura#371` names - so the
/// parameter is gone and the accumulation stays where it already was.
pub(crate) enum Looked {
    /// Judged by the caller's own rule, which reports through its own captured accumulator.
    Judged,
    /// Not this gate's business. A scope decision, and the only silent arm there is.
    OutOfScope,
    /// This gate was meant to read it and could not. Becomes a REFUSAL, never a `continue`.
    Unreachable(String),
}

/// Why a census could not produce a verdict. Each arm is a measured defect, not a hypothesis.
#[derive(Debug)]
pub(crate) enum Refusal {
    /// The repo root could not be determined, so nothing was discovered. `repo::root`'s own
    /// comment records this happening from a store path, where the walk found nothing and the gate
    /// announced `ok - 0 text file(s) checked` at exit 0.
    NoRoot,
    /// The walk was meant to reach subjects and could not. Names them.
    Unreachable(Vec<String>),
    /// A path the caller declared it cannot have a verdict without was not judged.
    ///
    /// Generalises `warm_start::pairing::nix_files`' `flake.nix` anchor, and it is strictly
    /// stronger than a count floor: it survives a scope predicate that stopped matching, which a
    /// `== 0` floor does not.
    NotJudged {
        /// The anchor that was declared and not judged.
        path: String,
        /// How many subjects the walk did discover, so a reader can tell an empty walk from a
        /// broken predicate.
        discovered: usize,
    },
    /// Subjects were discovered and the caller judged NONE of them.
    ///
    /// **This arm is why deleting a gate's `== 0` floor costs nothing**, and it was added because
    /// a mutation showed otherwise: with `must_judge` empty and the scope predicate broken,
    /// `check-expect-thresholds` printed `0 of 1166 subject(s) judged` at exit 0, where the
    /// `rs_files == 0` floor it replaced had refused. The two are not interchangeable and the tree
    /// needs both, exactly as `warm_start::Swept` keeps its empty arm beside its subset arm:
    /// **this catches a predicate that matched nothing, [`Refusal::NotJudged`] catches a predicate
    /// that matched plenty and not the file that mattered.**
    NothingJudged {
        /// What the walk did reach, so a reader can tell a broken predicate from an empty tree.
        discovered: usize,
    },
    /// Nothing was discovered at all.
    Empty,
}

impl Refusal {
    /// The sentence a gate prints. The wording lives here once, so no gate writes its own.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::NoRoot => String::from(
                "could not determine the repo root, so NOTHING was discovered - and a verdict over \
                 nothing is the failure a gate exists to prevent, not a pass",
            ),
            Self::Unreachable(subjects) => format!(
                "could not reach {} subject(s) this walk was meant to cover, so every count below \
                 would agree with itself over a tree it never looked at: {}",
                subjects.len(),
                subjects.join("; ")
            ),
            Self::NotJudged { path, discovered } => format!(
                "discovered {discovered} subject(s) and did not judge `{path}`, which this gate \
                 declared it cannot have a verdict without - so the scan is broken rather than the \
                 tree, whatever the count says"
            ),
            Self::NothingJudged { discovered } => format!(
                "discovered {discovered} subject(s) and judged NONE of them, so this gate's own \
                 rule never fired - the scan is broken rather than the tree satisfying it"
            ),
            Self::Empty => String::from(
                "discovered no subject at all, so this verdict would be about an empty tree rather \
                 than about this one",
            ),
        }
    }
}

/// A gate whose loop has NOT been moved inside [`Census::inspect`] yet.
///
/// **This is a closed, compiler-held list rather than a convention.** [`Census::into_listing`] is
/// the transitional door out of the census and it cannot be opened without naming yourself here,
/// so a new gate reaching for the plain `Vec` is a one-line diff in this file that a reviewer sees.
/// And the list cannot go stale: an enum variant nothing constructs is `dead_code`, which is an
/// error under this workspace's `-D warnings` - so the PR that migrates the last caller of a
/// variant is *forced* to delete the variant.
///
/// When the list is empty, `into_listing` and this enum go with it. `WarmStart` is the one entry
/// that stays: `check-warm-start` keeps its own witness deliberately (see this module's header).
pub(crate) enum Unmigrated {
    BootOrder,
    BoundedWait,
    Boundaries,
    Causality,
    Conformance,
    Docs,
    Examples,
    FeatureRemedies,
    Guidance,
    MaxLines,
    NewtypeLeaks,
    OneBound,
    Refusals,
    SerdeParse,
    ShippedBinaries,
    TextHygiene,
    UnusedDeps,
    Venues,
    WarmStart,
    Workflows,
}

impl Census {
    /// Mint one. `pub(super)`, so `crate::repo` is the only caller there can be.
    pub(super) const fn found(root: PathBuf, of: Vec<String>, unreachable: Vec<String>) -> Self {
        Self { of, root, unreachable }
    }

    /// The root, so a caller can join a path to it without holding the listing.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// **The only way to consume a census with a per-subject rule.**
    ///
    /// The loop is HERE, so the caller never receives an iterator and `.take(n)` has nowhere to be
    /// written. `FnMut`, so a gate accumulates findings by capturing its own `&mut Vec<_>` - which
    /// is how every walk in this tree already accumulates, so no gate changes how it reports.
    ///
    /// `must_judge` is the paths this gate declares it cannot have a verdict without. An empty set
    /// is permitted only for a gate whose subject may legitimately be absent, and that is a
    /// reviewable choice rather than a default.
    ///
    /// Refuses five ways before it returns, in this order: an unreachable subject from the walk,
    /// an empty discovery, an unreachable subject from the caller's own read, a scope predicate
    /// that judged nothing, and an anchor that was never judged. [`Refusal::NoRoot`] is
    /// `repo::all_files`' and cannot reach here.
    pub(crate) fn inspect(
        self,
        must_judge: &[&str],
        mut look: impl FnMut(&str) -> Looked,
    ) -> Result<Inspected, Refusal> {
        // BEFORE the loop: a partial tree makes every number below a number about a subset, so
        // there is nothing worth inspecting yet.
        if !self.unreachable.is_empty() {
            return Err(Refusal::Unreachable(self.unreachable));
        }
        if self.of.is_empty() {
            return Err(Refusal::Empty);
        }

        let discovered = self.of.len();
        let mut outstanding: Vec<&str> = must_judge.to_vec();
        let mut judged = 0_usize;
        let mut out_of_scope = 0_usize;
        let mut unreachable: Vec<String> = Vec::new();

        for rel in &self.of {
            match look(rel) {
                Looked::Judged => {
                    outstanding.retain(|anchor| anchor != rel);
                    judged = judged.saturating_add(1);
                }
                Looked::OutOfScope => out_of_scope = out_of_scope.saturating_add(1),
                Looked::Unreachable(why) => unreachable.push(why),
            }
        }

        if !unreachable.is_empty() {
            return Err(Refusal::Unreachable(unreachable));
        }
        if judged == 0 {
            return Err(Refusal::NothingJudged { discovered });
        }
        if let Some(anchor) = outstanding.first() {
            return Err(Refusal::NotJudged {
                path: String::from(*anchor),
                discovered,
            });
        }
        Ok(Inspected {
            judged,
            discovered,
            out_of_scope,
        })
    }

    /// The transitional door: the whole listing, for a gate whose loop has not moved inside
    /// [`Census::inspect`] yet.
    ///
    /// **State the limit next to the claim.** This still refuses on an unreachable subject and on
    /// an empty discovery, so the *discovery* half of the mechanism holds at every call site the
    /// day this lands. What it does NOT hold is the *loop* half: a caller holding this `Vec` can
    /// write `.take(n)` on it, exactly as it can today. Each migration in
    /// `github.com/telekom/sutura#414`'s stack deletes one [`Unmigrated`] variant, and the
    /// compiler refuses to let a spent one linger.
    ///
    /// Taking the token by value rather than by reference so a caller cannot keep one around to
    /// re-open the door with later.
    pub(crate) fn into_listing(self, _caller: Unmigrated) -> Result<Listing, Refusal> {
        if !self.unreachable.is_empty() {
            return Err(Refusal::Unreachable(self.unreachable));
        }
        if self.of.is_empty() {
            return Err(Refusal::Empty);
        }
        Ok((self.root, self.of))
    }
}

/// What an unmigrated gate gets: the root, and the discovered paths as a plain `Vec`.
///
/// Named rather than a tuple because `type_complexity` is tightened in this workspace, and because
/// a name is a place to say what this is: **the transitional shape, not the destination.**
pub(crate) type Listing = (PathBuf, Vec<String>);

/// A verdict that accounted for EVERY subject the walk discovered.
///
/// Fields private, no public constructor, and [`Census::inspect`] is the only thing that returns
/// one - after it refused four ways. So none of the accessors can state a number the walk did not
/// reach: `discovered` is set inside `inspect` from the census's own length, and
/// `judged.len() + out_of_scope == discovered` holds by construction.
pub(crate) struct Inspected {
    judged: usize,
    discovered: usize,
    out_of_scope: usize,
}

impl Inspected {
    /// One spelling, so no gate writes its own arithmetic into a `format!`.
    pub(crate) fn verdict(&self) -> String {
        format!(
            "{} of {} subject(s) judged, {} out of scope",
            self.judged, self.discovered, self.out_of_scope
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Census, Looked, Refusal, Unmigrated};
    use std::path::{Path, PathBuf};

    fn has_ext(rel: &str, ext: &str) -> bool {
        Path::new(rel).extension().is_some_and(|found| found == ext)
    }

    fn census(of: &[&str], unreachable: &[&str]) -> Census {
        Census::found(
            PathBuf::from("/nowhere"),
            of.iter().map(|s| String::from(*s)).collect(),
            unreachable.iter().map(|s| String::from(*s)).collect(),
        )
    }

    #[test]
    fn an_unreachable_subject_refuses_before_the_closure_runs() {
        // THE measured defect: `chmod 000 .github/actions` gave `ok - 2 literal(s) across 8
        // file(s)` at exit 0 because the denominator moved with the numerator. Here the walk's own
        // finding refuses, and it refuses BEFORE any count exists to compare - so no arithmetic a
        // gate writes can agree with itself over the missing subtree.
        let mut ran = 0_usize;
        let refused = census(&["a.rs", "b.rs"], &[".github/actions: Permission denied"]).inspect(&[], |_| {
            ran = ran.saturating_add(1);
            Looked::Judged
        });
        assert!(ran == 0, "the closure must not run over a partial tree, ran {ran} time(s)");
        match refused {
            Err(Refusal::Unreachable(subjects)) => {
                assert_eq!(subjects.len(), 1, "{subjects:?}");
            }
            Err(other) => panic!("wrong arm: {}", other.describe()),
            Ok(inspected) => panic!("a partial tree produced a verdict: {}", inspected.verdict()),
        }
    }

    #[test]
    fn a_subject_the_caller_cannot_read_is_a_refusal_not_a_continue() {
        let refused = census(&["a.rs", "b.rs"], &[]).inspect(&[], |rel| {
            if rel == "b.rs" {
                Looked::Unreachable(format!("{rel}: Permission denied"))
            } else {
                Looked::Judged
            }
        });
        let Err(Refusal::Unreachable(subjects)) = refused else {
            panic!("an unreadable subject was not a refusal");
        };
        assert_eq!(subjects, vec![String::from("b.rs: Permission denied")]);
    }

    #[test]
    fn the_printed_count_is_the_witness_own_length() {
        let inspected = census(&["a.rs", "b.md", "c.rs"], &[])
            .inspect(&[], |rel| {
                if has_ext(rel, "rs") {
                    Looked::Judged
                } else {
                    Looked::OutOfScope
                }
            })
            .expect("nothing to refuse");
        // Both numbers come off the census, and `judged + out_of_scope == discovered` by
        // construction - the gate has no way to print a third number of its own.
        assert_eq!(inspected.verdict(), "2 of 3 subject(s) judged, 1 out of scope");
    }

    #[test]
    fn a_predicate_that_matched_nothing_refuses_even_with_no_anchor_declared() {
        // The regression a mutation of this PR found: deleting a gate's `== 0` floor in favour of
        // `must_judge` is only free if an EMPTY anchor set still refuses a scan that judged
        // nothing. `check-expect-thresholds` printed `0 of 1166 subject(s) judged` at exit 0
        // before this arm existed.
        let refused = census(&["a.md", "b.md"], &[]).inspect(&[], |_| Looked::OutOfScope);
        match refused {
            Err(Refusal::NothingJudged { discovered }) => assert_eq!(discovered, 2),
            Err(other) => panic!("wrong arm: {}", other.describe()),
            Ok(inspected) => panic!("a predicate that matched nothing gave a verdict: {}", inspected.verdict()),
        }
    }

    #[test]
    fn an_anchor_that_was_never_judged_refuses_whatever_the_count_says() {
        // The other half, and the two are not interchangeable: here the predicate matched plenty
        // and missed the one file the gate is about, which no count floor can see.
        let refused = census(&["a.md", "flake.nix", "c.md"], &[]).inspect(&["flake.nix"], |rel| {
            if has_ext(rel, "md") {
                Looked::Judged
            } else {
                Looked::OutOfScope
            }
        });
        match refused {
            Err(Refusal::NotJudged { path, discovered }) => {
                assert_eq!(path, "flake.nix");
                assert_eq!(discovered, 3, "the refusal names what the walk DID reach");
                // and the count floor above it was satisfied: two of three subjects were judged.
            }
            Err(other) => panic!("wrong arm: {}", other.describe()),
            Ok(inspected) => panic!("a missing anchor produced a verdict: {}", inspected.verdict()),
        }
    }

    #[test]
    fn an_anchor_that_was_judged_is_satisfied() {
        let inspected = census(&["flake.nix", "a.md"], &[])
            .inspect(&["flake.nix"], |rel| {
                if has_ext(rel, "nix") {
                    Looked::Judged
                } else {
                    Looked::OutOfScope
                }
            })
            .expect("the anchor was judged");
        assert_eq!(inspected.verdict(), "1 of 2 subject(s) judged, 1 out of scope");
    }

    #[test]
    fn an_empty_discovery_is_a_failure_rather_than_a_pass() {
        // `repo::root`'s own comment records `ok - 0 text file(s) checked` at exit 0 from a store
        // path. Both doors out of a census refuse it now.
        assert!(matches!(census(&[], &[]).inspect(&[], |_| Looked::Judged), Err(Refusal::Empty)));
        assert!(matches!(census(&[], &[]).into_listing(Unmigrated::MaxLines), Err(Refusal::Empty)));
    }

    #[test]
    fn the_transitional_door_refuses_an_unreachable_subject_too() {
        // The property an unmigrated gate DOES get on the day this lands, stated as a test rather
        // than as a sentence in a PR body.
        let refused = census(&["a.rs"], &["docs: Permission denied"]).into_listing(Unmigrated::Docs);
        assert!(matches!(refused, Err(Refusal::Unreachable(_))));
    }

    #[test]
    fn every_refusal_says_what_it_refused() {
        for refusal in [
            Refusal::NoRoot,
            Refusal::NothingJudged { discovered: 7 },
            Refusal::Unreachable(vec![String::from("x")]),
            Refusal::NotJudged {
                path: String::from("flake.nix"),
                discovered: 3,
            },
            Refusal::Empty,
        ] {
            let said = refusal.describe();
            assert!(said.len() > 40, "a refusal a reader cannot act on: {said}");
        }
    }
}

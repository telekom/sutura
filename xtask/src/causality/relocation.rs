//! A commit may DECLARE itself a test-file cleanup, and this module is the check that holds the
//! claim rather than the permission that replaces it.
//!
//! WHY A DECLARATION EXISTS AT ALL. The 1000-line cap makes splitting a near-cap test file
//! necessary, and this gate charges for it: move assertions into a new module declared by a bare
//! `mod x;` and the file they left is revertible - nothing it ADDED is a test - so the base tree
//! loses the declaration, Rust compiles no unreferenced file, the filterset names nothing and the
//! verdict is `super::base::BaseOutcome::NotRun`, *the tests this diff added did not run on base*,
//! at exit 1. The remedy this gate prints (move the harness and leave every assertion where it
//! is) works once, and then the file has no lever left.
//!
//! WHAT THE TRAILER IS AND IS NOT. `Cleanup-Split: <path>` in a commit message says *this commit
//! moves test code and changes none*. That is a human judgement about intent, and it is the only
//! part a human can supply. It is **not** the permission: a trailer nothing checks is the gate
//! that has quietly stopped gating, because anyone can bypass red-before-green by typing a word.
//! So the trailer NARROWS what may pass and never disables the check - [`decide`] holds the claim,
//! and a trailer over a diff that added, removed or re-texted one counted line is a REFUSAL rather
//! than a pass. Both directions are measured in this module's own tests.
//!
//! A TRAILER AND NOT A FLAG, AN ENV VAR OR AN ALLOWLIST FILE, and each alternative fails in its
//! own way. The trailer rides on the commit, so *one-time* is a property of the object rather than
//! of an entry someone has to remember to delete - which is the objection to a
//! `max-lines-ignore`-shaped exemption. It is permanently visible in review and in `git log`,
//! which a flag typed once into a shell is not. And it cannot be supplied by the venue that runs
//! the gate, so CI and a hand run read the same claim off the same object.
//!
//! **THE CLAIM IS RANGE-WIDE AND THAT IS THE SAFE DIRECTION, stated because it is surprising.**
//! [`Claim::of`] reads every message in `base..HEAD` while [`decide`] checks the WHOLE diff, so a
//! trailer on one commit is held against everything the range did: a sibling commit that changed a
//! single counted line breaks the claim instead of riding on it. A net-zero pair of commits does
//! pass, and that is correct rather than a gap - the range is what merges, so the range is the
//! unit. What the trailer's PATH is for is the reviewer: a trailer copy-pasted forward onto an
//! unrelated commit names a file that commit does not touch, and [`Broken::Unnamed`] refuses it.
//!
//! **WHY THIS DOES NOT ASK `super::regions::has_non_test_additions`, and it is the whole of
//! `github.com/telekom/sutura#636`'s review finding.** Both checks below used to delegate *does
//! this line do anything* to that module's predicate, whose own docstring calls itself a
//! heuristic: it treats a blank line, a COMMENT and an ATTRIBUTE as carrying no behaviour, which
//! is right for deciding whether a file is separable and catastrophic as a PERMISSION. Measured on this
//! module's own head, every one of these passed at exit 0 with the trailer present: a deleted
//! `#[serde(deny_unknown_fields)]`, an added `#[ignore]`, a deleted `compile_fail` doctest, and -
//! the one that matters - a doctest assertion ADDED in a production file, under a verdict printing
//! *no assertion was added*. An added test that skipped red-before-green, which is this gate's
//! entire subject. So the exemption here is a SHORT LIST OF EXACT SHAPES rather than a category,
//! and it permits a SURPLUS on the added side only, never on the removed one: see
//! [`is_scaffold`] and [`balance`].
//!
//! **WHAT A PURE-RELOCATION VERDICT DOES NOT PROVE, stated here because an overstated control is
//! itself the defect.** It proves the diff MOVED test lines. It does not prove the assertions are
//! any good, that the coverage they carry is adequate, or anything at all about behaviour - there
//! is no behaviour in the diff to be about. It does not prove HEAD is green either: this arm
//! returns before the head run, like every other arm that answers before the proof block, and
//! *the suite is green* is `just test` and the nix `nextest` check's property rather than this
//! gate's. It does not see a REORDERING: the multiset reads counts, so two lines that swapped
//! places between two test functions balance. What bounds that is [`Broken::Outside`] - every
//! changed line except a BLANK one has to be test code in its own image - so a reordering of
//! production code cannot reach this arm at all, and what remains in reach is test code shuffled
//! within test code, which adds no untested change. And a `#[path]` declaration is resolved
//! against the DEFAULT candidates, so a relocated module reads as unresolved: a false refusal,
//! which is the direction this is allowed to be wrong in.

use std::collections::{BTreeMap, BTreeSet};

use crate::Verdict;
use crate::causality::coverage::{Attributed, Coverage};
use crate::causality::diff::ChangedFile;
use crate::causality::regions::{self, PostImage, TestScope};
use crate::causality::{is_compiled_rust, place};

/// The commit trailer that declares a commit to be a test-file cleanup.
const TRAILER: &str = "Cleanup-Split:";

/// What the commit messages in the measured range DECLARED.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Claim {
    /// The paths the trailers named, deduplicated and sorted.
    paths: Vec<String>,
}

impl Claim {
    /// The claim `log` carries, if it carries one.
    ///
    /// A TRAILER WITH NO PATH IS NO CLAIM, which is the fail-closed direction: it leaves the run
    /// exactly as it was, so a malformed declaration buys nothing. Requiring a path is what stops
    /// the trailer being copy-pasted forward onto an unrelated commit without being obviously
    /// wrong to a reviewer - [`Broken::Unnamed`] refuses a path the diff does not touch.
    pub(super) fn of(log: &str) -> Option<Self> {
        let paths: Vec<String> = log
            .lines()
            .filter_map(|line| line.trim().strip_prefix(TRAILER))
            .map(|rest| String::from(rest.trim()))
            .filter(|path| !path.is_empty())
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        (!paths.is_empty()).then_some(Self { paths })
    }
}

/// Whether the diff is the cleanup its commit message claims.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Relocation {
    /// No trailer. Nothing about this run changes, which is the whole of the untagged behaviour.
    Unclaimed,
    /// Claimed, and every check holds. Carries the paths the trailer named and the added scaffold
    /// lines that were exempt, because a pass has to NAME what it waved through.
    Pure {
        /// The paths the trailer declared.
        paths: Vec<String>,
        /// Every added line the multiset did not account for, deduplicated.
        exempt: Vec<String>,
    },
    /// Claimed, and refused. EVERY reason, so one run names them all rather than sending an author
    /// round the loop once per line.
    Refused(Vec<Broken>),
}

/// One reason a claimed cleanup is not one.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Broken {
    /// The trailer names a path this diff does not change.
    Unnamed(String),
    /// Git says this path changed and the diff reader produced no entry for it - a DELETED file,
    /// whose `+++` is `/dev/null`. Nothing below can account for a line it never saw, so a
    /// `git rm` of a test file would otherwise ride a claim that every changed line was accounted
    /// for. A relocation moves a file's contents; it does not remove the file from the reader.
    Unseen(String),
    /// A changed path that is not Rust this workspace compiles - a manifest, a page, a workflow.
    /// A cleanup split moves Rust; anything else in the diff is a second change.
    NotRust(String),
    /// A changed line that is not test code, read in the image it exists in.
    Outside {
        /// The file it is in.
        path: String,
        /// Its 1-based line number in that image.
        number: usize,
        /// The line itself, trimmed.
        text: String,
        /// Which image answered.
        side: Side,
    },
    /// An added `mod <name>;` whose module file is not in this diff, so the declaration puts a
    /// module of PRE-EXISTING tests into the build. `super::scoped` refuses the same shape, and
    /// this arm answers before the plan - so without this the refusal would simply be bypassed.
    Unresolved {
        /// The file carrying the declaration.
        path: String,
        /// The module it names.
        name: String,
    },
    /// A line one side has and the other does not: something added, removed or re-texted.
    Unbalanced {
        /// The line, as the multiset keyed it.
        text: String,
        /// How many times the diff added it.
        added: usize,
        /// How many times the diff removed it.
        removed: usize,
    },
}

/// Which image a line that is not test code was read in.
///
/// An added line exists only in the post-image and a removed one only in the base image, so the
/// question *was this test code* has two readers and the answer has to say which one gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    /// The post-image: the tree as the change leaves it.
    Head,
    /// The base image: the tree the diff is measured against.
    Base,
}

/// The two readers [`decide`] needs, by repo-relative path.
pub(super) struct Images<'reader> {
    /// The post-image, for an added line.
    pub(super) head: &'reader PostImage<'reader>,
    /// The base image, for a removed one.
    pub(super) base: &'reader PostImage<'reader>,
}

/// What the gate knows about the paths this diff covers.
pub(super) struct Changed<'diff> {
    /// The per-file added and removed lines, from the post-image diff reader.
    pub(super) files: &'diff [ChangedFile],
    /// Every path GIT says changed, **deletions included**. `super::diff` builds from the
    /// post-image, so a deleted file has no entry there at all - the hole [`Broken::Unseen`]
    /// closes, and the reason this is a second input rather than derived from the first.
    pub(super) touched: &'diff [String],
}

/// Is the diff the cleanup `claim` says it is?
///
/// FIVE CONDITIONS. Every reason is collected rather than short-circuited, for the reason
/// `super::remedies`' header gives about printed causes: an author told one of five breakages
/// fixes it and comes back to the next one.
///
/// 1. Every path the trailer names is one git says changed ([`Broken::Unnamed`]).
/// 2. Every path git says changed reached the diff reader ([`Broken::Unseen`]), and every one of
///    them is Rust this workspace compiles ([`Broken::NotRust`]).
/// 3. Every changed line except a blank one is test code - an added line in the POST-image, a
///    removed line in the BASE image ([`Broken::Outside`]). Two readers because a removed line
///    exists in one image only, which is the argument `super::reverted` makes as well.
/// 4. Every added `mod <name>;` resolves to a file this diff contains ([`Broken::Unresolved`]).
/// 5. The code-line multiset is equal on both sides ([`Broken::Unbalanced`]).
pub(super) fn decide(claim: Option<&Claim>, changed: &Changed<'_>, images: &Images<'_>) -> Relocation {
    let Some(claim) = claim else {
        return Relocation::Unclaimed;
    };
    let seen: BTreeSet<&str> = changed.files.iter().map(|file| file.path.as_str()).collect();
    let mut broken: Vec<Broken> = Vec::new();
    for path in &claim.paths {
        if !changed.touched.iter().any(|touched| touched == path) {
            broken.push(Broken::Unnamed(path.clone()));
        }
    }
    for path in changed.touched {
        if !seen.contains(path.as_str()) {
            broken.push(Broken::Unseen(path.clone()));
        }
    }
    for file in changed.files {
        if !is_compiled_rust(&file.path) {
            broken.push(Broken::NotRust(file.path.clone()));
            continue;
        }
        let head = regions::scope(&file.path, images.head);
        for line in &file.added {
            if let Some(reason) = outside(&file.path, &line.text, line.number, &head, Side::Head) {
                broken.push(reason);
            }
            // A DECLARATION IS EXEMPT FROM THE MULTISET, SO IT IS CHECKED HERE INSTEAD. An added
            // `mod legacy;` beside an untouched `legacy.rs` compiles a whole module of tests this
            // diff does not contain, and no added line names any of them.
            if let Some(name) = regions::module_name(line.text.trim()) {
                let candidates = place::declared_module_files(&file.path, name, None);
                if !candidates.iter().any(|candidate| seen.contains(candidate.as_str())) {
                    broken.push(Broken::Unresolved {
                        path: file.path.clone(),
                        name: String::from(name),
                    });
                }
            }
        }
        let base = regions::scope(&file.path, images.base);
        for line in &file.removed {
            if let Some(reason) = outside(&file.path, &line.text, line.before, &base, Side::Base) {
                broken.push(reason);
            }
        }
    }
    let balanced = balance(changed.files, images);
    broken.extend(balanced.reasons);
    if broken.is_empty() {
        Relocation::Pure {
            paths: claim.paths.clone(),
            exempt: balanced.exempt,
        }
    } else {
        Relocation::Refused(broken)
    }
}

/// Is this line outside the file's test code, and therefore a reason?
///
/// **ONLY A BLANK LINE IS EXEMPT.** A comment and an attribute are not: a doc comment carries
/// DOCTESTS, and an attribute is how `#[serde(deny_unknown_fields)]` and `#[ignore]` are spelled.
/// `super::regions::has_non_test_additions` exempts both, correctly for the question IT asks and
/// disastrously for this one - which is why this predicate is here and not shared.
fn outside(path: &str, text: &str, number: usize, scope: &TestScope, side: Side) -> Option<Broken> {
    let trimmed = text.trim();
    (!trimmed.is_empty() && !scope.covers(number)).then(|| Broken::Outside {
        path: String::from(path),
        number,
        text: String::from(trimmed),
        side,
    })
}

/// How many times each side of the diff carries one line of code: added first, then removed.
type Tally<'line> = BTreeMap<&'line str, (usize, usize)>;

/// What [`balance`] concluded: the imbalances, and the added lines nothing accounted for.
struct Balanced {
    /// Every key whose two sides do not reconcile, as the reasons they are not a relocation.
    reasons: Vec<Broken>,
    /// The scaffold lines in SURPLUS on the added side, for the verdict to name. A pass has to say
    /// what it waved through, or the exemption is a thing nobody can see.
    exempt: Vec<String>,
}

/// The multiset difference over the whole diff, and the added lines it did not account for.
///
/// THE WHOLE DIFF AND NOT PER FILE, which is the point: a relocation takes lines out of one file
/// and puts them into another, so per-file counts never balance and the only question that means
/// anything is asked across every changed file at once.
fn balance(files: &[ChangedFile], images: &Images<'_>) -> Balanced {
    let mut tally: Tally<'_> = BTreeMap::new();
    for file in files {
        let in_head = (images.head)(&file.path).map(|text| regions::inside_a_literal(&text));
        let in_base = (images.base)(&file.path).map(|text| regions::inside_a_literal(&text));
        for line in &file.added {
            if let Some(key) = counted(&line.text, in_head.as_ref().is_some_and(|at| at.contains(&line.number))) {
                tally.entry(key).or_default().0 += 1;
            }
        }
        for line in &file.removed {
            if let Some(key) = counted(&line.text, in_base.as_ref().is_some_and(|at| at.contains(&line.before))) {
                tally.entry(key).or_default().1 += 1;
            }
        }
    }
    let mut reasons = Vec::new();
    let mut exempt = Vec::new();
    for (text, (added, removed)) in tally {
        // **SCAFFOLD MAY BE IN SURPLUS ON THE ADDED SIDE AND NEVER ON THE REMOVED ONE**, which is
        // the whole of the exemption. A relocation ADDS a `mod` declaration, the `#[cfg(test)]`
        // that gates it and the imports the moved code needs; nothing legitimately REMOVES one, so
        // a re-pointed import - which changes what a byte-identical assertion asserts - is the
        // removed half having nothing to balance against. A scaffold line that MOVED is balanced
        // and needs no exemption at all, which the first version of this got wrong by exempting
        // the added side outright: a `use` carried along inside a moved test body was counted once
        // and exempt once, and so could never balance.
        let broken = if is_scaffold(text) {
            removed > added
        } else {
            added != removed
        };
        if broken {
            reasons.push(Broken::Unbalanced {
                text: String::from(text),
                added,
                removed,
            });
        } else if added > removed {
            exempt.push(String::from(text));
        }
    }
    Balanced { reasons, exempt }
}

/// The code `text` contributes to the multiset, or `None` when it contributes nothing.
///
/// **THE LITERAL TEST COMES FIRST, and the other arm is wrong inside one.** Indentation inside a
/// string IS the value, so the raw line is the key there - and a BLANK line inside a fixture is
/// part of the expected output rather than nothing. Trimming is a convenience everywhere else,
/// because a test leaving `mod tests { .. }` for its own file loses an indentation level.
///
/// **ONLY A BLANK LINE CONTRIBUTES NOTHING.** A comment and an attribute both do:
/// `github.com/telekom/sutura#636` measured a doctest assertion added in a production file passing
/// at exit 0 because `super::regions::carries_no_behaviour` waves both through.
fn counted(text: &str, in_literal: bool) -> Option<&str> {
    if in_literal {
        return Some(text);
    }
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// The shapes a relocation may legitimately add MORE of than it removes.
///
/// **A LIST OF SHAPES AND NOT A CATEGORY**, which is `#636`'s finding in one function: *any
/// attribute* would wave through `#[ignore]` and `#[serde(deny_unknown_fields)]`, and *any
/// comment* would wave through a doctest. `pub use` is deliberately absent - a re-export is API.
/// `mod` is read by `super::regions::module_name`, the one parser in this tree for that shape,
/// which also reads the single-line `#[cfg(test)] pub(crate) mod tests;` spelling.
///
/// **AND AN IMPORT MUST BE `super::`-RELATIVE, measured end to end before this line existed.** Any
/// `use` was exempt, so a split whose new module imported `use crate::harness::*;` instead of
/// `use super::*;` passed at exit 0 - a different name in scope, under a verdict saying the diff
/// changed nothing. `super::` can only reach the module the code came from and its ancestors,
/// which is what a relocation needs and the whole of what it needs; an import naming any other
/// root is counted, and an author who wants one moves it verbatim or writes a second commit.
fn is_scaffold(trimmed: &str) -> bool {
    matches!(trimmed, "#[cfg(test)]" | "#![cfg(test)]")
        || trimmed.strip_prefix("use ").is_some_and(|path| path.starts_with("super::"))
        || regions::module_name(trimmed).is_some()
}

/// The claim held: print what was read, what was exempt, and what it does not prove.
pub(super) fn report_pure(paths: &[String], exempt: &[String], coverage: &Coverage) -> Verdict {
    for line in pure_lines(paths, exempt, coverage) {
        println!("{line}");
    }
    Verdict::Pass
}

/// Every line the arm above prints, in order.
///
/// PURE for the reason `super::remedies` learned twice: a printed sentence is prose that nothing
/// derives, and the one thing that keeps a PASS honest is a test that can read its wording.
fn pure_lines(paths: &[String], exempt: &[String], coverage: &Coverage) -> Vec<String> {
    let mut lines = vec![String::from(
        "xtask test-causality: nothing to measure - a declared test-file cleanup",
    )];
    for path in paths {
        lines.push(format!("  declared:  {TRAILER} {path}"));
    }
    lines.push(format!("  measured:  {}", coverage.measured(Attributed::Nothing)));
    // WHAT WAS WAVED THROUGH, NAMED. The multiset cannot account for the declaration a split adds
    // or the imports the moved code needs, and an exemption nobody can see is the shape this whole
    // module exists to avoid. A re-pointed import is REFUSED - its removed half is counted - so
    // what these lines can still be is an import this diff only ADDED.
    for line in exempt {
        lines.push(format!(
            "  exempt:    {line}  (added scaffold; the multiset does not account for it)"
        ));
    }
    lines.extend(
        [
            "  Every changed path is Rust this workspace compiles, every changed line except a",
            "  blank one is test code in its own image, every added `mod` resolves into this",
            "  diff, and the code-line multiset is equal on both sides - so this diff MOVED test",
            "  code and changed none. Nothing counted was added, removed or re-texted, so there",
            "  is no test here that COULD be red against the base behaviour: red-before-green is",
            "  not a question this diff asks.",
            "  THE LIMIT: that is all this proves. It says nothing about whether those assertions",
            "  are any good, nothing about behaviour, and it did not run the head suite - `just",
            "  test` holds that. It does not see a REORDERING of test code within test code, and",
            "  the claim is RANGE-WIDE: it covers every commit in the measured range, so read the",
            "  exempt lines above against the diff. The trailer narrowed what may pass; it did",
            "  not disable the check, and a trailer over a diff that changed one counted line is",
            "  refused rather than passed.",
        ]
        .into_iter()
        .map(String::from),
    );
    lines
}

/// The claim refused: print every reason, then what the trailer does and does not do.
pub(super) fn report_refused(broken: &[Broken]) -> Verdict {
    for line in refused_lines(broken) {
        eprintln!("{line}");
    }
    Verdict::Fail
}

/// Every line the arm above prints, in order.
fn refused_lines(broken: &[Broken]) -> Vec<String> {
    let mut lines = vec![format!(
        "xtask test-causality: FAILED - the `{TRAILER}` trailer claims a cleanup this diff is not"
    )];
    lines.extend(broken.iter().map(cause));
    lines.extend([
        String::new(),
        String::from("The trailer is a CLAIM and this is the check that holds it, so it narrows what may"),
        String::from("pass and never disables red-before-green. Either make the commit a pure relocation"),
        String::from("- move lines, change none, and put the rest in a second commit - or drop the"),
        String::from("trailer and let the gate measure the change."),
    ]);
    lines
}

/// The one sentence a reason gets.
fn cause(broken: &Broken) -> String {
    match *broken {
        Broken::Unnamed(ref path) => format!("  not in this diff:  {path}  (the trailer names it; nothing changed it)"),
        Broken::Unseen(ref path) => {
            format!("  not read at all:   {path}  (git changed it and the diff reader saw no entry - a DELETED file)")
        }
        Broken::NotRust(ref path) => format!("  not Rust:          {path}  (a cleanup split moves Rust and nothing else)"),
        Broken::Outside {
            ref path,
            number,
            ref text,
            side,
        } => {
            let image = match side {
                Side::Head => "added at head",
                Side::Base => "removed at base",
            };
            format!("  not test code:     {path}:{number} {image}: {text}")
        }
        Broken::Unresolved { ref path, ref name } => {
            format!("  module not moved:  {path} declares `mod {name};` and this diff has no such file")
        }
        Broken::Unbalanced {
            ref text,
            added,
            removed,
        } => {
            format!("  did not balance:   +{added} -{removed}: {text}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Broken, Changed, Claim, Images, Relocation, Side, decide, pure_lines, refused_lines};
    use crate::causality::coverage::Coverage;
    use crate::causality::diff::ChangedFile;
    use crate::causality::fixtures::{changed, changed_removing, tree};
    use crate::causality::regions::PostImage;

    /// The two-file split every test below varies: a test module's assertions move to a new file.
    ///
    /// Both under `crates/x/tests/`, which is what `super::regions::is_dedicated_test_target`
    /// answers off the path alone - so condition 3 is satisfied without a fixture and the tests
    /// below are about the multiset. The two that are ABOUT condition 3 name a path outside it.
    const HELD: &str = "crates/x/tests/held.rs";
    const MOVED: &str = "crates/x/tests/held/moved.rs";

    /// The four assertion lines that move, as they read in the file they leave.
    const ASSERTIONS: [&str; 4] = [
        "    assert_eq!(one(), 1);",
        "    assert_eq!(two(), 2);",
        "    assert!(three());",
        "    assert_ne!(four(), 5);",
    ];

    /// The same four, at the indentation the file they arrive in gives them.
    ///
    /// DEDENTED ON PURPOSE: a test leaving `mod tests { .. }` for its own file loses a level, so a
    /// raw comparison would call every relocation impure. The multiset is over TRIMMED lines -
    /// except inside a string literal, which
    /// `an_indentation_change_inside_a_string_literal_is_refused` is about.
    const RELOCATED: [&str; 4] = [
        "assert_eq!(one(), 1);",
        "assert_eq!(two(), 2);",
        "assert!(three());",
        "assert_ne!(four(), 5);",
    ];

    /// A pure relocation: [`HELD`] loses the four assertions and gains a `mod`, [`MOVED`] gains
    /// them at a different indentation.
    fn pure_move() -> Vec<ChangedFile> {
        vec![
            changed_removing(HELD, 1, &["mod moved;"], 1, &ASSERTIONS),
            changed(MOVED, 1, &RELOCATED),
        ]
    }

    /// The verdict for `files`, with the trailer naming [`HELD`] and no image for any file.
    fn tagged(files: &[ChangedFile]) -> Relocation {
        let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
        verdict(files, &paths, &tree(&[]), &tree(&[]), true)
    }

    /// The same diff with no trailer at all.
    fn untagged(files: &[ChangedFile]) -> Relocation {
        let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
        verdict(files, &paths, &tree(&[]), &tree(&[]), false)
    }

    /// The general form: the diff, what git says it touched, both images, and whether it is tagged.
    fn verdict(
        files: &[ChangedFile],
        touched: &[String],
        head: &impl Fn(&str) -> Option<String>,
        base: &impl Fn(&str) -> Option<String>,
        with_trailer: bool,
    ) -> Relocation {
        let head: &PostImage<'_> = head;
        let base: &PostImage<'_> = base;
        let claim = Claim::of(&format!("chore: split\n\nCleanup-Split: {HELD}\n"));
        decide(
            with_trailer.then_some(()).and(claim.as_ref()),
            &Changed { files, touched },
            &Images { head, base },
        )
    }

    /// The reasons `tagged` gives, or a panic naming what it said instead.
    fn refused(files: &[ChangedFile]) -> Vec<Broken> {
        match tagged(files) {
            Relocation::Refused(broken) => broken,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_diff_that_moves_every_line_and_changes_none_is_the_cleanup_its_trailer_claims() {
        // ATTACK (a), and the whole point of the trailer: the four assertions arrive dedented, the
        // declaration that puts them into the build is exempt AND resolves into this diff, and the
        // multiset balances - so there is no test here that could be red against a behaviour
        // nobody changed. The exempt line is NAMED, because an exemption nobody can see is the
        // shape this module exists to avoid.
        assert_eq!(
            tagged(&pure_move()),
            Relocation::Pure {
                paths: vec![String::from(HELD)],
                exempt: vec![String::from("mod moved;")],
            }
        );
    }

    #[test]
    fn the_trailer_is_necessary_and_the_same_pure_move_without_one_changes_nothing() {
        // THE HALF THAT MAKES THIS NOT A LOOPHOLE. Without the trailer the answer is `Unclaimed` -
        // the gate proceeds exactly as it did - so the declaration is load-bearing and no diff
        // acquires a pass merely by being shaped a certain way.
        assert_eq!(untagged(&pure_move()), Relocation::Unclaimed);
        // A trailer with no path is no claim either, which is the fail-closed direction for a
        // malformed declaration: it leaves the run as it was rather than buying anything.
        assert_eq!(Claim::of("chore: split\n\nCleanup-Split:\n"), None);
        assert_eq!(Claim::of("chore: split with no trailer at all\n"), None);
    }

    #[test]
    fn an_added_assertion_is_refused_although_every_other_line_moved() {
        // ATTACK (b). One line nothing removed, and the trailer buys nothing: this is the shape
        // that would land an unmeasured test, so it is the shape that must not pass.
        let mut with_extra = RELOCATED.to_vec();
        with_extra.push("assert_eq!(five(), 5);");
        let diff = vec![
            changed_removing(HELD, 1, &["mod moved;"], 1, &ASSERTIONS),
            changed(MOVED, 1, &with_extra),
        ];
        assert_eq!(
            refused(&diff),
            vec![Broken::Unbalanced {
                text: String::from("assert_eq!(five(), 5);"),
                added: 1,
                removed: 0,
            }]
        );
    }

    #[test]
    fn a_reworded_assertion_is_refused_although_the_count_is_unchanged() {
        // ATTACK (c), the one a COUNT comparison would miss: four lines out and four lines in, one
        // of them saying something else. The multiset is keyed on the TEXT, so the changed line is
        // reported from both sides - which is what tells the author which line it was.
        let mut reworded = RELOCATED.to_vec();
        reworded[1] = "assert_eq!(two(), 99);";
        let diff = vec![
            changed_removing(HELD, 1, &["mod moved;"], 1, &ASSERTIONS),
            changed(MOVED, 1, &reworded),
        ];
        assert_eq!(
            refused(&diff),
            vec![
                Broken::Unbalanced {
                    text: String::from("assert_eq!(two(), 2);"),
                    added: 0,
                    removed: 1,
                },
                Broken::Unbalanced {
                    text: String::from("assert_eq!(two(), 99);"),
                    added: 1,
                    removed: 0,
                },
            ]
        );
    }

    #[test]
    fn a_deleted_assertion_is_refused_rather_than_passing_in_silence() {
        // ATTACK (e), the nastiest: deleting coverage while claiming a pure move. Three lines
        // arrive where four left, and the missing one is NAMED - a bare count would have said
        // "four out, three in" without saying which, and a SET comparison would have said nothing
        // was wrong whenever a duplicate line covered for the deletion.
        let diff = vec![
            changed_removing(HELD, 1, &["mod moved;"], 1, &ASSERTIONS),
            changed(MOVED, 1, &RELOCATED[..3]),
        ];
        assert_eq!(
            refused(&diff),
            vec![Broken::Unbalanced {
                text: String::from("assert_ne!(four(), 5);"),
                added: 0,
                removed: 1,
            }]
        );
    }

    #[test]
    fn a_production_line_in_the_diff_is_refused_on_the_side_it_changed() {
        // ATTACK (d), refused TWICE over. A production file is not test code in either image, so
        // condition 3 names the line AND the side it changed on; and its added line balances
        // nothing, so condition 5 names it again. The reader has no post-image for
        // `crates/x/src/thing.rs` and the path is not a dedicated test target, so it holds no test
        // region at all - the position a production file is in.
        let production = "crates/x/src/thing.rs";
        let mut diff = pure_move();
        diff.push(changed_removing(production, 1, &["let limit = 9;"], 1, &["let limit = 5;"]));
        let broken = refused(&diff);
        for (text, side) in [("let limit = 9;", Side::Head), ("let limit = 5;", Side::Base)] {
            assert!(
                broken.contains(&Broken::Outside {
                    path: String::from(production),
                    number: 1,
                    text: String::from(text),
                    side,
                }),
                "{text} is named on its own side: {broken:?}"
            );
        }
    }

    #[test]
    fn a_deleted_attribute_is_counted_rather_than_waved_through_as_carrying_no_behaviour() {
        // `github.com/telekom/sutura#636`'s FIRST review finding, and the class of all of them:
        // both checks delegated *does this line do anything* to
        // `super::regions::carries_no_behaviour`, whose own docstring calls itself a heuristic
        // that exempts every attribute. So `#[serde(deny_unknown_fields)]` - the line
        // `sutura/invariants` names as a MECHANISM - could be deleted under this verdict at exit
        // 0. It is a counted line now, on both sides.
        let production = "crates/x/src/query.rs";
        let mut diff = pure_move();
        diff.push(changed_removing(production, 1, &[], 1, &["#[serde(deny_unknown_fields)]"]));
        let broken = refused(&diff);
        assert!(
            broken.contains(&Broken::Unbalanced {
                text: String::from("#[serde(deny_unknown_fields)]"),
                added: 0,
                removed: 1,
            }),
            "{broken:?}"
        );
    }

    #[test]
    fn an_added_ignore_attribute_is_counted_and_no_census_would_have_caught_it() {
        // #636's SECOND finding. `#[ignore]` on a live test silences it, nothing in
        // `cargo xtask --help` censuses ignored tests, and the attribute exemption meant the
        // trailer passed it at exit 0. The moved test's own `#[test]` balances; the added
        // `#[ignore]` does not.
        let mut silenced = RELOCATED.to_vec();
        silenced.insert(0, "#[ignore = \"flaky\"]");
        let diff = vec![
            changed_removing(HELD, 1, &["mod moved;"], 1, &ASSERTIONS),
            changed(MOVED, 1, &silenced),
        ];
        assert!(refused(&diff).contains(&Broken::Unbalanced {
            text: String::from("#[ignore = \"flaky\"]"),
            added: 1,
            removed: 0,
        }));
    }

    #[test]
    fn a_doctest_assertion_added_in_a_production_file_is_refused_twice_over() {
        // #636's THIRD finding and the worst of them: a doc comment carries DOCTESTS, so an added
        // `/// assert_eq!(..)` in a production file is an ADDED TEST - and it passed under a
        // verdict printing *no assertion was added*, which is this gate's entire subject. A
        // comment is counted now (condition 5) and a production line is still not test code
        // (condition 3), so it is refused on both.
        let production = "crates/x/src/secret.rs";
        let mut diff = pure_move();
        diff.push(changed(
            production,
            40,
            &["/// assert_eq!(Secret::new(\"x\").to_string(), \"x\");"],
        ));
        let broken = refused(&diff);
        assert!(
            broken.contains(&Broken::Unbalanced {
                text: String::from("/// assert_eq!(Secret::new(\"x\").to_string(), \"x\");"),
                added: 1,
                removed: 0,
            }),
            "counted: {broken:?}"
        );
        assert!(
            broken.iter().any(|one| matches!(
                *one,
                Broken::Outside {
                    ref path,
                    side: Side::Head,
                    ..
                } if path == production
            )),
            "outside test code: {broken:?}"
        );
    }

    #[test]
    fn a_re_pointed_import_is_refused_although_the_assertion_text_is_identical() {
        // #636's import finding: a `use` re-point changes what a byte-identical assertion ASSERTS,
        // so base green -> head red with the multiset none the wiser. The exemption is ADDED-SIDE
        // ONLY for exactly this reason - the added `use` is exempt and its REMOVED counterpart is
        // counted, so a re-point has nothing to balance against and is refused. A relocation that
        // only ADDS imports still passes, which is the case a new module actually needs.
        let mut diff = pure_move();
        diff.push(changed_removing(
            MOVED,
            20,
            &["use crate::other::Thing;"],
            20,
            &["use super::Thing;"],
        ));
        assert!(
            refused(&diff).contains(&Broken::Unbalanced {
                text: String::from("use super::Thing;"),
                added: 0,
                removed: 1,
            }),
            "the removed half of a re-point is counted"
        );
        // The control one line away: a `super::`-relative import this diff only ADDS is scaffold,
        // and passes - that is the case a new module actually needs.
        let mut only_added = pure_move();
        only_added.push(changed(MOVED, 20, &["use super::Thing;"]));
        assert!(matches!(tagged(&only_added), Relocation::Pure { .. }));

        // AND AN ADDED IMPORT NAMING ANOTHER ROOT IS NOT SCAFFOLD, which the first version of this
        // check got wrong and a run measured: a split whose new module wrote
        // `use crate::harness::*;` rather than `use super::*;` passed at exit 0 with a different
        // name in scope. `super::` reaches the module the code came from; anything else is a
        // reach, so it is counted.
        let mut elsewhere = pure_move();
        elsewhere.push(changed(MOVED, 20, &["use crate::other::*;"]));
        assert!(
            refused(&elsewhere).contains(&Broken::Unbalanced {
                text: String::from("use crate::other::*;"),
                added: 1,
                removed: 0,
            }),
            "an import naming another root is counted"
        );
    }

    #[test]
    fn an_indentation_change_inside_a_string_literal_is_refused() {
        // #636's whitespace finding. Trimming is right for code - a test leaving `mod tests { .. }`
        // loses a level - and WRONG inside a string literal, where the indentation is the VALUE.
        // An expected-output fixture is exactly what a test asserts on, so this is base green ->
        // head red with the multiset seeing two equal trimmed lines. The lexer answers which lines
        // begin inside a literal and those are keyed on their RAW text.
        let base_image = concat!(
            "#[test]\n",
            "fn fixture() {\n",
            "    let expected = \"\\\n",
            "    one\n",
            "    two\";\n",
            "    assert_eq!(render(), expected);\n",
            "}\n",
        );
        let reindented = concat!(
            "#![cfg(test)]\n",
            "\n",
            "use super::*;\n",
            "\n",
            "#[test]\n",
            "fn fixture() {\n",
            "    let expected = \"\\\n",
            "        one\n",
            "    two\";\n",
            "    assert_eq!(render(), expected);\n",
            "}\n",
        );
        let moved_lines: Vec<&str> = reindented.lines().collect();
        let left: Vec<&str> = base_image.lines().collect();
        let files = vec![
            changed_removing(HELD, 1, &["mod moved;"], 1, &left),
            changed(MOVED, 1, &moved_lines),
        ];
        let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
        let head = tree(&[(MOVED, reindented), (HELD, "mod moved;\n")]);
        let base = tree(&[(HELD, base_image)]);
        let Relocation::Refused(broken) = verdict(&files, &paths, &head, &base, true) else {
            panic!("a re-indented fixture is not a pure relocation");
        };
        assert!(
            broken.contains(&Broken::Unbalanced {
                text: String::from("        one"),
                added: 1,
                removed: 0,
            }),
            "the raw added line is keyed untrimmed: {broken:?}"
        );
        assert!(
            broken.contains(&Broken::Unbalanced {
                text: String::from("    one"),
                added: 0,
                removed: 1,
            }),
            "and so is the raw removed one: {broken:?}"
        );

        // THE CONTROL ONE LINE AWAY: the same move with the fixture's indentation preserved is
        // pure, so this is not a check that refuses every literal.
        let kept = reindented.replace("        one", "    one");
        let kept_lines: Vec<&str> = kept.lines().collect();
        let same = vec![
            changed_removing(HELD, 1, &["mod moved;"], 1, &left),
            changed(MOVED, 1, &kept_lines),
        ];
        let head = tree(&[(MOVED, kept.as_str()), (HELD, "mod moved;\n")]);
        assert!(matches!(verdict(&same, &paths, &head, &base, true), Relocation::Pure { .. }));
    }

    #[test]
    fn a_path_git_changed_that_the_diff_reader_never_saw_is_refused() {
        // #636's DELETED-FILE finding, and it is pre-existing blind spot this arm newly asserts
        // over: `super::diff` builds from the POST-image, so a file this branch `git rm`'d has no
        // entry at all - its `+++` is `/dev/null`. A 785-line test file could be deleted under
        // *this diff MOVED test code and changed none*, because not one of its lines was ever
        // read. Git's own `--name-only` list is the second input that closes it.
        let deleted = "crates/x/tests/gone.rs";
        let files = pure_move();
        let mut touched: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
        touched.push(String::from(deleted));
        let Relocation::Refused(broken) = verdict(&files, &touched, &tree(&[]), &tree(&[]), true) else {
            panic!("a deleted file is not a relocation");
        };
        assert!(broken.contains(&Broken::Unseen(String::from(deleted))), "{broken:?}");
    }

    #[test]
    fn an_added_module_declaration_whose_file_is_not_in_the_diff_is_refused() {
        // A `mod` declaration is exempt from the multiset, so it needs its own check - and this
        // arm answers BEFORE the plan, so `super::scoped`'s refusal for the same shape is bypassed
        // rather than inherited. `mod legacy;` beside an untouched `legacy.rs` compiles a whole
        // module of pre-existing tests, and no added line names any of them.
        let mut diff = pure_move();
        diff[0] = changed_removing(HELD, 1, &["mod moved;", "mod legacy;"], 1, &ASSERTIONS);
        assert!(refused(&diff).contains(&Broken::Unresolved {
            path: String::from(HELD),
            name: String::from("legacy"),
        }));
    }

    #[test]
    fn a_path_the_trailer_names_and_the_diff_does_not_touch_is_refused() {
        // So a trailer cannot be copy-pasted forward onto an unrelated commit and go unnoticed:
        // the path it names has to be a path this diff changed.
        let diff = vec![changed_removing(MOVED, 1, &RELOCATED, 1, &ASSERTIONS)];
        assert!(refused(&diff).contains(&Broken::Unnamed(String::from(HELD))));
    }

    #[test]
    fn anything_in_the_diff_that_is_not_compiled_rust_is_refused() {
        // A manifest, a page and a workflow, because each is an implementation this gate reverts
        // or a build input it may not - and none is test code a cleanup moves. `super::features`
        // already records what a manifest ALONE can put into the build.
        for path in ["crates/x/Cargo.toml", "docs/architecture.md", ".github/workflows/ci.yml"] {
            let mut diff = pure_move();
            diff.push(changed(path, 1, &["something"]));
            assert!(
                refused(&diff).contains(&Broken::NotRust(String::from(path))),
                "{path} is not test code"
            );
        }
    }

    #[test]
    fn the_pass_states_what_it_did_not_measure_and_the_refusal_states_what_the_trailer_is_not() {
        // THE WORDING, read rather than reviewed, for the reason `super::remedies` gives: a PASS
        // whose limit is prose is a PASS whose limit rots. Four claims have to be in the pass - a
        // zero numerator, the scaffold it waved through, that the claim is range-wide, and that
        // the trailer did not disable the check. The sentence this asserts on used to say *every
        // added and removed line is test code*, which was FALSE while attributes were exempt: an
        // overstated claim about a gate that verifies rather than assumes is the same defect the
        // gate exists to catch.
        let empty = tree(&[]);
        let read: &PostImage<'_> = &empty;
        let lines = pure_lines(
            &[String::from(HELD)],
            &[String::from("use super::*;")],
            &Coverage::of(&[], &[], read),
        );
        // WHITESPACE-NORMALISED, and that is not tidiness: the first version of this assertion
        // searched the joined lines for a sentence that WRAPS, and a line-oriented search cannot
        // find one - it read as absent while present, which is the same blind spot as the
        // `#[cfg(test)]` line above a `mod`. Every claim below is a sentence, so the text is
        // compared as a sentence.
        let flowed = lines.join(" ");
        let whole = flowed.split_whitespace().collect::<Vec<&str>>().join(" ");
        assert!(lines[0].contains("nothing to measure"), "{:?}", lines[0]);
        for claim in [
            "declared: Cleanup-Split: crates/x/tests/held.rs",
            "0 of 0 added tests measured",
            "exempt: use super::*;",
            "every changed line except a blank one is test code in its own image",
            "every added `mod` resolves into this diff",
            "Nothing counted was added, removed or re-texted",
            "the claim is RANGE-WIDE",
            "it did not disable the check",
            "`just test` holds that",
        ] {
            assert!(whole.contains(claim), "the pass has to say {claim:?}: {whole}");
        }

        // And the refusal has to say the trailer is a claim rather than a permission, or an author
        // reads it as the gate malfunctioning.
        let flowed = refused_lines(&[Broken::NotRust(String::from("crates/x/Cargo.toml"))]).join(" ");
        let refused = flowed.split_whitespace().collect::<Vec<&str>>().join(" ");
        assert!(refused.contains("FAILED"), "{refused}");
        assert!(
            refused.contains("narrows what may pass and never disables red-before-green"),
            "{refused}"
        );
    }
}

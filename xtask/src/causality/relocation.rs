//! A commit may DECLARE itself a test-file cleanup, and this module is the check that holds the
//! claim rather than the permission that replaces it.
//!
//! WHY A DECLARATION EXISTS AT ALL. The 1000-line cap makes splitting a near-cap test file
//! necessary, and this gate charges for it: move assertions into a new module and the file they
//! left is revertible - nothing it ADDED is a test - so the base tree loses the declaration, Rust
//! compiles no unreferenced file, the filterset names nothing and the verdict is
//! `super::base::BaseOutcome::NotRun`, *the tests this diff added did not run on base*, at exit 1.
//! The remedy this gate prints - move the harness and leave every assertion where it is - works
//! once, and then the file has no lever left.
//!
//! WHAT THE TRAILER IS AND IS NOT. `Cleanup-Split: <path>` in a commit message says *this commit
//! moves test code and changes none*. That is a human judgement about intent, and it is the only
//! part a human can supply. It is **not** the permission: a trailer nothing checks is the gate
//! that has quietly stopped gating, because anyone can bypass red-before-green by typing a word.
//! So the trailer NARROWS what may pass and never disables the check - [`decide`] holds the claim,
//! and a trailer over a diff that added, removed or re-texted one line of test code is a
//! REFUSAL rather than a pass. Both directions are measured in this module's own tests.
//!
//! A TRAILER AND NOT A FLAG, AN ENV VAR OR AN ALLOWLIST FILE, and each alternative fails in its
//! own way. The trailer rides on the commit, so *one-time* is a property of the object rather than
//! of an entry someone has to remember to delete - which is the objection to a
//! `max-lines-ignore`-shaped exemption. It is permanently visible in review and in `git log`,
//! which a flag typed once into a shell is not. And it cannot be supplied by the venue that runs
//! the gate, so CI and a hand run read the same claim off the same object.
//!
//! **WHAT A PURE-RELOCATION VERDICT DOES NOT PROVE, stated here because an overstated control is
//! itself the defect.** It proves the diff MOVED test lines. It does not prove the assertions are
//! any good, that the coverage they carry is adequate, or anything at all about behaviour - there
//! is no behaviour in the diff to be about. It does not prove HEAD is green either: this arm
//! returns before the head run, like every other arm that answers before the proof block, and
//! *the suite is green* is `just test` and the nix `nextest` check's property rather than this
//! gate's. And it does not see a REORDERING: the multiset check reads counts, so two assertions
//! that swapped places between two test functions balance. What bounds that is [`Broken::Outside`]:
//! every changed line has to be test code in its own image, so a reordering of PRODUCTION
//! statements cannot reach this arm, and what remains in reach is test code shuffled between test
//! functions, which adds no untested change.

use std::collections::{BTreeMap, BTreeSet};

use crate::Verdict;
use crate::causality::coverage::{Attributed, Coverage};
use crate::causality::diff::ChangedFile;
use crate::causality::regions::{self, AddedLine, PostImage};

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
    /// The RANGE's messages and not HEAD's alone: a branch is reviewed whole, and the purity check
    /// below reads the whole diff - so a trailer on any commit in it is checked against everything
    /// the range did, and a sibling commit that changed behaviour breaks the claim rather than
    /// riding on it.
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
    /// Claimed, and every check holds. Carries the paths, so the verdict names what it read.
    Pure(Vec<String>),
    /// Claimed, and refused. EVERY reason, so one run names them all rather than sending an author
    /// round the loop once per line.
    Refused(Vec<Broken>),
}

/// One reason a claimed cleanup is not one.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Broken {
    /// The trailer names a path this diff does not change.
    Unnamed(String),
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
    /// A line one side has and the other does not: an assertion added, removed or re-texted.
    Unbalanced {
        /// The line, trimmed.
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

/// Is the diff the cleanup `claim` says it is?
///
/// FOUR CONDITIONS, and the third and fourth are the ones that make the trailer safe.
///
/// 1. Every path the trailer names is in the diff ([`Broken::Unnamed`]).
/// 2. Every changed path is Rust this workspace compiles ([`Broken::NotRust`]). A manifest, a page
///    or a workflow in the diff is a second change, and `super::features` already records what a
///    manifest alone can put into the build.
/// 3. Every added line is test code in the POST-image, and every removed line is test code in the
///    BASE image ([`Broken::Outside`]). This is what confines the whole diff to test code, and it
///    needs both readers because a removed line exists in one image only - the same argument
///    `super::reverted` makes for reading the base tree.
/// 4. The trimmed code-line multiset is the same on both sides ([`Broken::Unbalanced`]).
///
/// TRIMMED, because a relocation changes indentation by construction - a test moving out of
/// `mod tests { .. }` into its own file loses a level - and indentation carries no assertion. What
/// trimming cannot hide is an assertion's TEXT, which is the thing this condition is about.
///
/// EVERY REASON IS COLLECTED rather than short-circuited, for the reason `super::remedies`' header
/// gives about printed causes: an author who is told one of four breakages fixes it and comes back
/// to the next one.
pub(super) fn decide(claim: Option<&Claim>, files: &[ChangedFile], images: &Images<'_>) -> Relocation {
    let Some(claim) = claim else {
        return Relocation::Unclaimed;
    };
    let mut broken: Vec<Broken> = claim
        .paths
        .iter()
        .filter(|path| !files.iter().any(|file| &&file.path == path))
        .map(|path| Broken::Unnamed(path.clone()))
        .collect();
    for file in files {
        if !crate::causality::is_compiled_rust(&file.path) {
            broken.push(Broken::NotRust(file.path.clone()));
            continue;
        }
        let head = regions::scope(&file.path, images.head);
        broken.extend(
            regions::outside_test_code(&file.added, &head)
                .into_iter()
                .map(|line| not_test_code(&file.path, line, Side::Head)),
        );
        // THE REMOVED SIDE IS ITS OWN IMAGE AND ITS OWN NUMBERS. A removed line carries the line
        // it occupied in the tree the diff is measured against, which is the only image it exists
        // in - so it is re-presented as an `AddedLine` against that image and asked the same
        // question by the same predicate. One predicate, two readers.
        let restated: Vec<AddedLine> = file
            .removed
            .iter()
            .map(|line| AddedLine::new(line.before, line.text.clone()))
            .collect();
        let base = regions::scope(&file.path, images.base);
        broken.extend(
            regions::outside_test_code(&restated, &base)
                .into_iter()
                .map(|line| not_test_code(&file.path, line, Side::Base)),
        );
    }
    broken.extend(unbalanced(files));
    if broken.is_empty() {
        Relocation::Pure(claim.paths.clone())
    } else {
        Relocation::Refused(broken)
    }
}

/// One line that is not test code, as the reason it breaks the claim.
fn not_test_code(path: &str, line: &AddedLine, side: Side) -> Broken {
    Broken::Outside {
        path: String::from(path),
        number: line.number,
        text: String::from(line.text.trim()),
        side,
    }
}

/// How many times each side of the diff carries one line of code: added first, then removed.
type Tally<'line> = BTreeMap<&'line str, (usize, usize)>;

/// The lines one side has and the other does not, as a multiset difference over the whole diff.
///
/// THE WHOLE DIFF AND NOT PER FILE, which is the point: a relocation takes lines out of one file
/// and puts them into another, so per-file counts never balance and the only question that means
/// anything is asked across every changed file at once.
fn unbalanced(files: &[ChangedFile]) -> Vec<Broken> {
    let mut tally: Tally<'_> = BTreeMap::new();
    for file in files {
        for line in &file.added {
            if let Some(text) = counted(&line.text) {
                tally.entry(text).or_default().0 += 1;
            }
        }
        for line in &file.removed {
            if let Some(text) = counted(&line.text) {
                tally.entry(text).or_default().1 += 1;
            }
        }
    }
    tally
        .into_iter()
        .filter(|&(_, (added, removed))| added != removed)
        .map(|(text, (added, removed))| Broken::Unbalanced {
            text: String::from(text),
            added,
            removed,
        })
        .collect()
}

/// The code this line carries that the multiset has to account for, trimmed.
///
/// Blank lines, comments and attributes carry no behaviour - `super::regions` owns that predicate
/// and this is the same one `has_non_test_additions` uses, so there is one answer to *does this
/// line do anything* rather than two.
///
/// AND `use`/`mod` DECLARATIONS ARE EXEMPT, which is the one loosening here and it is what makes
/// the check usable at all: a split ADDS a `mod` declaration by construction and re-points the
/// imports the moved code needs, so those lines legitimately do not balance. The cost is bounded
/// by condition 3 of [`decide`]: an exempted line still has to be test code in its own image, so
/// what a changed `use` can reach is what a test imports and never what the program does.
fn counted(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    (!regions::carries_no_behaviour(line) && !is_declaration(trimmed)).then_some(trimmed)
}

/// A `use` or `mod` declaration, whatever visibility it carries.
fn is_declaration(trimmed: &str) -> bool {
    let rest = ["pub(crate) ", "pub(super) ", "pub "]
        .iter()
        .find_map(|vis| trimmed.strip_prefix(*vis))
        .unwrap_or(trimmed);
    rest.starts_with("use ") || rest.starts_with("mod ")
}

/// The claim held: print what was read and what it does not prove.
pub(super) fn report_pure(paths: &[String], coverage: &Coverage) -> Verdict {
    for line in pure_lines(paths, coverage) {
        println!("{line}");
    }
    Verdict::Pass
}

/// Every line the arm above prints, in order.
///
/// PURE for the reason `super::remedies` learned twice: a printed sentence is prose that nothing
/// derives, and the one thing that keeps a PASS honest is a test that can read its wording.
fn pure_lines(paths: &[String], coverage: &Coverage) -> Vec<String> {
    let mut lines = vec![String::from(
        "xtask test-causality: nothing to measure - a declared test-file cleanup",
    )];
    for path in paths {
        lines.push(format!("  declared:  {TRAILER} {path}"));
    }
    // THE RATIO BESIDE THE PASS, the same accounting `super::remedies` prints on every arm that
    // answers before either run: the filterset would have NAMED these, and none of them ran.
    lines.push(format!("  measured:  {}", coverage.measured(Attributed::Nothing)));
    lines.extend(
        [
            "  Every changed path is Rust this workspace compiles, every added and removed",
            "  line is test code in its own image, and the trimmed code-line multiset is the",
            "  same on both sides - so this diff MOVED test code and changed none.",
            "  No assertion was added, removed or re-texted, so there is no test here that",
            "  COULD be red against the base behaviour: red-before-green is not a question",
            "  this diff asks. THE LIMIT: that is all this proves. It says nothing about",
            "  whether those assertions are any good, nothing about behaviour, and it did not",
            "  run the head suite - `just test` holds that. The trailer narrowed what may",
            "  pass; it did not disable the check, and a trailer over a diff that changed one",
            "  line of test code is refused rather than passed.",
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
    use super::{Broken, Claim, Images, Relocation, Side, decide, pure_lines, refused_lines};
    use crate::causality::coverage::Coverage;
    use crate::causality::diff::ChangedFile;
    use crate::causality::fixtures::{changed, changed_removing, tree};
    use crate::causality::regions::PostImage;

    /// The two-file split every test below varies: a test module's assertions move to a new file.
    ///
    /// Both under `crates/x/tests/`, which is what `super::regions::is_dedicated_test_target`
    /// answers off the path alone - so the region question is settled without a second fixture and
    /// the tests below are about the multiset. `a_production_line_in_the_diff_is_refused_on_the_side_it_changed`
    /// is the one that needs a path outside it, and it is the one testing that condition.
    const HELD: &str = "crates/x/tests/held.rs";
    const MOVED: &str = "crates/x/tests/moved.rs";

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
    /// raw line comparison would call every relocation impure. The multiset is over TRIMMED lines.
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

    /// The verdict for `files` with the trailer naming [`HELD`].
    fn tagged(files: &[ChangedFile]) -> Relocation {
        let empty = tree(&[]);
        let read: &PostImage<'_> = &empty;
        let claim = Claim::of(&format!("chore: split\n\nCleanup-Split: {HELD}\n")).expect("a trailer is a claim");
        decide(Some(&claim), files, &Images { head: read, base: read })
    }

    /// The same diff with no trailer at all.
    fn untagged(files: &[ChangedFile]) -> Relocation {
        let empty = tree(&[]);
        let read: &PostImage<'_> = &empty;
        decide(None, files, &Images { head: read, base: read })
    }

    #[test]
    fn a_diff_that_moves_every_line_and_changes_none_is_the_cleanup_its_trailer_claims() {
        // ATTACK (a), and the whole point of the trailer: the four assertions arrive dedented, the
        // declaration that puts them into the build is exempt, and the multiset balances - so
        // there is no test here that could be red against a behaviour nobody changed.
        assert_eq!(tagged(&pure_move()), Relocation::Pure(vec![String::from(HELD)]));
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
            tagged(&diff),
            Relocation::Refused(vec![Broken::Unbalanced {
                text: String::from("assert_eq!(five(), 5);"),
                added: 1,
                removed: 0,
            }])
        );
    }

    #[test]
    fn a_reworded_assertion_is_refused_although_the_count_is_unchanged() {
        // ATTACK (c), and it is the one a COUNT comparison would miss: four lines out and four
        // lines in, one of them saying something else. The multiset is keyed on the TEXT, so the
        // changed line is short on one side and long on the other and BOTH are reported - which is
        // what tells the author which line it was.
        let mut reworded = RELOCATED.to_vec();
        reworded[1] = "assert_eq!(two(), 99);";
        let diff = vec![
            changed_removing(HELD, 1, &["mod moved;"], 1, &ASSERTIONS),
            changed(MOVED, 1, &reworded),
        ];
        assert_eq!(
            tagged(&diff),
            Relocation::Refused(vec![
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
            ])
        );
    }

    #[test]
    fn a_deleted_assertion_is_refused_rather_than_passing_in_silence() {
        // ATTACK (e), and the nastiest one: deleting coverage while claiming a pure move. Three
        // lines arrive where four left, and the missing one is NAMED - a bare count would have said
        // "four out, three in" without saying which, and a SET comparison would have said nothing
        // was wrong at all whenever a duplicate line covered for the deletion.
        let diff = vec![
            changed_removing(HELD, 1, &["mod moved;"], 1, &ASSERTIONS),
            changed(MOVED, 1, &RELOCATED[..3]),
        ];
        assert_eq!(
            tagged(&diff),
            Relocation::Refused(vec![Broken::Unbalanced {
                text: String::from("assert_ne!(four(), 5);"),
                added: 0,
                removed: 1,
            }])
        );
    }

    #[test]
    fn a_production_line_in_the_diff_is_refused_on_the_side_it_changed() {
        // ATTACK (d), refused TWICE over, which is the belt this arm needs. A production file is
        // not test code in either image, so condition 3 names the line AND the side it changed on;
        // and its added line balances nothing, so condition 4 names it again. The reader has no
        // post-image for `crates/x/src/thing.rs` and the path is not a dedicated test target, so it
        // holds no test region at all - which is the position a production file is in.
        let production = "crates/x/src/thing.rs";
        let mut diff = pure_move();
        diff.push(changed_removing(production, 1, &["let limit = 9;"], 1, &["let limit = 5;"]));
        let Relocation::Refused(broken) = tagged(&diff) else {
            panic!("a production edit is not a cleanup");
        };
        assert!(
            broken.contains(&Broken::Outside {
                path: String::from(production),
                number: 1,
                text: String::from("let limit = 9;"),
                side: Side::Head,
            }),
            "the added production line is named at head: {broken:?}"
        );
        assert!(
            broken.contains(&Broken::Outside {
                path: String::from(production),
                number: 1,
                text: String::from("let limit = 5;"),
                side: Side::Base,
            }),
            "the removed production line is named at base: {broken:?}"
        );
    }

    #[test]
    fn a_path_the_trailer_names_and_the_diff_does_not_touch_is_refused() {
        // So a trailer cannot be copy-pasted forward onto an unrelated commit and go unnoticed:
        // the path it names has to be a path this diff changed.
        let diff = vec![changed_removing(MOVED, 1, &RELOCATED, 1, &ASSERTIONS)];
        let Relocation::Refused(broken) = tagged(&diff) else {
            panic!("a trailer naming nothing in the diff is not a cleanup");
        };
        assert!(broken.contains(&Broken::Unnamed(String::from(HELD))), "{broken:?}");
    }

    #[test]
    fn anything_in_the_diff_that_is_not_compiled_rust_is_refused() {
        // A manifest, a page and a workflow, because each is an implementation this gate reverts or
        // a build input it may not - and none of them is test code a cleanup moves.
        // `super::features` already records what a manifest ALONE can put into the build.
        for path in ["crates/x/Cargo.toml", "docs/architecture.md", ".github/workflows/ci.yml"] {
            let mut diff = pure_move();
            diff.push(changed(path, 1, &["something"]));
            let Relocation::Refused(broken) = tagged(&diff) else {
                panic!("{path} is not test code");
            };
            assert!(broken.contains(&Broken::NotRust(String::from(path))), "{path}: {broken:?}");
        }
    }

    #[test]
    fn the_pass_states_what_it_did_not_measure_and_the_refusal_states_what_the_trailer_is_not() {
        // THE WORDING, read rather than reviewed, for the reason `super::remedies` gives: a PASS
        // whose limit is prose is a PASS whose limit rots. Two claims have to be in the pass - a
        // zero numerator, and that the trailer did not disable the check.
        let empty = tree(&[]);
        let read: &PostImage<'_> = &empty;
        let lines = pure_lines(&[String::from(HELD)], &Coverage::of(&[], &[], read));
        let whole = lines.join("\n");
        assert!(lines[0].contains("nothing to measure"), "{:?}", lines[0]);
        assert!(whole.contains(&format!("Cleanup-Split: {HELD}")), "{whole}");
        assert!(whole.contains("0 of 0 added tests measured"), "{whole}");
        assert!(whole.contains("did not disable the check"), "{whole}");
        assert!(whole.contains("`just test` holds that"), "{whole}");

        // And the refusal has to say the trailer is a claim rather than a permission, or an author
        // reads it as the gate malfunctioning.
        let refused = refused_lines(&[Broken::NotRust(String::from("crates/x/Cargo.toml"))]).join("\n");
        assert!(refused.contains("FAILED"), "{refused}");
        assert!(refused.contains("never disables red-before-green"), "{refused}");
    }
}

//! Whether the files this gate REVERTED could have reddened the tests it measured.
//!
//! **A GREEN BASE RUN IS A FACT ABOUT THE PARTITION UNTIL SOMETHING SAYS OTHERWISE.** The gate's
//! premise is *these tests pass with the implementation reverted, so they do not test it* - and
//! that reads as a statement about the tests while it is, first, a statement about which files the
//! gate chose to put back. Measured on `github.com/telekom/sutura#397`: three reverted files, one
//! scoped test exercising a key-set parse in a fourth, `FAILED - green against base behaviour`.
//! Nothing in that revert could have made that test red, so the run collected no evidence either
//! way and the verdict named a defect it had not observed.
//!
//! **IT IS `super::provenance::Reach::BuildInput`'S ARGUMENT ONE STEP OVER.** A manifest is held at
//! HEAD and NAMED because reverting it changes what cargo resolves rather than what the tests
//! measure - *this file cannot be the thing under test, so reverting it proves nothing*. The same
//! sentence is true of a file that IS reverted and that no test in scope can observe. The
//! difference is only that the first is decided by the path alone and this one needs the tests in
//! scope as well.
//!
//! **WHAT IT MAY NOT DO, and it is the whole reason the predicate is shaped the way it is.** A test
//! that pins EXISTING behaviour, added beside an implementation change that can reach it, is the
//! case this gate exists to catch, and it has to keep failing. So the excuse is not *the tests look
//! unrelated* - it is *reverting these files restores no line any test in scope could execute*, and
//! a single reverted file carrying a real implementation change ends the question at
//! [`Reverted::Behaviour`] whatever else the diff holds.
//!
//! TWO excuses and nothing else, each with the argument that makes it sound, and the row that is
//! not one:
//!
//! | The reverted file | Why reverting it cannot redden a test in scope |
//! | --- | --- |
//! | changed only blank lines and `//` comments | base and head are the same program |
//! | changed only lines inside its own `#[cfg(test)]` regions, in a package holding none of the tests in scope | `#[cfg(test)]` code is compiled into its own package's test build and into nothing else, so no dependent sees it |
//! | anything else | no excuse: [`Reverted::Behaviour`] |
//!
//! **THE SECOND ROW IS WHY A PACKAGE IS READ AT ALL, and the same-package case is the one that
//! makes it non-obvious.** `#[cfg(test)]` does not cross a crate boundary, so another package's
//! test code is invisible here; the SAME package's is not - its test build is the one the scoped
//! test compiles into, and a helper it edits is a path this module cannot follow. That case stays
//! in reach. On #397 the same-package file was excused by the first row instead: its whole diff was
//! six comment lines.
//!
//! **WHAT IT DOES NOT REACH**, stated beside the claim because an excuse that overstates itself is
//! worse than none:
//!
//! * **A revertible file cargo does not compile** - a page, a `justfile` recipe, a nix file - is
//!   never excused. Any test may read any of them at runtime and nothing here reads which, so the
//!   class whose whole point is that reverting it reddens a documentation-driven suite is left
//!   alone. A branch whose only implementation change is such a file still gets the verdict it
//!   gets today.
//! * **A comment inside a string literal.** *Blank or a plain `//`* is a lexical test on one line,
//!   so a line of a raw string that begins `//` reads as a comment. It fails towards the excuse,
//!   and what it costs is a failure that becomes an exit 3 for a diff whose only implementation
//!   change is text that looks like commentary. A block comment fails the other way - a `*`
//!   continuation line is not excused - which is why no `/*` form is matched.
//! * **A `.rs` FILE IS ALSO DATA, and *the same program* is a claim about the compiled program
//!   only.** This workspace's own gates read repository source as TEXT, and their `#[test]`s do
//!   too - `check-max-lines` counts a file's lines, `check-guidance` reads its spans - so a
//!   comment-only change is not a no-op for one of those. No count is written here: the argument
//!   holds at any number above zero. **Doc comments are NOT in this class** and were, until review:
//!   `///` and `//!` reach a tool schema's `description` through `schemars`, so they are program
//!   text and [`inert`] refuses them.
//! * **`super::regions` FINDS `#[cfg(test)]` BY TEXT, and this module promoted that scanner from
//!   deciding separability to deciding whether a FAILED is suppressed.** Its `item_end` counts
//!   braces without knowing it began inside a string, so a fixture string holding an unbalanced
//!   `#[cfg(test)] mod demo {` would make the rest of that file read as one region - towards the
//!   EXCUSE. **What keeps it harmless today is an ORDERING ACCIDENT, not a mechanism, and the
//!   ingredient is already here.** Measured over `crates/`, `xtask/` and `dev/` on 2026-09-06:
//!   FOUR `#[cfg(test)]` line-starts sit inside string literals - all four in `xtask` fixtures of
//!   Rust source, which is the crate that writes such fixtures - and ZERO of them are reached
//!   first, because `cfg_test_regions` resumes after each region it opens and every one of the
//!   four already sits inside its own file's real test module. Move such a fixture ABOVE the
//!   file's `#[cfg(test)] mod tests` and it fires. It is stated rather than closed because the
//!   fix is `regions`' own lexer and this module cannot answer it locally.
//!   **Both halves of that were measured against a first attempt that was wrong in each
//!   direction**: a scan claiming eighteen, whose lexer had no case for the char literal `'"'`
//!   and swallowed whole files, and a report that every occurrence was outside a string.
//! * **Cross-package reachability itself.** Nothing here asks whether the scoped tests' package
//!   even depends on the reverted one, which is the `cargo metadata` question
//!   `github.com/telekom/sutura#358` left open. A production change in an unrelated package is
//!   still `Behaviour`, so this closes none of that - it is strictly the *no line any test could
//!   execute* half.
//! * **A removed line's position.** `super::diff::RemovedLine` anchors a removal beside the gap it
//!   left, on a side that depends on the hunk shape, so [`neighbourhood`] asks about all three
//!   lines. The first version asked about one and claimed the error direction was `Behaviour`;
//!   review falsified both, end to end, and that paragraph now sits on the type.
//!
//! **AND IT HAS A FALSIFIER RATHER THAN ONLY AN ARGUMENT.** *Out of reach* and *a red base run*
//! are contradictory answers about one run: this module says the revert restores nothing a test can
//! execute, and the run says something failed once it was restored. `super::base` prints that
//! pairing on BOTH red arms - `RedByAssertion` and `RedOutsideTheDiff`, which is equally
//! contradictory and printed nothing until review said so - rather than resolving it, because the
//! two answers come from different places, one from the diff and one from nextest, and a rule with
//! a hole in it is what the pairing would be evidence of.
//!
//! **WHAT THE FALSIFIER DOES NOT SEE, because it is scoped exactly as the runs are.** It fires on a
//! failure the run REPORTED, and the base run is filtered to the tests this diff added. A revert
//! that reddens a PRE-EXISTING test - one of this repository's own gate cells that `include_str!`s
//! its source, say - is outside that filter, so the contradiction is never printed and the excuse
//! stands unchallenged. Widening it would mean an unfiltered base run, which is the wide-run defect
//! `super::base`'s header records; the honest statement is that this catches the contradiction it
//! can see.
//!
//! **AND THE BOUND THIS ARM MAY CLAIM, restated because the first wording was too generous.** It is
//! reached only under a base run that SUCCEEDED, so it cannot turn a failure into
//! `ok - red on base, green on head` - the verdict that claims a proof. It is NOT "never a failure
//! turned into a pass": exit 3 is a green step at both venues, carrying a warning that
//! `.agents/skills/sutura/gates/SKILL.md` records as never reaching the pull-request conversation.
//! A verdict this arm gets wrong is therefore a check somebody may read as clean.

use std::collections::BTreeSet;

use crate::Verdict;

use super::diff::ChangedFile;
use super::names::CargoName;
use super::place;
use super::provenance::Reach;
use super::regions::{PostImage, TestScope, scope};

/// What the revert restores, AS FAR AS THE TESTS IN SCOPE CAN SEE - which is what a green base run
/// is then evidence of.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reverted {
    /// At least one reverted file restores a line a test in scope could execute, so a green base
    /// run is exactly the defect this gate exists to name.
    Behaviour,
    /// Not one of them does, each with the reason. A green base run then measured the partition.
    Nothing(Vec<Excused>),
}

impl Reverted {
    /// Can anything in `revert` reach the tests the files in `test_files` declare?
    ///
    /// **Every answer that is not an excuse is [`Self::Behaviour`]**, including the ones that are
    /// really "this cannot tell": a path with no entry in the diff, a file whose owning package
    /// does not resolve, a test file whose package does not either. The excuse is what has to be
    /// earned, because it is the half that turns a failure into a non-answer.
    ///
    /// An EMPTY revert set is [`Self::Behaviour`] as well, and deliberately: `Nothing(vec![])`
    /// would be a claim about the partition with nothing in the output to read it off. That case
    /// has its own arm anyway - `super::remedies::report_nothing_to_revert` - and never arrives.
    pub(crate) fn of(revert: &[String], test_files: &[String], files: &[ChangedFile], read: &PostImage<'_>) -> Self {
        if revert.is_empty() {
            return Self::Behaviour;
        }
        let Some(measured) = packages(test_files, read) else {
            return Self::Behaviour;
        };
        let mut excused = Vec::with_capacity(revert.len());
        for path in revert {
            let Some(file) = files.iter().find(|changed| changed.path == *path) else {
                return Self::Behaviour;
            };
            let Some(one) = excuse(file, &measured, read) else {
                return Self::Behaviour;
            };
            excused.push(one);
        }
        Self::Nothing(excused)
    }

    /// The files this says nothing reverted could have reached, or nothing.
    pub(crate) fn out_of_reach(&self) -> &[Excused] {
        match *self {
            Self::Behaviour => &[],
            Self::Nothing(ref excused) => excused,
        }
    }
}

/// The same question asked of BOTH attempts, because the second one reverts more.
///
/// **THE RETRY IS A DIFFERENT PARTITION AND THEREFORE A DIFFERENT ANSWER.** `super::prove`'s
/// second attempt puts the held-back files at base as well - which is the whole point of it, a
/// tree coherently at base - so a classification taken over the first attempt's revert set would
/// be excusing a revert that did not happen. A held-back file is one carrying an implementation
/// change and a test together, so this is exactly the direction that must not be wrong: with one
/// of those in the set the answer is [`Reverted::Behaviour`] and the failure stands.
#[derive(Debug)]
pub(crate) struct Attempts {
    first: Reverted,
    retried: Reverted,
}

impl Attempts {
    /// Classify the first attempt's `revert`, and the second's `revert` plus `held`.
    pub(crate) fn of(
        revert: &[String],
        held: &[String],
        test_files: &[String],
        files: &[ChangedFile],
        read: &PostImage<'_>,
    ) -> Self {
        let everything: Vec<String> = revert.iter().chain(held.iter()).cloned().collect();
        Self {
            first: Reverted::of(revert, test_files, files, read),
            retried: Reverted::of(&everything, test_files, files, read),
        }
    }

    /// The answer for the attempt that produced the outcome being reported.
    pub(crate) const fn attempt(&self, retried: bool) -> &Reverted {
        if retried { &self.retried } else { &self.first }
    }
}

/// One reverted file that cannot have reddened a test in scope, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Excused {
    path: String,
    why: Why,
}

impl Excused {
    /// The file this excuses, for the contradiction `super::base` prints beside a red base run.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// The line the verdict prints for it.
    pub(crate) fn line(&self) -> String {
        match self.why {
            Why::SameProgram => format!("    out of reach: {}  (blank lines and comments only)", self.path),
            Why::TestCodeElsewhere(ref package) => format!(
                "    out of reach: {}  (`{}` test code, and no test in scope is in `{}`)",
                self.path,
                package.as_str(),
                package.as_str()
            ),
        }
    }
}

/// Say that a green base run measured the partition, and how each reverted file is out of reach.
///
/// **The sentence lives beside the rule that decides it.** Every other verdict arm's words live in
/// `super::base`; this one's whole content is which excuse applied to which file, so an excuse and
/// the words explaining it are one thing to keep true rather than two. It also kept that module
/// under the unexemptable 1000-line cap, which is the honest second reason.
pub(crate) fn explain(excused: &[Excused], measured: &str) -> Verdict {
    for line in unreachable_lines(excused, measured) {
        println!("{line}");
    }
    Verdict::Inconclusive
}

/// Every line [`explain`] prints, in order.
///
/// PURE for the reason `super::remedies::scope_lines` is: nothing in this venue captures stdout,
/// and what a verdict may CLAIM is exactly the thing an assertion has to be able to read. The claim
/// that matters here is that each excused file is NAMED - a verdict saying *nothing was in reach*
/// over a list nobody printed is the shape this gate has already been wrong in twice.
fn unreachable_lines(excused: &[Excused], measured: &str) -> Vec<String> {
    let mut lines = vec![String::from(
        "  base: green - and nothing that was reverted is in reach of the tests in scope",
    )];
    lines.extend(excused.iter().map(Excused::line));
    lines.push(String::new());
    lines.extend(
        [
            "xtask test-causality: INCONCLUSIVE - the revert could not have reddened these tests.",
            "Every file this run put back at base restores no line a test in scope can execute,",
            "so they passed there for a reason that is a fact about the PARTITION rather than",
            "about them: nothing that could have made them red was reverted. That is the argument",
            "a build input already gets - a file that cannot be the thing under test proves",
            "nothing when it is reverted - reaching a file that WAS reverted.",
            "It is not a pass: this run carries NO red-before-green evidence. Prove each added",
            "test by MUTATION - break what it claims, one at a time, scoped to a whole test",
            "binary and never to a name pattern - or, if the change really is test-only, say",
            "plainly that it is not a regression test.",
        ]
        .map(String::from),
    );
    lines.push(format!(
        "This exit is INCONCLUSIVE (code 3) rather than a pass, and {measured}."
    ));
    lines
}

/// The lines a RED base run prints when this module said the revert could not reach these tests.
///
/// **A FALSIFIER, from two different places.** This module reads the diff and says the revert
/// restores nothing a scoped test can execute; nextest says a scoped test failed once it was
/// restored. Both cannot be right, and the pairing is the only evidence available that the reach
/// rule has a hole in it. Empty for [`Reverted::Behaviour`], because there is no claim to
/// contradict - so the ordinary passing run prints nothing new.
pub(crate) fn contradiction(reverted: &Reverted) -> Vec<String> {
    reverted
        .out_of_reach()
        .iter()
        .map(|one| {
            format!(
                "    CONTRADICTED: {} was classified as out of reach of these tests",
                one.path()
            )
        })
        .collect()
}

/// Which of the two arguments excuses a reverted file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Why {
    /// Base and head are the same program: nothing it changed is a line of code.
    SameProgram,
    /// Its whole change is `#[cfg(test)]` code of a package that compiles no test in scope, and
    /// `#[cfg(test)]` code reaches no dependent.
    TestCodeElsewhere(CargoName),
}

/// The packages the scoped tests compile into, or `None` if any of them does not resolve.
///
/// `None` rather than a partial set: the set is used to REFUSE an excuse, so a package missing
/// from it would excuse a file in the very package a test in scope lives in.
fn packages(test_files: &[String], read: &PostImage<'_>) -> Option<BTreeSet<String>> {
    test_files
        .iter()
        .map(|path| place::package(path, read).map(|name| String::from(name.as_str())))
        .collect()
}

/// The argument that excuses reverting `file`, if there is one.
fn excuse(file: &ChangedFile, measured: &BTreeSet<String>, read: &PostImage<'_>) -> Option<Excused> {
    // A page, a recipe or a nix file is the implementation of every test that READS it, and
    // nothing here reads which - so neither argument below is available for one. `//` is not a
    // comment in markdown either, which would make the first one plainly wrong there.
    if Reach::of(&file.path) != Reach::Compiled {
        return None;
    }
    // **AN EMPTY CHANGE SET IS NOT AN UNCHANGED FILE**, and `all` over an empty iterator is `true`,
    // so this arm used to excuse a file the PARSER could not read. Reachable: `super::diff` dropped
    // any removed line spelling `--- ..`, which a `--` SQL comment at column 0 of a raw string is
    // one fixture away from in a repository whose subject is generated SQL - and a file whose whole
    // change was such a line arrived here with nothing in it. That parser no longer drops the line;
    // this refuses the shape anyway, because *the parser saw nothing* and *nothing changed* are two
    // statements and only one of them is an excuse.
    changed_lines(file).next()?;
    if changed_lines(file).all(inert) {
        return Some(Excused {
            path: file.path.clone(),
            why: Why::SameProgram,
        });
    }
    let within = scope(&file.path, read);
    if !only_its_own_tests(file, &within) {
        return None;
    }
    let package = place::package(&file.path, read)?;
    (!measured.contains(package.as_str())).then(|| Excused {
        path: file.path.clone(),
        why: Why::TestCodeElsewhere(package),
    })
}

/// Every line this diff changed in `file`, added and removed, with the post-image line each sits
/// at - the gap a removal left, per [`super::diff::RemovedLine`].
fn changed_lines(file: &ChangedFile) -> impl Iterator<Item = (usize, &str)> {
    let added = file.added.iter().map(|line| (line.number, line.text.as_str()));
    let removed = file.removed.iter().map(|line| (line.anchor, line.text.as_str()));
    added.chain(removed)
}

/// Blank, or a comment that is not COMPILED.
///
/// **Narrower than `super::regions`' own exemption twice over, and each narrowing is a defect that
/// was reachable without it.** That one lets an ATTRIBUTE through, because it is answering *did the
/// author put an implementation change outside the test module* and an attribute is usually
/// punctuation there; here the answer decides whether reverting a file can change what RUNS, and a
/// `#[derive(..)]` or a `#[serde(..)]` on a production type changes exactly that.
///
/// **AND `///` / `//!` ARE NOT COMMENTS FOR THIS QUESTION - THEY ARE PROGRAM TEXT.** A doc comment
/// is an attribute in disguise, and this workspace reads it at runtime rather than only in
/// documentation: `schemars` puts a root doc comment into a tool schema's `description`, which is
/// text a model reads before it calls the tool, and `utoipa::ToSchema` does the same for the HTTP
/// surface. Review reproduced it in the real tree - changing exactly ONE `///` line in
/// `crates/sutura-mcp/src/wire.rs` reddens a committed-snapshot test - so *the same program* would
/// have been false about a change that alters what a caller is told. Only a plain `//` is inert.
///
/// `////` and longer runs are ordinary comments in Rust and are refused here anyway: the prefix
/// test cannot separate them, and refusing is the direction that keeps a failure a failure.
fn inert(line: (usize, &str)) -> bool {
    let (_, text) = line;
    let trimmed = text.trim();
    let plain_comment = trimmed.starts_with("//") && !trimmed.starts_with("///") && !trimmed.starts_with("//!");
    trimmed.is_empty() || plain_comment
}

/// Does every line this file changed sit inside its own test regions, or change no program at all?
///
/// The two are OR-ed per line rather than checked as two whole-file questions, because the common
/// shape is both at once: a comment corrected at the top of a file and an assertion rewritten in
/// its `mod tests`. Neither line can reach another package's build, for its own reason.
///
/// **AN ADDED LINE HAS A POSITION; A REMOVED ONE HAS A GAP, and asking about one side of it was a
/// door.** `super::diff::RemovedLine`'s anchor is the last surviving line before the gap for a pure
/// deletion and the first replacement line for a replacement, and `regions`' ranges INCLUDE the
/// region's own closing line - so a production deletion immediately after a
/// `#[cfg(test)] mod x;` anchored on that declaration and read as test code. Review drove it end to
/// end against a two-package workspace: `INCONCLUSIVE`, exit 3, over a diff whose head compiles
/// differently and whose added test pins the old behaviour. [`neighbourhood`] is the fix, and it is
/// deliberately blunt.
fn only_its_own_tests(file: &ChangedFile, within: &TestScope) -> bool {
    let added = file
        .added
        .iter()
        .all(|line| inert((line.number, &line.text)) || within.covers(line.number));
    let removed = file
        .removed
        .iter()
        .all(|line| inert((line.anchor, &line.text)) || neighbourhood(within, line.anchor));
    added && removed
}

/// Is the whole neighbourhood of a gap at `anchor` test code?
///
/// **All three lines, because the anchor names one side of the gap and WHICH side depends on the
/// hunk.** Deriving the exact side would mean reading the new-side COUNT out of every hunk header
/// and threading two numbers where one is; asking for `anchor - 1`, `anchor` and `anchor + 1`
/// costs a line and is correct for both shapes plus any context width. What it costs is precision
/// at a region's first line, where a removal is refused an excuse it might have earned - and that
/// direction is a failure that stays a failure, which is this gate's default and not a new one.
fn neighbourhood(within: &TestScope, anchor: usize) -> bool {
    within.covers(anchor.saturating_sub(1)) && within.covers(anchor) && within.covers(anchor + 1)
}

#[cfg(test)]
mod tests {
    use super::{Excused, Reverted};
    use crate::Verdict;
    use crate::causality::base::{BaseOutcome, classify_base, report_base};
    use crate::causality::diff::ChangedFile;
    use crate::causality::fixtures::{changed, changed_removing, manifest, named, scoped, tree};
    use crate::causality::provenance::Moved;

    /// The four files `github.com/telekom/sutura#397`'s diff changed, as the gate read them.
    ///
    /// Two of the three it reverted are another package's test code; the third is in the SAME
    /// package as the measured test and is excused only because its whole change is comments.
    fn pr_397_diff() -> Vec<ChangedFile> {
        vec![
            changed_removing(
                "crates/app/src/prompt/tests.rs",
                529,
                &["    assert!(text.contains(&format!(\"At most {MAX} rows\")));"],
                &["    assert!(text.contains(&MAX.to_string()));"],
            ),
            changed(
                "crates/http/src/identity_e2e.rs",
                353,
                &[
                    "    // A SUBSTRING over a haystack whose tail is random, and it is safe by",
                    "    // ARITHMETIC rather than by alphabet.",
                ],
            ),
            changed_removing(
                "crates/runtime/src/banner.rs",
                345,
                &["        assert_eq!(layers, path);"],
                &["        assert!(!layers.contains(PORT));"],
            ),
            changed(
                "crates/http/src/inbound/tests.rs",
                552,
                &["#[test]", "fn a_coordinate_spelling_the_member_name_still_names_no_key() {}"],
            ),
        ]
    }

    /// The post-image those four files sit in: three packages, and the declarations that make two
    /// of them wholly test code.
    fn pr_397_tree() -> impl Fn(&str) -> Option<String> {
        tree(&[
            ("crates/app/Cargo.toml", &manifest("app")),
            ("crates/http/Cargo.toml", &manifest("http")),
            ("crates/runtime/Cargo.toml", &manifest("runtime")),
            // A whole file of tests, declared `#[cfg(test)] mod tests;` one level up.
            ("crates/app/src/prompt.rs", "#[cfg(test)]\nmod tests;\n"),
            ("crates/app/src/prompt/tests.rs", &format!("{}\n", "// tests\n".repeat(600))),
            (
                "crates/http/src/lib.rs",
                "#[cfg(test)]\nmod identity_e2e;\n#[cfg(test)]\nmod inbound;\n",
            ),
            ("crates/http/src/identity_e2e.rs", &format!("{}\n", "// e2e\n".repeat(400))),
            (
                "crates/http/src/inbound/tests.rs",
                "#[test]\nfn a_coordinate_spelling_the_member_name_still_names_no_key() {}\n",
            ),
            // `mod tests` opens at 344 and runs to the end, so both hunks at 345 are inside it.
            (
                "crates/runtime/src/banner.rs",
                &format!(
                    "{}#[cfg(test)]\nmod tests {{\n{}}}\n",
                    "fn banner() {}\n".repeat(342),
                    "    let x = 1;\n".repeat(20)
                ),
            ),
        ])
    }

    /// #397's measured partition, through the real classifier: `Excused`'s fields are private to
    /// this module, so nothing outside it can build the variant, and a hand-built one would let an
    /// assertion pin a shape the classifier cannot produce.
    fn out_of_reach() -> Reverted {
        let (files, read) = (pr_397_diff(), pr_397_tree());
        Reverted::of(
            &[
                String::from("crates/app/src/prompt/tests.rs"),
                String::from("crates/http/src/identity_e2e.rs"),
                String::from("crates/runtime/src/banner.rs"),
            ],
            &[String::from("crates/http/src/inbound/tests.rs")],
            &files,
            &read,
        )
    }

    #[test]
    fn a_green_run_over_a_revert_nothing_in_scope_could_read_is_not_that_defect() {
        // `github.com/telekom/sutura#394`, and the measured shape of #397: the only thing reverted
        // is another package's test code, so the one test in scope passed on base because nothing
        // that could have reddened it was put back. That is a fact about the PARTITION, and
        // FAILED - the verdict it used to get - names a defect this run did not observe.
        let under_test = scoped(
            "http",
            "crates/http/src/inbound/tests.rs",
            &["a_coordinate_spelling_the_member_name_still_names_no_key"],
        );
        let green = "test result: ok. 1 passed; 0 failed";
        let unreachable = out_of_reach();
        assert!(!unreachable.out_of_reach().is_empty(), "{unreachable:?}");
        let outcome = classify_base(green, true, &under_test, &Moved::Nothing, &unreachable);
        assert!(
            matches!(outcome, BaseOutcome::GreenOverAnUnreachableRevert { .. }),
            "{outcome:?}"
        );
        // EXIT 3, not 0 and not 1. Not a pass - this run carries no red-before-green evidence at
        // all - and not the failure it was, which blamed an author for the gate's own partition.
        assert_eq!(
            report_base(&outcome, green, false, &named(1), &Moved::Nothing, &unreachable),
            Verdict::Inconclusive
        );
        // AND THE DIRECTION THAT MAY NOT MOVE. The same green run, over a revert that CAN reach
        // the tests, is still the defect the gate exists for.
        assert_eq!(
            classify_base(green, true, &under_test, &Moved::Nothing, &Reverted::Behaviour),
            BaseOutcome::Green
        );
        assert_eq!(
            report_base(
                &BaseOutcome::Green,
                green,
                false,
                &named(1),
                &Moved::Nothing,
                &Reverted::Behaviour
            ),
            Verdict::Fail
        );
    }

    #[test]
    fn a_red_base_run_contradicting_the_reach_rule_says_so_and_still_passes() {
        // THE FALSIFIER. *Out of reach* and *red by assertion* are contradictory answers about one
        // run, from two different places - the diff and nextest - so a run that reports both is a
        // reach rule with a hole in it. It is PRINTED rather than acted on: this run produced the
        // evidence the gate exists for, and reddening it would be the wrong trade.
        let unreachable = out_of_reach();
        let red = BaseOutcome::RedByAssertion {
            failed: vec![String::from(
                "http inbound::tests::a_coordinate_spelling_the_member_name_still_names_no_key",
            )],
        };
        assert_eq!(
            report_base(
                &red,
                "FAIL [ 0.0s ] http inbound::tests::a_coordinate_spelling_the_member_name_still_names_no_key",
                false,
                &named(1),
                &Moved::Nothing,
                &unreachable
            ),
            Verdict::Pass
        );
    }

    #[test]
    fn a_revert_that_restores_no_line_the_measured_tests_can_execute_is_out_of_reach() {
        // The measured shape of #397: `app` and `runtime` test code, plus a same-package file
        // whose whole diff is comments. The verdict this feeds says the run measured the
        // PARTITION, so every one of the three has to be excused - one short-circuits to
        // `Behaviour` and the failure stands.
        let (files, read) = (pr_397_diff(), pr_397_tree());
        let revert = vec![
            String::from("crates/app/src/prompt/tests.rs"),
            String::from("crates/http/src/identity_e2e.rs"),
            String::from("crates/runtime/src/banner.rs"),
        ];
        let measured = vec![String::from("crates/http/src/inbound/tests.rs")];
        let verdict = Reverted::of(&revert, &measured, &files, &read);
        let lines: Vec<String> = verdict.out_of_reach().iter().map(Excused::line).collect();
        assert_eq!(lines.len(), 3, "{verdict:?}");
        assert!(
            lines
                .iter()
                .any(|line| line.contains("prompt/tests.rs") && line.contains("`app` test code")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("identity_e2e.rs") && line.contains("blank lines and comments only")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("banner.rs") && line.contains("`runtime` test code")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_test_pinning_existing_behaviour_beside_an_implementation_change_stays_in_reach() {
        // THE CASE THE GATE EXISTS FOR, and the one this module may not excuse: a production hunk
        // and, in another file, a test that passes with it reverted. The implementation change is
        // outside every test region, so no excuse is available and the green base run stays the
        // failure it is.
        let files = vec![
            changed("crates/x/src/parse.rs", 12, &["    let widened = accept(anything);"]),
            changed("crates/x/src/parse/tests.rs", 4, &["#[test]", "fn a_name_still_parses() {}"]),
        ];
        let read = tree(&[
            ("crates/x/Cargo.toml", &manifest("x")),
            ("crates/x/src/parse.rs", "fn accept() {}\n".repeat(20).as_str()),
            ("crates/x/src/parse/tests.rs", "#[test]\nfn a_name_still_parses() {}\n"),
        ]);
        let revert = vec![String::from("crates/x/src/parse.rs")];
        let measured = vec![String::from("crates/x/src/parse/tests.rs")];
        assert_eq!(Reverted::of(&revert, &measured, &files, &read), Reverted::Behaviour);

        // AND AN ATTRIBUTE IS A PROGRAM, which is where this module's `inert` has to be NARROWER
        // than `super::regions`'. That one exempts an attribute wherever it lands, because it is
        // answering *did the author put an implementation change outside the test module*; an added
        // `#[derive(..)]` or `#[serde(..)]` on a production type changes exactly what a test sees.
        let attributed = vec![
            changed("crates/x/src/parse.rs", 12, &["#[derive(Clone)]"]),
            changed("crates/x/src/parse/tests.rs", 4, &["#[test]", "fn a_name_still_parses() {}"]),
        ];
        assert_eq!(Reverted::of(&revert, &measured, &attributed, &read), Reverted::Behaviour);
    }

    #[test]
    fn the_test_code_of_a_package_a_measured_test_is_in_is_still_in_reach() {
        // The non-obvious half of the cross-package argument. `#[cfg(test)]` code reaches no
        // dependent, so another package's is invisible - but the SAME package's is compiled into
        // the very test binary the measured test lands in, and a helper this module cannot follow
        // is exactly what lives there.
        let files = vec![
            changed_removing(
                "crates/x/src/harness.rs",
                6,
                &["    fn fixture() -> u8 { 2 }"],
                &["    fn fixture() -> u8 { 1 }"],
            ),
            changed(
                "crates/x/src/thing/tests.rs",
                3,
                &["#[test]", "fn a_fixture_backed_cell() {}"],
            ),
        ];
        let read = tree(&[
            ("crates/x/Cargo.toml", &manifest("x")),
            ("crates/x/src/lib.rs", "#[cfg(test)]\nmod harness;\n"),
            ("crates/x/src/harness.rs", "fn fixture() {}\n".repeat(20).as_str()),
            ("crates/x/src/thing/tests.rs", "#[test]\nfn a_fixture_backed_cell() {}\n"),
        ]);
        let revert = vec![String::from("crates/x/src/harness.rs")];
        let measured = vec![String::from("crates/x/src/thing/tests.rs")];
        assert_eq!(Reverted::of(&revert, &measured, &files, &read), Reverted::Behaviour);
    }

    #[test]
    fn a_deletion_of_production_code_is_in_reach_though_it_added_nothing() {
        // WHY `super::super::diff::RemovedLine` EXISTS. Reading additions only, this file changed
        // nothing but a comment - and reverting it puts a guard clause back into a function the
        // measured test calls. `changed_removing` anchors the removal at the hunk, which is
        // outside the file's test region.
        let files = vec![
            changed_removing(
                "crates/x/src/guard.rs",
                9,
                &["    // the early return is gone"],
                &["    if wrong { return Err(e); }"],
            ),
            changed("crates/x/tests/guarded.rs", 1, &["#[test]", "fn it_no_longer_refuses() {}"]),
        ];
        let read = tree(&[
            ("crates/x/Cargo.toml", &manifest("x")),
            ("crates/x/src/guard.rs", "fn guard() {}\n".repeat(30).as_str()),
            ("crates/x/tests/guarded.rs", "#[test]\nfn it_no_longer_refuses() {}\n"),
        ]);
        let revert = vec![String::from("crates/x/src/guard.rs")];
        let measured = vec![String::from("crates/x/tests/guarded.rs")];
        assert_eq!(Reverted::of(&revert, &measured, &files, &read), Reverted::Behaviour);
    }

    #[test]
    fn the_retry_is_asked_about_the_larger_revert_set_it_actually_performs() {
        // `super::super::prove`'s second attempt puts the HELD-BACK files at base too, and a held
        // file is one carrying an implementation change and a test together. Classifying the retry
        // over the first attempt's set would excuse a revert that did happen, which is the one
        // direction this module may not be wrong in.
        let files = vec![
            changed("crates/other/src/prompt/tests.rs", 4, &["    assert_eq!(text, rendered);"]),
            changed(
                "crates/x/src/parse.rs",
                7,
                &[
                    "    let widened = accept(anything);",
                    "    #[cfg(test)]",
                    "    mod tests {",
                    "        #[test]",
                ],
            ),
            changed("crates/x/src/other/tests.rs", 2, &["#[test]", "fn a_new_cell() {}"]),
        ];
        let read = tree(&[
            ("crates/other/Cargo.toml", &manifest("other")),
            ("crates/other/src/prompt.rs", "#[cfg(test)]\nmod tests;\n"),
            ("crates/other/src/prompt/tests.rs", "// a whole file of tests\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
            ("crates/x/src/parse.rs", "fn accept() {}\n".repeat(30).as_str()),
            ("crates/x/src/other/tests.rs", "#[test]\nfn a_new_cell() {}\n"),
        ]);
        let both = super::Attempts::of(
            &[String::from("crates/other/src/prompt/tests.rs")],
            &[String::from("crates/x/src/parse.rs")],
            &[String::from("crates/x/src/other/tests.rs")],
            &files,
            &read,
        );
        // First attempt: only another package's test code went back, so a green run there measured
        // the partition rather than the tests.
        assert!(!both.attempt(false).out_of_reach().is_empty(), "{both:?}");
        // Retry: the held file went back too, and it carries an implementation change.
        assert_eq!(*both.attempt(true), Reverted::Behaviour, "{both:?}");
    }

    #[test]
    fn the_inconclusive_verdict_names_every_file_it_excused_and_claims_no_proof() {
        // WHAT THE SENTENCE MAY SAY. A verdict asserting *nothing reverted was in reach* over a
        // list nobody printed is a claim with no witness, which is the defect class this gate has
        // already carried twice. So every excused file is named, and the same paste says in words
        // that the run proved nothing - the exit code alone has been misread here before.
        let excused = out_of_reach();
        let lines = super::unreachable_lines(excused.out_of_reach(), "1 of 1 added tests measured");
        for path in [
            "crates/app/src/prompt/tests.rs",
            "crates/http/src/identity_e2e.rs",
            "crates/runtime/src/banner.rs",
        ] {
            assert!(lines.iter().any(|line| line.contains(path)), "{path} is not named: {lines:?}");
        }
        assert!(
            lines
                .iter()
                .any(|line| line.contains("INCONCLUSIVE (code 3) rather than a pass")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("NO red-before-green evidence")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_red_run_contradicts_this_rule_per_file_and_an_ordinary_pass_prints_nothing() {
        // The falsifier's own shape: it says which file the contradiction is about, and it is
        // silent on every run where no excuse was given - so a passing verdict does not grow a
        // line for a claim nobody made.
        assert!(super::contradiction(&Reverted::Behaviour).is_empty());
        let lines = super::contradiction(&out_of_reach());
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines.iter().all(|line| line.contains("CONTRADICTED")), "{lines:?}");
    }

    #[test]
    fn a_production_deletion_after_a_test_module_declaration_is_in_reach() {
        // THE DOOR REVIEW DROVE A NON-CAUSAL DIFF THROUGH, end to end, to `INCONCLUSIVE` exit 3.
        // `#[cfg(test)] mod probe;` is a one-line region, `regions` includes its own last line, and
        // a pure deletion of the production line under it anchors ON that declaration. Asking
        // about the anchor alone called a deleted `use` statement test code; asking about the whole
        // neighbourhood does not, because the line after the gap is production.
        let files = vec![
            changed_removing("crates/pkg-a/src/lib.rs", 2, &[], &["use strict::accepts;"]),
            changed("crates/pkg-b/tests/pinned.rs", 2, &["#[test]", "fn a_pinned_cell() {}"]),
        ];
        let read = tree(&[
            ("crates/pkg-a/Cargo.toml", &manifest("pkg-a")),
            ("crates/pkg-b/Cargo.toml", &manifest("pkg-b")),
            // Line 1 is the attribute, line 2 the declaration - so the region is 1..=2 and the
            // deleted line's anchor, 2, is inside it. Line 3 is production.
            (
                "crates/pkg-a/src/lib.rs",
                "#[cfg(test)]\nmod probe;\nuse loose::*;\npub fn accepts() {}\n",
            ),
            ("crates/pkg-b/tests/pinned.rs", "#[test]\nfn a_pinned_cell() {}\n"),
        ]);
        let revert = vec![String::from("crates/pkg-a/src/lib.rs")];
        let measured = vec![String::from("crates/pkg-b/tests/pinned.rs")];
        assert_eq!(Reverted::of(&revert, &measured, &files, &read), Reverted::Behaviour);
        // The neighbourhood is what decides it: inside the region on every side, it is test code.
        let within = crate::causality::regions::scope("crates/pkg-a/src/lib.rs", &read);
        assert!(!super::neighbourhood(&within, 2), "the line after the gap is production");
        let whole = crate::causality::regions::scope("crates/pkg-b/tests/pinned.rs", &read);
        assert!(
            super::neighbourhood(&whole, 2),
            "a dedicated test target is test code on every side of any gap"
        );
    }

    #[test]
    fn a_change_set_the_parser_could_not_read_earns_no_excuse() {
        // `all` OVER AN EMPTY ITERATOR IS TRUE, so a file whose parsed change set is empty was
        // vacuously *blank lines and comments only*. `super::super::diff` no longer drops the line
        // that produced it, and this refuses the shape regardless: *the parser saw nothing* and
        // *nothing changed* are two statements and only one of them is an excuse.
        let files = vec![
            changed("crates/x/src/render.rs", 1, &[]),
            changed("crates/y/src/pinned/tests.rs", 2, &["#[test]", "fn a_pinned_cell() {}"]),
        ];
        let read = tree(&[
            ("crates/x/Cargo.toml", &manifest("x")),
            ("crates/y/Cargo.toml", &manifest("y")),
            ("crates/x/src/render.rs", "fn statement() {}\n"),
            ("crates/y/src/pinned/tests.rs", "#[test]\nfn a_pinned_cell() {}\n"),
        ]);
        let revert = vec![String::from("crates/x/src/render.rs")];
        let measured = vec![String::from("crates/y/src/pinned/tests.rs")];
        assert_eq!(Reverted::of(&revert, &measured, &files, &read), Reverted::Behaviour);
    }

    #[test]
    fn a_doc_comment_is_program_text_and_a_plain_comment_is_not() {
        // `///` REACHES A CALLER. `schemars` puts a root doc comment into a tool schema's
        // `description`, and `utoipa::ToSchema` does the same for the HTTP surface, so a
        // `///`-only change alters what a model is told before it calls the tool - reproduced in
        // this tree against a committed snapshot. A plain `//` still changes nothing compiled,
        // which is the excuse #397 relies on.
        let read = tree(&[
            ("crates/x/Cargo.toml", &manifest("x")),
            ("crates/x/src/wire.rs", "pub struct Tool;\n".repeat(20).as_str()),
            ("crates/y/Cargo.toml", &manifest("y")),
            ("crates/y/src/pinned/tests.rs", "#[test]\nfn a_pinned_cell() {}\n"),
        ]);
        let revert = vec![String::from("crates/x/src/wire.rs")];
        let measured = vec![String::from("crates/y/src/pinned/tests.rs")];
        let pinned = changed("crates/y/src/pinned/tests.rs", 2, &["#[test]", "fn a_pinned_cell() {}"]);
        for doc in [
            "/// what the model reads before it calls this",
            "//! the crate's own description",
        ] {
            let files = vec![
                changed("crates/x/src/wire.rs", 4, &[doc]),
                changed("crates/y/src/pinned/tests.rs", 2, &["#[test]", "fn a_pinned_cell() {}"]),
            ];
            assert_eq!(
                Reverted::of(&revert, &measured, &files, &read),
                Reverted::Behaviour,
                "a doc comment is compiled into the program: {doc}"
            );
        }
        // The plain form is still inert, and this is the arm #397 lands on.
        let plain = vec![
            changed("crates/x/src/wire.rs", 4, &["// a note to whoever edits this"]),
            pinned,
        ];
        assert!(
            !Reverted::of(&revert, &measured, &plain, &read).out_of_reach().is_empty(),
            "a plain comment changes no program"
        );
    }

    #[test]
    fn a_file_cargo_does_not_compile_is_never_excused() {
        // The class whose whole point is that reverting it reddens a suite whose implementation is
        // prose. `//` is not a comment there, and nothing here reads which test opens which file.
        let files = vec![
            changed(
                "docs/architecture.md",
                40,
                &["// this line begins like a comment and is page text"],
            ),
            changed("crates/x/tests/pages.rs", 1, &["#[test]", "fn the_page_says_it() {}"]),
        ];
        let read = tree(&[
            ("crates/x/Cargo.toml", &manifest("x")),
            ("crates/x/tests/pages.rs", "#[test]\nfn the_page_says_it() {}\n"),
        ]);
        let revert = vec![String::from("docs/architecture.md")];
        let measured = vec![String::from("crates/x/tests/pages.rs")];
        assert_eq!(Reverted::of(&revert, &measured, &files, &read), Reverted::Behaviour);
    }

    #[test]
    fn an_unresolvable_package_and_an_empty_revert_set_both_refuse_to_excuse() {
        // Both directions of "this cannot tell". An excuse is what has to be earned: no manifest
        // above the reverted file means no package to compare, and an empty revert set would make
        // `Nothing` a claim with nothing in the output to read it off.
        let files = vec![changed("stray/src/a.rs", 2, &["fn f() {}"])];
        let read = tree(&[
            ("crates/x/Cargo.toml", &manifest("x")),
            ("crates/x/tests/t.rs", "#[test]\nfn t() {}\n"),
        ]);
        let measured = vec![String::from("crates/x/tests/t.rs")];
        let revert = vec![String::from("stray/src/a.rs")];
        assert_eq!(Reverted::of(&revert, &measured, &files, &read), Reverted::Behaviour);
        assert_eq!(Reverted::of(&[], &measured, &files, &read), Reverted::Behaviour);
        // And a path the diff does not carry: nothing to classify, so nothing is excused.
        let absent = vec![String::from("crates/x/src/missing.rs")];
        assert_eq!(Reverted::of(&absent, &measured, &files, &read), Reverted::Behaviour);
    }
}

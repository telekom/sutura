//! Which tests the diff added, and the nextest filter that runs exactly those.
//!
//! WHY THE RUN IS SCOPED AT ALL. An unfiltered `--workspace` base run makes the verdict a property
//! of the WHOLE suite rather than of the tests the diff added; `super::base` carries the run that
//! measured what that cost. Scoping is the half that stops an unrelated failure from happening at
//! all, and that module is the half that stops one being read as evidence if it happens anyway.
//!
//! A BARE FUNCTION NAME IS NOT A KEY IN THIS TREE, and `super::place` owns that key, the
//! measurement behind it and the collisions it still does not separate. Read it before believing
//! a filter here identifies one test.
//!
//! FAIL CLOSED ON AN EMPTY SCAN, and that is what [`Scoped`] is for. A scan naming no test may
//! not fall back to "no filter", because an unfiltered run makes the verdict a property of the
//! suite. So the non-empty set is a TYPE rather than a check somebody remembers to write at the
//! call site: [`Scan::of`] answers [`Scan::Unnamed`], and the gate refuses instead of measuring
//! the suite.
//!
//! FAIL CLOSED ON A PARTIAL ONE TOO, which took longer to see because the scan is AGGREGATE: it
//! answered `Runnable` the moment ONE provable file named a test, so a second provable file whose
//! added `#[test]` yielded no name rode along unmeasured and unmentioned - the common shape, not a
//! corner. [`Scan::Unreadable`] is that case and it refuses AHEAD of `Runnable`. It is counted
//! PER ATTRIBUTE rather than per file, which is the second half of the same finding: `named > 0`
//! used to end the file's inspection, so a second unnameable attribute BESIDE a nameable one was
//! invisible in both of `super::coverage`'s numbers. **The precision cost was measured before it
//! was taken** (2026-09-05, every `.rs` file under `crates/`, `xtask/` and `dev/` walked through
//! this extractor): **zero** test-declaring attributes fail to name a function, so the
//! per-attribute rule refuses nothing this tree writes. Only the zero is written down, because it
//! is the whole argument and a total would rot inside one PR. What it WOULD refuse is a
//! test-declaring attribute with no function under it, which does not compile, and a
//! `#[test]`-shaped line inside a raw string literal, of which there is none.
//!
//! WHAT A SILENT FILE IS ALLOWED TO CLAIM, and this one was asserted rather than checked. A
//! `#[cfg(test)] mod ..` declaration names nothing by design, so refusing per file would redden
//! the ordinary way a test module is added - it is [`Scoped::silent`] and the gate prints it. The
//! sentence printed beside it said *its own file names the tests*, and nothing read whether that
//! file was in the diff at all: a `#[cfg(test)] mod legacy;` added to `lib.rs` while `legacy.rs`
//! sits untouched puts a whole pre-existing module of tests into the build, with no added line
//! naming any of them and no run measuring any of them - and, because `lib.rs` is held at HEAD,
//! those tests are in both trees and cannot be red on base either. That is
//! [`Scan::Enabled`], it refuses, and `super::place::declared_module_files` is what resolves the
//! declaration instead of asserting it.
//!
//! AN `#[ignore]`d TEST IS NAMED AND DROPPED: a filterset naming only ignored tests matches
//! nothing, and nextest exits 4 with *error: no tests to run* over legitimate work.
//! Running them is the wrong direction for the reason a tier-backed cell is not required in the
//! reconstructed worktree (see [`super::runner::nextest`]): they are ignored because this venue lacks what
//! they need, so forcing them in a tree nothing provisioned fails CLOSED and reads as
//! red-on-base. So they leave the
//! scope, and a diff whose every added test is ignored gets [`Scan::OnlyIgnored`] - a statement
//! that this gate has not verified the change, not a claim that the extractor is broken.
//!
//! WHAT IT DOES NOT REACH. `super::attributes` accepts an added `#[cfg(test)] mod ..` or
//! `mod tests {` marker, which names no function - so a diff whose ONLY test signal is such a
//! marker is a file the plan calls a test file and this cannot name. **A `#[cfg(test)]` item that
//! is not a module is NOT such a marker**, and treating it as one was a refusal no author could
//! act on; that module's header carries the reasoning. Both attribute lists live THERE, in one
//! file, because a name this cannot extract from a marker it accepts is exactly the disagreement
//! that would reopen the unfiltered run.
//!
//! A NAME NEED NOT COME FROM AN ADDED ATTRIBUTE ANY MORE - `github.com/telekom/sutura#1025`.
//! `super::edited` reads the same [`function_name`] over a PRE-existing attribute whose item an
//! added line lands inside, so an edited assertion is named the same way an added test is.
//! `github.com/telekom/sutura#1031` closes the sibling: an added line inside a `#[cfg(test)]`
//! HELPER fn a test calls is named through `edited::edited_helper_caller`, so it reaches the same
//! proof arms. `super::edited`'s own header carries the shapes that still name nothing - a pure
//! deletion (now a `DeletedTests` refusal, not a pass) and a helper in a DIFFERENT file.

use std::collections::BTreeSet;

use crate::causality::attributes::{attached, decides_a_run_unevaluably, declares_a_test, item_below};
use crate::causality::diff::ChangedFile;
use crate::causality::edited;
use crate::causality::features::{Because, Enabled};
use crate::causality::names::Ident;
use crate::causality::place::{AddedTest, Declares, accounted_for, place};
use crate::causality::regions::{AddedLine, PostImage};
use crate::serde_parse::scan::{code_lines_blanking_all_strings, starts_in_code};

/// A provable file that named no test, and where its tests actually are.
///
/// The second field is the change: the printed sentence used to ASSERT that the declared module's
/// own file names the tests, and nothing read whether that file was in the diff. It is resolved
/// now, so a declaration this cannot account for is [`Scan::Enabled`] rather than a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Silent {
    /// The provable file that named nothing.
    pub(crate) path: String,
    /// The declared module's own file, which IS in this diff and is where its tests are named -
    /// or `None` for an INLINE module, whose body is in this same file and whose lines were
    /// already compiled, so nothing arrived for this to name.
    pub(crate) module: Option<String>,
}

/// The tests a diff added: at least one, by construction.
#[derive(Debug)]
pub(crate) struct Scoped {
    tests: Vec<AddedTest>,
    /// Provable files that named no test and whose reason for naming none is accounted for.
    ///
    /// Carried because this scan is AGGREGATE, so a file it could not name is otherwise invisible
    /// whenever a sibling could. Stated rather than refused, and the statement is now checked.
    silent: Vec<Silent>,
    /// The added tests that left the scope for being `#[ignore]`d.
    ///
    /// **CARRIED RATHER THAN DROPPED, which is `github.com/telekom/sutura#314`.** They are in
    /// neither of `super::coverage`'s numbers by design, and dropping them here meant an
    /// all-`#[ignore]`d diff that reached any arm other than [`Scan::OnlyIgnored`] printed
    /// `0 of 0 added tests measured` and named none of them - which reads as *this diff added no
    /// tests*. A runnable neighbour is what makes that reachable, so the list has to survive
    /// alongside one.
    ignored: Vec<Ident>,
}

impl Scoped {
    /// The keys, for comparing a failure against the set under test.
    pub(crate) fn tests(&self) -> &[AddedTest] {
        &self.tests
    }

    /// Provable files this named no test in, with what accounts for each.
    pub(crate) fn silent(&self) -> &[Silent] {
        &self.silent
    }

    /// The added tests no run in this venue reaches, by name.
    pub(crate) fn ignored(&self) -> &[Ident] {
        &self.ignored
    }

    /// The nextest filter expression that runs exactly these tests.
    pub(crate) fn filterset(&self) -> String {
        self.tests.iter().map(AddedTest::term).collect::<Vec<String>>().join(" + ")
    }

    /// Every one of `tests` as its own claim, with no diff behind it at all.
    ///
    /// `pub(super)` for `super::rot` (`github.com/telekom/sutura#950`): re-verifying a COMMITTED
    /// mutation asks about a cell a past commit's trailer already proved once, and there is no
    /// diff to scan for it a second time - [`Scan::of`] is the only other constructor and it is
    /// diff-shaped throughout. `silent` and `ignored` are empty by construction: both describe
    /// what a DIFF failed to name, which does not apply to a name this already has in hand.
    pub(super) const fn of_named(tests: Vec<AddedTest>) -> Self {
        Self {
            tests,
            silent: Vec::new(),
            ignored: Vec::new(),
        }
    }

    /// A COPY of this scope with the named tests removed, for the composite half of
    /// `github.com/telekom/sutura#954`.
    ///
    /// The claim arm and the ordinary proof used to be mutually exclusive: a diff with a
    /// `Claim-Cell:` trailer went wholly to the claim, and any other added test in the range was
    /// refused (`Undeclared`). Issue #954 moved that refusal OUT - a declaration answers for the
    /// tests its own commit added, and the rest are still the ordinary proof's to run. This is
    /// the seam that composes the two: the claim arm proves the declared set, and the ordinary
    /// proof proves what is left. `None` when nothing remains, so the caller can keep the claim
    /// verdict alone; the `silent`/`ignored` lists are carried because the two halves are still
    /// reported from one diff, and dropping a named-but-ignored test here would un-print it.
    /// It matches the bare fn NAME, which [`AddedTest::name`] says is not a key: two added tests
    /// sharing a name across binaries are both removed (as the old range-wide bijection was).
    pub(crate) fn minus(&self, names: &BTreeSet<String>) -> Option<Self> {
        let kept: Vec<AddedTest> = self.tests.iter().filter(|one| !names.contains(one.name())).cloned().collect();
        (!kept.is_empty()).then_some(Self {
            tests: kept,
            silent: self.silent.clone(),
            ignored: self.ignored.clone(),
        })
    }
}

/// What scanning the diff's test files found.
#[derive(Debug)]
pub(crate) enum Scan {
    /// Tests this venue can run, so there is something to measure.
    Runnable(Scoped),
    /// A provable file added an attribute that DECLARES a test and no name came out of it, or its
    /// post-image could not be read at all. Refuses, and refuses ahead of every other answer,
    /// which is the point: this scan is aggregate, so one nameable test elsewhere in the diff
    /// used to mask the file completely and the run measured a subset with nothing saying so. The
    /// fix is the extractor, which is why it comes first: any other verdict over the same diff is
    /// a verdict this gate cannot read its own inputs for.
    Unreadable(Vec<String>),
    /// A provable file declares a test module whose own file is not in this diff, so a module of
    /// pre-existing tests becomes compiled and no added line names any of them. Refuses - the
    /// remedy is stated evidence, not an extractor fix, which is why it is its own answer.
    ///
    /// **ITS INVERSE IS THE SAME REFUSAL NOW, and the asymmetry was strict until it was.** This
    /// arm reads a `.rs` diff. A pre-existing `#[cfg(feature = "x")] mod tests;` whose `x` a
    /// **`Cargo.toml`-only** diff declares compiles the same whole module of tests with ZERO added
    /// `.rs` lines, so `super::plan` found no changed test file, answered `Plan::NotRequired`, and
    /// the gate passed with *no changed tests - nothing to prove*. That input asks for the same
    /// evidence and had no answer, which is why it is one verdict with two causes rather than two
    /// verdicts: `super::features` reads the feature table on each side of the base commit and
    /// contributes an [`Enabled`] carrying `Because::Feature`, and this arm's own findings carry
    /// `Because::Declared`. `github.com/telekom/sutura#343` is the record; that module's header
    /// carries what the feature scan still does not read.
    Enabled(Vec<Enabled>),
    /// Every test the diff added is `#[ignore]`d. Named, and unreachable by any run here.
    OnlyIgnored(Vec<Ident>),
    /// No test could be named at all.
    Unnamed,
}

impl Scan {
    /// The tests the `provable` files added.
    ///
    /// `provable` is the plan's own list - a file carrying both an implementation change and its
    /// tests is excluded there, and its tests are not part of the proof, so they are not part of
    /// the run either.
    pub(crate) fn of(files: &[ChangedFile], provable: &[String], read: &PostImage<'_>) -> Self {
        let mut runnable: Vec<AddedTest> = Vec::new();
        let mut ignored: Vec<Ident> = Vec::new();
        let mut silent: Vec<Silent> = Vec::new();
        let mut enabled: Vec<Enabled> = Vec::new();
        let mut unreadable: Vec<String> = Vec::new();
        for file in files.iter().filter(|file| provable.contains(&file.path)) {
            // PER ATTRIBUTE, not per file: the count is what `named` is compared against, so an
            // unnameable attribute beside a nameable one is refused rather than dropped.
            let declared = file.added.iter().filter(|line| declares_a_test(line.text.trim())).count();
            let mut named = 0_usize;
            let Some((text, at)) = read(&file.path).zip(place(&file.path, read)) else {
                // ONE fail-closed arm for *this gate cannot say where this file's tests would
                // land*, and nothing may be claimed about such a file - including that a test
                // module arrived in it. `attributes::adds` answers `TestModule` for a post-image it
                // cannot read, which is the fail-closed direction, and THIS is where that direction
                // is delivered: it used to land on the passing `silent` arm, whose printed sentence
                // said a test module arrived. A path no `Cargo.toml` owns is the same answer for
                // the same reason - cargo compiles nothing from it, so no sentence about its tests
                // can be true.
                unreadable.push(file.path.clone());
                continue;
            };
            let lines: Vec<&str> = text.lines().collect();
            let code = Code::of(&text);
            for declaration in file.added.iter().filter_map(|added| declared_under(&lines, &code, added)) {
                named += 1;
                match declaration {
                    Declared::Ignored(name) => {
                        if !ignored.contains(&name) {
                            ignored.push(name);
                        }
                    }
                    Declared::Runs(name, gate) => {
                        let one = AddedTest::at(&file.path, &at, name).with_gate(gate);
                        if !runnable.contains(&one) {
                            runnable.push(one);
                        }
                    }
                }
            }
            // Body edits in an existing test or a helper it calls name that test without
            // changing the added-attribute count.
            let scope = crate::causality::regions::scope(&file.path, read);
            let called = edited::edited_helper_caller(&lines, &file.added, &scope);
            let touched_helper = !called.is_empty();
            let touched = edited::touched_in(&lines, &file.added);
            let touched_any = touched_helper || !touched.is_empty();
            for one in called.into_iter().chain(touched) {
                match one {
                    edited::Touched::Ignored(name) => {
                        if !ignored.contains(&name) {
                            ignored.push(name);
                        }
                    }
                    edited::Touched::Runs(name) => {
                        let gate = gate_named(&lines, &code, &name);
                        let one = AddedTest::at(&file.path, &at, name).with_gate(gate).edited();
                        if !runnable.contains(&one) {
                            runnable.push(one);
                        }
                    }
                }
            }
            if named > 0 && named == declared {
                continue;
            }
            // WHICH kind of unnameable, because they ask for different things - and each file
            // carries its own cause, so a printed remedy cannot offer one this input cannot have.
            if declared > 0 {
                // An added attribute that declares a test and yielded no name: the extractor is
                // the fix.
                unreadable.push(file.path.clone());
                continue;
            }
            if touched_any {
                continue;
            }
            match accounted_for(file, &lines) {
                Some(Declares::Inline) => silent.push(Silent {
                    path: file.path.clone(),
                    module: None,
                }),
                Some(Declares::OutOfLine(candidates)) => {
                    match candidates.iter().find(|one| files.iter().any(|f| f.path == **one)) {
                        Some(present) => silent.push(Silent {
                            path: file.path.clone(),
                            module: Some(present.clone()),
                        }),
                        None => enabled.push(Enabled {
                            path: file.path.clone(),
                            module: candidates.into_iter().next().unwrap_or_default(),
                            because: Because::Declared,
                        }),
                    }
                }
                // Nothing in the added lines declares a module, so what made this a test file
                // names nothing and cannot be accounted for: a dangling attribute, or a form this
                // gate does not read. Same remedy as an unreadable name - fix the extractor.
                None => unreadable.push(file.path.clone()),
            }
        }
        // Ahead of everything: a subset the caller cannot see is the defect, and one sibling that
        // names a test is exactly what used to hide it.
        if !unreadable.is_empty() {
            return Self::Unreadable(unreadable);
        }
        if !enabled.is_empty() {
            return Self::Enabled(enabled);
        }
        if !runnable.is_empty() {
            return Self::Runnable(Scoped {
                tests: runnable,
                silent,
                // The ignored ones go WITH the runnable answer rather than being dropped: they are
                // in neither of `super::coverage`'s numbers, so this list is the only thing that
                // can name them, and a runnable neighbour is exactly when that used to be lost.
                ignored,
            });
        }
        if ignored.is_empty() {
            Self::Unnamed
        } else {
            Self::OnlyIgnored(ignored)
        }
    }
}

/// A test an added attribute declares, and whether a run in this venue reaches it.
enum Declared {
    /// A test that runs here.
    Runs(Ident, Option<String>),
    /// `#[ignore]`d, so no filter can make it run and naming it in one matches nothing.
    Ignored(Ident),
}

/// The test an added attribute line declares, read out of the post-image below it.
///
/// The post-image rather than the added set, because the attribute and its function are two
/// lines and only one of them has to be new: appending `#[test]` above an existing helper, or
/// adding the attribute and the signature in one hunk, both have to name the same test.
fn declared_under(lines: &[&str], code: &Code, added: &AddedLine) -> Option<Declared> {
    if !declares_a_test(added.text.trim()) {
        return None;
    }
    // `number` is 1-based, so it IS the 0-based index of the line after the attribute.
    let (index, _) = item_below(lines, added.number)?;
    let name = function_name(code, index)?;
    Some(if is_ignored(lines, index) {
        Declared::Ignored(name)
    } else {
        Declared::Runs(name, gate_at(lines, index))
    })
}

/// The attached build condition over the fn at `index` that this venue cannot evaluate, if any.
fn gate_at(lines: &[&str], index: usize) -> Option<String> {
    let mut block = attached(lines, index).into_iter().map(|(_, opening)| opening);
    block.find(|opening| decides_a_run_unevaluably(opening)).map(String::from)
}

/// [`gate_at`] for an EDITED test, which only its name locates.
// ponytail: the first fn of that name in the file; a same-named fn in a sibling module can lend its gate.
fn gate_named(lines: &[&str], code: &Code, name: &Ident) -> Option<String> {
    let index = (0..lines.len()).find(|at| function_name(code, *at).as_ref() == Some(name))?;
    gate_at(lines, index)
}

/// Does the attribute block attached to the function at `index` carry an `#[ignore]`?
///
/// `attributes::attached` owns the block, because `#[ignore]` is legal on either side of `#[test]`
/// and a WRAPPED `#[ignore = ".."]` is one attribute over several lines - the shape that used to
/// put an ignored test into the filterset and cost the loud `OnlyIgnored` pass.
/// `#[cfg_attr(.., ignore)]` is not recognised - no such spelling exists in this tree, and the
/// direction of missing one is the `no tests to run` failure this dropping exists to prevent,
/// which is loud.
///
/// `pub(super)` for `super::edited`, which asks the same question of a PRE-existing declaration -
/// one attribute, one answer, and a second copy would be a second thing to keep in step.
pub(super) fn is_ignored(lines: &[&str], index: usize) -> bool {
    attached(lines, index)
        .iter()
        .any(|(_, opening)| opening.starts_with("#[ignore"))
}

/// A file as [`function_name`] reads it: lexed ONCE over the whole text, with every comment and
/// every string literal's content blanked.
///
/// Lexing one line at a time read the interior of a multi-line `/* .. */` or string literal as code,
/// so a fixture's `fn run()` or a doc comment's `fn in` named a helper that does not exist
/// (`github.com/telekom/sutura#1270`). The fields are private and [`Code::of`] holds the only
/// `Code` literal, so outside this module no caller can hand [`function_name`] a line lexed without
/// the lines above it.
pub(super) struct Code {
    lines: Vec<String>,
    /// Per line, whether it opens in code: [`starts_in_code`].
    starts: Vec<bool>,
}

impl Code {
    /// Fails closed when the two walks disagree on the line count, as they do over a string still
    /// open at the end of the text: no line then opens in code, so no added or removed line is
    /// skipped as a comment.
    pub(super) fn of(text: &str) -> Self {
        let lines = code_lines_blanking_all_strings(text);
        let starts = starts_in_code(text);
        let starts = if starts.len() == lines.len() { starts } else { Vec::new() };
        Self { lines, starts }
    }

    /// The 0-based lines `first..=last`, joined: a search over them reads no comment or string.
    pub(super) fn span(&self, first: usize, last: usize) -> String {
        self.lines.get(first..=last).map_or_default(|lines| lines.join("\n"))
    }

    /// Every function the file declares, in line order.
    pub(super) fn functions(&self) -> impl Iterator<Item = Ident> + '_ {
        (0..self.lines.len()).filter_map(|index| function_name(self, index))
    }

    /// Is `written`, the 1-based line `number`, a `//` comment or blank, and is that what the lexer
    /// says rather than the text: a line opening inside a multi-line string is string content
    /// whatever it spells. A line opening inside a block comment counts as code, the direction that
    /// asks for proof.
    pub(super) fn is_comment_or_blank(&self, number: usize, written: &str) -> bool {
        let trimmed = written.trim();
        self.opens_in_code(number) && (trimmed.is_empty() || trimmed.starts_with("//"))
    }

    /// [`Self::is_comment_or_blank`], or an attribute: a removed line that carries no behaviour.
    pub(super) fn carries_no_behaviour(&self, number: usize, written: &str) -> bool {
        let trimmed = written.trim();
        self.is_comment_or_blank(number, written) || (self.opens_in_code(number) && trimmed.starts_with('#'))
    }

    fn opens_in_code(&self, number: usize) -> bool {
        self.starts.get(number.saturating_sub(1)) == Some(&true)
    }
}

/// The name in `fn NAME(`, if the 0-based line `index` of `code` declares a function.
///
/// One line rather than a parser, and it reaches further than "a test's signature is written on one
/// line in this tree" - which is what this said, and what `super::attributes` overstated into *a
/// wrapped signature names nothing*. The name sits before the `(`, and rustfmt breaks a signature
/// too long for one line AFTER that `(`, so the FIRST line of a wrapped `async fn very_long_...(`
/// still carries `fn <name>(` and is still named. The shapes that occur are `fn`, `async fn` and a
/// visibility in front of either. This function ITSELF never walks UP from an interior line to the
/// signature above it - it only ever reads the one line it is asked about.
///
/// **ASSERTED NOW, by the test below rather than by this paragraph**
/// (`github.com/telekom/sutura#347`): `a_signature_the_formatter_wrapped_after_the_paren_is_still_named`
/// drives a wrapped `async fn` with a return type through [`Scan::of`] and pins the name that comes
/// out.
///
/// **`github.com/telekom/sutura#1025` closed the gap that used to sit here.** A def-interior edit -
/// an added line that is not a signature's first line, most often a changed assertion in the BODY -
/// used to be refused rather than named, because [`declared_under`] only ever calls this on the item
/// BELOW an ADDED attribute. `super::edited` asks the opposite direction: from a PRE-existing
/// attribute DOWN to this same function, so a diff whose only line inside `fn <name>(..) { .. }` is
/// an edited assertion - or, since the span is the whole item, an edited parameter - is named the
/// same way an added test is. `an_added_line_inside_a_signature_this_diff_did_not_open_names_nothing`
/// pins the fixed answer now, not the old limit.
///
/// **Why the naming alone is not the whole proof.** A test named this way still has to clear the
/// same rule as an added one: it applies to anything pinning behaviour a branch did not change, and
/// AGENTS.md calls a test that passes both ways worse than none. `super::plan` puts such a file into
/// `test_files` and `causality::run`'s tests-only arm then asks for a `Claim-Cell:` declaration and a
/// killing mutation - the same proof an added test pinning existing behaviour needs.
pub(super) fn function_name(code: &Code, index: usize) -> Option<Ident> {
    let declared = code
        .lines
        .get(index)?
        .split_whitespace()
        .skip_while(|word| *word != "fn")
        .nth(1)?;
    Ident::parse(declared.split(['(', '<', ':']).next()?)
}

#[cfg(test)]
mod helper_tests;
#[cfg(test)]
mod tests;

//! The attribute vocabulary this gate reads, and what a hunk of added lines makes of the file it
//! landed in.
//!
//! TWO GATES READ IT NOW, and the second one is why [`cells`] is here rather than in
//! `crate::examples`. That gate asks *which of this file's tests does a run in this venue reach*,
//! which is the same vocabulary this module already owns, read over a whole file instead of over
//! a diff's added lines. A second copy of "what declares a test, and what makes one `#[ignore]`d"
//! is a second thing to keep true, and the paragraphs below are about what one such disagreement
//! already cost.
//!
//! ONE MODULE FOR BOTH LISTS, which is the constraint `super::scoped`'s header states: the forms
//! that decide *this file is test code* and the attributes that NAME a test have to agree, because
//! a form accepted here whose name the extractor cannot read is the empty scan, and the empty scan
//! is a refusal. They live in one file so that disagreement is visible in one place.
//!
//! FOUR ANSWERS, NOT A BOOLEAN, and the boolean is what the refusal cost. `adds_test` answered yes
//! to a bare added `#[cfg(test)]` whatever sat beneath it, so a diff adding a test-only HELPER - a
//! `#[cfg(test)] fn` with no `#[test]` in the same file's hunk - was called a test file, entered
//! the proof, and then failed to name a test that was never there:
//! *FAILED - the added tests could not be NAMED*, over printed causes neither of which was the
//! real one. **That is not fail-closed, it is stuck** - no extractor improvement can find a name in
//! an item that is not a test, so nothing the author does to their own change clears it.
//!
//! THE EDGE WAS SHARPER THAN THE FAILURE, and it is the half worth remembering: with that one file
//! out of the proof, every other test file in the diff was inseparable, `super::plan` answered
//! `NotSeparable`, and the gate PASSED. One `#[cfg(test)]` attribute on a helper was the whole
//! difference between a hard refusal and a silent pass, so a fix that only stopped the failure
//! would have converted a loud wrong answer into a quiet one.
//!
//! THE BOOLEAN CONFLATED TWO QUESTIONS, and a `#[cfg(test)]` helper answers them differently:
//!
//! | | `NamedTest` | `TestModule` | `TestOnlyItem` | `Nothing` |
//! | --- | --- | --- | --- | --- |
//! | May the reconstruction revert this file? | no | no | no | yes |
//! | Is a test it cannot name a REFUSAL? | yes | yes | **no** | - |
//!
//! The split is on that second row alone, which is what keeps
//! `scoped::tests::a_marker_whose_module_file_is_in_the_diff_names_nothing_and_refuses` intact: a
//! diff whose only test signal is `#[cfg(test)] mod tests;` still refuses, because a test MODULE
//! plausibly gained tests this extractor could not read and the fix for that is the extractor.
//!
//! WHY A `TestOnlyItem` IS HELD AND NOT REVERTED - the half a narrower `adds_test` would have lost.
//! Reverting the file takes the helper out of the base tree while the tests that call it are held
//! at HEAD: `E0425`, `DidNotCompile`, and after the retry
//! *INCONCLUSIVE - the base tree does not build* - a pass that proves nothing, over the COMMON
//! case, because a new test helper usually exists precisely because a new test needed it. Holding
//! it is also what the gate's own model says: production at base, test code at HEAD, and
//! `#[cfg(test)]` code is not production behaviour.
//!
//! WHAT HOLDS THAT, precisely, because the obvious answer is wrong and was written here.
//! `super::plan`'s inseparability filter reads `test_files.contains(..) && has_non_test_additions(..)`
//! and a [`Adds::TestOnlyItem`] file goes to `test_only`, never to `test_files` - so
//! `regions::has_non_test_additions` is never CONSULTED for one. What holds it is
//! `super::Separable::held`, which chains `test_only` in: the file is kept at HEAD on the first
//! attempt and restored with the held-back files on the second. Nothing got weaker - before this
//! change the same file was `held_back`, held at HEAD for a different reason - but a reader
//! auditing the mechanism was told a check runs that does not.
//!
//! ONE BEHAVIOURAL CONSEQUENCE OF THE SAME REROUTE, and it belongs beside it. A diff whose only
//! test-ish file adds an implementation change AND a `#[cfg(test)]` helper used to answer
//! `NOT MECHANICALLY SEPARABLE` - *state the evidence in the handoff* - and now answers
//! *no changed tests - nothing to prove*, because `test_files` is empty and `plan` returns
//! `NotRequired`. Same exit code, and the ask is gone: `no changed tests` is not true of a branch
//! that changed test code, and `NotRequired` prints neither the `test_only` list nor a ratio. It
//! is the same shape as the `-D dead-code` residual recorded on `github.com/telekom/sutura#307`,
//! and it is stated here rather than left to be found.
//!
//! WHAT IT STILL DOES NOT READ, and the first entry is not theoretical. Only the exact
//! `#[cfg(test)]` spelling counts, the same limit `super::regions` states for the same reason:
//! `#[cfg(all(test, ..))]`, `#[cfg(any(test, ..))]` and `#[cfg_attr(.., cfg(test))]` are
//! [`Adds::Nothing`] here and production code there. **One such line is in the tree** -
//! `crates/sutura-runtime/src/lib.rs` declares `pub mod testing` under
//! `#[cfg(any(test, feature = ".."))]` - so a diff ADDING that line puts its file in `revert`
//! while the module's own tests are held, which is the orphaning shape and lands as a loud
//! `NotRun` rather than a pass. The direction is right; the cause a reader would have to find is
//! this paragraph rather than their own change.
//!
//! The ITEM under an attribute is still read as ONE line, and what that costs is NARROWER than
//! this said - `github.com/telekom/sutura#319` measured the overstatement. It claimed a `fn`
//! signature the formatter had to wrap names nothing; it does not, because a name is read off the
//! FIRST signature line and rustfmt breaks a long signature after the `(`, never before the name.
//! **Both halves are tests rather than this paragraph** since
//! `github.com/telekom/sutura#347`, and they are in `super::scoped`'s module because `Scan::of`
//! is what drives the extractor: one adds `#[tokio::test]` over a wrapped `async fn` with a return
//! type and pins the name, the other adds only that signature's PARAMETER line and pins that no
//! name comes out. What names nothing is a
//! **body-only or def-interior edit**: added lines inside an existing test, or inside a signature
//! whose first line this diff did not touch. The ATTRIBUTE is not read as one line -
//! [`item_below`] balances its brackets, because reading one as a single line was a defect and
//! that module's own doc carries the measurement.
//! And nothing here is keyed on a file's extension - `super::plan` decides that, and a test whose
//! subject is not Rust at all is outside this module and outside the gate.

use core::ops::Range;

use crate::causality::regions::{AddedLine, PostImage, attribute_end, item_end, item_head};

/// Attributes that mark the function below them as a test.
///
/// Deliberately syntactic and deliberately generous: a false positive costs a slower gate, a
/// false negative lets a vacuous test through, so the bias goes one way on purpose. Written
/// without the closing `]` where an argument list is legal, so `#[tokio::test(flavor = "..")]`
/// is recognised too.
const DECLARES_A_TEST: &[&str] = &["#[test]", "#[tokio::test", "#[rstest", "#[test_case"];

/// What a diff hunk's added lines make of the file they landed in.
///
/// Ordered, and the order IS the precedence: [`adds`] takes the strongest thing any one added line
/// says, because a hunk adding both a helper and a `#[test]` is a test file whatever else is in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Adds {
    /// Nothing this gate reads as test code. The reconstruction reverts the file, and that is
    /// what makes it the base behaviour a test can be red against.
    Nothing,
    /// `#[cfg(test)]` on an item that is not a `mod`: a helper `fn`, a `use`, a `const`, an
    /// `impl`. Test code that names no test and never can, so requiring a name is the refusal
    /// this module's header records.
    TestOnlyItem,
    /// A test module arrives here - `#[cfg(test)] mod ..`, or a new `mod tests {`. It names no
    /// function itself, and the file may not be reverted or the module's own file is orphaned.
    TestModule,
    /// An attribute that names the test below it. The file is what the proof measures.
    NamedTest,
}

/// What `added` makes of the file at `path`, reading its post-image through `read`.
pub(crate) fn adds(added: &[AddedLine], path: &str, read: &PostImage<'_>) -> Adds {
    let text = read(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    added.iter().map(|line| says(line, &lines)).max().unwrap_or(Adds::Nothing)
}

/// Is this line one of the attributes that names the test below it?
pub(super) fn declares_a_test(trimmed: &str) -> bool {
    DECLARES_A_TEST.iter().any(|attribute| trimmed.starts_with(attribute))
}

/// The item an attribute applies to, searching down from 0-based `from`: its index in `lines`
/// and its trimmed text.
///
/// Blank lines, comments and further attributes legitimately sit between an attribute and its
/// item, so they are skipped and anything else is the item - which stops a stray attribute from
/// reaching down the file and naming something unrelated.
///
/// AN ATTRIBUTE IS SKIPPED AS A WHOLE, over however many lines the formatter wrapped it across,
/// and reading one as a single line was a defect rather than a stated limit. A continuation line
/// starts with none of the three, so the search stopped on `clippy::disallowed_methods,` and
/// `super::scoped::function_name` was handed that instead of a signature: no name came out, and
/// after `Scan::Unreadable` began refusing ahead of `Runnable` that became a hard red on a
/// spelling this tree wrote ten times. `super::regions::attribute_end` is the lexer, shared with
/// the one that finds where a `{ .. }` item ends.
///
/// `None` when the file runs out, and `None` for an attribute whose brackets never balance: an
/// unclosed attribute is an error rather than an answer, and both callers read that as the
/// fail-closed direction their own doc states.
///
/// Deliberately not `super::regions::carries_no_behaviour`, whose three clauses look the same: it
/// answers *does this ADDED line change what the code does*, which is a question about a diff, and
/// this answers *what item does this attribute apply to*, which is a question about Rust's
/// grammar. Only one of them should follow `#![..]` inner attributes.
pub(super) fn item_below<'l>(lines: &[&'l str], from: usize) -> Option<(usize, &'l str)> {
    let mut index = from;
    loop {
        let trimmed = lines.get(index)?.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            index += 1;
        } else if trimmed.starts_with("#[") {
            index = attribute_end(lines, index)? + 1;
        } else {
            return Some((index, trimmed));
        }
    }
}

/// The attributes attached to the item at 0-based `index`, as their 0-based line and their
/// opening line, trimmed.
///
/// THE LINE COMES OUT WITH THE TEXT because the block's FIRST line is where the item's region
/// starts, and reading the region from the DECLARING attribute instead was a measured fail-open:
/// `#[ignore]` is legal above `#[test]`, and a reach written on the `#[ignore = ".."]` line itself
/// then sat one line above the region and counted as evidence -
/// `ok .. multi-player: reached from 1 file - ../multi_player.rs:88`, exit 0, where line 88 was
/// the `#[ignore]` (`github.com/telekom/sutura#400`). It is rustfmt-stable, so nothing else moves
/// it.
///
/// Read TOP-DOWN over the file rather than upward from the item, which is the change: `#[ignore]`
/// is legal on either side of `#[test]`, so both sides have to be seen - and an upward walk that
/// stops on the first line not starting with `#[` stops on a wrapped attribute's continuation
/// line, which is how a wrapped `#[ignore = ".."]` reached the filterset as a runnable test. The
/// two shapes in this tree are exactly that.
///
/// A BLANK LINE ENDS A BLOCK, so an attribute belonging to an earlier item cannot be borrowed -
/// and so does an ITEM, which the upward walk got for free and this has to state: two `#[test]`
/// functions with no blank line between them must not share the first one's `#[ignore]`.
/// A `//` COMMENT DOES NOT END A BLOCK - a doc comment between an attribute and its item is
/// ordinary here - AND NO OTHER SPELLING IS TRANSPARENT. A `/* .. */` line reaches the `else`
/// below and CLEARS the block, so an `#[ignore]` above one is dropped while the `#[test]`
/// under it stays: the block comes back NON-EMPTY and wrong, which is a different failure
/// from the empty one and invisible to a caller that only asks whether the declaration is
/// in it. [`cells`] refuses on that as well, by asking the blanked image where a comment
/// has already gone rather than teaching this walk a second comment lexer.
///
/// THAT RULE IS RIGHT HERE AND DISAGREES WITH [`item_below`], WHICH IS THE CALLER'S PROBLEM AND NOT
/// THIS FUNCTION'S. The question `super::scoped` asks is *what is attached to this item*, and for
/// that a blank line is a boundary. [`item_below`] SKIPS a blank line on its way to the item, so a
/// caller pairing the two gets a resolved item and an EMPTY block from one blank line - which reads
/// as a cell with no attributes at all. [`cells`] refuses on exactly that, by requiring the block to
/// contain the declaration it is standing on; `super::scoped::is_ignored` does not, because there a
/// missed `#[ignore]` puts the test in a filterset and nextest exits 4 with `no tests to run`, which
/// is loud. Two callers, one lexer, two directions of failure - stated here because the next caller
/// has to pick one.
///
/// Empty for an attribute that never closes, which is [`attribute_end`]'s refusal reaching this
/// far: nothing is claimed about a file that does not compile.
pub(super) fn attached<'l>(lines: &[&'l str], index: usize) -> Vec<(usize, &'l str)> {
    let mut block: Vec<(usize, &'l str)> = Vec::new();
    let mut cursor = 0_usize;
    while cursor < index {
        let Some(trimmed) = lines.get(cursor).map(|line| line.trim()) else {
            break;
        };
        if trimmed.starts_with("//") {
            cursor += 1;
        } else if trimmed.starts_with("#[") {
            block.push((cursor, trimmed));
            match attribute_end(lines, cursor) {
                Some(last) => cursor = last + 1,
                None => return Vec::new(),
            }
        } else {
            // A blank line or an item of its own: whatever preceded it is not attached to
            // `index`.
            block.clear();
            cursor += 1;
        }
    }
    block
}

/// The tests a file declares, split by whether a run in this venue reaches them.
///
/// [`Adds`] above answers *what did this DIFF add*; this answers *what does this FILE declare*,
/// which is a different question over the same vocabulary and is why it lives beside it rather
/// than in a second list somewhere else. `super::scoped` already resolves an added attribute to
/// the test below it and already separates `Declared::Runs` from `Declared::Ignored`; what this
/// adds is the resolution in the other direction - from a LINE to the test containing it - which
/// is what a gate scanning whole files needs and what `crate::examples` had no way to ask.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Cells {
    /// 1-based, half-open line ranges, one per test no run in this venue reaches: its attribute
    /// BLOCK through the last line of the function under it.
    unreached: Vec<Range<usize>>,
    /// Tests declared here that a run in this venue reaches.
    runs: usize,
    /// Cells whose run is decided by an attribute this cannot evaluate, with the spelling.
    ///
    /// STATED AND NOT EVIDENCE, rather than refused, and the difference was measured. Routing
    /// these to [`Self::unresolved`] - which is what `github.com/telekom/sutura#400`'s remedy 3
    /// asks for - turns `crate::examples` RED on the healthy tree: eight cells in
    /// `crates/sutura-cli/src/sources/bigquery.rs` and `crates/sutura-serve/src/tests.rs` are
    /// written under `#[cfg(feature = "bigquery")]` and `#[cfg(not(feature = "bigquery"))]`, and
    /// the remedy for a legitimate feature-gated test cannot be to delete it. So the answer is the
    /// one that is fail-closed for the CLAIM instead of for the file: the cell is not a run this
    /// venue reaches, so its lines are not evidence, and a variant whose only reach sits in one
    /// fails on the variant.
    undecidable: Vec<String>,
    /// Tests this cannot say run or not, each with the reason: no item under the declaring
    /// attribute, a block that does not reach the item the walk resolved, or an attribute a
    /// comment pushed out of that block. All three mean nothing may be claimed about the
    /// lines around them, and none of them is `Self::undecidable`, which is a cell this CAN
    /// see and whose RUN is what it cannot decide.
    ///
    /// The reason travels with the entry because the two ask for opposite things - one is fixed in
    /// this scanner and one in the tree - and a caller printing "could not resolve the item" over
    /// a `#[cfg(feature = "..")]` sends the reader to the wrong half.
    unresolved: Vec<String>,
}

impl Cells {
    /// Is the line at 1-based `number` inside a test no run here reaches?
    pub(crate) fn unreached(&self, number: usize) -> bool {
        self.unreached.iter().any(|region| region.contains(&number))
    }

    /// Does this file declare tests of which NONE runs here?
    ///
    /// The coarser of the two answers, and it is needed BESIDE [`Self::ignores`] rather than
    /// instead of it, because the two catch different halves and this repository's own evidence
    /// is the sharp case: the only line reaching `examples/multi-player` sits in a HELPER the
    /// tests call, not in a test body, so a per-line rule alone leaves `#[ignore]` on every test
    /// in that file with the reach still standing. A helper in a file where nothing runs is
    /// unreachable from this venue THROUGH THAT FILE, which is what this says.
    pub(crate) const fn nothing_runs(&self) -> bool {
        self.runs == 0 && !self.unreached.is_empty()
    }

    /// Tests declared here that a run reaches - the number a caller states beside its own count.
    pub(crate) const fn runs(&self) -> usize {
        self.runs
    }

    /// Test-declaring attributes whose item could not be resolved, each with the spelling. A
    /// non-empty list is a broken scan, not a clean file: a caller that cannot see the test cannot
    /// say whether it runs.
    pub(crate) fn unresolved(&self) -> &[String] {
        &self.unresolved
    }

    /// Cells gated by an attribute this cannot evaluate - see [`Self::undecidable`]'s field doc for
    /// why these are stated rather than refused.
    pub(crate) fn undecidable(&self) -> &[String] {
        &self.undecidable
    }
}

/// Attributes that decide whether the test below them runs and that this cannot evaluate.
///
/// `#[cfg(..)]` selects the item in or out per build, and `#[cfg_attr(.., ignore)]` selects the
/// `#[ignore]` in or out, so either one over a cell makes "does a run here reach it" a question
/// about features, targets and flags this scanner does not have. Exactly `#[cfg(test)]` is
/// EVALUABLE and true - a test target is compiled with it - so it is not here.
///
/// Measured on merged `main` before this existed (`github.com/telekom/sutura#400`):
/// `#[cfg(feature = "..")]` on both cells of the one file reaching a variant, and
/// `#[cfg_attr(all(), ignore)]` re-measured independently, each left the verdict BYTE-IDENTICAL to
/// the healthy one at exit 0. `super::scoped` tolerates the same gap for a reason that does not
/// transfer: there a missed `#[ignore]` is a loud `no tests to run`, and in `crate::examples` it
/// is a silent exit 0.
fn decides_a_run_unevaluably(opening: &str) -> bool {
    (opening.starts_with("#[cfg(") && opening != "#[cfg(test)]") || opening.starts_with("#[cfg_attr(")
}

/// The tests `text` declares, and which of them no run in this venue reaches.
///
/// TWO IMAGES OF ONE FILE, and both are load-bearing. `text` is the file as written, because
/// [`attached`]'s contract is over that: it SKIPS comment lines, so a doc comment between
/// `#[test]` and its `fn` leaves the attribute block intact. `code` is the same file with its
/// comments and multi-line string interiors blanked out
/// ([`code_lines`](crate::serde_parse::scan::code_lines)), and an attribute counts as a
/// declaration only if it survives there - so a `#[test]` inside a `/* .. */` block or a raw
/// string is not one. Reading only the written image would count a commented-out test as running
/// and let a file past [`Cells::nothing_runs`]; reading only the blanked one turns every
/// comment into a blank line, which ENDS an attribute block and would lose the `#[ignore]` of any
/// test with a doc comment under its attributes.
///
/// `#[cfg(..)]` AND `#[cfg_attr(..)]` OVER A CELL ARE A REFUSAL, not a cell that runs - see
/// [`decides_a_run_unevaluably`] for what each was measured to do before that. What that does NOT
/// reach is the same attribute over something CONTAINING the cell: a `#[cfg(unix)] mod tests {`
/// or a `#[cfg(feature = "..")] impl` holds tests whose own block is clean, and this reads them as
/// running. Live in this tree rather than theoretical -
/// `git grep -n '#\[cfg(' -- 'crates/**/*.rs'` beside `git grep -n '#\[test\]'` finds the shapes,
/// and `crates/sutura-serve/tests/served.rs` gates a whole `mod tests` on `#[cfg(unix)]`. Held by
/// review, and stated because the arm above reads stronger than it is: closing it needs the cfg
/// item's brace range, which is `regions::item_end`'s instrument aimed one level out.
pub(crate) fn cells(text: &str, code: &str) -> Cells {
    let lines: Vec<&str> = text.lines().collect();
    let blanked: Vec<&str> = code.lines().collect();
    let mut found = Cells::default();
    for index in 0..lines.len() {
        let written = lines.get(index).map(|line| line.trim()).unwrap_or_default();
        let visible = blanked.get(index).map(|line| line.trim()).unwrap_or_default();
        if !declares_a_test(written) || !declares_a_test(visible) {
            continue;
        }
        // FROM THE ATTRIBUTE'S OWN LINE, not one below it, and the difference is a defect this
        // gate would have shipped. `item_below`'s `#[` arm skips an attribute AS A WHOLE through
        // `attribute_end`, which is the reason that function exists; starting one line down hands
        // it a WRAPPED attribute's continuation line - `flavor = "multi_thread"` - as the item,
        // and `attached` then reads the block above THAT line and never sees the `#[ignore]`
        // below it. Measured on this tree: one cell written as a wrapped `#[tokio::test(..)]`
        // plus `#[ignore]` and the other as an ordinary `#[test]` plus `#[ignore]` left the
        // verdict at `0 file(s) dropped` and exit 0 - every cell ignored, both of this gate's
        // levels defeated at once, which is the exact state it exists to redden. Latent rather
        // than live only because no wrapped test-declaring attribute is written in this tree
        // today, and "no such spelling exists yet" is not a mechanism.
        let Some((at, _)) = item_below(&lines, index) else {
            // FAIL CLOSED at the caller: an attribute that declares a test and whose item this
            // cannot reach says nothing about whether the test runs, and a silent zero there is
            // the whole failure mode this resolution exists to remove.
            found
                .unresolved
                .push(format!("no item under `{written}` at line {}", index.saturating_add(1)));
            continue;
        };
        let block = attached(&lines, at);
        if !block.iter().any(|(line, _)| *line == index) {
            // FAIL CLOSED ON THE PAIR DISAGREEING, which is a FIFTH door into this failure mode and
            // the one none of the four fixes above reaches. The single resolver is two halves that
            // read a BLANK LINE differently: [`item_below`] SKIPS one on its way to the item, and
            // [`attached`] CLEARS the block on one - correctly, for the upward question
            // `super::scoped` asks it. Paired, one blank line between the attribute block and the
            // `fn` resolves the item AND returns an empty block, so no `#[ignore]`, no `#[cfg]`, and
            // the cell counted as a run. Measured on `110591d5` and on this branch with every fix
            // present: a reach in the BODY of an `#[ignore]`d cell, one blank line below its
            // attributes, printed `reached from 1 file - ..multi_player.rs:120` at exit 0, with the
            // per-line rule, the block-anchored region and the per-file floor all defeated at once
            // and `cargo xtask hygiene: ok - 33 gate(s)` over the same tree. `rustfmt --check` exits
            // 0 on it, so nothing else moves it, and it is not over-determined.
            //
            // The test is exact rather than a heuristic: a block that does not CONTAIN the
            // declaration this loop is standing on is not that cell's block, whatever the cause -
            // the blank line, or `attribute_end` refusing an attribute that never closes, which
            // also comes back empty. Zero occurrences in this tree today, and that is deliberately
            // not the argument: this module's own doc calls "no such spelling exists yet" not a
            // mechanism, so it may not be one here either.
            found.unresolved.push(format!(
                "the attribute block of `{written}` at line {} does not reach its item",
                index.saturating_add(1)
            ));
            continue;
        }

        // AND IT MUST BE THE WHOLE BLOCK, which is the same door one comment token further
        // out and the one the guard above cannot see: it asks whether the block CONTAINS the
        // declaration, and this shape gives a block that contains it and is MISSING an
        // attribute. [`attached`] is transparent to a `//` comment and to nothing else, so a
        // `/* .. */` line between an `#[ignore]` and its `#[test]` CLEARS the block: the
        // `#[ignore]` goes, the `#[test]` stays, and the cell counted as a run. Measured on
        // `110591d5` and on the commit that closed the blank line, with the reach in the BODY
        // of the `#[ignore]`d cell and the other cell left running so the per-file floor
        // could not fire: `reached from 1 file - ..multi_player.rs:120` at exit 0, with
        // `rustfmt --edition 2024 --check` exit 0, `xtask fmt --check` exit 0 and
        // `xtask hygiene: ok - 33 gate(s)` over the same tree, and `rustc --test` reporting
        // that same cell as `ignored`. Not over-determined, and rustfmt-stable in the
        // single-line spelling - it DELETES a blank line between two attributes but keeps a
        // comment, which is why this spelling is the sharp one.
        //
        // The blanked image is where a comment has already gone, so this asks that rather
        // than teaching the walk `super::scoped` shares a second comment lexer: a line blank
        // THERE and non-blank HERE is a comment, whatever its spelling.
        let top = block.first().map_or(index, |(line, _)| *line);
        let mut above = top;
        while above > 0
            && blanked
                .get(above.saturating_sub(1))
                .is_some_and(|line| line.trim().is_empty())
            && lines.get(above.saturating_sub(1)).is_some_and(|line| !line.trim().is_empty())
        {
            above = above.saturating_sub(1);
        }
        if above > 0
            && lines
                .get(above.saturating_sub(1))
                .is_some_and(|line| line.trim().starts_with("#["))
        {
            found.unresolved.push(format!(
                "an attribute above a comment is outside the block of `{written}` at line {}",
                index.saturating_add(1)
            ));
            continue;
        }
        let gated = block
            .iter()
            .find(|(_, opening)| decides_a_run_unevaluably(opening))
            .map(|(_, opening)| *opening);
        if let Some(opening) = gated {
            // NOT A RUN THIS VENUE REACHES, which is the fail-closed answer to the question the
            // caller asks. See the field doc for why this is not the `unresolved` refusal.
            found
                .undecidable
                .push(format!("`{opening}` over the test at line {}", at.saturating_add(1)));
        }
        if gated.is_some() || block.iter().any(|(_, opening)| opening.starts_with("#[ignore")) {
            // FROM THE BLOCK'S FIRST LINE, not from the declaring attribute: `#[ignore]` is legal
            // ABOVE `#[test]`, and a region starting at the declaration left the `#[ignore]` line
            // itself outside it - so a reach written in the ignore's own reason string counted as
            // evidence, measured at exit 0 on merged `main`. The refusal above is what makes this
            // safe without a `min`: the block CONTAINS `index` and is filled in ascending line
            // order, so its first line is at or above the declaration and can never move the region
            // DOWN. An empty block is a refusal now rather than a silent fallback.
            let start = block.first().map_or(index, |(line, _)| *line).saturating_add(1);
            found.unreached.push(start..item_end(&lines, at).saturating_add(2));
        } else {
            found.runs = found.runs.saturating_add(1);
        }
    }
    found
}

/// The strongest thing ONE added line says about its file.
fn says(added: &AddedLine, lines: &[&str]) -> Adds {
    let trimmed = added.text.trim();
    if declares_a_test(trimmed) {
        return Adds::NamedTest;
    }
    // A new inline `mod tests {`. Its body arrives in the same hunk, so every test inside is an
    // added line of its own and this form only has to keep the file from being reverted.
    if trimmed.starts_with("mod tests") && trimmed.contains('{') {
        return Adds::TestModule;
    }
    let Some(rest) = trimmed.strip_prefix("#[cfg(test)]") else {
        return Adds::Nothing;
    };
    // The item the attribute applies to: on the attribute's own line - `#[cfg(test)] mod tests;`
    // is one line in this tree - or below it. Below is read from the POST-IMAGE for the reason
    // `super::scoped::declared_under` reads it there: the attribute and its item are two lines and
    // only the attribute has to be new.
    //
    // An item this cannot SEE is taken to be a module, which is the fail-closed direction: a
    // module is the answer under which a test it cannot name stays a refusal.
    // `number` is 1-based, so it IS the 0-based index of the line after the attribute.
    let module = if rest.trim().is_empty() {
        item_below(lines, added.number).is_none_or(|(_, item)| is_module_item(item))
    } else {
        is_module_item(rest)
    };
    if module { Adds::TestModule } else { Adds::TestOnlyItem }
}

/// Does this line declare a `mod`, whatever visibility or attribute prefixes it carries?
fn is_module_item(line: &str) -> bool {
    item_head(line).starts_with("mod ")
}

#[cfg(test)]
mod tests {
    use super::{Adds, Cells, adds, attached, cells};
    use crate::causality::fixtures::{added_from, tree};
    use crate::causality::regions::AddedLine;
    use crate::serde_parse::scan::code_lines;

    /// What `text` declares, read through the two images the gate reads it through.
    fn of_file(text: &str) -> Cells {
        cells(text, &code_lines(text).join("\n"))
    }

    #[test]
    fn a_test_is_ignored_with_the_attribute_on_either_side_of_it() {
        // `#[ignore]` is legal above or below `#[test]`, and both spellings are in this tree.
        for source in [
            "#[test]\n#[ignore]\nfn t() {\n    let p = 1;\n}\n",
            "#[ignore]\n#[test]\nfn t() {\n    let p = 1;\n}\n",
            "#[ignore = \"a reason the formatter\n    wrapped\"]\n#[test]\nfn t() {\n    let p = 1;\n}\n",
        ] {
            let found = of_file(source);
            assert!(found.nothing_runs(), "{source}");
            assert_eq!(found.runs(), 0, "{source}");
            assert!(found.unresolved().is_empty(), "{source}");
        }
        let runs = of_file("#[test]\nfn t() {\n    let p = 1;\n}\n");
        assert!(!runs.nothing_runs());
        assert_eq!(runs.runs(), 1);
    }

    #[test]
    fn a_wrapped_test_declaring_attribute_still_finds_its_item_and_its_ignore() {
        // THE SHAPE THAT DEFEATED BOTH LEVELS AT ONCE. Resolving the item from one line BELOW the
        // attribute hands a wrapped one its own continuation line as the item, and the block
        // `attached` then reads is the one above THAT line - so the `#[ignore]` underneath is
        // invisible and the cell counts as running. Measured through the gate before the fix:
        // one cell wrapped and both `#[ignore]`d left `0 file(s) dropped` and exit 0.
        for source in [
            "#[tokio::test(\n    flavor = \"multi_thread\"\n)]\n#[ignore = \"needs a broker\"]\nfn t() {\n    let p = 1;\n}\n",
            "#[test_case(\n    1,\n    2\n)]\n#[ignore]\nfn t() {\n    let p = 1;\n}\n",
            "#[rstest(\n    case(1)\n)]\n#[ignore]\nfn t() {\n    let p = 1;\n}\n",
        ] {
            let found = of_file(source);
            assert!(found.nothing_runs(), "{source}");
            assert_eq!(found.runs(), 0, "{source}");
            assert!(found.unresolved().is_empty(), "{source}");
            assert!(found.unreached(5), "a line inside the wrapped, ignored cell: {source}");
        }
        // The wrapped attribute over a cell that RUNS is still named, so the fix is the item
        // resolution rather than a rule that wrapped attributes are ignored.
        let runs = of_file("#[tokio::test(\n    flavor = \"multi_thread\"\n)]\nfn t() {\n    let p = 1;\n}\n");
        assert!(!runs.nothing_runs());
        assert_eq!(runs.runs(), 1);
    }

    #[test]
    fn the_ignored_region_is_the_test_and_stops_at_its_closing_brace() {
        let source = "#[test]\n#[ignore]\nfn t() {\n    let p = 1;\n}\n#[test]\nfn u() {\n    let q = 2;\n}\n";
        let found = of_file(source);
        assert!(found.unreached(1), "the declaring attribute");
        assert!(found.unreached(4), "the body of the ignored test");
        assert!(found.unreached(5), "its closing brace");
        assert!(!found.unreached(8), "the body of the one that runs");
        assert_eq!(found.runs(), 1, "and the neighbour is still counted");
        assert!(!found.nothing_runs());
    }

    #[test]
    fn the_ignored_region_starts_at_the_attribute_block_rather_than_at_the_declaration() {
        // THE FAIL-OPEN #400 MEASURED ON MERGED `main`: `#[ignore]` is legal ABOVE `#[test]`, and a
        // region starting at the DECLARING attribute left the `#[ignore = ".."]` line one line
        // above itself - so a path named in the ignore's own reason string was read as a reach from
        // a running test. `reached from 1 file - ../multi_player.rs:88`, exit 0, where line 88 was
        // the `#[ignore]`. Rustfmt-stable, so nothing else moved it.
        let source = "#[ignore = \"needs ../../examples/x/q.json\"]\n#[test]\nfn t() {\n    let p = 1;\n}\n#[test]\nfn u() {}\n";
        let found = of_file(source);
        assert!(found.unreached(1), "the `#[ignore]` line itself: {found:?}");
        assert!(found.unreached(2), "the declaring attribute below it");
        assert!(found.unreached(4), "the body");
        assert!(!found.unreached(7), "and the cell that runs is untouched");
        assert_eq!(found.runs(), 1, "{found:?}");
        // The block's first line, not merely one line up: an ordinary attribute above the pair is
        // part of the same cell and a reach written on it is inside the region too.
        let with_expect =
            "#[expect(clippy::x, reason = \"../../examples/x/q.json\")]\n#[ignore]\n#[test]\nfn t() {}\n#[test]\nfn u() {}\n";
        let block = of_file(with_expect);
        assert!(block.unreached(1), "the first line of the block: {block:?}");
    }

    #[test]
    fn a_run_deciding_attribute_this_cannot_evaluate_is_unresolved_rather_than_running() {
        // #400's third fail-open: `#[cfg(feature = "..")]` on both cells of the one file reaching a
        // variant left the caller's verdict BYTE-IDENTICAL to the healthy one at exit 0, and
        // `#[cfg_attr(all(), ignore)]` did the same. Neither is evaluable here, so neither may be
        // read as a cell that runs.
        for source in [
            "#[cfg(feature = \"bigquery\")]\n#[test]\nfn t() {}\n",
            "#[test]\n#[cfg(unix)]\nfn t() {}\n",
            "#[cfg_attr(all(), ignore)]\n#[test]\nfn t() {}\n",
            "#[test]\n#[cfg_attr(target_os = \"linux\", ignore)]\nfn t() {}\n",
        ] {
            let found = of_file(source);
            assert_eq!(found.runs(), 0, "{source}");
            assert_eq!(found.undecidable().len(), 1, "{source} -> {found:?}");
            assert!(
                found
                    .undecidable()
                    .first()
                    .is_some_and(|entry| entry.contains("over the test at line")),
                "{source} -> {found:?}"
            );
            assert!(found.unreached(2), "its lines are not evidence: {source} -> {found:?}");
            assert!(
                found.nothing_runs(),
                "and a file whose every cell is gated reaches the file-level floor: {source}"
            );
            assert!(found.unresolved().is_empty(), "not the broken-scanner refusal: {found:?}");
        }
        // Exactly `#[cfg(test)]` IS evaluable and true - a test target is compiled with it - so it
        // is not gated, or every unit test in the workspace would be.
        let ordinary = of_file("#[cfg(test)]\n#[test]\nfn t() {}\n");
        assert_eq!(ordinary.runs(), 1, "{ordinary:?}");
        assert!(ordinary.undecidable().is_empty(), "{ordinary:?}");
    }

    #[test]
    fn a_declaration_the_lexer_blanked_is_not_one() {
        // A `#[test]`-shaped line inside a block comment or a raw string is not a test. Read over
        // the written image alone it is, which puts a file whose real cells are all `#[ignore]`d
        // back on the passing side.
        let commented = of_file("/*\n#[test]\nfn dead() {}\n*/\n#[test]\n#[ignore]\nfn t() {}\n");
        assert!(commented.nothing_runs(), "{commented:?}");
        assert_eq!(commented.runs(), 0, "{commented:?}");
    }

    #[test]
    fn a_doc_comment_between_the_attributes_and_the_item_does_not_detach_them() {
        // The other direction, and why the WRITTEN image is read for the block: over the blanked
        // one a comment is a blank line, a blank line ends an attribute block, and the `#[ignore]`
        // would be lost.
        let found = of_file("#[test]\n#[ignore]\n/// What this would prove.\nfn t() {}\n");
        assert!(found.nothing_runs(), "{found:?}");
    }

    #[test]
    fn a_blank_line_between_the_attributes_and_the_item_is_unresolved_rather_than_running() {
        // THE FIFTH DOOR, and the one none of the four region fixes reaches: the two halves of this
        // one resolver read a blank line differently. `item_below` SKIPS it and reaches the `fn`;
        // `attached` CLEARS the block on it and comes back empty. Paired, that is a cell with no
        // attributes - so no `#[ignore]`, no `#[cfg]`, and `runs += 1`. Measured through
        // `crate::examples` on `110591d5` and with every other fix present: a reach in the BODY of
        // an `#[ignore]`d cell one blank line below its attributes printed
        // `reached from 1 file - ..multi_player.rs:120` at exit 0, `rustfmt --check` clean.
        for source in [
            "#[test]\n#[ignore]\n\nfn t() {\n    let p = 1;\n}\n",
            "#[ignore]\n#[test]\n\nfn t() {\n    let p = 1;\n}\n",
            "#[cfg(feature = \"x\")]\n#[test]\n\nfn t() {\n    let p = 1;\n}\n",
            // Several blank lines, and one with only whitespace on it - `trim` sees both.
            "#[test]\n#[ignore]\n\n   \n\nfn t() {}\n",
            // A plain `#[test]` too: the refusal is about the PAIR disagreeing, not about `#[ignore]`.
            "#[test]\n\nfn t() {}\n",
            // THE SECOND CAUSE of an empty block, and it needs no blank line: an attribute that
            // never closes EARLIER in the file makes `attribute_end` refuse, `attached` returns
            // empty for every item below it, and the cell read as running.
            "#[expect(\n    clippy::x,\n#[test]\n#[ignore]\nfn t() {}\n",
        ] {
            let found = of_file(source);
            assert_eq!(found.runs(), 0, "{source} -> {found:?}");
            assert_eq!(found.unresolved().len(), 1, "{source} -> {found:?}");
            assert!(
                found
                    .unresolved()
                    .first()
                    .is_some_and(|entry| entry.contains("does not reach its item")),
                "{source} -> {found:?}"
            );
        }
        // AND THE HEALTHY SHAPES MUST NOT MOVE, or the refusal is a false red on every file in the
        // tree: the two line classes the halves AGREE on are a comment and a further attribute.
        for source in [
            "#[test]\n#[ignore]\nfn t() {}\n",
            "#[test]\n#[ignore]\n/// What this would prove.\nfn t() {}\n",
            "#[test]\n#[ignore]\n// a line comment\n/* and a block one */\nfn t() {}\n",
            "#[test]\n#[ignore]\n#[expect(clippy::x, reason = \"..\")]\nfn t() {}\n",
        ] {
            let found = of_file(source);
            assert!(found.unresolved().is_empty(), "{source} -> {found:?}");
            assert!(found.nothing_runs(), "{source} -> {found:?}");
        }
    }

    #[test]
    fn an_attribute_a_comment_pushed_out_of_the_block_is_unresolved_rather_than_running() {
        // THE SAME DOOR ONE COMMENT TOKEN FURTHER OUT, and the guard above is blind to it by
        // construction: it asks whether the block CONTAINS the declaration, and here the
        // block contains the declaration and is MISSING an attribute. `attached` skips a
        // `//` line and clears on every other, so a `/* .. */` between an `#[ignore]` and its
        // `#[test]` deletes the `#[ignore]` while leaving the cell resolvable. Measured
        // through `crate::examples` on `110591d5` and on the commit that closed the blank
        // line: the reach in the BODY of the `#[ignore]`d cell, the other cell left running
        // so the per-file floor could not fire, printed
        // `multi-player: reached from 1 file - ..multi_player.rs:120` at exit 0 with
        // `rustfmt --edition 2024 --check` exit 0, `xtask fmt --check` exit 0 and
        // `xtask hygiene: ok - 33 gate(s)` over the same tree - and `rustc --test` reports
        // that cell as `ignored`, so the published reach really is inside a test no run
        // here reaches.
        for source in [
            "#[ignore]\n/* held */\n#[test]\nfn t() {\n    let p = 1;\n}\n",
            "#[cfg(feature = \"x\")]\n/* held */\n#[test]\nfn t() {\n    let p = 1;\n}\n",
            // Several comment lines, and a block comment spanning them: the blanked image
            // has lost all of it, so the walk up reaches the attribute either way.
            "#[ignore]\n/* held\n   over two lines */\n#[test]\nfn t() {}\n",
            "#[ignore]\n// a line comment\n/* and a block one */\n#[test]\nfn t() {}\n",
        ] {
            let found = of_file(source);
            assert_eq!(found.runs(), 0, "{source} -> {found:?}");
            assert_eq!(found.unresolved().len(), 1, "{source} -> {found:?}");
            assert!(
                found
                    .unresolved()
                    .first()
                    .is_some_and(|entry| entry.contains("outside the block")),
                "{source} -> {found:?}"
            );
        }
        // AND THE HEALTHY SHAPES MUST NOT MOVE, or this is a false red on every file that
        // writes a comment near a test: a comment BELOW the block is where `item_below`
        // answers and the block above it is whole, and a `//` or `///` line inside the block
        // is skipped rather than cleared, so neither is an attribute pushed out of anything.
        for source in [
            "#[test]\n#[ignore]\n/* and a block one */\nfn t() {}\n",
            "#[ignore]\n// held\n#[test]\nfn t() {}\n",
            "#[ignore]\n/// held\n#[test]\nfn t() {}\n",
        ] {
            let found = of_file(source);
            assert!(found.unresolved().is_empty(), "{source} -> {found:?}");
            assert!(found.nothing_runs(), "{source} -> {found:?}");
        }
    }

    #[test]
    fn a_declaration_with_no_item_under_it_is_unresolved_rather_than_running() {
        // A caller that cannot see the test cannot say whether it runs, and saying it runs is the
        // fail-open direction.
        let found = of_file("fn a() {}\n\n#[test]\n");
        assert_eq!(found.unresolved().len(), 1, "{found:?}");
        assert_eq!(found.runs(), 0, "{found:?}");
        assert!(!found.nothing_runs(), "an unresolved declaration is not an ignored one");
    }

    #[test]
    fn a_file_that_declares_no_test_declares_no_ignored_one_either() {
        // A helper module under `tests/` is neither running nor ignored, and the floor must not
        // fire on it.
        let found = of_file("pub fn corpus() -> &'static str {\n    \"../../examples/x\"\n}\n");
        assert!(!found.nothing_runs());
        assert_eq!(found.runs(), 0);
        assert!(
            found.unresolved().is_empty(),
            "no declarations are unresolved in a file that declares no tests"
        );
    }

    /// What one added line, at line 1 of a file whose post-image is `text`, makes of that file.
    fn of(line: &str, text: &str) -> Adds {
        let read = tree(&[("crates/x/src/a.rs", text)]);
        adds(&[AddedLine::new(1, line)], "crates/x/src/a.rs", &read)
    }

    #[test]
    fn an_attribute_that_names_a_test_is_a_named_test() {
        assert_eq!(of("    #[test]", "    #[test]\n    fn t() {}\n"), Adds::NamedTest);
        assert_eq!(of("#[tokio::test]", "#[tokio::test]\nasync fn t() {}\n"), Adds::NamedTest);
        assert_eq!(of("fn thing() {}", "fn thing() {}\n"), Adds::Nothing);
    }

    #[test]
    fn a_cfg_test_over_a_module_is_a_test_module() {
        // THE REFUSAL THAT MUST SURVIVE. Both spellings of a test module arriving, on one line
        // and on two: this is the shape whose unnameable test is a deliberate refusal, so it stays
        // a candidate for the proof and `Scan::Unnamed` stays reachable from it.
        assert_eq!(of("#[cfg(test)]", "#[cfg(test)]\nmod tests;\n"), Adds::TestModule);
        assert_eq!(of("#[cfg(test)] mod tests;", "#[cfg(test)] mod tests;\n"), Adds::TestModule);
        assert_eq!(
            of("#[cfg(test)]", "#[cfg(test)]\npub(crate) mod testing;\n"),
            Adds::TestModule
        );
        assert_eq!(of("mod tests {", "mod tests {\n}\n"), Adds::TestModule);
    }

    #[test]
    fn a_cfg_test_over_anything_else_names_no_test_and_never_will() {
        // THE DEFECT. Each of these is test-only code that cannot name a test, and calling it a
        // test file is what turned a diff the gate had nothing to say about into
        // `FAILED - the added tests could not be NAMED` - a refusal with no fix.
        assert_eq!(
            of("#[cfg(test)]", "#[cfg(test)]\npub(crate) fn helper() -> u8 { 1 }\n"),
            Adds::TestOnlyItem
        );
        assert_eq!(of("#[cfg(test)]", "#[cfg(test)]\nuse std::fmt;\n"), Adds::TestOnlyItem);
        assert_eq!(
            of("#[cfg(test)]", "#[cfg(test)]\nconst SAMPLE: &str = \"x\";\n"),
            Adds::TestOnlyItem
        );
        assert_eq!(of("#[cfg(test)]", "#[cfg(test)]\nimpl Fake {\n}\n"), Adds::TestOnlyItem);
    }

    #[test]
    fn the_item_under_the_attribute_is_read_past_comments_and_other_attributes() {
        // The doc comment and the second attribute are exactly what a real helper carries, and
        // reading only the next line would call every one of them a module by default.
        let text = concat!(
            "#[cfg(test)]\n",                                               // 1
            "/// What it does.\n",                                          // 2
            "#[expect(clippy::needless_pass_by_value, reason = \"..\")]\n", // 3
            "pub(crate) fn helper(v: String) {}\n",                         // 4
        );
        assert_eq!(of("#[cfg(test)]", text), Adds::TestOnlyItem);
    }

    #[test]
    fn an_unreadable_post_image_is_a_test_module() {
        // Fail closed on not knowing: `TestModule` is the answer that keeps an unnameable test a
        // refusal, and `TestOnlyItem` is the answer that would silently drop the file from the
        // proof. The reader answers `None` for a path it has no content for.
        let read = tree(&[]);
        assert_eq!(
            adds(&[AddedLine::new(1, "#[cfg(test)]")], "crates/x/src/gone.rs", &read),
            Adds::TestModule
        );
    }

    #[test]
    fn the_item_is_read_past_an_attribute_the_formatter_wrapped() {
        // THE DEFECT, and the assertion that carries it is the MODULE one. Reading the attribute as
        // one line stopped the search on `clippy::disallowed_methods,`, which is not a module - so
        // a test module behind a wrapped attribute read as `TestOnlyItem`, and a `TestOnlyItem` is
        // asked for no name, which is exactly the exemption that must not reach a test module.
        let module = concat!(
            "#[cfg(test)]\n",                    // 1
            "#[expect(\n",                       // 2
            "    unused_imports,\n",             // 3
            "    reason = \"only some cfgs\"\n", // 4
            ")]\n",                              // 5
            "mod tests;\n",                      // 6
        );
        assert_eq!(of("#[cfg(test)]", module), Adds::TestModule);
        // And the helper it is told apart from, in the spelling the tree actually writes - eight
        // `#[test]`s sit over exactly this shape. Unchanged by the fix, and here as the guard that
        // reading further down does not turn every wrapped item into a module.
        let helper = concat!(
            "#[cfg(test)]\n",                        // 1
            "#[expect(\n",                           // 2
            "    clippy::needless_pass_by_value,\n", // 3
            "    reason = \"the test owns it\"\n",   // 4
            ")]\n",                                  // 5
            "pub(crate) fn helper(v: String) {}\n",  // 6
        );
        assert_eq!(of("#[cfg(test)]", helper), Adds::TestOnlyItem);
    }

    #[test]
    fn an_attribute_that_never_closes_leaves_the_file_a_test_module() {
        // `attribute_end` gives no answer for an unclosed attribute, and `item_below` passes that
        // through as `None` - which lands on the SAME fail-closed arm as an unreadable
        // post-image, so an unnameable test stays a refusal rather than dropping out of the proof.
        let text = concat!("#[cfg(test)]\n", "#[expect(\n", "    clippy::x,\n");
        assert_eq!(of("#[cfg(test)]", text), Adds::TestModule);
    }

    #[test]
    fn the_attribute_block_attached_to_an_item_stops_at_a_blank_line_and_at_an_item() {
        // The two boundaries, because the upward walk this replaced got both for free and a
        // top-down read has to state them. `#[ignore]` on `first` may not be borrowed by
        // `second` - neither across the blank line nor across `first`'s own signature.
        let lines: Vec<&str> = vec![
            "#[test]",           // 0
            "#[ignore]",         // 1
            "fn first() {}",     // 2
            "#[test]",           // 3
            "fn second() {}",    // 4
            "",                  // 5
            "#[test]",           // 6
            "/// what it does.", // 7
            "fn third() {}",     // 8
        ];
        assert_eq!(attached(&lines, 2), vec![(0, "#[test]"), (1, "#[ignore]")]);
        assert_eq!(attached(&lines, 4), vec![(3, "#[test]")]);
        // A comment between an attribute and its item is ordinary here and does not end a block.
        assert_eq!(attached(&lines, 8), vec![(6, "#[test]")]);
    }

    #[test]
    fn a_wrapped_attribute_is_one_entry_in_the_block_rather_than_a_boundary() {
        // The half `is_ignored` needs, in the spelling `crates/sutura-catalog-datahub/tests`
        // writes twice: a reason string continued with a trailing `\` makes `#[ignore = ".."]` one
        // attribute over two lines. Reading the second line as an ITEM cleared the block, so the
        // `#[ignore]` was not attached to anything, the test entered the filterset as runnable,
        // and `Scan::OnlyIgnored`'s loud pass was unreachable for it.
        let wrapped = "#[ignore = \"needs a tier; run `just datahub-acceptance` \\";
        let lines: Vec<&str> = vec![
            wrapped,                 // 0
            "            for it\"]", // 1
            "#[test]",               // 2
            "fn acceptance() {}",    // 3
        ];
        assert_eq!(attached(&lines, 3), vec![(0, wrapped), (2, "#[test]")]);
    }

    #[test]
    fn the_strongest_line_in_the_hunk_wins() {
        // A hunk that adds a helper AND a test is a test file: the precedence is the enum's own
        // order, so this cannot be lost by the order the lines happen to arrive in.
        let text = concat!(
            "#[cfg(test)]\n",                           // 1
            "fn helper() -> u8 { 1 }\n",                // 2
            "#[cfg(test)]\n",                           // 3
            "mod tests {\n",                            // 4
            "    #[test]\n",                            // 5
            "    fn t() { assert_eq!(helper(), 1) }\n", // 6
            "}\n",                                      // 7
        );
        let read = tree(&[("crates/x/src/a.rs", text)]);
        let hunk = added_from(
            1,
            &[
                "#[cfg(test)]",
                "fn helper() -> u8 { 1 }",
                "#[cfg(test)]",
                "mod tests {",
                "    #[test]",
                "    fn t() { assert_eq!(helper(), 1) }",
                "}",
            ],
        );
        assert_eq!(adds(&hunk, "crates/x/src/a.rs", &read), Adds::NamedTest);
    }
}

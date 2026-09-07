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

/// The attributes attached to the item at 0-based `index`: WHERE THE BLOCK STARTS, and its
/// opening lines, trimmed.
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
/// Comments do not end a block: a doc comment between an attribute and its item is ordinary here.
///
/// THE START INDEX IS RETURNED because a caller turning a block into a LINE RANGE cannot
/// reconstruct it. The block is whatever survived the last clear, which sits ABOVE the declaring
/// attribute whenever `#[ignore]` is written first, and a range anchored on the declaring
/// attribute leaves every line above it outside - a measured fail-open, recorded on [`cells`].
/// `index` itself when the block is empty, so a range built on it is never wider than the item.
///
/// Empty for an attribute that never closes, which is [`attribute_end`]'s refusal reaching this
/// far: nothing is claimed about a file that does not compile.
pub(super) fn block<'l>(lines: &[&'l str], index: usize) -> (usize, Vec<&'l str>) {
    let mut attached: Vec<&'l str> = Vec::new();
    let mut start = index;
    let mut cursor = 0_usize;
    while cursor < index {
        let Some(trimmed) = lines.get(cursor).map(|line| line.trim()) else {
            break;
        };
        if trimmed.starts_with("//") {
            cursor += 1;
        } else if trimmed.starts_with("#[") {
            if attached.is_empty() {
                start = cursor;
            }
            attached.push(trimmed);
            match attribute_end(lines, cursor) {
                Some(last) => cursor = last + 1,
                None => return (index, Vec::new()),
            }
        } else {
            // A blank line or an item of its own: whatever preceded it is not attached to
            // `index`.
            attached.clear();
            start = index;
            cursor += 1;
        }
    }
    (start, attached)
}

/// The attached attributes alone, for the callers that ask what a block SAYS rather than where it
/// begins.
pub(super) fn attached<'l>(lines: &[&'l str], index: usize) -> Vec<&'l str> {
    block(lines, index).1
}

/// Does this attribute decide whether the item under it runs, in a way this scan cannot evaluate?
///
/// `#[cfg(..)]` and `#[cfg_attr(..)]` are the two, and neither is a hypothetical: this workspace
/// writes `#[cfg(not(feature = ".."))]` and `#[cfg(feature = "..")]` directly on test cells, and
/// the vendored allocator writes `#[cfg(all(feature = "..", target_vendor = ".."))]` on one.
/// Evaluating them needs the feature resolution and the target of the run being asked about, and
/// this module has neither - so the honest answer is *unknown*, and [`cells`] spends it in the
/// direction where unknown costs a false red rather than a silent pass.
///
/// `#[cfg(test)]` is the one exact spelling that IS evaluable: every venue that runs a `#[test]`
/// runs it with `cfg(test)` on, so the attribute cannot be what stops the cell. Only that
/// spelling - `#[cfg(all(test, ..))]` is a conjunction whose other arms are unknown, which is the
/// same limit `super::regions` states for the same reason.
fn undecidable(opening: &str) -> bool {
    if opening.starts_with("#[cfg_attr(") {
        return true;
    }
    opening.starts_with("#[cfg(") && opening != "#[cfg(test)]"
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
    /// 1-based, half-open line ranges, one per test no run here is known to reach: the first line
    /// of its ATTRIBUTE BLOCK through the last line of the function under it.
    ///
    /// From the block rather than from the declaring attribute, and the difference was measured
    /// through `crate::examples`: with `#[ignore = ".."]` written above `#[test]`, a range
    /// anchored on the declaring attribute leaves the `#[ignore]` line itself outside, and the
    /// gate reported the reach from the very attribute that stops the test running - exit 0.
    unreached: Vec<Range<usize>>,
    /// Tests declared here that a run in this venue reaches.
    runs: usize,
    /// Test-declaring attributes whose item this could not resolve, so nothing may be claimed
    /// about whether they run.
    unresolved: usize,
}

impl Cells {
    /// Is the line at 1-based `number` inside a test no run here is known to reach?
    pub(crate) fn unreached(&self, number: usize) -> bool {
        self.unreached.iter().any(|region| region.contains(&number))
    }

    /// Does this file declare tests of which NONE runs here?
    ///
    /// The coarser of the two answers, and it is needed BESIDE [`Self::unreached`] rather than
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

    /// Test-declaring attributes whose item could not be resolved. Non-zero is a broken scan,
    /// not a clean file: a caller that cannot see the test cannot say whether it runs.
    pub(crate) const fn unresolved(&self) -> usize {
        self.unresolved
    }
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
/// AN UNEVALUABLE RUN-DECIDING ATTRIBUTE IS NOT RUNNING, which is the direction the caller's
/// failure mode chooses. [`undecidable`] says which those are; the argument
/// `super::scoped::is_ignored` gives for tolerating them does NOT transfer here, and importing it
/// would be the overstated control `AGENTS.md` calls a defect in itself. There, missing an
/// `#[ignore]` puts an ignored test into a filterset and nextest exits 4 with *no tests to run*:
/// loud. Here, missing one leaves a cell counted as running and the caller's verdict is a silent
/// exit 0. Both spellings were measured through that gate: `#[cfg_attr(all(), ignore)]` on both
/// cells of the one file reaching a variant, and `#[cfg(feature = "..")]` on the same two, each
/// left the verdict BYTE-IDENTICAL to the healthy one.
///
/// WHAT THAT COSTS, stated because it is the price and not a footnote: `just test` runs
/// `--all-features`, so a `#[cfg(feature = "x")]` cell DOES run there and this calls it
/// unreached. The error is a false RED - the gate asks for a reach from a test that runs
/// unconditionally - and never a green one. Cells written that way ARE live in this workspace
/// (`#[cfg(feature = "bigquery")]` and its `not(..)`), and none of them reaches `examples/`, so
/// today the whole cost is a smaller declaration count in that gate's verdict. No figure here:
/// the verdict prints the number and a copy of it rots first.
///
/// WHAT IT STILL DOES NOT READ: a `cfg` on an ANCESTOR. `#[cfg(feature = "x")] mod tests { .. }`
/// puts the attribute on the module, an item resets the block, and every `#[test]` inside is
/// counted as running. The block is a line scan, not a scope walk; closing it needs the enclosing
/// item, which is `super::regions`' question rather than this one's.
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
            found.unresolved = found.unresolved.saturating_add(1);
            continue;
        };
        let (top, attrs) = block(&lines, at);
        if attrs
            .iter()
            .any(|opening| opening.starts_with("#[ignore") || undecidable(opening))
        {
            // FROM THE TOP OF THE BLOCK, not from the declaring attribute: `#[ignore]` is legal
            // above `#[test]`, and every attribute line above the declaring one is then outside a
            // region anchored on it. Measured through `crate::examples` on this tree, with the
            // reason text naming the variant - `reached from 1 file - ..:95`, where line 95 IS the
            // `#[ignore = ".."]` line, exit 0. `top` is the declaring attribute's own index when
            // there is no block, so the region never widens past the item.
            found
                .unreached
                .push(top.saturating_add(1)..item_end(&lines, at).saturating_add(2));
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
            assert_eq!(found.unresolved(), 0, "{source}");
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
            assert_eq!(found.unresolved(), 0, "{source}");
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
    fn the_region_starts_at_the_top_of_the_block_not_at_the_declaring_attribute() {
        // MEASURED THROUGH `crate::examples` AT EXIT 0: `#[ignore]` is legal above `#[test]`, and
        // a region anchored on the declaring attribute leaves every line above it outside - so a
        // reach written into the `#[ignore = ".."]` reason was reported as the evidence that the
        // test runs.
        let above = of_file("#[ignore = \"needs a deployment\"]\n#[test]\nfn t() {\n    let p = 1;\n}\n");
        assert!(above.unreached(1), "the `#[ignore]` line is inside the cell it describes");
        assert!(above.unreached(2), "and so is the declaring attribute under it");
        assert!(above.unreached(4), "and the body");
        assert!(!above.unreached(6), "and nothing past the closing brace");
        // A block with something ELSE at the top pulls the region up to that line too, because
        // what the region is about is the cell, not the `#[ignore]`.
        let decorated = of_file("#[allow(dead_code)]\n#[test]\n#[ignore]\nfn t() {\n    let p = 1;\n}\n");
        assert!(decorated.unreached(1), "{decorated:?}");
        // A cell with NO block above the declaring attribute is unchanged: the region may never
        // widen past its own item.
        let bare = of_file("fn a() {\n    let p = 1;\n}\n#[test]\n#[ignore]\nfn t() {}\n");
        assert!(!bare.unreached(1), "the neighbour above is not part of the cell: {bare:?}");
        assert!(bare.unreached(4), "{bare:?}");
    }

    #[test]
    fn an_attribute_this_cannot_evaluate_leaves_the_cell_unreached() {
        // Both spellings were measured as a fail-OPEN through `crate::examples`, each leaving that
        // gate's verdict byte-identical to the healthy one at exit 0. Evaluating either needs the
        // feature resolution and the target of the run, which this module has not got, so the
        // answer is *unknown* - spent here in the direction where unknown costs a false red.
        for source in [
            "#[cfg_attr(all(), ignore)]\n#[test]\nfn t() {\n    let p = 1;\n}\n",
            "#[test]\n#[cfg_attr(unix, ignore)]\nfn t() {\n    let p = 1;\n}\n",
            "#[cfg(feature = \"off\")]\n#[test]\nfn t() {\n    let p = 1;\n}\n",
            "#[test]\n#[cfg(not(feature = \"on\"))]\nfn t() {\n    let p = 1;\n}\n",
            "#[test]\n#[cfg(all(test, unix))]\nfn t() {\n    let p = 1;\n}\n",
        ] {
            let found = of_file(source);
            assert!(found.nothing_runs(), "{source}");
            assert_eq!(found.runs(), 0, "{source}");
            assert_eq!(found.unresolved(), 0, "{source} - it resolved; it is not known to run");
            assert!(found.unreached(4), "the body is inside the cell: {source}");
        }
        // The one spelling that IS evaluable: every venue that runs a `#[test]` runs it with
        // `cfg(test)` on, so this attribute cannot be what stops the cell.
        let evaluable = of_file("#[cfg(test)]\n#[test]\nfn t() {\n    let p = 1;\n}\n");
        assert_eq!(evaluable.runs(), 1, "{evaluable:?}");
        assert!(!evaluable.nothing_runs(), "{evaluable:?}");
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
    fn a_declaration_with_no_item_under_it_is_unresolved_rather_than_running() {
        // A caller that cannot see the test cannot say whether it runs, and saying it runs is the
        // fail-open direction.
        let found = of_file("fn a() {}\n\n#[test]\n");
        assert_eq!(found.unresolved(), 1, "{found:?}");
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
        assert_eq!(found.unresolved(), 0);
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
        assert_eq!(attached(&lines, 2), vec!["#[test]", "#[ignore]"]);
        assert_eq!(attached(&lines, 4), vec!["#[test]"]);
        // A comment between an attribute and its item is ordinary here and does not end a block.
        assert_eq!(attached(&lines, 8), vec!["#[test]"]);
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
        assert_eq!(attached(&lines, 3), vec![wrapped, "#[test]"]);
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

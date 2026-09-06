//! The attribute vocabulary this gate reads, and what a hunk of added lines makes of the file it
//! landed in.
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
//! Driven over a `tests/`-target file whose only added content is `#[test]` plus a wrapped
//! `async fn a_very_long_...(`, the scan returns `Runnable` and NAMES it. What names nothing is a
//! **body-only or def-interior edit**: added lines inside an existing test, or inside a signature
//! whose first line this diff did not touch. The ATTRIBUTE is not read as one line -
//! [`item_below`] balances its brackets, because reading one as a single line was a defect and
//! that module's own doc carries the measurement.
//! And nothing here is keyed on a file's extension - `super::plan` decides that, and a test whose
//! subject is not Rust at all is outside this module and outside the gate.

use crate::causality::regions::{AddedLine, PostImage, attribute_end, item_head};

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

/// The attributes attached to the item at 0-based `index`, as their opening lines, trimmed.
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
/// Empty for an attribute that never closes, which is [`attribute_end`]'s refusal reaching this
/// far: nothing is claimed about a file that does not compile.
pub(super) fn attached<'l>(lines: &[&'l str], index: usize) -> Vec<&'l str> {
    let mut block: Vec<&'l str> = Vec::new();
    let mut cursor = 0_usize;
    while cursor < index {
        let Some(trimmed) = lines.get(cursor).map(|line| line.trim()) else {
            break;
        };
        if trimmed.starts_with("//") {
            cursor += 1;
        } else if trimmed.starts_with("#[") {
            block.push(trimmed);
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
    use super::{Adds, adds, attached};
    use crate::causality::fixtures::{added_from, tree};
    use crate::causality::regions::AddedLine;

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

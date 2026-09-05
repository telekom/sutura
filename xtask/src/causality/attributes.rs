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
//! `scoped::tests::a_marker_with_no_test_function_names_nothing` intact: a diff whose only test
//! signal is `#[cfg(test)] mod tests;` still refuses, because a test MODULE plausibly gained tests
//! this extractor could not read and the fix for that is the extractor.
//!
//! WHY A `TestOnlyItem` IS HELD AND NOT REVERTED - the half a narrower `adds_test` would have lost.
//! Reverting the file takes the helper out of the base tree while the tests that call it are held
//! at HEAD: `E0425`, `DidNotCompile`, and after the retry
//! *INCONCLUSIVE - the base tree does not build* - a pass that proves nothing, over the COMMON
//! case, because a new test helper usually exists precisely because a new test needed it. Holding
//! it is also what the gate's own model says: production at base, test code at HEAD, and
//! `#[cfg(test)]` code is not production behaviour. A production change in the same file is
//! unaffected - `super::regions::has_non_test_additions` still sees it, and the file is held with
//! its own tests out of the proof either way.
//!
//! WHAT IT STILL DOES NOT READ. Only the exact `#[cfg(test)]` spelling, the same limit
//! `super::regions` states for the same reason: `#[cfg(all(test, ..))]` and
//! `#[cfg_attr(.., cfg(test))]` are [`Adds::Nothing`] here and production code there. The item
//! under an attribute is read as ONE line, so a declaration the formatter would have to wrap is
//! not seen. And nothing here is keyed on a file's extension - `super::plan` decides that, and a
//! test whose subject is not Rust at all is outside this module and outside the gate.

use crate::causality::regions::{AddedLine, PostImage, item_head};

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

/// Blank, a comment or another attribute: the things that legitimately sit between an attribute
/// and the item it applies to. Anything else ends a search, so a stray attribute does not reach
/// down the file and name an unrelated item.
///
/// The same three clauses as `super::regions::carries_no_behaviour`, and deliberately not that
/// function: it answers *does this ADDED line change what the code does*, which is a question
/// about a diff, and this answers *may this line sit between an attribute and its item*, which is
/// a question about Rust's grammar. They agree today by coincidence, and only one of them should
/// follow `#![..]` inner attributes.
pub(super) fn sits_between(trimmed: &str) -> bool {
    trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("#[")
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
    let module = if rest.trim().is_empty() {
        below(lines, added.number).is_none_or(is_module_item)
    } else {
        is_module_item(rest)
    };
    if module { Adds::TestModule } else { Adds::TestOnlyItem }
}

/// Does this line declare a `mod`, whatever visibility or attribute prefixes it carries?
fn is_module_item(line: &str) -> bool {
    item_head(line).starts_with("mod ")
}

/// The line carrying the item an attribute at 1-based `number` applies to.
fn below<'l>(lines: &[&'l str], number: usize) -> Option<&'l str> {
    // `number` is 1-based, so skipping that many lands on the line AFTER the attribute.
    lines
        .iter()
        .skip(number)
        .map(|line| line.trim())
        .find(|trimmed| !sits_between(trimmed))
}

#[cfg(test)]
mod tests {
    use super::{Adds, adds};
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
